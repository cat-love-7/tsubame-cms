// The Strapi v3 side: the content API the migration reads entries and media from, and the
// content-type-builder API it can read definitions from when the project directory is not at
// hand.
//
// v3 differs from v4 in ways this client has to remember: the REST prefix is empty by default
// (v4 introduced `/api`), a list answers a bare JSON array (v4 wraps everything in
// `{ data: [ { attributes: ... } ] }`), and paging is `_start` / `_limit` (v4 uses
// `pagination[start]`). The v3 content API has no API tokens at all; it authenticates with the
// Users & Permissions plugin (`POST /auth/local`) or not at all, through the public role.

import { HttpError, fetchWithRetry, requestJson } from './util.mjs';

/** Strapi answers a list as a bare array; a wrapper is tolerated in case of a proxy or a v4. */
function asArray(json) {
  if (Array.isArray(json)) return json;
  if (json && Array.isArray(json.data)) return json.data;
  if (json && Array.isArray(json.results)) return json.results;
  return [];
}

export class StrapiV3Client {
  /**
   * @param {object} options
   * @param {string} options.baseUrl  e.g. `http://localhost:1337`.
   * @param {string} [options.prefix] `config/server.js`'s `prefix`; empty in a stock v3 app.
   * @param {string} [options.jwt]    Content-API JWT from `/auth/local`, when already held.
   */
  constructor({ baseUrl, prefix = '', jwt = null }) {
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    // v3's default prefix is the empty string, so a route is `http://host:1337/articles`.
    this.prefix = prefix === '/' ? '' : prefix.replace(/\/+$/, '');
    this.jwt = jwt;
    this.origin = new URL(this.baseUrl).origin;
  }

  url(path) {
    return `${this.baseUrl}${this.prefix}${path}`;
  }

  /** Sign in through the Users & Permissions plugin, which is how v3 authenticates a client. */
  async login(identifier, password) {
    const { json } = await requestJson(this.url('/auth/local'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ identifier, password }),
    });
    if (!json?.jwt) throw new Error('Strapi /auth/local answered no jwt');
    this.jwt = json.jwt;
    return json.user;
  }

  async #get(path, params = {}) {
    const search = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
      if (value === undefined || value === null || value === '') continue;
      search.set(key, String(value));
    }
    const query = search.toString();
    const { json } = await requestJson(this.url(`${path}${query ? `?${query}` : ''}`), {
      token: this.jwt,
    });
    return json;
  }

  /**
   * One page of a collection type.
   *
   * v3 **auto-populates** relations, media and components (there is no REST `populate`
   * parameter - a bare `?author=1` is a *filter*, not a population switch), so nothing has to be
   * asked for by default. `populate` exists only as an escape hatch for a project that turned
   * `autoPopulate` off and wired population into a custom controller.
   *
   * `publicationState` defaults to Strapi's own `live`, which hides drafts; the migration asks
   * for `preview` so a draft is carried over as a draft rather than silently dropped.
   */
  async listPage(route, { start = 0, limit = 100, locale, populate = [], publicationState } = {}) {
    const json = await this.#get(route, {
      _start: start,
      _limit: limit,
      _sort: 'id:ASC',
      _locale: locale,
      _publicationState: publicationState,
      _populate: populate.length > 0 ? populate.join(',') : undefined,
    });
    return asArray(json);
  }

  /**
   * How many entries a collection type has.
   *
   * v3 has a dedicated `/count` route, and it answers a **bare number** (the admin panel's
   * variant answers `{ "count": n }`; a tolerant read costs nothing). A project that disabled
   * the route answers 404, and `null` says so, which leaves the caller paging until a short page
   * rather than trusting a number.
   */
  async count(route, { locale, publicationState } = {}) {
    try {
      const json = await this.#get(`${route}/count`, {
        _locale: locale,
        _publicationState: publicationState,
      });
      const count = typeof json === 'number' ? json : json?.count;
      return Number.isFinite(count) ? Number(count) : null;
    } catch (error) {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    }
  }

  /** A single type answers one object. */
  async getSingle(route, { locale, populate = [], publicationState } = {}) {
    const json = await this.#get(route, {
      _locale: locale,
      _publicationState: publicationState,
      _populate: populate.length > 0 ? populate.join(',') : undefined,
    });
    // v4-style wrapping is unwrapped too, for a project sitting behind a v4-shaped proxy.
    return json?.data && !Array.isArray(json.data) ? json.data : json;
  }

  /** Every media file in the library, oldest first so the ids are stable across runs. */
  async listUploadFiles({ pageSize = 100, max = Number.POSITIVE_INFINITY } = {}) {
    const files = [];
    for (let start = 0; files.length < max; start += pageSize) {
      const json = await this.#get('/upload/files', {
        _start: start,
        _limit: pageSize,
        _sort: 'id:ASC',
      });
      const page = asArray(json);
      files.push(...page);
      if (page.length < pageSize) break;
    }
    return files.slice(0, max);
  }

  /** The bytes of one media file, following `url` (relative on a local provider). */
  async downloadUploadFile(file) {
    const source = file.url ?? file.formats?.large?.url ?? null;
    if (!source) throw new Error(`media ${file.id} has no url`);
    return this.downloadMedia(source);
  }

  /**
   * The bytes behind one media URL: the original, or a format Strapi generated for it.
   *
   * `url` is relative on the local provider and absolute on a remote one, so it is resolved
   * against the Strapi base either way.
   */
  async downloadMedia(url) {
    const target = new URL(url, `${this.baseUrl}/`);
    const { buffer, headers } = await fetchWithRetry(target.href, { timeoutMs: 120_000 });
    return { buffer, contentType: headers.get('content-type') ?? null };
  }

  // === Definitions from the admin content-type-builder ======================================
  //
  // The project directory is the better source (it is what the running app was built from), but
  // a migration run by someone without the checkout can read the same definitions from the
  // admin API. These need an *admin* token, which is not the content API's JWT.

  /** Sign in to the admin panel, for the content-type-builder routes. */
  async adminLogin(email, password) {
    const { json } = await requestJson(this.url('/admin/login'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email, password }),
    });
    if (!json?.data?.token) throw new Error('Strapi /admin/login answered no token');
    this.adminToken = json.data.token;
    return json.data.user;
  }

  set adminToken(token) {
    this._adminToken = token;
  }

  async fetchContentTypesFromBuilder() {
    const { json } = await requestJson(this.url('/content-type-builder/content-types'), {
      token: this._adminToken,
    });
    return asArray(json);
  }

  async fetchComponentsFromBuilder() {
    const { json } = await requestJson(this.url('/content-type-builder/components'), {
      token: this._adminToken,
    });
    return asArray(json);
  }
}
