// The target CMS client: exactly the contract `docs/content-api.md` describes.
//
// Every call lives under `API_PREFIX` ("/api"), takes a bearer token except the byte-serving
// `GET /images/{file_name}` (which a browser cannot authenticate), and expects the mutations to
// answer `200` with an empty body. Values travel **untagged** - the schema is the only type
// information - so this client hands `createItem` / `updateSinglePageItem` plain JSON objects.

import { HttpError, fetchWithRetry, requestJson, requestVoid } from './util.mjs';

export class CmsClient {
  /**
   * @param {object} options
   * @param {string} options.baseUrl  Origin of the deployment, e.g. `http://127.0.0.1:8080`.
   * @param {string} [options.prefix] API prefix; `/api` unless the deployment says otherwise.
   * @param {string} [options.token]  Bearer token, when one is already held.
   */
  constructor({ baseUrl, prefix = '/api', token = null }) {
    this.baseUrl = baseUrl.replace(/\/+$/, '');
    this.prefix = prefix.replace(/\/+$/, '');
    this.token = token;
    this.origin = new URL(this.baseUrl).origin;
  }

  url(path) {
    return `${this.baseUrl}${this.prefix}${path}`;
  }

  /**
   * Sign in with a username and password.
   *
   * This is the on-premises deployment's route. A Cognito deployment answers `501` here by
   * design (see `http::password_auth::unavailable_public`), which is why `--cms-token` exists.
   */
  async login(username, password) {
    const { json } = await requestJson(this.url('/auth/login'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ username, password }),
    });
    if (!json?.token) throw new Error('login answered no token');
    this.token = json.token;
    return json.user;
  }

  /** Read a schema, or `null` when the resource does not exist yet. */
  async #getSchema(path) {
    try {
      const { json } = await requestJson(this.url(path), { token: this.token });
      return json;
    } catch (error) {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    }
  }

  async getCollectionNames() {
    const { json } = await requestJson(this.url('/models/collections'), { token: this.token });
    return json ?? [];
  }

  async getSinglePageNames() {
    const { json } = await requestJson(this.url('/models/single_pages'), { token: this.token });
    return json ?? [];
  }

  // === Structural definitions ===============================================================

  /**
   * Create the collection when it is new, replace its schema when it is not.
   *
   * The routes for the two are separate (`POST` creates, `PUT` updates), so which one to use has
   * to be decided by asking, and asking is also what makes a re-run safe: a half-finished
   * migration can simply be run again.
   *
   * @returns {Promise<'created'|'updated'>}
   */
  async ensureCollectionSchema(name, schema) {
    const path = `/models/collections/${encodeURIComponent(name)}/schema`;
    const existing = await this.#getSchema(path);
    await requestVoid(this.url(path), {
      method: existing === null ? 'POST' : 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(schema),
    });
    return existing === null ? 'created' : 'updated';
  }

  async getCollectionSchema(name) {
    return this.#getSchema(`/models/collections/${encodeURIComponent(name)}/schema`);
  }

  async ensureSinglePageSchema(name, schema) {
    const path = `/models/single_pages/${encodeURIComponent(name)}/schema`;
    const existing = await this.#getSchema(path);
    await requestVoid(this.url(path), {
      method: existing === null ? 'POST' : 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(schema),
    });
    return existing === null ? 'created' : 'updated';
  }

  async getSinglePageSchema(name) {
    return this.#getSchema(`/models/single_pages/${encodeURIComponent(name)}/schema`);
  }

  /**
   * Make a collection's **name** exist, without disturbing a schema it already has.
   *
   * A composite definition that holds a relation names a collection or a page, and the CMS
   * refuses to save it while that target does not exist - but the collection's own schema embeds
   * the composite, so neither can be written first. Creating the names first breaks the knot; an
   * empty schema is allowed, and a collection that already exists is left exactly as it is
   * (writing `[]` over it would throw away everything it declares).
   */
  async ensureCollectionExists(name) {
    const path = `/models/collections/${encodeURIComponent(name)}/schema`;
    if ((await this.#getSchema(path)) !== null) return 'exists';
    await requestVoid(this.url(path), {
      method: 'POST',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: '[]',
    });
    return 'created';
  }

  async ensureSinglePageExists(name) {
    const path = `/models/single_pages/${encodeURIComponent(name)}/schema`;
    if ((await this.#getSchema(path)) !== null) return 'exists';
    await requestVoid(this.url(path), {
      method: 'POST',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: '[]',
    });
    return 'created';
  }

  /**
   * Create or replace a composite field definition.
   *
   * Composite definitions are referenced by id from other schemas, and the CMS refuses a schema
   * that names one it does not have, so every definition this run needs must exist first.
   */
  async ensureCompositeField(id, schema) {
    const path = `/models/composite_fields/${encodeURIComponent(id)}`;
    const existing = await this.#getSchema(path);
    await requestVoid(this.url(path), {
      method: existing === null ? 'POST' : 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(schema),
    });
    return existing === null ? 'created' : 'updated';
  }

  // === Items =================================================================================

  /** Create an item from an object of untagged values, answering the new id. */
  async createCollectionItem(name, values) {
    const { json } = await requestJson(this.url(`/models/collections/${encodeURIComponent(name)}/item`), {
      method: 'POST',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(values),
    });
    return json;
  }

  async publishCollectionItem(name, id) {
    await requestVoid(
      this.url(`/models/collections/${encodeURIComponent(name)}/items/${id}/publish`),
      { method: 'POST', token: this.token, expect: 'none' },
    );
  }

  /**
   * Replace an item's values.
   *
   * The write is the whole value set again, which is how the second pass of a relation migration
   * fills in references once every target has an id. Unchanged fields are written back as they
   * were; the CMS stores the value, not a diff.
   */
  async updateCollectionItem(name, id, values) {
    await requestVoid(this.url(`/models/collections/${encodeURIComponent(name)}/items/${id}`), {
      method: 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(values),
    });
  }

  /**
   * State the dates the content had in the CMS it came from.
   *
   * A patch: only the fields present change, so this is safe to run beside an editor. It is the
   * last step of a migration because publishing stamps `published_at` with "now", and because a
   * date ahead of this CMS's clock is refused (see `ItemDates`).
   *
   * @param {{created_at?: string, updated_at?: string, published_at?: string}} dates
   */
  async setCollectionItemDates(name, id, dates) {
    await requestVoid(
      this.url(`/models/collections/${encodeURIComponent(name)}/items/${id}/metadata`),
      {
        method: 'PUT',
        token: this.token,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(dates),
      },
    );
  }

  async setSinglePageDates(name, dates) {
    await requestVoid(this.url(`/models/single_pages/${encodeURIComponent(name)}/item/metadata`), {
      method: 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(dates),
    });
  }

  /** Every item as `[[id, values], ...]`, paging until `X-Total-Count` is covered. */
  async listCollectionItems(name, { pageSize = 100 } = {}) {
    const items = [];
    for (let offset = 0; ; offset += pageSize) {
      const url = `${this.url(`/models/collections/${encodeURIComponent(name)}/items`)}?limit=${pageSize}&offset=${offset}`;
      const { json, headers } = await requestJson(url, { token: this.token });
      const page = json ?? [];
      items.push(...page);
      const total = Number(headers.get('x-total-count') ?? page.length);
      if (page.length === 0 || items.length >= total) break;
    }
    return items;
  }

  async getSinglePageItem(name) {
    try {
      const { json } = await requestJson(
        this.url(`/models/single_pages/${encodeURIComponent(name)}/item`),
        { token: this.token },
      );
      return json;
    } catch (error) {
      if (error instanceof HttpError && error.status === 404) return null;
      throw error;
    }
  }

  async updateSinglePageItem(name, values) {
    await requestVoid(this.url(`/models/single_pages/${encodeURIComponent(name)}/item`), {
      method: 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(values),
    });
  }

  async publishSinglePage(name) {
    await requestVoid(this.url(`/models/single_pages/${encodeURIComponent(name)}/publish`), {
      method: 'POST',
      token: this.token,
      expect: 'none',
    });
  }

  /** Existing library entries, so a re-run can reuse an upload instead of making a second one. */
  async listImages() {
    const { json } = await requestJson(this.url('/models/images'), { token: this.token });
    return json ?? [];
  }

  /**
   * State when an image arrived, for a library migrated from another CMS.
   *
   * `uploaded_at` is display-only (the library is listed by id), which is why an image is the one
   * date that can be stated after the fact without anything else having to move.
   */
  async setImageUploadedAt(id, uploadedAt) {
    await requestVoid(this.url(`/models/images/${id}`), {
      method: 'PUT',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ uploaded_at: uploadedAt }),
    });
  }

  /**
   * Upload one file, following the two-step contract: ask where, then put the bytes there.
   *
   * The id exists as soon as the first step answers, which is what lets content reference the
   * image; the bytes follow. On-premises the URL is a CMS path with a one-shot key; on AWS it is
   * a presigned S3 URL on another origin, so the CMS bearer token is attached only when the
   * target is the CMS itself (an S3 presigned PUT carries its own signature, and supplying an
   * `Authorization` header as well is not what the signature was computed for).
   */
  async uploadImage({ filename, ext, bytes }) {
    const { json: info } = await requestJson(this.url('/models/images/get_upload_url'), {
      method: 'POST',
      token: this.token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ original_filename: filename, ext, size: bytes.length }),
    });
    const target = new URL(info.upload_url, `${this.baseUrl}/`);
    const sameOrigin = target.origin === this.origin;
    const { status } = await fetchWithRetry(target.href, {
      method: 'PUT',
      token: sameOrigin ? this.token : undefined,
      headers: { 'Content-Type': 'application/octet-stream' },
      body: bytes,
      expect: 'none',
      // An image is the one body big enough for a slow link to matter.
      timeoutMs: 120_000,
    });
    return { id: info.id, url: info.url, status };
  }
}
