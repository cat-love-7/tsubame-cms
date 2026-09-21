// Tests for the parts of the migration that decide things: the type mapping, the value
// translation, the component ordering, and reading definitions out of a v3 checkout.
//
// Run with: node --test scripts/migrate-from-strapi/test/
//
// Nothing here needs a network or a running Strapi; the end-to-end run against a real CMS is
// `test/fake-strapi-v3.mjs` plus `migrate.mjs`, and is described in the README.

import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

import { loadDefinitionsFromProject, sanitizeComponentId } from '../lib/definitions.mjs';
import {
  chooseTitleField,
  convertEntry,
  entryDates,
  mapAttribute,
  mutualRelationPair,
  planContentType,
  relationOwnerProblems,
  relationReport,
  shouldAdoptRelation,
} from '../lib/mapping.mjs';
import {
  displayFilename,
  fileExtension,
  orderComponents,
  orderForPublish,
  sanitizeName,
  shouldPublish,
  withoutUnavailableReferences,
} from '../lib/plan.mjs';
import { MigrationState } from '../lib/state.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(here, 'fixtures', 'strapi-v3');

/**
 * A context over a set of component definitions, wired the way `migrate.mjs` wires it.
 *
 * Images resolve to their Strapi id unchanged, which is what a dry run does, so the shape a test
 * checks is the shape a dry run shows. `contentTypes` is what a relation resolves against, and
 * `itemIds` stands in for the state file's Strapi id -> CMS id map.
 */
function makeContext(
  components = {},
  { relationsMode = 'skip', contentTypes = {}, itemIds = {}, relationOwners = [] } = {},
) {
  const byUid = new Map(Object.entries(components));
  const contentTypeByUid = new Map(
    Object.entries(contentTypes).map(([uid, contentType]) => [uid.toLowerCase(), contentType]),
  );
  const plans = new Map();
  const ctx = {
    relationsMode,
    // The side an operator named with `--relation-owner`, which wins over the automatic choice.
    relationOwners: new Set(relationOwners),
    // The creating pass writes references empty; a test that wants them resolved says so.
    relationsReady: relationsMode === 'relation',
    itemIds: new Map(Object.entries(itemIds)),
    contentTypeFor: (uid) => (uid ? contentTypeByUid.get(String(uid).toLowerCase()) ?? null : null),
    pageMigrated: (name) =>
      Object.values(contentTypes).some((ct) => ct.kind === 'singleType' && ct.cmsName === name),
    itemIdOf: (collection, strapiId) => ctx.itemIds.get(`${collection}:${strapiId}`) ?? null,
    componentIdOf: (uid) => (byUid.has(uid) ? sanitizeComponentId(uid) : null),
    componentPlan: (id) => plans.get(id),
    componentDeclaresValues: (id) =>
      Object.prototype.hasOwnProperty.call(byUid.get(id)?.attributes ?? {}, 'values'),
    imageIdOf: (reference) => {
      if (reference === null || reference === undefined) return null;
      return typeof reference === 'object' ? reference.id : reference;
    },
    warn: () => {},
  };
  for (const [uid, attributes] of byUid) {
    plans.set(sanitizeComponentId(uid), planContentType(attributes, ctx));
  }
  return ctx;
}

