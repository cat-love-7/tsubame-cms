'use strict';

/**
 * Reading the CMS's field types, and the value shapes they imply.
 *
 * A value on the wire carries no type tag: the CMS sends `"..."` for both `Text` and `Markdown`,
 * `{"id":3,"url":"..."}` for an `Image`, and `{"target":"authors","item":7}` for a `Relation`. A
 * relation holds one reference; several are an `Array` whose item types are all relations
 * (`[{"target":"authors","item":7}]`), so the schema and not the value's shape says which is which
 * and every decision here takes the field's `field_type`.
 *
 * The wire shapes are the contract in `docs/content-api.md` (sections 3 and 3.1) and the Rust
 * `FieldValueResponse` in `backend/crates/core/src/models/values.rs`.
 *
 * This is the same reading the build-time source plugin does
 * (`packages/gatsby-source-tsubame/src/fields.js`), kept separately because a preview runs where no
 * Gatsby does - and because the two disagree about what a resolved value *is*: a build points a
 * relation at a node, a preview has to hold the content itself.
 */

/**
 * A field type as the API spells it on the wire.
 *
 * The CMS serialises an enum variant either as a bare string (`"Number"`, `"Image"`) or as a
 * single-key object holding its options (`{"Text":{}}`, `{"Array":[{"Image"}]}`), so both spellings
 * have to be understood by the same reader. What a key's options hold is the API's business and
 * differs per variant, so they stay free-form JSON here and each reader narrows the ones it knows.
 *
 * @typedef {string | Record<string, any>} WireFieldType
 */

/**
 * A field type read into its kind and options. `options` is only meaningful for the kinds that carry
 * them - an `Array` holds its item types, a `CompositeField` its id - and is undefined for the rest.
 *
 * @typedef {object} FieldTypeDescriptor
 * @property {string} kind
 * @property {unknown} [options]
 */

/**
 * One field of a schema: the name its value is keyed by, and the type that says how to read it.
 *
 * @typedef {object} SchemaField
 * @property {string} name
 * @property {WireFieldType} [field_type]
 */

/**
 * The values of one document, keyed by field name. A value carries no type tag, so it stays free-form
 * JSON until the neighbouring schema says how to read it.
 *
 * @typedef {Record<string, any>} WireValues
 */

/**
 * One document as the preview and delivery APIs answer it: the schema, and the values it describes.
 *
 * @typedef {object} DeliveryPayload
 * @property {SchemaField[]} schema
 * @property {WireValues} values
 */

/**
 * A relation as the wire carries it: one `{target, item}` reference. `item` is absent for a single
 * page, whose identity is its `target` name (`docs/content-api.md` §3.1).
 *
 * @typedef {object} RelationReference
 * @property {string} target
 * @property {number} [item]
 */

/**
 * An image as the wire carries it: a bare id, or the `{id, url}` object the API sends.
 *
 * @typedef {number | { id?: number, url?: string }} ImageWire
 */

/**
 * An image as a page wants it: the address to render, plus the id link that survives a replacement.
 * `absoluteUrl` and `stableUrl` are null when there is no usable path or id.
 *
 * @typedef {object} ImageValue
 * @property {number | null} id
 * @property {string} url
 * @property {string | null} absoluteUrl
 * @property {string | null} stableUrl
 */

/**
 * The address builders a resolved value needs: one for a path the API returned, one for a path this
 * package built. A site may inject its own instead of using `createUrlResolver`'s.
 *
 * @typedef {object} UrlResolvers
 * @property {(url: unknown) => string | null} resolveUrl
 * @property {(path: unknown) => string} resolveApiPath
 */

/**
 * `createUrlResolver`'s answer: the two builders, plus the normalized root and prefix they were made
 * from, which a page rendering an absolute address also needs.
 *
 * @typedef {UrlResolvers & { apiUrl: string, apiPrefix: string }} UrlResolver
 */

/**
 * A published target a preview asks for. The name is a collection name or a single-page name; `item`
 * is null for a single page, whose identity is the name alone (`docs/content-api.md` §3.1).
 *
 * @typedef {object} PublishedReference
 * @property {'collection' | 'single_page'} kind
 * @property {string} name
 * @property {number | null} item
 */

/**
 * The kind of a field type, with its options.
 *
 * The CMS serialises an enum variant either as a bare string (`"Number"`, `"Image"`) or as a
 * single-key object holding the options (`{"Text":{}}`, `{"Array":[{"Image"}]}`), so both spellings
 * have to be understood by the same reader.
 *
 * @param {WireFieldType | undefined} fieldType
 * @returns {FieldTypeDescriptor}
 */
