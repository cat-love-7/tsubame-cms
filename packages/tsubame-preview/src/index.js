'use strict';

/**
 * `tsubame-preview`: a signed working copy, rendered without a build.
 *
 * A deployment mints a preview link (`docs/content-api.md` §5.6) and an admin screen copies it. What
 * the API serves at that link is JSON, so a link worth handing to a client or a translator is one
 * the *preview site* opens. This package is everything such a site needs, and nothing about the
 * framework it is written in:
 *
 * 1. Read the target out of its own address: `parsePreviewRoute(location.pathname)`.
 * 2. Fetch the working copy with the token from the query string: `fetchPreview(...)`.
 * 3. Resolve it against the schema, fetching only *published* relations and composite definitions:
 *    `resolvePreview(...)`, with a `renderMarkdown` the site supplies so the preview renders the way
 *    the build does.
 * 4. Draw it with the site's own components.
 *
 * An admin client that has to build the link in the first place uses `previewSiteUrl` (or
 * `previewApiPath` / `previewRoutePath` for the two halves separately), which is the same rule the
 * Angular screen applies in `frontend/src/app/shared/share-link.ts`.
 *
 * `docs/preview-site.md` is the contract, including the shape `resolvePreview` answers and the fields
 * a preview cannot fill in (a build's downloaded images, excerpts, inverse relations).
 */

const { describeFieldType, normalizePrefix, createUrlResolver, toImageValue } = require('./field-types');
const {
  checkTarget,
  previewRoutePath,
  previewApiPath,
  parsePreviewRoute,
  previewSiteUrl,
} = require('./routes');
const { joinUrl, fetchPreview, createContentClient } = require('./client');
const { resolvePreview } = require('./resolve');

module.exports = {
  // Field types and values.
  describeFieldType,
  normalizePrefix,
  createUrlResolver,
  toImageValue,
  resolvePreview,
  // Where a preview lives.
  checkTarget,
  previewRoutePath,
  previewApiPath,
  parsePreviewRoute,
  previewSiteUrl,
  // Talking to the CMS.
  joinUrl,
  fetchPreview,
  createContentClient,
};
