'use strict';

const { createUrlResolver, describeFieldType, toImageValue } = require('./field-types');

/**
 * A working copy, shaped for a page rather than for a build.
 *
 * The preview link answers `{schema, values}`, and the values carry no type tags: the schema is what
 * says that a string is Markdown, that an object is an image, and that a list of `{target, item}` is
 * a relation. Resolving means walking the schema and the value together - through composites and
 * arrays, which can nest without limit - and answering a plain object a template can read.
 *
 * Two things are different from a build, and both are deliberate:
 *
 * - **A relation is resolved by fetching the published target**, not by pointing at a node. Preview
 *   has no node store, so the content itself is put in the field. A target that is not published is
 *   `null` in a single field and `null` in its place in a list - the same thing a build shows, where
 *   the relation points at a node that does not exist. Following relations costs a request each, so
 *   `relationDepth` bounds it: the default is one hop, which is what "show the author" needs and
 *   what keeps a mutual reference from becoming an infinite walk.
 * - **Markdown is rendered where it is read**, because there is no build to make `gatsby-
 *   transformer-remark`'s node. The renderer is injected (`renderMarkdown`) so a site can use the
 *   same pipeline its build uses rather than a second one that only nearly agrees; without one the
 *   text is kept raw and `html` is null.
 *
 * The result is framework-neutral on purpose: plain objects, strings and arrays, snake_case names as
 * the CMS spells them. A Gatsby site renames them through its adapter
 * (`gatsby-source-tsubame/src/preview-adapter.js`); a Next.js or plain SPA site reads them as they
 * are. See `doc/preview-site.md` for the shape and where it differs from a build.
 */
async function resolvePreview(options = {}) {
  const defaults = createUrlResolver({ apiUrl: options.apiUrl, apiPrefix: options.apiPrefix });
  const depth = Number.isInteger(options.relationDepth) ? options.relationDepth : 1;

  const context = {
    loadPublished: typeof options.loadPublished === 'function' ? options.loadPublished : async () => null,
    loadCompositeSchema:
      typeof options.loadCompositeSchema === 'function' ? options.loadCompositeSchema : async () => null,
    renderMarkdown: options.renderMarkdown,
    onProblem: typeof options.onProblem === 'function' ? options.onProblem : () => {},
    url: {
      resolveUrl: typeof options.resolveUrl === 'function' ? options.resolveUrl : defaults.resolveUrl,
      resolveApiPath:
        typeof options.resolveApiPath === 'function' ? options.resolveApiPath : defaults.resolveApiPath,
    },
    definitions: new Map(),
  };

  return resolveSchema(options.schema, options.values, context, depth);
}

async function resolveSchema(schema, values, context, relationDepth) {
  const source = values !== null && typeof values === 'object' && !Array.isArray(values) ? values : {};
  const result = {};

  for (const field of Array.isArray(schema) ? schema : []) {
    if (field === null || typeof field !== 'object') {
      continue;
    }
    const name = String(field.name);
    result[name] = await resolveFieldValue(
      field.field_type,
      source[name],
      context,
      relationDepth,
      name,
    );
  }
  return result;
}

async function resolveFieldValue(fieldTypeDescriptor, value, context, relationDepth, path) {
  const { kind, options } = describeFieldType(fieldTypeDescriptor);

  switch (kind) {
    case 'Markdown': {
      // An empty string is a value, not a missing one: a half-written body still has a raw text,
      // and the page can show the empty result rather than having to guard for null.
      if (typeof value !== 'string') {
        return null;
      }
      return { raw: value, html: await renderMarkdownSafely(value, context, path) };
    }
    case 'Array': {
      if (!Array.isArray(value)) {
        return [];
      }
      // A list whose element type is mixed or unknown stays the JSON the API sent: without a single
      // element schema there is no way to tell one entry's type from another's.
      if (!Array.isArray(options) || options.length !== 1) {
        return value;
      }
      const resolved = [];
      for (let index = 0; index < value.length; index += 1) {
        resolved.push(
          await resolveFieldValue(options[0], value[index], context, relationDepth, `${path}.${index}`),
        );
      }
      return resolved;
    }
    case 'CompositeField':
      return resolveComposite(value, context, relationDepth);
    case 'Relation':
      return resolveRelation(options, value, context, relationDepth);
    case 'Image':
      return toImageValue(value, context.url);
    case 'Text':
    case 'Slug':
      return typeof value === 'string' ? value : null;
    case 'Number':
      return typeof value === 'number' ? value : null;
    case 'Boolean':
      return typeof value === 'boolean' ? value : null;
    case 'Date':
    case 'DateTime':
      // The wire value is an ISO string, and it stays one: a preview is a page, and turning it into
      // a Date here would make this package depend on how the consuming site formats dates.
      return typeof value === 'string' ? value : null;
    case 'TextEnum':
      return Array.isArray(value) ? value.filter((entry) => typeof entry === 'string') : [];
    default:
      return value === undefined ? null : value;
  }
}

