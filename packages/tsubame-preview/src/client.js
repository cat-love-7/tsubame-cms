'use strict';

const { normalizePrefix } = require('./field-types');
const { checkTarget, previewApiPath } = require('./routes');

/**
 * Talking to the CMS from a preview.
 *
 * Two different APIs are involved and they are not interchangeable:
 *
 * - The **preview** API (`/api/preview/...`) answers the unpublished working copy. The signature in
 *   the query string *is* the credential - no account, no Authorization header - which is what makes
 *   a preview link something to hand to a client or a translator
 *   (`docs/content-api.md` §5.6).
 * - The **delivery** API (`/api/content/...`) answers published content only, and needs no
 *   credential at all. A preview uses it for the things a working copy references: a relation that
 *   is already published, and the composite field definitions that say how to read a value tree.
 *
 * Everything here takes a `fetchImpl` so a test (or a runtime with its own fetch) can supply one.
 */

function joinUrl(base, path) {
  const root = String(base ?? '').replace(/\/+$/, '');
  const rest = String(path ?? '');
  return `${root}${rest.startsWith('/') ? rest : `/${rest}`}`;
}

/** A refusal from the CMS, as an Error carrying what a screen needs to say. */
function refusal(response, message) {
  const error = new Error(message);
  error.status = response.status;
  return error;
}

async function readMessage(response) {
  try {
    const body = await response.json();
    return typeof body?.message === 'string' ? body.message : null;
  } catch {
    return null;
  }
}

/**
 * The unpublished working copy a signed link points at.
 *
 * Answers `{schema, values}` - the same shape the delivery API sends, so one reader understands
 * both. `token` is the `expires.signature` value from the link's query string; the API refuses a
 * link that is malformed (401), not ours (401) or out of date (403), and those travel as an Error
 * carrying `status` rather than as an empty preview.
 */
async function fetchPreview({
  apiUrl = '',
  apiPrefix = '/api',
  target,
  token,
  fetchImpl = globalThis.fetch,
} = {}) {
  const problem = checkTarget(target);
  if (problem !== null) {
    throw new Error(problem);
  }
  if (typeof token !== 'string' || token === '') {
    throw new Error('a preview link carries a token, and this one has none');
  }
  if (typeof fetchImpl !== 'function') {
    throw new Error('no fetch: pass one, or run where globalThis.fetch exists');
  }

  const url = `${joinUrl(apiUrl, previewApiPath(target, apiPrefix))}?token=${encodeURIComponent(token)}`;
  const response = await fetchImpl(url, { headers: { accept: 'application/json' } });
  if (!response.ok) {
    const message = await readMessage(response);
    throw refusal(response, message ?? `the preview link was refused (${response.status})`);
  }

  const body = await response.json();
  return {
    schema: Array.isArray(body?.schema) ? body.schema : [],
    values: body?.values ?? {},
  };
}

/**
 * The published content a preview resolves against.
 *
 * `loadPublished` answers null for content that is not published (the delivery API's 404), which is
 * the honest answer to "show me the author of this article" when the author is not on the site yet
 * - the same answer a build gives, where the relation simply points at no node.
 *
 * `loadCompositeSchema` reads the definitions once and remembers them: a page full of blocks would
 * otherwise ask for the same definition once per block.
 */
function createContentClient({
  apiUrl = '',
  apiPrefix = '/api',
  fetchImpl = globalThis.fetch,
} = {}) {
  const prefix = normalizePrefix(apiPrefix);
  let compositeSchemas = null;

  async function get(path) {
    const response = await fetchImpl(joinUrl(apiUrl, path), {
      headers: { accept: 'application/json' },
    });
    if (response.status === 404) {
      return null;
    }
    if (!response.ok) {
      const message = await readMessage(response);
      throw refusal(response, message ?? `the content API refused the request (${response.status})`);
    }
    return response.json();
  }

  async function loadPublished(reference) {
    const name = typeof reference?.name === 'string' ? encodeURIComponent(reference.name) : '';
    if (name === '') {
      return null;
    }
    const path =
      reference.kind === 'single_page'
        ? `${prefix}/content/single-pages/${name}`
        : `${prefix}/content/collections/${name}/items/${reference.item}`;
    const body = await get(path);
    if (body === null) {
      return null;
    }
    return {
      schema: Array.isArray(body?.schema) ? body.schema : [],
      values: body?.values ?? {},
    };
  }

  async function loadCompositeSchema(id) {
    if (compositeSchemas === null) {
      const body = await get(`${prefix}/content/composite-fields`);
      const definitions = body !== null && typeof body === 'object' ? body : {};
      compositeSchemas = new Map(Object.entries(definitions));
    }
    return compositeSchemas.get(id) ?? null;
  }

  return { loadPublished, loadCompositeSchema };
}

module.exports = {
  joinUrl,
  fetchPreview,
  createContentClient,
};
