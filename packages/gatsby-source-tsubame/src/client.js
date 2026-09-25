'use strict';

const { collectRelationTargets } = require('./fields');

/**
 * The delivery API client.
 *
 * Only `/api/content/*` is used: it is unauthenticated and answers published content only, which
 * is exactly what a site build is allowed to see (`doc/content-api.md`, sections 1-3). Nothing here
 * is aware of the admin API or of drafts.
 *
 * Two details of the contract are load-bearing:
 *
 * - A page is followed by `next_offset`, never by counting items: a page is cut by bytes as well as
 *   by count, so `offset + limit` can skip items that did not fit (`doc/content-api.md` §3.1).
 * - An unpublished page or item answers 404, not 403. Between listing and reading, content can be
 *   unpublished, and that is a race a build has to tolerate rather than crash on.
 */

/** Statuses worth another attempt: the CMS is busy or briefly broken, not wrong about the request. */
const RETRYABLE_STATUS = new Set([408, 425, 429, 500, 502, 503, 504]);

/** A stop for a walk that is not making progress; 10k pages of 200 is far past any real site. */
const MAX_PAGES_PER_COLLECTION = 10_000;

class TsubameHttpError extends Error {
  constructor(message, { status, url, body } = {}) {
    super(message);
    this.name = 'TsubameHttpError';
    this.status = status;
    this.url = url;
    this.body = body;
  }
}

class TsubameClient {
  /**
   * @param {object} options normalized plugin options (`src/options.js`)
   * @param {object} [dependencies] `reporter` for retry warnings, `fetchImpl` to replace fetch
   */
  constructor(options, { reporter = { warn() {} }, fetchImpl } = {}) {
    this.apiUrl = options.apiUrl;
    this.apiPrefix = options.apiPrefix;
    this.pageSize = options.pageSize;
    this.requestTimeout = options.requestTimeout;
    this.retries = options.retries;
    this.concurrency = options.concurrency;
    this.fetchOptions = options.fetchOptions || {};
    this.reporter = reporter;
    // Captured once, so a test that replaces `globalThis.fetch` before constructing the client
    // gets the replacement and a later replacement does not sneak into an in-flight build.
    this.fetchImpl = fetchImpl || globalThis.fetch;
  }

  /** An absolute URL for an API path, with query parameters. */
  endpoint(path, params) {
    const url = new URL(`${this.apiUrl}${this.apiPrefix}${path}`);
    for (const [key, value] of Object.entries(params || {})) {
      if (value !== undefined && value !== null) {
        url.searchParams.set(key, String(value));
      }
    }
    return url.toString();
  }

  /** An absolute URL from a path the API returned (`/api/images/...`). */
  absoluteUrl(pathOrUrl) {
    if (typeof pathOrUrl !== 'string' || pathOrUrl === '') {
      return null;
    }
    try {
      return new URL(pathOrUrl, `${this.apiUrl}/`).toString();
    } catch {
      return null;
    }
  }

  /**
   * An absolute URL for a path *under the API prefix* (`/images/by-id/3`).
   *
   * `absoluteUrl` is for a path that came back from the API and already holds the prefix; this is
   * for a path the plugin builds, where adding the prefix is its own job.
   */
  apiPathUrl(path) {
    return this.absoluteUrl(`${this.apiPrefix}${path}`);
  }