/**
 * A composite value: the definition's fields resolved beside the raw `values`.
 *
 * `values` stays the untyped object the API sent even when the definition is known. A field the
 * definition no longer declares is then still readable rather than silently dropped, which matters
 * more in a preview than in a build: the point of looking at a draft is to see what is there.
 */
async function resolveComposite(value, context, relationDepth) {
  if (value === null || value === undefined || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const id = typeof value.id === 'string' ? value.id : '';
  const source =
    value.values !== null && typeof value.values === 'object' && !Array.isArray(value.values)
      ? value.values
      : {};
  const result = { id, values: source };

  const definition = await loadDefinition(id, context);
  if (definition === null) {
    return result;
  }

  for (const field of definition) {
    if (field === null || typeof field !== 'object') {
      continue;
    }
    const name = String(field.name);
    result[name] = await resolveFieldValue(
      field.field_type,
      source[name],
      context,
      relationDepth,
      name,
    );
  }
  return result;
}

/** A definition, read once per id: a page of blocks would otherwise ask for the same one per block. */
async function loadDefinition(id, context) {
  if (id === '') {
    return null;
  }
  if (context.definitions.has(id)) {
    return context.definitions.get(id);
  }
  let definition = null;
  try {
    const answer = await context.loadCompositeSchema(id);
    definition = Array.isArray(answer) ? answer : null;
  } catch (error) {
    // A failed definition read is not a failed preview: the composite keeps its raw values, which is
    // what a deployment without `/content/composite-fields` gets too (an older CMS answers 404).
    context.onProblem({ path: id, message: `composite definition ${id} could not be read: ${error?.message ?? error}` });
    return null;
  }
  context.definitions.set(id, definition);
  return definition;
}

async function resolveRelation(options, value, context, relationDepth) {
  const relation = options !== null && typeof options === 'object' ? options : {};
  const target = relation.target;
  const many = Boolean(relation.has_many) && !(target !== null && typeof target === 'object' && target.kind === 'single_page');

  if (!Array.isArray(value)) {
    return many ? [] : null;
  }
  const references = value.filter(
    (reference) =>
      reference !== null && typeof reference === 'object' && typeof reference.target === 'string',
  );

  if (!many) {
    return references.length > 0
      ? resolveReference(references[0], context, relationDepth)
      : null;
  }

  const resolved = [];
  for (const reference of references) {
    resolved.push(await resolveReference(reference, context, relationDepth));
  }
  return resolved;
}

/**
 * One reference, as the published content it names - or null.
 *
 * A reference with an `item` is a collection item; one without is a single page, whose identity is
 * its name (`doc/content-api.md` §3.1). The budget is spent here rather than on the whole field,
 * because this is the only step that can reach back into the document that points here.
 */
async function resolveReference(reference, context, relationDepth) {
  if (relationDepth <= 0) {
    return null;
  }
  const item = typeof reference.item === 'number' ? reference.item : null;
  const descriptor = {
    kind: item === null ? 'single_page' : 'collection',
    name: reference.target,
    item,
  };

  let published = null;
  try {
    published = await context.loadPublished(descriptor);
  } catch (error) {
    context.onProblem({
      path: reference.target,
      message: `relation target ${reference.target} could not be read: ${error?.message ?? error}`,
    });
    return null;
  }
  if (published === null || published === undefined) {
    return null;
  }
  return resolveSchema(published.schema, published.values, context, relationDepth - 1);
}

/**
 * The injected renderer's answer, or null.
 *
 * A renderer that throws must not blank the page: the raw text is still there and still shows what
 * the working copy holds, which is the whole reason someone opened the preview.
 */
async function renderMarkdownSafely(raw, context, path) {
  if (typeof context.renderMarkdown !== 'function') {
    return null;
  }
  try {
    const html = await context.renderMarkdown(raw);
    return typeof html === 'string' ? html : null;
  } catch (error) {
    context.onProblem({ path, message: `markdown could not be rendered: ${error?.message ?? error}` });
    return null;
  }
}

module.exports = { resolvePreview };