describe('attribute mapping', () => {
  const ctx = makeContext();

  const text = { max_length: null, min_length: null };
  const cases = [
    ['string', { type: 'string' }, { Text: text }],
    ['text', { type: 'text' }, { Text: text }],
    ['email', { type: 'email' }, { Text: text }],
    ['richtext', { type: 'richtext' }, { Markdown: text }],
    ['uid', { type: 'uid', targetField: 'title' }, { Slug: {} }],
    ['integer', { type: 'integer' }, 'Number'],
    ['biginteger', { type: 'biginteger' }, 'Number'],
    ['float', { type: 'float' }, 'Number'],
    ['decimal', { type: 'decimal' }, 'Number'],
    ['boolean', { type: 'boolean' }, 'Boolean'],
    ['date', { type: 'date' }, 'Date'],
    ['datetime', { type: 'datetime' }, 'DateTime'],
    ['timestamp', { type: 'timestamp' }, 'DateTime'],
    ['time', { type: 'time' }, { Text: text }],
    ['json', { type: 'json' }, { Text: text }],
    ['enumeration', { type: 'enumeration', enum: ['a', 'b'] }, { TextEnum: ['a', 'b'] }],
  ];

  for (const [name, attribute, expected] of cases) {
    it(`maps v3 ${name} to ${JSON.stringify(expected)}`, () => {
      assert.deepEqual(mapAttribute(attribute, ctx).fieldType, expected);
    });
  }

  it('keeps the character limits a v3 string declares', () => {
    const mapped = mapAttribute({ type: 'string', maxLength: 120, minLength: 3 }, ctx);
    assert.deepEqual(mapped.fieldType, { Text: { max_length: 120, min_length: 3 } });
  });

  it('does not mistake the numeric max/min of a number for a character limit', () => {
    // `max`/`min` on a v3 attribute bound a *number*; a Text length would be a different rule.
    const mapped = mapAttribute({ type: 'integer', max: 10, min: 1 }, ctx);
    assert.equal(mapped.fieldType, 'Number');
  });

  it('maps a single media attribute to Image and a multiple one to an array of images', () => {
    const single = mapAttribute({ model: 'file', via: 'related', plugin: 'upload' }, ctx);
    assert.equal(single.fieldType, 'Image');
    assert.equal(single.multiple, false);

    const many = mapAttribute({ collection: 'file', via: 'related', plugin: 'upload' }, ctx);
    assert.deepEqual(many.fieldType, { Array: ['Image'] });
    assert.equal(many.multiple, true);
  });

  it('reads the normalised media shape the content-type-builder answers with', () => {
    const single = mapAttribute({ type: 'media', multiple: false, required: false }, ctx);
    assert.equal(single.fieldType, 'Image');
    const many = mapAttribute({ type: 'media', multiple: true }, ctx);
    assert.deepEqual(many.fieldType, { Array: ['Image'] });
  });

  it('maps a component and a repeatable component', () => {
    const componentCtx = makeContext({ 'default.seo': {} });
    const single = mapAttribute({ type: 'component', repeatable: false, component: 'default.seo' }, componentCtx);
    assert.deepEqual(single.fieldType, { CompositeField: { id: 'default.seo' } });
    assert.equal(single.multiple, false);

    const many = mapAttribute({ type: 'component', repeatable: true, component: 'default.seo' }, componentCtx);
    assert.deepEqual(many.fieldType, { Array: [{ CompositeField: { id: 'default.seo' } }] });
  });

  it('maps a dynamic zone to an array of the components it allows', () => {
    const componentCtx = makeContext({ 'default.seo': {}, 'blog.quote': {} });
    const mapped = mapAttribute(
      { type: 'dynamiczone', components: ['blog.quote', 'default.seo'] },
      componentCtx,
    );
    assert.deepEqual(mapped.fieldType, {
      Array: [{ CompositeField: { id: 'blog.quote' } }, { CompositeField: { id: 'default.seo' } }],
    });
  });

  it('skips relations, and carries the ids as text only when asked', () => {
    const relation = { model: 'user', via: 'articles' };
    assert.equal(mapAttribute(relation, ctx).skip, true);

    const asText = mapAttribute(relation, makeContext({}, { relationsMode: 'text' }));
    assert.deepEqual(asText.fieldType, { Text: { max_length: null, min_length: null } });

    const many = { collection: 'category', via: 'articles' };
    const manyAsText = mapAttribute(many, makeContext({}, { relationsMode: 'text' }));
    assert.deepEqual(manyAsText.fieldType, { Array: [{ Text: { max_length: null, min_length: null } }] });
  });

  it('reads the normalised relation shape the content-type-builder answers with', () => {
    assert.equal(mapAttribute({ nature: 'oneToMany', target: 'user' }, ctx).skip, true);
  });

  it('skips a password and an unknown type rather than inventing a field', () => {
    assert.equal(mapAttribute({ type: 'password' }, ctx).skip, true);
    assert.match(mapAttribute({ type: 'nonsense' }, ctx).reason, /unsupported attribute type/);
  });
});

describe('planContentType', () => {
  it('builds a schema in declaration order, marking required and unique', () => {
    const { schema, unmapped } = planContentType(
      {
        title: { type: 'string', required: true },
        slug: { type: 'uid' },
        code: { type: 'string', unique: true },
        count: { type: 'integer', unique: true },
        author: { model: 'user' },
      },
      makeContext(),
    );

    assert.deepEqual(
      schema.map((field) => field.name),
      ['title', 'slug', 'code', 'count'],
    );
    assert.equal(schema[0].required, true);
    assert.equal(schema[1].required, false);
    // A slug is unique by being a slug, so the flag is not repeated.
    assert.equal(schema[1].unique, undefined);
    assert.equal(schema[2].unique, true);
    // `unique` is only meaningful on text, so it is dropped rather than sent to be refused.
    assert.equal(schema[3].unique, undefined);
    assert.deepEqual(unmapped, [{ name: 'author', reason: unmapped[0].reason }]);
  });
});

