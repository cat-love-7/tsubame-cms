'use strict';

const { buildContentModel, contentFieldNames, ownerNodeKey, schemaKey } = require('./model');
const { imageKey } = require('./fields');
const { downloadImages } = require('./images');
const { collectImageValues, collectReferenceKeys, resolveFieldValue } = require('./values');

/**
 * Turning the delivery API's content into Gatsby nodes.
 *
 * Three kinds of node come out of one run:
 *
 * - one `<Collection>` node per collection with published items, holding the schema and the count
 *   (what a site iterates to build its section pages);
 * - one node per published item, of a type named after its collection (`SlCmsBlogItem`);
 * - one node per published single page, of a type named after the page (`SlCmsHomePage`).
 *
 * On top of that, every Markdown value becomes a `<Markdown>` node of its own with
 * `internal.mediaType = 'text/markdown'`. That is the whole trick for gatsby-transformer-remark:
 * it transforms *any* node with that media type, reading the source from `internal.content`
 * (`gatsby/src/utils/nodes.ts`: `loadNodeContent` returns `node.internal.content` when it is a
 * string). The transformer adds a `MarkdownRemark` child, so the item's `body` field - linked to
 * the markdown node - is queried as `body { childMarkdownRemark { html } }`. A Markdown field
 * inside a composite is a child of the same item, with its full path as `path`.
 *
 * Why not put the media type on the item node itself: an item is not Markdown, it *has* Markdown
 * fields, possibly several, and a node has one `internal.content`. One node per value is the shape
 * that keeps the fields apart and still lets the transformer do the parsing.
 *
 * Relations are links, both ways. A forward relation holds the target's node id, and a reverse one
 * (`inverse_name`) holds the ids of the content that references this node. The reverse direction is
 * answered from the content this run already read rather than from the delivery API's
 * `?populate=<inverse_name>`: a build holds every published item anyway, while the route would be
 * one request per item, and the API's own rule - the published copy decides, and a reference inside
 * a composite counts - is what the index here applies.
 *
 * `dependencies.createRemoteFileNode` is the sharp integration's seam: production leaves it out and
 * the plugin loads `gatsby-source-filesystem` itself, while a test passes a stand-in.
 */
async function sourceAll(gatsbyApi, client, options, dependencies = {}) {
  const { actions, createNodeId, createContentDigest, reporter } = gatsbyApi;
  const snapshot = await client.fetchSchemaSnapshot();
  const model = buildContentModel(snapshot, options);

  // Read the whole site before creating anything: a reverse reference is answered from the
  // published content on the other side, which may be read before or after the node it points at.
  const collections = [];
  for (const collectionName of [...snapshot.collectionNames].sort()) {
    const typeName = model.plan.collections.get(collectionName);
    const mapping = model.fieldNames.collections.get(collectionName);
    if (typeName === undefined || mapping === undefined) {
      continue;
    }

    const content = await client.fetchCollection(collectionName);
    if (content.schema === null) {
      continue;
    }

    collections.push({
      name: collectionName,
      typeName,
      mapping,
      // The schema the type was declared from, not the one this response carried: a field the
      // declaration does not know has no GraphQL name to be put under.
      schema: snapshot.collections.get(collectionName) ?? content.schema,
      items: content.items,
      total: content.total,
    });
  }

  const pages = [];
  for (const pageName of [...snapshot.pageNames].sort()) {
    const typeName = model.plan.pages.get(pageName);
    const mapping = model.fieldNames.pages.get(pageName);
    if (typeName === undefined || mapping === undefined) {
      continue;
    }

    const page = await client.fetchSinglePage(pageName);
    if (page === null) {
      continue;
    }

    pages.push({
      name: pageName,
      typeName,
      mapping,
      schema: snapshot.pages.get(pageName) ?? [],
      values: page.values,
      publishedAt: page.published_at === undefined ? null : page.published_at,
      lastPublishedAt: page.last_published_at === undefined ? null : page.last_published_at,
    });
  }

  const reverse = buildReverseIndex(model, collections, pages);

  // The bytes are not in the API response, so they are fetched before any node is created: an image
  // value has to hold the File node's id at the moment it is resolved.
  const imageFiles = options.images.download
    ? await downloadImages({
        gatsbyApi,
        client,
        images: collectAllImages(model, collections, pages),
        options,
        createRemoteFileNode: dependencies.createRemoteFileNode,
      })
    : null;

  const counters = { collections: 0, items: 0, pages: 0, markdown: 0 };

  // Sorted, because the model allocated the type names in sorted order and the two have to agree
  // about which collection got which type when two names collide.
  for (const collection of collections) {
    createCollectionNode(gatsbyApi, model, {
      name: collection.name,
      typeName: collection.typeName,
      schema: collection.schema,
      itemCount: collection.total,
      mapping: collection.mapping,
    });
    counters.collections += 1;

    for (const item of collection.items) {
      if (item === null || typeof item !== 'object' || typeof item.id !== 'number') {
        reporter.warn(`[gatsby-source-sl-cms] collection "${collection.name}": an item without a numeric id was skipped`);
        continue;
      }
      counters.markdown += createContentNode(gatsbyApi, model, client, {
        kind: 'item',
        typeName: collection.typeName,
        schema: collection.schema,
        mapping: collection.mapping,
        values: item.values,
        remoteId: item.id,
        collection: collection.name,
        pageName: null,
        publishedAt: item.published_at === undefined ? null : item.published_at,
        lastPublishedAt: item.last_published_at === undefined ? null : item.last_published_at,
        inverse: lookupInverse(reverse, {
          kind: 'collection',
          name: collection.name,
          item: item.id,
        }),
        imageFiles,
      });
      counters.items += 1;
    }
  }

  for (const page of pages) {
    counters.markdown += createContentNode(gatsbyApi, model, client, {
      kind: 'page',
      typeName: page.typeName,
      schema: page.schema,
      mapping: page.mapping,
      values: page.values,
      remoteId: null,
      collection: null,
      pageName: page.name,
      publishedAt: page.publishedAt,
      lastPublishedAt: page.lastPublishedAt,
      inverse: lookupInverse(reverse, { kind: 'single_page', name: page.name, item: null }),
      imageFiles,
    });
    counters.pages += 1;
  }

  return counters;
}

