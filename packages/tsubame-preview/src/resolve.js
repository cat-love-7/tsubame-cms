'use strict';

const { createUrlResolver, describeFieldType, isRelationArray, toImageValue } = require('./field-types');

/**
 * A working copy, shaped for a page rather than for a build.
 *
 * The preview link answers `{schema, values}`, and the values carry no type tags: the schema is what
 * says that a string is Markdown, that an object is an image, and that a `{target, item}` object (or
 * an array of them) is a relation. Resolving means walking the schema and the value together -
 * through composites and arrays, which can nest without limit - and answering a plain object a
 * template can read.
 *
 * Two things are different from a build, and both are deliberate:
 *
 * - **A relation is resolved by fetching the published target**, not by pointing at a node. Preview
 *   has no node store, so the content itself is put in the field. A target that is not published is
 *   `null` for one reference and `null` in its place in an array - the same thing a build shows,
 *   where the relation points at a node that does not exist. Following relations costs a request
 *   each, so `relationDepth` bounds it: the default is one hop, which is what "show the author"
 *   needs and what keeps a mutual reference from becoming an infinite walk.
 * - **Markdown is rendered where it is read**, because there is no build to make `gatsby-
 *   transformer-remark`'s node. The renderer is injected (`renderMarkdown`) so a site can use the
 *   same pipeline its build uses rather than a second one that only nearly agrees; without one the
 *   text is kept raw and `html` is null.
 *
 * The result is framework-neutral on purpose: plain objects, strings and arrays, snake_case names as
 * the CMS spells them. A Gatsby site renames them through its adapter
 * (`gatsby-source-tsubame/src/preview-adapter.js`); a Next.js or plain SPA site reads them as they
 * are. See `docs/preview-site.md` for the shape and where it differs from a build.
 */

/**
 * A problem `onProblem` reports: where it happened, and what went wrong. A preview carries on with
 * that part unresolved, so this is a notification rather than an error.
 *
 * @typedef {object} PreviewProblem
 * @property {string} path
 * @property {string} message
 */

/**
 * What a preview site hands `resolvePreview`. The schema and the values are the working copy itself;
 * the rest is the site's own wiring, and each has a safe default: no relation loader leaves
 * relations null, no renderer leaves Markdown raw, and no `onProblem` keeps a partial failure quiet
 * rather than fatal.
 *
 * @typedef {object} PreviewOptions
 * @property {string} [apiUrl]
 * @property {string} [apiPrefix]
 * @property {import('./field-types.js').SchemaField[]} [schema]
 * @property {import('./field-types.js').WireValues} [values]
 * @property {number} [relationDepth]
 * @property {(reference: import('./field-types.js').PublishedReference) => Promise<import('./field-types.js').DeliveryPayload | null>} [loadPublished]
 * @property {(id: string) => Promise<unknown>} [loadCompositeSchema]
 * @property {(raw: string) => unknown} [renderMarkdown]
 * @property {(problem: PreviewProblem) => void} [onProblem]
 * @property {(url: unknown) => string | null} [resolveUrl]
 * @property {(path: unknown) => string} [resolveApiPath]
 */

/**
 * The state one resolution walks with: where published content comes from, how Markdown is rendered,
 * how addresses are built, and the composite definitions already read - a page of blocks would
 * otherwise ask for the same definition once per block.
 *
 * @typedef {object} ResolveContext
 * @property {(reference: import('./field-types.js').PublishedReference) => Promise<import('./field-types.js').DeliveryPayload | null>} loadPublished
 * @property {(id: string) => Promise<unknown>} loadCompositeSchema
 * @property {((raw: string) => unknown) | undefined} renderMarkdown
 * @property {(problem: PreviewProblem) => void} onProblem
 * @property {import('./field-types.js').UrlResolvers} url
 * @property {Map<string, import('./field-types.js').SchemaField[] | null>} definitions
 */

/**
 * A composite value after resolution: the composite's own `id`, the raw `values` the API sent, and
 * one property per field the definition declares, under the CMS spelling.
 *
 * @typedef {Record<string, unknown> & { id: string, values: import('./field-types.js').WireValues }} ResolvedComposite
 */

/**
 * Something a `catch` bound. Anything can be thrown, so only `message` is assumed, and only when it
 * is there.
 *
 * @typedef {{ message?: unknown }} CaughtError
 */

/**
 * Resolve one working copy for a page.
 *
 * @param {PreviewOptions} [options]
 * @returns {Promise<Record<string, unknown>>}
 */