function describeFieldType(fieldType) {
  if (typeof fieldType === 'string') {
    return { kind: fieldType, options: undefined };
  }
  if (fieldType === null || typeof fieldType !== 'object') {
    return { kind: 'Unknown', options: undefined };
  }
  for (const key of ['Text', 'Slug', 'Markdown', 'TextEnum', 'CompositeField', 'Relation', 'Array']) {
    if (key in fieldType) {
      return { kind: key, options: fieldType[key] };
    }
  }
  return { kind: 'Unknown', options: undefined };
}

/**
 * Whether a field type is several references: an `Array` whose item types are all `Relation`.
 *
 * That is how the schema says "this field holds many" since `has_many` was replaced by the array
 * (`docs/relations-design.md` §3), and the item types may name different targets. A mixed array is
 * not one: its elements are not all references, and there is no per-element type tag to tell them
 * apart.
 *
 * @param {WireFieldType | undefined} fieldType
 * @returns {boolean}
 */
function isRelationArray(fieldType) {
  const { kind, options } = describeFieldType(fieldType);
  return (
    kind === 'Array' &&
    Array.isArray(options) &&
    options.length > 0 &&
    options.every((item) => describeFieldType(item).kind === 'Relation')
  );
}

/**
 * A prefix the way every path is built with it: one leading slash, no trailing one.
 *
 * @param {unknown} apiPrefix
 * @returns {string}
 */
function normalizePrefix(apiPrefix) {
  const raw = typeof apiPrefix === 'string' && apiPrefix !== '' ? apiPrefix : '/api';
  const withSlash = raw.startsWith('/') ? raw : `/${raw}`;
  return withSlash.replace(/\/+$/, '');
}

/**
 * Whether a URL is already complete and must not be prefixed again.
 *
 * @param {string} url
 * @returns {boolean}
 */
function isAbsoluteUrl(url) {
  return /^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith('//');
}

/**
 * Turning the API's paths into addresses a browser can open.
 *
 * The API answers image values with a path (`/api/images/<file>`), which is not openable from a
 * page served by another origin, so `absoluteUrl` is what a page renders. `apiUrl` is the CMS's
 * root; an empty one means "this origin", which is what a preview site served beside the API uses.
 *
 * Two ways of joining, because prefixing `/api/images/...` twice is the obvious way to get this
 * wrong: `resolveUrl` takes a path the API returned (it may already carry the prefix) and
 * `resolveApiPath` takes a path this package built (it never does).
 *
 * @param {{ apiUrl?: string, apiPrefix?: string }} [options]
 * @returns {UrlResolver}
 */
function createUrlResolver({ apiUrl = '', apiPrefix = '/api' } = {}) {
  const base = String(apiUrl).replace(/\/+$/, '');
  const prefix = normalizePrefix(apiPrefix);

  /** @type {UrlResolvers['resolveUrl']} */
  const resolveUrl = (url) => {
    if (typeof url !== 'string' || url === '') {
      return null;
    }
    if (isAbsoluteUrl(url)) {
      return url;
    }
    const path = url.startsWith(prefix)
      ? url
      : `${prefix}${url.startsWith('/') ? url : `/${url}`}`;
    return `${base}${path}`;
  };

  /** @type {UrlResolvers['resolveApiPath']} */
  const resolveApiPath = (path) => {
    const rest = typeof path === 'string' ? path : '';
    return `${base}${prefix}${rest.startsWith('/') ? rest : `/${rest}`}`;
  };

  return { resolveUrl, resolveApiPath, apiUrl: base, apiPrefix: prefix };
}

/**
 * An image value as a page wants it.
 *
 * No download and no `localFile`: a preview is not a build, so `gatsby-transformer-sharp` and
 * friends have nothing to point at. What a page gets is the address it can put in an `<img>`, plus
 * the id link that survives a replacement (`/api/images/by-id/{id}` - the file name changes when an
 * image is replaced, the id does not).
 *
 * @param {ImageWire | null | undefined} value
 * @param {UrlResolvers} resolve
 * @returns {ImageValue | null}
 */
function toImageValue(value, resolve) {
  if (value === null || value === undefined) {
    return null;
  }
  const id = typeof value === 'number' ? value : value.id;
  const url = typeof value === 'object' && value !== null && typeof value.url === 'string' ? value.url : '';
  const imageId = typeof id === 'number' ? id : null;

  return {
    id: imageId,
    url,
    absoluteUrl: resolve.resolveUrl(url),
    stableUrl: imageId === null ? null : resolve.resolveApiPath(`/images/by-id/${imageId}`),
  };
}

module.exports = {
  describeFieldType,
  isRelationArray,
  normalizePrefix,
  createUrlResolver,
  toImageValue,
};