/**
 * Who references whom, keyed by the target's node key.
 *
 * A content type declares a name for a relation it holds (`inverse_name`), and this walks the
 * content that was read for the targets it actually names. The declared name is what the target
 * answers to; the walk is what decides whether this particular copy holds the reference, which is
 * the same pair of questions the delivery API's `?populate=<inverse_name>` asks.
 *
 * @returns {Map<string, Map<string, string[]>>} target node key -> inverse name -> referrer keys
 */
function buildReverseIndex(model, collections, pages) {
  const reverse = new Map();

  const add = (targetNodeKey, inverseName, referrerKey) => {
    let byName = reverse.get(targetNodeKey);
    if (byName === undefined) {
      byName = new Map();
      reverse.set(targetNodeKey, byName);
    }
    let referrers = byName.get(inverseName);
    if (referrers === undefined) {
      referrers = [];
      byName.set(inverseName, referrers);
    }
    if (!referrers.includes(referrerKey)) {
      referrers.push(referrerKey);
    }
  };

  const visit = (schemaKind, name, schema, values, referrerKey) => {
    const declarations = model.inverseDeclarationsBySchema.get(schemaKey(schemaKind, name));
    if (declarations === undefined) {
      return;
    }
    const referenced = collectReferenceKeys(schema, values, model.snapshot.composites);
    for (const declaration of declarations) {
      for (const reference of referenced) {
        if (reference.kind !== declaration.target.kind || reference.name !== declaration.target.name) {
          continue;
        }
        add(ownerNodeKey(reference), declaration.inverseName, referrerKey);
      }
    }
  };

  for (const collection of collections) {
    for (const item of collection.items) {
      if (item === null || typeof item !== 'object' || typeof item.id !== 'number') {
        continue;
      }
      visit(
        'collection',
        collection.name,
        collection.schema,
        item.values,
        ownerNodeKey({ kind: 'collection', name: collection.name, item: item.id }),
      );
    }
  }
  for (const page of pages) {
    visit('page', page.name, page.schema, page.values, ownerNodeKey({ kind: 'single_page', name: page.name, item: null }));
  }

  return reverse;
}

function lookupInverse(reverse, owner) {
  return reverse.get(ownerNodeKey(owner)) ?? new Map();
}

/**
 * Every image the run holds, deduped by id.
 *
 * The same image is usually referenced from several items (and from Markdown inside composites), so
 * this is what keeps one download per image rather than one per reference.
 */
function collectAllImages(model, collections, pages) {
  const found = new Map();

  const take = (schema, values) => {
    for (const image of collectImageValues(schema, values, model.snapshot.composites)) {
      const key = imageKey(image);
      if (key !== null) {
        found.set(key, image);
      }
    }
  };

  for (const collection of collections) {
    for (const item of collection.items) {
      if (item !== null && typeof item === 'object') {
        take(collection.schema, item.values);
      }
    }
  }
  for (const page of pages) {
    take(page.schema, page.values);
  }

  return [...found.values()];
}

