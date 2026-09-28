'use strict';

const { normalizePrefix } = require('./field-types');

/**
 * Where a preview lives, as a path.
 *
 * The API serves a signed working copy at `/api/preview/collections/{c}/items/{id}` and
 * `/api/preview/single_pages/{p}` (`backend/crates/core/src/preview_link.rs`). The preview site
 * serves the same routes minus the API prefix, so a link copies across with nothing to translate
 * but that one segment: `/preview/collections/{c}/items/{id}`.
 *
 * The spelling is the API's, not a choice made here: `single_pages` has the underscore the API's
 * preview routes use, while the *delivery* API spells the same thing `single-pages`
 * (`docs/content-api.md`). Two spellings for one idea is a bug waiting to be written, so both live
 * in this file and nowhere else.
 */

/**
 * A target this package understands, once `checkTarget` has passed it: a collection item, or a
 * single page. The `kind` is what tells the two apart.
 *
 * @typedef {{ kind: 'collection', collection: string, id: number }} CollectionTarget
 * @typedef {{ kind: 'single_page', page: string }} SinglePageTarget
 * @typedef {CollectionTarget | SinglePageTarget} PreviewTarget
 */

/**
 * A target as it arrives from an untyped caller - a query string, a stored link - before
 * `checkTarget` has judged it. Every property is `unknown` because anything may be there, and the
 * checks in `checkTarget` are what narrow it.
 *
 * @typedef {object} TargetLike
 * @property {unknown} [kind]
 * @property {unknown} [collection]
 * @property {unknown} [id]
 * @property {unknown} [page]
 */

const COLLECTION_ROUTE = /(?:^|\/)preview\/collections\/([^/]+)\/items\/(\d+)\/?$/;
const SINGLE_PAGE_ROUTE = /(?:^|\/)preview\/single_pages\/([^/]+)\/?$/;

/**
 * A preview target, or a message saying what is wrong with the one given.
 *
 * @param {TargetLike | null | undefined} target
 * @returns {string | null}
 */
function checkTarget(target) {
  if (target === null || typeof target !== 'object') {
    return 'a preview target is an object';
  }
  if (target.kind === 'collection') {
    if (typeof target.collection !== 'string' || target.collection === '') {
      return 'a collection target needs a collection name';
    }
    if (!Number.isInteger(target.id) || /** @type {number} */ (target.id) < 1) {
      return 'a collection target needs an item id';
    }
    return null;
  }
  if (target.kind === 'single_page') {
    if (typeof target.page !== 'string' || target.page === '') {
      return 'a single page target needs a page name';
    }
    return null;
  }
  return 'a preview target is a collection item or a single page';
}

/**
 * The route the preview site serves, without the API prefix.
 *
 * An empty string for a target this package does not understand, rather than a throw: this is
 * called while building a link, and a link that cannot be built should show up as one rather than
 * as a screen that failed to draw.
 *
 * @param {TargetLike | null | undefined} target
 * @returns {string}
 */
function previewRoutePath(target) {
  if (checkTarget(target) !== null) {
    return '';
  }
  // `checkTarget` passing is exactly what makes the target one of the two shapes; a cast here says
  // so once rather than at each property.
  const valid = /** @type {PreviewTarget} */ (target);
  if (valid.kind === 'collection') {
    return `/preview/collections/${encodeURIComponent(valid.collection)}/items/${valid.id}`;
  }
  return `/preview/single_pages/${encodeURIComponent(valid.page)}`;
}

/**
 * The API route that serves the working copy, prefix included - what a preview link carries.
 *
 * @param {TargetLike | null | undefined} target
 * @param {unknown} [apiPrefix]
 * @returns {string}
 */
function previewApiPath(target, apiPrefix = '/api') {
  const route = previewRoutePath(target);
  return route === '' ? '' : `${normalizePrefix(apiPrefix)}${route}`;
}

/**
 * The target a preview site's own address names, or null when it names something else.
 *
 * A preview site reads its own URL: the whole target travels in the path, and the token in the
 * query string is the credential the rest of the work needs. The leading part of the path is
 * deliberately not required to be exactly `/preview/...`: a site may be mounted under its own base
 * path, and where it is mounted is a deployment's business rather than this parser's.
 *
 * @param {unknown} pathname
 * @returns {PreviewTarget | null}
 */
function parsePreviewRoute(pathname) {
  if (typeof pathname !== 'string') {
    return null;
  }
  const path = pathname.split('?')[0].split('#')[0];

  const collection = COLLECTION_ROUTE.exec(path);
  if (collection !== null) {
    return {
      kind: 'collection',
      collection: safeDecode(collection[1]),
      id: Number(collection[2]),
    };
  }

  const page = SINGLE_PAGE_ROUTE.exec(path);
  if (page !== null) {
    return { kind: 'single_page', page: safeDecode(page[1]) };
  }

  return null;
}

/**
 * A preview link as the address a reviewer opens.
 *
 * The token travels through untouched, because it is the whole credential and the preview site
 * hands it straight back to the API. `siteOrigin` is an origin - scheme, host and port - which is
 * what a deployment reports through `preview_site_url`
 * (`tsubame_core::config::parse_preview_site_url`), so nothing here has to decide whether a base
 * path and a route meet with one slash or two.
 *
 * The same rule the admin screen applies in TypeScript
 * (`frontend/src/app/shared/share-link.ts`): an admin client in any other language gets it
 * from here.
 *
 * @param {unknown} apiPath
 * @param {unknown} siteOrigin
 * @param {unknown} [apiPrefix]
 * @returns {string}
 */
function previewSiteUrl(apiPath, siteOrigin, apiPrefix = '/api') {
  const origin = String(siteOrigin ?? '').replace(/\/+$/, '');
  const prefix = normalizePrefix(apiPrefix);
  const path = String(apiPath ?? '');
  const route = path.startsWith(prefix) ? path.slice(prefix.length) : path;
  return `${origin}${route.startsWith('/') ? route : `/${route}`}`;
}

/**
 * Decode a path segment, keeping the encoded spelling when it is not valid percent-encoding rather
 * than throwing while a page is being read.
 *
 * @param {string} value
 * @returns {string}
 */
function safeDecode(value) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

module.exports = {
  checkTarget,
  previewRoutePath,
  previewApiPath,
  parsePreviewRoute,
  previewSiteUrl,
};
