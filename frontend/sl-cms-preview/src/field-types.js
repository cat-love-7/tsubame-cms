'use strict';

/**
 * Reading the CMS's field types, and the value shapes they imply.
 *
 * A value on the wire carries no type tag: the CMS sends `"..."` for both `Text` and `Markdown`,
 * `{"id":3,"url":"..."}` for an `Image`, and `[{"target":"authors","item":7}]` for a `Relation`.
 * The schema returned next to the values is the only thing that says which is which, so every
 * decision here takes the field's `field_type` and never the value's own shape.
 *
 * The wire shapes are the contract in `doc/content-api.md` (sections 3 and 3.1) and the Rust
 * `FieldValueResponse` in `sl_cms/crates/core/src/models/values.rs`.
 *
 * This is the same reading the build-time source plugin does
 * (`frontend/gatsby-source-sl-cms/src/fields.js`), kept separately because a preview runs where no
 * Gatsby does - and because the two disagree about what a resolved value *is*: a build points a
 * relation at a node, a preview has to hold the content itself.
 */

/**
 * The kind of a field type, with its options.
 *
 * The CMS serialises an enum variant either as a bare string (`"Number"`, `"Image"`) or as a
 * single-key object holding the options (`{"Text":{}}`, `{"Array":[{"Image"}]}`), so both spellings
 * have to be understood by the same reader.
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

/** A prefix the way every path is built with it: one leading slash, no trailing one. */
function normalizePrefix(apiPrefix) {
  const raw = typeof apiPrefix === 'string' && apiPrefix !== '' ? apiPrefix : '/api';
  const withSlash = raw.startsWith('/') ? raw : `/${raw}`;
  return withSlash.replace(/\/+$/, '');
}

/** Whether a URL is already complete and must not be prefixed again. */
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
 */
function createUrlResolver({ apiUrl = '', apiPrefix = '/api' } = {}) {
  const base = String(apiUrl).replace(/\/+$/, '');
  const prefix = normalizePrefix(apiPrefix);

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
  normalizePrefix,
  createUrlResolver,
  toImageValue,
};