describe('value conversion', () => {
  it('carries scalars, empties, enums, json and dates', () => {
    const ctx = makeContext();
    const { fields } = planContentType(
      {
        title: { type: 'string' },
        excerpt: { type: 'text' },
        body: { type: 'richtext' },
        views: { type: 'integer' },
        featured: { type: 'boolean' },
        status: { type: 'enumeration', enum: ['draft', 'live'] },
        meta: { type: 'json' },
        publishedOn: { type: 'date' },
        reviewedAt: { type: 'datetime' },
      },
      ctx,
    );
    const plan = { fields };
    const { values, missingRequired } = convertEntry(
      {
        title: 'Hello',
        excerpt: null,
        body: '',
        views: '120',
        featured: 1,
        status: 'live',
        meta: { tags: ['a'] },
        publishedOn: '2024-03-01T00:00:00.000Z',
        reviewedAt: '2024-03-02T10:00:00',
      },
      plan,
      ctx,
    );

    assert.deepEqual(values, {
      title: 'Hello',
      excerpt: '',
      body: '',
      views: 120,
      featured: true,
      status: ['live'],
      meta: '{"tags":["a"]}',
      publishedOn: '2024-03-01',
      // The offset the CMS cannot read is repaired rather than dropped.
      reviewedAt: '2024-03-02T10:00:00Z',
    });
    assert.deepEqual(missingRequired, []);
  });

  it('reports an enumeration value that is not an option', () => {
    const ctx = makeContext();
    const plan = planContentType({ status: { type: 'enumeration', enum: ['live'] } }, ctx);
    const { values, problems } = convertEntry({ status: 'gone' }, plan, ctx);
    assert.deepEqual(values.status, []);
    assert.match(problems[0], /not one of the enumeration's options/);
  });

  it('wraps a component as { id, values }, and a dynamic zone element per its __component', () => {
    const ctx = makeContext({
      'default.seo': { title: { type: 'string' }, description: { type: 'text' } },
      'blog.quote': { text: { type: 'text' } },
    });
    const { fields } = planContentType(
      {
        seo: { type: 'component', repeatable: false, component: 'default.seo' },
        sections: { type: 'component', repeatable: true, component: 'blog.quote' },
        blocks: { type: 'dynamiczone', components: ['default.seo', 'blog.quote'] },
      },
      ctx,
    );

    const { values } = convertEntry(
      {
        seo: { id: 11, title: 'T', description: 'D', created_at: 'x' },
        sections: [{ id: 21, text: 'one' }],
        blocks: [
          { __component: 'blog.quote', id: 31, text: 'quoted' },
          { __component: 'default.seo', id: 32, title: 'BT' },
        ],
      },
      { fields },
      ctx,
    );

    // The Strapi row id is dropped and the *definition* id takes its place; the component's own
    // timestamps are not fields of the schema and would be refused as unknown keys.
    assert.deepEqual(values.seo, { id: 'default.seo', values: { title: 'T', description: 'D' } });
    assert.deepEqual(values.sections, [{ id: 'blog.quote', values: { text: 'one' } }]);
    assert.deepEqual(values.blocks, [
      { id: 'blog.quote', values: { text: 'quoted' } },
      { id: 'default.seo', values: { title: 'BT', description: '' } },
    ]);
  });

  it('maps media through the image map, reporting a reference it does not have', () => {
    const ctx = makeContext();
    ctx.imageIdOf = (reference) => {
      const id = typeof reference === 'object' ? reference.id : reference;
      return id === 1 ? 42 : null;
    };
    const { fields } = planContentType(
      { cover: { model: 'file', plugin: 'upload' }, gallery: { collection: 'file', plugin: 'upload' } },
      ctx,
    );
    const { values, problems } = convertEntry({ cover: { id: 1 }, gallery: [{ id: 1 }, { id: 2 }] }, { fields }, ctx);
    assert.equal(values.cover, 42);
    assert.deepEqual(values.gallery, [42]);
    assert.match(problems[0], /1 media reference\(s\) were not migrated/);
  });

  it('lists a required field that ended up empty, without inventing content for it', () => {
    const ctx = makeContext();
    const plan = planContentType({ title: { type: 'string', required: true } }, ctx);
    const { values, missingRequired } = convertEntry({ title: '' }, plan, ctx);
    assert.equal(values.title, '');
    assert.deepEqual(missingRequired, ['title']);
  });
});

describe('component ordering', () => {
  it('writes a component after the ones it references', () => {
    const { order, bootstrap, unmigratable } = orderComponents([
      { id: 'a', attributes: { child: { type: 'component', component: 'b' } } },
      { id: 'b', attributes: {} },
    ]);
    assert.deepEqual(order, ['b', 'a']);
    assert.deepEqual(bootstrap, []);
    assert.deepEqual(unmigratable, []);
  });

  it('orders through a dynamic zone too', () => {
    const { order } = orderComponents([
      { id: 'page', attributes: { blocks: { type: 'dynamiczone', components: ['quote'] } } },
      { id: 'quote', attributes: {} },
    ]);
    assert.deepEqual(order, ['quote', 'page']);
  });

  it('allows a component that reaches itself through an array', () => {
    // The CMS accepts this (the definition being saved counts as existing), so it must not be
    // deferred and must not be ordered against itself.
    const { order, bootstrap, unmigratable } = orderComponents([
      { id: 'tree', attributes: { children: { type: 'component', repeatable: true, component: 'tree' } } },
    ]);
    assert.deepEqual(order, ['tree']);
    assert.deepEqual(bootstrap, []);
    assert.deepEqual(unmigratable, []);
  });

  it('defers a mutual array cycle for bootstrapping rather than dropping it', () => {
    // Neither side can be written first *in full*, but the pair is storable: one is written with
    // the reference left out, the other in full, and then the first in full.
    const { order, bootstrap, unmigratable } = orderComponents([
      { id: 'a', attributes: { b: { type: 'component', repeatable: true, component: 'b' } } },
      { id: 'b', attributes: { a: { type: 'component', repeatable: true, component: 'a' } } },
    ]);
    assert.deepEqual(order, []);
    assert.deepEqual(bootstrap, ['a', 'b']);
    assert.deepEqual(unmigratable, []);
  });

  it('reports a direct cycle, which no write order can produce', () => {
    // Every step is a plain component field, so the CMS refuses the second write whichever order
    // they are tried in. Strapi cannot serve the shape either, so nothing is lost by dropping it.
    const { order, bootstrap, unmigratable } = orderComponents([
      { id: 'a', attributes: { b: { type: 'component', component: 'b' } } },
      { id: 'b', attributes: { a: { type: 'component', component: 'a' } } },
    ]);
    assert.deepEqual(order, []);
    assert.deepEqual(bootstrap, []);
    assert.deepEqual(unmigratable, ['a', 'b']);
  });

  it('reports a direct self-reference, and lets a referencing component through without it', () => {
    const { order, unmigratable } = orderComponents([
      // A plain field pointing at itself: the CMS refuses it.
      { id: 'loop', attributes: { self: { type: 'component', component: 'loop' } } },
      // A repeatable field pointing at itself is an array, which is allowed.
      { id: 'tree', attributes: { children: { type: 'component', repeatable: true, component: 'tree' } } },
      // This one cannot be written with the reference, but can be written without it.
      { id: 'user', attributes: { loop: { type: 'component', component: 'loop' } } },
    ]);
    assert.deepEqual(unmigratable, ['loop']);
    // The edge into `loop` is not an ordering edge, so `user` is still written (its field is
    // dropped when the schema is built).
    assert.deepEqual(order, ['tree', 'user']);
  });

  it('drops the references that do not exist yet, and the arrays they leave empty', () => {
    const schema = [
      { name: 'line', field_type: { Text: {} } },
      { name: 'child', field_type: { CompositeField: { id: 'b' } } },
      {
        name: 'children',
        field_type: { Array: [{ CompositeField: { id: 'b' } }, { CompositeField: { id: 'a' } }] },
      },
      { name: 'only', field_type: { Array: [{ CompositeField: { id: 'b' } }] } },
    ];
    const partial = withoutUnavailableReferences(schema, new Set(['a']));

    assert.deepEqual(
      partial.map((field) => field.name),
      ['line', 'children'],
    );
    // The array kept the item type that exists; the field left with none is dropped rather than
    // sent as an empty array, which the CMS refuses.
    assert.deepEqual(partial[1].field_type, { Array: [{ CompositeField: { id: 'a' } }] });
  });
});

describe('small decisions', () => {
  it('makes a name safe to put in a URL path', () => {
    assert.equal(sanitizeName('blog/post'), 'blog-post');
    assert.equal(sanitizeName('a b'), 'a b');
  });

  it('preserves the draft/published state Strapi recorded', () => {
    // A content type with the draft/publish plugin: `published_at` decides.
    assert.equal(shouldPublish({ published_at: '2024-01-01T00:00:00Z' }, {}, true), true);
    assert.equal(shouldPublish({ published_at: null }, {}, true), false);
    assert.equal(shouldPublish({ published_at: null }, { publishAll: true }, true), true);
    assert.equal(shouldPublish({ published_at: '2024-01-01T00:00:00Z' }, { noPublish: true }, true), false);
  });

  it('publishes a content type that has no draft state at all', () => {
    // v3's flag defaults to off, and such a content type has no `published_at` field: every entry
    // is live, so reading the missing value as "a draft" would leave it invisible.
    assert.equal(shouldPublish({}, {}), true);
    assert.equal(shouldPublish({ published_at: null }, {}, false), true);
    assert.equal(shouldPublish({}, { noPublish: true }), false);
  });

  it('works out a media extension and a display name', () => {
    assert.equal(fileExtension({ ext: '.PNG' }), 'png');
    assert.equal(fileExtension({ name: 'photo.jpeg' }), 'jpeg');
    assert.equal(fileExtension({ url: '/uploads/x.webp?v=2' }), 'webp');
    assert.equal(fileExtension({ mime: 'image/gif' }), 'gif');
    assert.equal(displayFilename({ name: 'hero' }, 'png'), 'hero.png');
    assert.equal(displayFilename({ name: 'hero.png' }, 'png'), 'hero.png');
  });
});

describe('definitions from a v3 checkout', () => {
  it('finds content types, their routes and the components', async () => {
    const { contentTypes, components } = await loadDefinitionsFromProject(FIXTURES);

    const article = contentTypes.find((contentType) => contentType.apiId === 'article');
    assert.equal(article.kind, 'collectionType');
    // The custom route from api/article/config/routes.json wins over the pluralised guess.
    assert.equal(article.route, '/blog-posts');
    assert.equal(article.attributes.cover.plugin, 'upload');
    // Strapi's draft/publish plugin is off unless `options.draftAndPublish` says otherwise.
    assert.equal(article.draftAndPublish, true);

    const home = contentTypes.find((contentType) => contentType.apiId === 'home');
    assert.equal(home.kind, 'singleType');
    assert.equal(home.route, '/home');

    assert.deepEqual(
      components.map((component) => component.id),
      ['blog.branch', 'blog.quote', 'blog.tree', 'default.seo'],
    );
  });

  it('falls back to the pluralised API id when no routes.json says otherwise', async () => {
    const { contentTypes } = await loadDefinitionsFromProject(FIXTURES);
    // `home` is a single type, so its route is its own name and not pluralised.
    assert.equal(contentTypes.find((contentType) => contentType.apiId === 'home').route, '/home');
  });

  it('sanitises a component uid into a usable id', () => {
    assert.equal(sanitizeComponentId('default.seo'), 'default.seo');
    assert.equal(sanitizeComponentId('blog/a b'), 'blog-a-b');
  });
});

describe('migration state', () => {
  it('records ids, deduplicates a repeated failure, and counts what it holds', () => {
    const state = new MigrationState('/tmp/does-not-matter.json', {});
    state.markItem('article', 12, 3);
    assert.equal(state.itemId('article', 12), 3);
    assert.equal(state.itemId('article', '12'), 3, 'a state file read back has string keys');
    assert.equal(state.itemId('article', 99), null);

    state.fail({ scope: 'item', contentType: 'article', strapiId: 3, reason: 'first' });
    state.fail({ scope: 'item', contentType: 'article', strapiId: 3, reason: 'second' });
    assert.equal(state.data.failures.length, 1);
    assert.equal(state.data.failures[0].reason, 'second');

    assert.deepEqual(state.summary(), {
      composites: 0,
      schemas: 0,
      images: 0,
      items: 1,
      singlePages: 0,
      failures: 1,
      warnings: 0,
    });
  });
});

describe('relations', () => {
  // The fixture project's shapes, reduced to what the relation logic reads.
  const articleType = {
    apiId: 'article',
    name: 'article',
    cmsName: 'articles',
    kind: 'collectionType',
    attributes: {
      title: { type: 'string' },
      category: { model: 'category' },
      tags: { collection: 'tag', via: 'articles' },
      author: { model: 'user', via: 'articles' },
    },
  };
  const categoryType = {
    apiId: 'category',
    name: 'category',
    cmsName: 'categories',
    kind: 'collectionType',
    attributes: {
      name: { type: 'string' },
      section: { model: 'section', required: true },
      articles: { collection: 'article', via: 'category' },
    },
  };
  const tagType = {
    apiId: 'tag',
    name: 'tag',
    cmsName: 'tags',
    kind: 'collectionType',
    attributes: { name: { type: 'string' }, articles: { collection: 'article', via: 'tags' } },
  };
  const homeType = { apiId: 'home', name: 'home', cmsName: 'home', kind: 'singleType', attributes: {} };

  const owner = (apiId) => ({ apiId, name: apiId });

  it('adopts the side without `via`, and reports the inverse', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { article: articleType, category: categoryType } });
    // `article.category` declares no inverse, so it owns the link.
    assert.equal(shouldAdoptRelation({ model: 'category' }, 'category', owner('article'), ctx).adopt, true);
    // `category.articles` names `category` as its inverse, so the link is already kept.
    const inverse = shouldAdoptRelation({ collection: 'article', via: 'category' }, 'articles', owner('category'), ctx);
    assert.equal(inverse.adopt, false);
    assert.match(inverse.reason, /owns the relation/);
  });

  it('picks one side of a many-to-many, the same way every run', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { article: articleType, tag: tagType } });
    // "article.tags" sorts before "tag.articles", so the article side is the one kept.
    assert.equal(shouldAdoptRelation({ collection: 'tag', via: 'articles' }, 'tags', owner('article'), ctx).adopt, true);
    assert.equal(shouldAdoptRelation({ collection: 'article', via: 'tags' }, 'articles', owner('tag'), ctx).adopt, false);
  });

  it('adopts a `via` that names a field which does not point back', () => {
    const stale = {
      apiId: 'category',
      name: 'category',
      cmsName: 'categories',
      kind: 'collectionType',
      attributes: { articles: { collection: 'tag', via: 'category' } },
    };
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { article: articleType, category: stale } });
    assert.equal(shouldAdoptRelation({ collection: 'category', via: 'articles' }, 'tags', owner('article'), ctx).adopt, true);
  });

  it('builds a Relation field for a collection target, with its inverse name', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { tag: tagType } });
    const mapped = mapAttribute({ collection: 'tag', via: 'articles' }, ctx, owner('article'), 'tags');
    assert.deepEqual(mapped.fieldType, {
      Relation: {
        target: { kind: 'collection', name: 'tags' },
        has_many: true,
        inverse_name: 'articles',
      },
    });
  });

  it('builds a single-valued Relation for a one-to-one target', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { category: categoryType } });
    const mapped = mapAttribute({ model: 'category' }, ctx, owner('article'), 'category');
    assert.deepEqual(mapped.fieldType, {
      Relation: { target: { kind: 'collection', name: 'categories' }, has_many: false },
    });
  });

  it('folds has_many for a single-page target, which has no ids to hold several of', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { home: homeType } });
    const mapped = mapAttribute({ collection: 'home' }, ctx, owner('article'), 'featuredOn');
    assert.deepEqual(mapped.fieldType, {
      Relation: { target: { kind: 'single_page', name: 'home' }, has_many: false },
    });
  });

  it('skips a relation whose target is not being migrated', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { article: articleType } });
    const mapped = mapAttribute({ model: 'user', via: 'articles' }, ctx, owner('article'), 'author');
    assert.equal(mapped.skip, true);
    assert.match(mapped.reason, /not among the content types/);
  });

  it('reads the content-type-builder spelling, where the inverse is `targetAttribute`', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { tag: tagType } });
    const mapped = mapAttribute(
      { nature: 'manyToMany', target: 'tag', targetAttribute: 'articles' },
      ctx,
      owner('article'),
      'tags',
    );
    assert.equal(mapped.fieldType.Relation.inverse_name, 'articles');
    assert.equal(mapped.fieldType.Relation.has_many, true);
  });

  it('writes references only in the second pass', () => {
    const ctx = makeContext(
      {},
      {
        contentTypes: { article: articleType, category: categoryType },
        relationsMode: 'relation',
        itemIds: { 'categories:7': 3 },
      },
    );
    const plan = planContentType({ category: { model: 'category' } }, ctx, owner('article'));

    // The field is not in the schema yet when the item is created (a required relation cannot be
    // saved empty), so the first pass leaves the key out rather than writing an empty set.
    ctx.relationsReady = false;
    const first = convertEntry({ category: { id: 7 } }, plan, ctx);
    assert.equal('category' in first.values, false);
    assert.deepEqual(first.missingRequired, []);

    ctx.relationsReady = true;
    assert.deepEqual(convertEntry({ category: { id: 7 } }, plan, ctx).values.category, [
      { target: 'categories', item: 3 },
    ]);
  });

  it('reports a reference whose target was not migrated, and keeps the rest', () => {
    const ctx = makeContext(
      {},
      {
        contentTypes: { article: articleType, tag: tagType },
        relationsMode: 'relation',
        itemIds: { 'tags:1': 11 },
      },
    );
    const plan = planContentType({ tags: { collection: 'tag', via: 'articles' } }, ctx, owner('article'));
    const { values, problems } = convertEntry({ tags: [{ id: 1 }, { id: 9 }] }, plan, ctx);
    assert.deepEqual(values.tags, [{ target: 'tags', item: 11 }]);
    assert.match(problems[0], /references tags #9, which was not migrated/);
  });

  it('keeps one reference for a single-valued field and reports the rest', () => {
    const ctx = makeContext(
      {},
      {
        contentTypes: { article: articleType, category: categoryType },
        relationsMode: 'relation',
        itemIds: { 'categories:1': 1, 'categories:2': 2 },
      },
    );
    const plan = planContentType({ category: { model: 'category' } }, ctx, owner('article'));
    const { values, problems } = convertEntry({ category: [{ id: 1 }, { id: 2 }] }, plan, ctx);
    assert.deepEqual(values.category, [{ target: 'categories', item: 1 }]);
    assert.match(problems[0], /holds one reference but 2/);
  });

  it('names a single page instead of numbering it', () => {
    const ctx = makeContext({}, { contentTypes: { home: homeType }, relationsMode: 'relation' });
    const plan = planContentType({ featuredOn: { model: 'home' } }, ctx, owner('article'));
    assert.deepEqual(convertEntry({ featuredOn: { id: 1, headline: 'x' } }, plan, ctx).values.featuredOn, [
      { target: 'home' },
    ]);

    // A page that is not one of the migrated content types is reported as an unmapped field.
    const missing = makeContext({}, { contentTypes: { article: articleType }, relationsMode: 'relation' });
    const skipped = planContentType({ featuredOn: { model: 'home' } }, missing, owner('article'));
    assert.deepEqual(
      skipped.unmapped.map((entry) => entry.name),
      ['featuredOn'],
    );
  });
  it('names both fields of a mutual relation, so a choice can be made about it', () => {
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { article: articleType, category: categoryType } });
    const pair = mutualRelationPair({ model: 'category' }, 'category', owner('article'), ctx);
    assert.deepEqual([pair.mine, pair.theirs].sort(), ['article.category', 'category.articles']);

    // A one-way relation has no pair to choose between.
    assert.equal(mutualRelationPair({ model: 'user' }, 'author', owner('article'), ctx), null);
  });

  it('keeps the single reference by default, and the operator can say otherwise', () => {
    const automatic = makeContext({}, {
      relationsMode: 'relation',
      contentTypes: { article: articleType, category: categoryType },
    });
    // The automatic rule prefers the side that holds the single reference.
    assert.equal(shouldAdoptRelation({ model: 'category' }, 'category', owner('article'), automatic).adopt, true);
    assert.equal(shouldAdoptRelation({ collection: 'article', via: 'category' }, 'articles', owner('category'), automatic).adopt, false);

    // `--relation-owner category.articles` turns that around, and says so about the other side.
    const chosen = makeContext({}, {
      relationsMode: 'relation',
      contentTypes: { article: articleType, category: categoryType },
      relationOwners: ['category.articles'],
    });
    assert.equal(shouldAdoptRelation({ collection: 'article', via: 'category' }, 'articles', owner('category'), chosen).adopt, true);
    const skipped = shouldAdoptRelation({ model: 'category' }, 'category', owner('article'), chosen);
    assert.equal(skipped.adopt, false);
    assert.match(skipped.reason, /--relation-owner/);
  });

  it('lets the operator settle a many-to-many either way', () => {
    const chosen = makeContext({}, {
      relationsMode: 'relation',
      contentTypes: { article: articleType, tag: tagType },
      relationOwners: ['tag.articles'],
    });
    // Without the flag the article side wins ("article.tags" sorts first).
    assert.equal(shouldAdoptRelation({ collection: 'article', via: 'tags' }, 'articles', owner('tag'), chosen).adopt, true);
    assert.equal(shouldAdoptRelation({ collection: 'tag', via: 'articles' }, 'tags', owner('article'), chosen).adopt, false);
  });

  it('reports a choice that names nothing, or both sides of one relation', () => {
    const ctx = makeContext({}, { contentTypes: { article: articleType, category: categoryType } });
    const selected = [articleType, categoryType];

    assert.deepEqual(relationOwnerProblems(selected, new Set(['category.articles']), ctx), []);
    assert.deepEqual(relationOwnerProblems(selected, new Set(), ctx), []);

    const typo = relationOwnerProblems(selected, new Set(['category.artcles']), ctx);
    assert.equal(typo.length, 1);
    assert.match(typo[0], /not a relation field/);

    const both = relationOwnerProblems(selected, new Set(['article.category', 'category.articles']), ctx);
    assert.equal(both.length, 1);
    assert.match(both[0], /both sides of one relation/);
  });

  it('writes the chosen side as the Relation field', () => {
    const ctx = makeContext({}, {
      relationsMode: 'relation',
      contentTypes: { article: articleType, category: categoryType },
      relationOwners: ['category.articles'],
    });
    const plan = planContentType({ articles: { collection: 'article', via: 'category' } }, ctx, owner('category'));
    assert.deepEqual(plan.schema[0].field_type, {
      Relation: { target: { kind: 'collection', name: 'articles' }, has_many: true, inverse_name: 'category' },
    });
  });

  it('lists every relation, the side it keeps by default, and what is dropped', () => {
    const ctx = makeContext({}, {
      relationsMode: 'relation',
      contentTypes: { article: articleType, category: categoryType, tag: tagType, home: homeType },
    });
    const report = relationReport([articleType, categoryType, tagType], [], ctx);

    assert.deepEqual(
      report.mutual.map((row) => `${row.mine}|${row.theirs}|${row.owner}`),
      ['article.category|category.articles|article.category', 'article.tags|tag.articles|article.tags'],
    );
    // The first pair is the one-to-many: the single side is kept, and the flag names the other.
    assert.equal(report.mutual[0].mineMany, false);
    assert.equal(report.mutual[0].theirsMany, true);
    assert.equal(report.mutual[0].other, 'category.articles');
    // Both sides of a many-to-many hold a set.
    assert.equal(report.mutual[1].mineMany, true);
    assert.equal(report.mutual[1].theirsMany, true);

    assert.deepEqual(report.oneWay, []);
    assert.deepEqual(
      report.dropped.map((row) => `${row.field}->${row.target}`),
      ['article.author->user', 'category.section->section'],
    );
  });

  it('lists a relation inside a composite as one-way, and says where it lives', () => {
    const pageBinderType = {
      apiId: 'page-binder',
      name: 'page-binder',
      cmsName: 'page-binder',
      kind: 'collectionType',
      attributes: {},
    };
    const ctx = makeContext({}, { relationsMode: 'relation', contentTypes: { 'page-binder': pageBinderType } });
    const report = relationReport(
      [],
      [{ id: 'links.page-binder-link', attributes: { page_binder: { model: 'page-binder' } } }],
      ctx,
    );
    assert.deepEqual(report.oneWay, [
      {
        field: 'links.page-binder-link.page_binder',
        target: 'page-binder',
        hasMany: false,
        where: 'composite',
      },
    ]);
  });

  it('does not mistake media for a relation', () => {
    const withMedia = {
      apiId: 'thing',
      name: 'thing',
      cmsName: 'thing',
      kind: 'collectionType',
      attributes: { cover: { model: 'file', via: 'related', plugin: 'upload' } },
    };
    const report = relationReport([withMedia], [], makeContext({}, { relationsMode: 'relation' }));
    assert.deepEqual([report.mutual, report.oneWay, report.dropped], [[], [], []]);
  });
});