  /**
   * GET a JSON document, retrying what is worth retrying.
   *
   * A 404 is only swallowed when the caller says so: "the page was unpublished while we looked at
   * it" is a normal race, "the collection list names a collection that is not there" is not.
   */
  async requestJson(path, params, { allowNotFound = false } = {}) {
    const url = this.endpoint(path, params);
    let lastError;

    for (let attempt = 0; attempt <= this.retries; attempt += 1) {
      if (attempt > 0) {
        const delayMs = 250 * 2 ** (attempt - 1);
        this.reporter.warn(
          `[gatsby-source-tsubame] ${url} failed (${lastError.message}); retrying in ${delayMs}ms ` +
            `(attempt ${attempt + 1} of ${this.retries + 1})`,
        );
        await delay(delayMs);
      }

      let response;
      try {
        response = await this.fetchImpl(url, {
          ...this.fetchOptions,
          headers: { accept: 'application/json', ...(this.fetchOptions.headers || {}) },
          signal: AbortSignal.timeout(this.requestTimeout),
        });
      } catch (error) {
        // Network failure or timeout: worth another attempt.
        lastError = error;
        continue;
      }

      if (response.status === 404 && allowNotFound) {
        return null;
      }

      if (response.ok) {
        return await response.json();
      }

      const body = await readBodyText(response);
      const error = new TsubameHttpError(
        `GET ${url} answered ${response.status}${body ? `: ${body.slice(0, 300)}` : ''}`,
        { status: response.status, url, body },
      );
      if (!RETRYABLE_STATUS.has(response.status)) {
        throw error;
      }
      lastError = error;
    }

    throw lastError;
  }

  /** The names of collections that currently have at least one published item. */
  async fetchCollectionNames() {
    const names = await this.requestJson('/content/collections');
    return Array.isArray(names) ? names.map(String) : [];
  }

  /** The names of single pages that are currently published. */
  async fetchSinglePageNames() {
    const names = await this.requestJson('/content/single-pages');
    return Array.isArray(names) ? names.map(String) : [];
  }

  /** Both indexes, in one round trip each. */
  async fetchIndex() {
    const [collectionNames, pageNames] = await Promise.all([this.fetchCollectionNames(), this.fetchSinglePageNames()]);
    return { collectionNames, pageNames };
  }

  /** One page of a collection: `{schema, items, total, limit, offset, next_offset}` or null. */
  fetchCollectionPage(name, { limit, offset }) {
    return this.requestJson(`/content/collections/${encodeURIComponent(name)}`, { limit, offset }, { allowNotFound: true });
  }

  /** One published single page, or null when it was unpublished in the meantime. */
  fetchSinglePage(name) {
    return this.requestJson(`/content/single-pages/${encodeURIComponent(name)}`, undefined, { allowNotFound: true });
  }

  /**
   * Every published item of a collection, walking `next_offset`.
   *
   * `schema` comes from the first page (every page carries it, but it does not change while we
   * read) and is null when the collection disappeared before the first page could be read.
   */
  async fetchCollection(name) {
    const items = [];
    let schema = null;
    let total = 0;
    let offset = 0;

    for (let page = 0; page < MAX_PAGES_PER_COLLECTION; page += 1) {
      const body = await this.fetchCollectionPage(name, { limit: this.pageSize, offset });
      if (body === null) {
        this.reporter.warn(
          `[gatsby-source-tsubame] collection "${name}" disappeared while it was being read (unpublished?); skipping it`,
        );
        return { schema: null, items: [], total: 0 };
      }

      schema = body.schema === undefined ? schema : body.schema;
      total = typeof body.total === 'number' ? body.total : total;
      if (Array.isArray(body.items)) {
        items.push(...body.items);
      }

      const next = body.next_offset;
      if (next === null || next === undefined) {
        return { schema, items, total };
      }
      if (typeof next !== 'number' || next <= offset) {
        throw new Error(
          `[gatsby-source-tsubame] collection "${name}": next_offset (${JSON.stringify(next)}) does not move ` +
            `past the current offset (${offset}); refusing to loop`,
        );
      }
      offset = next;
    }

    throw new Error(
      `[gatsby-source-tsubame] collection "${name}": gave up after ${MAX_PAGES_PER_COLLECTION} pages`,
    );
  }

  /**
   * The composite definitions the schemas name, by id.
   *
   * A schema points at a composite by id (`{"CompositeField": {"id": "block"}}`); the fields of a
   * block exist only in the definition, and since `5ed924a` the delivery API answers them without a
   * token. Without this, a Markdown field or a relation inside a composite could not be recognised
   * at all.
   */
  async fetchCompositeFields() {
    const body = await this.requestJson('/content/composite-fields', undefined, { allowNotFound: true });
    if (body === null) {
      // A CMS older than the route still builds: a composite stays the opaque `{id, values}` it
      // used to be, and the warning says why its fields are not typed.
      this.reporter.warn(
        '[gatsby-source-tsubame] /content/composite-fields answered 404; composite fields stay untyped ' +
          '(the CMS predates the route)',
      );
      return new Map();
    }
    const composites = new Map();
    if (typeof body === 'object' && !Array.isArray(body)) {
      for (const [id, schema] of Object.entries(body)) {
        composites.set(id, Array.isArray(schema) ? schema : []);
      }
    }
    return composites;
  }