function createCollectionNode({ actions, createNodeId, createContentDigest }, model, { name, typeName, schema, itemCount, mapping }) {
  const data = {
    name,
    itemTypeName: typeName,
    itemCount,
    schema,
    fieldNames: contentFieldNames(model, 'collection', name),
  };
  actions.createNode({
    id: createNodeId(`sl-cms-collection:${name}`),
    parent: null,
    children: [],
    ...data,
    internal: {
      type: model.names.collection,
      description: `sl_cms collection "${name}"`,
      contentDigest: createContentDigest(data),
    },
  });
}

/**
 * One item or single page, with its Markdown values as child nodes and its relations as links.
 *
 * The Markdown nodes are created *after* their owner, but their ids are computed before it, because
 * the owner's `@link` fields have to hold those ids. `createNodeId` is a pure function of its key,
 * so knowing the id early costs nothing and avoids creating a node and then mutating it - which
 * Gatsby does not allow.
 */
function createContentNode({ actions, createNodeId, createContentDigest }, model, client, { kind, typeName, schema, mapping, values, remoteId, collection, pageName, publishedAt, lastPublishedAt, inverse, imageFiles }) {
  const typedValues = values !== null && typeof values === 'object' ? values : {};
  const schemaKind = kind === 'item' ? 'collection' : 'page';
  const schemaName = kind === 'item' ? collection : pageName;

  const ownerKey = ownerNodeKey(
    kind === 'item'
      ? { kind: 'collection', name: collection, item: remoteId }
      : { kind: 'single_page', name: pageName, item: null },
  );
  const ownerId = createNodeId(ownerKey);

  const data =
    kind === 'item'
      ? {
          remoteId,
          collection,
          publishedAt,
          lastPublishedAt,
          values: typedValues,
          fieldNames: contentFieldNames(model, schemaKind, schemaName),
        }
      : {
          name: pageName,
          publishedAt,
          lastPublishedAt,
          values: typedValues,
          fieldNames: contentFieldNames(model, schemaKind, schemaName),
        };

  const markdownPlans = [];
  const planMarkdown = (path, raw, field) => {
    const id = createNodeId(`sl-cms-markdown:${ownerId}:${path}`);
    markdownPlans.push({ id, path, raw, field });
    return id;
  };
  const resolveUrl = (path) => client.absoluteUrl(path);
  const resolveApiPath = (path) => client.apiPathUrl(path);

  for (const field of Array.isArray(schema) ? schema : []) {
    const originalName = String(field.name);
    const graphqlName = mapping.get(originalName);
    if (graphqlName === undefined) {
      continue;
    }
    data[graphqlName] = resolveFieldValue(field.field_type, typedValues[originalName], {
      model,
      createNodeId,
      planMarkdown,
      resolveUrl,
      resolveApiPath,
      imageFiles,
      topField: originalName,
      path: originalName,
    });
  }

  // The reverse references: this node is the target, and the ids are the content that names it.
  const inverseNames = model.inverseFieldNames.get(schemaKey(schemaKind, schemaName)) ?? new Map();
  for (const [inverseName, graphqlName] of inverseNames) {
    data[graphqlName] = (inverse.get(inverseName) ?? []).map((referrerKey) => createNodeId(referrerKey));
  }

  const ownerNode = {
    id: ownerId,
    parent: null,
    children: [],
    ...data,
    internal: {
      type: typeName,
      contentDigest: createContentDigest(data),
    },
  };
  actions.createNode(ownerNode);

  for (const planEntry of markdownPlans) {
    const markdownData = {
      raw: planEntry.raw,
      field: planEntry.field,
      path: planEntry.path,
      collection: collection ?? null,
      pageName: pageName ?? null,
      itemId: remoteId ?? null,
    };
    const markdownNode = {
      id: planEntry.id,
      parent: ownerId,
      children: [],
      ...markdownData,
      internal: {
        // `text/markdown` is the whole interface with gatsby-transformer-remark; `content` is what
        // it reads the source from.
        type: model.names.markdown,
        mediaType: 'text/markdown',
        content: planEntry.raw,
        contentDigest: createContentDigest(markdownData),
      },
    };
    actions.createNode(markdownNode);
    actions.createParentChildLink({ parent: ownerNode, child: markdownNode });
  }

  return markdownPlans.length;
}

module.exports = { sourceAll, buildReverseIndex };