describe('the field that names an item', () => {
  it('prefers a field called title, then name, then the first field that reads as one line', () => {
    assert.equal(
      chooseTitleField([{ name: 'body', field_type: { Markdown: {} } }, { name: 'title', field_type: { Text: {} } }]),
      'title',
    );
    assert.equal(chooseTitleField([{ name: 'label', field_type: { Text: {} } }]), 'label');
    assert.equal(
      chooseTitleField([{ name: 'cover', field_type: 'Image' }, { name: 'summary', field_type: { Text: {} } }]),
      'summary',
    );
    // An image cannot be a name, and a schema of only those has none.
    assert.equal(chooseTitleField([{ name: 'cover', field_type: 'Image' }]), null);
  });
});

describe('dates', () => {
  it('reads the timestamps v3 writes', () => {
    assert.deepEqual(
      entryDates({
        created_at: '2024-01-01T00:00:00.000Z',
        updated_at: '2024-01-02T00:00:00.000Z',
        published_at: '2024-01-03T00:00:00.000Z',
      }),
      {
        // Already RFC 3339, so it travels unchanged; only a missing offset is repaired.
        created_at: '2024-01-01T00:00:00.000Z',
        updated_at: '2024-01-02T00:00:00.000Z',
        published_at: '2024-01-03T00:00:00.000Z',
      },
    );
    assert.equal(entryDates({ created_at: '2024-01-01T00:00:00' }).created_at, '2024-01-01T00:00:00Z');
    // A MongoDB-backed project answers camelCase.
    assert.deepEqual(entryDates({ createdAt: '2024-01-01T00:00:00Z' }, { published: false }), {
      created_at: '2024-01-01T00:00:00Z',
    });
  });

  it('gives a draft no publication date, and never an update before the creation', () => {
    assert.equal(entryDates({ published_at: '2024-01-01T00:00:00Z' }, { published: false }).published_at, undefined);
    // The CMS refuses `updated_at < created_at`; the creation date is written instead.
    assert.equal(
      entryDates({ created_at: '2024-02-01T00:00:00Z', updated_at: '2024-01-01T00:00:00Z' }).updated_at,
      '2024-02-01T00:00:00Z',
    );
  });
});

describe('publish order', () => {
  it('publishes a referenced item before the item that requires it', () => {
    const dependencies = new Map([['collection:categories:1', new Set(['collection:sections:1'])]]);
    const { order, cyclic } = orderForPublish(
      ['collection:categories:1', 'collection:sections:1'],
      dependencies,
    );
    assert.deepEqual(order, ['collection:sections:1', 'collection:categories:1']);
    assert.deepEqual(cyclic, []);
  });

  it('ignores a dependency that is not in this run', () => {
    const dependencies = new Map([['collection:articles:1', new Set(['page:home'])]]);
    // `page:home` will not be published, so it cannot order anything; the publish attempt is
    // where the server gets to refuse.
    assert.deepEqual(orderForPublish(['collection:articles:1'], dependencies).order, ['collection:articles:1']);
  });

  it('reports items that require each other, which no order can publish', () => {
    const dependencies = new Map([
      ['collection:a:1', new Set(['collection:b:1'])],
      ['collection:b:1', new Set(['collection:a:1'])],
    ]);
    const { order, cyclic } = orderForPublish(['collection:a:1', 'collection:b:1'], dependencies);
    assert.deepEqual(order, []);
    assert.deepEqual(cyclic, ['collection:a:1', 'collection:b:1']);
  });
});