  /**
   * Something to declare the GraphQL schema from, and to source the values with.
   *
   * The collections with published items come from the index, but their schemas can name more
   * collections than that: a relation may point at an editor who is not published yet, and the
   * target still needs a type for the relation field to be declared against (`limit=1` is the whole
   * request - only the schema is wanted). The walk repeats until it settles, because a fetched
   * schema can name a target of its own; composite definitions are walked with it, since a relation
   * can sit inside one.
   *
   * A single-page target that is not published has no public schema at all - the route answers 404
   * - so its name is reported instead, and the type is declared with the plugin's own fields only.
   */
  async fetchSchemaSnapshot() {
    const { collectionNames, pageNames } = await this.fetchIndex();
    const composites = await this.fetchCompositeFields();

    const collections = new Map();
    const pages = new Map();

    const collectionRows = await mapWithConcurrency(collectionNames, this.concurrency, async (name) => {
      const body = await this.fetchCollectionPage(name, { limit: 1, offset: 0 });
      return body === null ? null : [name, schemaOf(body)];
    });
    for (const row of collectionRows) {
      if (row !== null) {
        collections.set(row[0], row[1]);
      }
    }

    const pageRows = await mapWithConcurrency(pageNames, this.concurrency, async (name) => {
      const body = await this.fetchSinglePage(name);
      return body === null ? null : [name, schemaOf(body)];
    });
    for (const row of pageRows) {
      if (row !== null) {
        pages.set(row[0], row[1]);
      }
    }

    // Every collection a relation names, whether or not it was in the index. `tried` holds the
    // names already asked for - including the ones that answered 404 - so a target that cannot be
    // read is not asked for again on the next round.
    const tried = new Set(collections.keys());
    for (;;) {
      const targets = collectRelationTargets([...collections.values(), ...pages.values()], composites);
      const missing = targets
        .filter((target) => target.kind === 'collection' && !tried.has(target.name))
        .map((target) => target.name)
        .sort();
      if (missing.length === 0) {
        break;
      }
      for (const name of missing) {
        tried.add(name);
        const body = await this.fetchCollectionPage(name, { limit: 1, offset: 0 });
        if (body !== null) {
          collections.set(name, schemaOf(body));
        }
      }
    }

    const unpublishedPageTargets = collectRelationTargets([...collections.values(), ...pages.values()], composites)
      .filter((target) => target.kind === 'single_page' && !pages.has(target.name))
      .map((target) => target.name)
      .sort();

    return { collectionNames, pageNames, collections, pages, composites, unpublishedPageTargets };
  }
}

function schemaOf(body) {
  return Array.isArray(body.schema) ? body.schema : [];
}

async function readBodyText(response) {
  try {
    return await response.text();
  } catch {
    return '';
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Map with a bounded number of requests in flight.
 *
 * The on-premises adapter serialises its storage operations anyway, so a build that fires a
 * thousand requests at it gains nothing and holds a thousand sockets; four at a time is enough to
 * keep a build moving without turning a CMS into the bottleneck.
 */
async function mapWithConcurrency(values, limit, mapper) {
  const results = new Array(values.length);
  let nextIndex = 0;

  const workers = Array.from({ length: Math.max(1, Math.min(limit, values.length)) }, async () => {
    for (;;) {
      const index = nextIndex;
      nextIndex += 1;
      if (index >= values.length) {
        return;
      }
      results[index] = await mapper(values[index], index);
    }
  });

  await Promise.all(workers);
  return results;
}

module.exports = {
  TsubameClient,
  TsubameHttpError,
  mapWithConcurrency,
  MAX_PAGES_PER_COLLECTION,
};