async function resolvePreview(options = {}) {
  const defaults = createUrlResolver({ apiUrl: options.apiUrl, apiPrefix: options.apiPrefix });
  const depth = Number.isInteger(options.relationDepth) ? /** @type {number} */ (options.relationDepth) : 1;

  /** @type {ResolveContext} */
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

/**
 * Resolve a whole document: every field the schema declares, read out of the values by the type the
 * schema gives it. A field the schema does not declare is not in the answer at all.
 *
 * @param {import('./field-types.js').SchemaField[] | undefined} schema
 * @param {import('./field-types.js').WireValues | undefined} values
 * @param {ResolveContext} context
 * @param {number} relationDepth
 * @returns {Promise<Record<string, unknown>>}
 */
async function resolveSchema(schema, values, context, relationDepth) {
  /** @type {import('./field-types.js').WireValues} */
  const source = values !== null && typeof values === 'object' && !Array.isArray(values) ? values : {};
  /** @type {Record<string, unknown>} */
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

/**
 * One value, read by the field type that describes it.
 *
 * `value` is the free-form JSON the API sent: it carries no tag of its own, which is exactly why the
 * field type has to be walked beside it. An unknown kind is answered as the JSON it arrived as,
 * rather than guessed at.
 *
 * @param {import('./field-types.js').WireFieldType | undefined} fieldTypeDescriptor
 * @param {any} value
 * @param {ResolveContext} context
 * @param {number} relationDepth
 * @param {string} path
 * @returns {Promise<unknown>}
 */
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
      // Several references are an `Array` whose item types are all relations. Every element carries
      // its own target, so each is resolved on its own; an unpublished one keeps its place as null.
      if (isRelationArray(fieldTypeDescriptor)) {
        return resolveRelationArray(value, context, relationDepth);
      }
      // A list whose element type is mixed or unknown stays the JSON the API sent: without a single
      // element schema there is no way to tell one entry's type from another's.
      if (!Array.isArray(options) || options.length !== 1) {
        return value;
      }
      /** @type {unknown[]} */
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
      return resolveRelation(value, context, relationDepth);
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
 *
 * @param {any} value
 * @param {ResolveContext} context
 * @param {number} relationDepth
 * @returns {Promise<ResolvedComposite | null>}
 */
async function resolveComposite(value, context, relationDepth) {
  if (value === null || value === undefined || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const id = typeof value.id === 'string' ? value.id : '';
  /** @type {import('./field-types.js').WireValues} */
  const source =
    value.values !== null && typeof value.values === 'object' && !Array.isArray(value.values)
      ? value.values
      : {};
  /** @type {ResolvedComposite} */
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

/**
 * A definition, read once per id: a page of blocks would otherwise ask for the same one per block.
 *
 * @param {string} id
 * @param {ResolveContext} context
 * @returns {Promise<import('./field-types.js').SchemaField[] | null>}
 */
async function loadDefinition(id, context) {
  if (id === '') {
    return null;
  }
  if (context.definitions.has(id)) {
    // `has` just guaranteed an entry, so the answer cannot be `undefined`; only a definition that
    // turned out not to be a schema array is stored as null.
    return /** @type {import('./field-types.js').SchemaField[] | null} */ (context.definitions.get(id));
  }
  /** @type {import('./field-types.js').SchemaField[] | null} */
  let definition = null;
  try {
    const answer = await context.loadCompositeSchema(id);
    definition = Array.isArray(answer) ? answer : null;
  } catch (error) {
    // A failed definition read is not a failed preview: the composite keeps its raw values, which is
    // what a deployment without `/content/composite-fields` gets too (an older CMS answers 404).
    context.onProblem({ path: id, message: `composite definition ${id} could not be read: ${/** @type {CaughtError} */ (error)?.message ?? error}` });
    return null;
  }
  context.definitions.set(id, definition);
  return definition;
}

/**
 * A relation value: one `{target, item}` reference, or `null` when nothing is referenced.
 *
 * A reference with an `item` is a collection item; one without is a single page, whose identity is
 * its name (`docs/content-api.md` §3.1). The schema's declared target is not consulted: the element
 * itself says what it points at, and one array may name several targets.
 *
 * @param {any} value
 * @param {ResolveContext} context
 * @param {number} relationDepth
 * @returns {Promise<Record<string, unknown> | null>}
 */
async function resolveRelation(value, context, relationDepth) {
  if (
    value === null ||
    value === undefined ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    typeof value.target !== 'string'
  ) {
    return null;
  }
  return resolveReference(value, context, relationDepth);
}

/**
 * An `Array` of relations, each resolved in place.
 *
 * The elements may name different targets (the schema declares one item type per target), so each is
 * read on its own. A reference that cannot be resolved stays `null` rather than being dropped: the
 * order is the value, and a page that skips a missing entry would show the wrong one.
 *
 * @param {unknown[]} value
 * @param {ResolveContext} context
 * @param {number} relationDepth
 * @returns {Promise<unknown[]>}
 */
async function resolveRelationArray(value, context, relationDepth) {
  /** @type {unknown[]} */
  const resolved = [];
  for (const reference of value) {
    resolved.push(await resolveRelation(reference, context, relationDepth));
  }
  return resolved;
}

/**
 * One reference, as the published content it names - or null.
 *
 * A reference with an `item` is a collection item; one without is a single page, whose identity is
 * its name (`docs/content-api.md` §3.1). The budget is spent here rather than on the whole field,
 * because this is the only step that can reach back into the document that points here.
 *
 * @param {import('./field-types.js').RelationReference} reference
 * @param {ResolveContext} context
 * @param {number} relationDepth
 * @returns {Promise<Record<string, unknown> | null>}
 */
async function resolveReference(reference, context, relationDepth) {
  if (relationDepth <= 0) {
    return null;
  }
  const item = typeof reference.item === 'number' ? reference.item : null;
  /** @type {import('./field-types.js').PublishedReference} */
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
      message: `relation target ${reference.target} could not be read: ${/** @type {CaughtError} */ (error)?.message ?? error}`,
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
 *
 * @param {string} raw
 * @param {ResolveContext} context
 * @param {string} path
 * @returns {Promise<string | null>}
 */
async function renderMarkdownSafely(raw, context, path) {
  if (typeof context.renderMarkdown !== 'function') {
    return null;
  }
  try {
    const html = await context.renderMarkdown(raw);
    return typeof html === 'string' ? html : null;
  } catch (error) {
    context.onProblem({ path, message: `markdown could not be rendered: ${/** @type {CaughtError} */ (error)?.message ?? error}` });
    return null;
  }
}

module.exports = { resolvePreview };
