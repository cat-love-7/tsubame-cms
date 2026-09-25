// The client is what stands between a build and a CMS that is slow, busy, or being edited while the
// build reads it. These tests pin the parts of the contract that a build depends on: pages are
// followed by `next_offset`, an unpublished thing is skipped rather than fatal, and a retry happens
// for the failures that are worth retrying.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import clientModule from '../src/client.js';
import { BLOG_ITEMS, createApiFetch } from './fixtures.mjs';

const { SlCmsClient, SlCmsHttpError, mapWithConcurrency } = clientModule;

function options(overrides = {}) {
  return {
    apiUrl: 'https://cms.example.com',
    apiPrefix: '/api',
    pageSize: 2,
    requestTimeout: 1_000,
    retries: 0,
    concurrency: 2,
    fetchOptions: {},
    ...overrides,
  };
}

function jsonResponse(body, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async json() {
      return body;
    },
    async text() {
      return JSON.stringify(body);
    },
  };
}

describe('endpoint', () => {
  it('builds an API URL under the configured prefix', () => {
    const client = new SlCmsClient(options(), { fetchImpl: async () => jsonResponse({}) });
    assert.equal(client.endpoint('/content/collections'), 'https://cms.example.com/api/content/collections');
  });

  it('adds the query parameters it was given and no others', () => {
    const client = new SlCmsClient(options(), { fetchImpl: async () => jsonResponse({}) });
    assert.equal(
      client.endpoint('/content/collections/blog', { limit: 2, offset: 0, missing: undefined }),
      'https://cms.example.com/api/content/collections/blog?limit=2&offset=0',
    );
  });

  it('resolves a path the API returned against the CMS', () => {
    const client = new SlCmsClient(options(), { fetchImpl: async () => jsonResponse({}) });
    assert.equal(client.absoluteUrl('/api/images/logo.png'), 'https://cms.example.com/api/images/logo.png');
    assert.equal(client.absoluteUrl(''), null);
  });
});

describe('fetchCollection', () => {
  it('walks next_offset until there is none', async () => {
    const fetchImpl = createApiFetch();
    const client = new SlCmsClient(options(), { fetchImpl });

    const content = await client.fetchCollection('blog');

    assert.deepEqual(
      content.items.map((item) => item.id),
      BLOG_ITEMS.map((item) => item.id),
    );
    assert.equal(content.total, BLOG_ITEMS.length);
    assert.equal(content.schema[0].name, 'title');
    assert.ok(fetchImpl.requests.some((url) => url.includes('offset=2')));
  });

  it('does not loop for ever when next_offset does not advance', async () => {
    const client = new SlCmsClient(options(), {
      fetchImpl: async () =>
        jsonResponse({ schema: [], items: [{ id: 1, values: {} }], total: 2, limit: 2, offset: 0, next_offset: 0 }),
    });

    await assert.rejects(() => client.fetchCollection('blog'), /does not move past the current offset/);
  });

  it('skips a collection that disappeared before it could be read', async () => {
    const client = new SlCmsClient(options(), { fetchImpl: async () => jsonResponse({ code: 'not_found' }, 404) });
    const content = await client.fetchCollection('blog');
    assert.deepEqual(content, { schema: null, items: [], total: 0 });
  });
});

describe('requestJson', () => {
  it('answers an unpublished single page with null instead of an error', async () => {
    const client = new SlCmsClient(options(), { fetchImpl: async () => jsonResponse({ code: 'not_found' }, 404) });
    assert.equal(await client.fetchSinglePage('gone'), null);
  });

  it('retries a failure that is worth retrying', async () => {
    let attempts = 0;
    const fetchImpl = async () => {
      attempts += 1;
      return attempts === 1 ? jsonResponse({ code: 'busy' }, 503) : jsonResponse(['blog']);
    };
    const client = new SlCmsClient(options({ retries: 1 }), { fetchImpl, reporter: { warn() {} } });

    assert.deepEqual(await client.fetchCollectionNames(), ['blog']);
    assert.equal(attempts, 2);
  });

  it('fails immediately on a request the CMS rejected', async () => {
    let attempts = 0;
    const fetchImpl = async () => {
      attempts += 1;
      return jsonResponse({ code: 'bad_request' }, 400);
    };
    const client = new SlCmsClient(options({ retries: 2 }), { fetchImpl, reporter: { warn() {} } });

    await assert.rejects(
      () => client.fetchCollectionNames(),
      (error) => error instanceof SlCmsHttpError && error.status === 400,
    );
    assert.equal(attempts, 1);
  });
});

describe('fetchCompositeFields', () => {
  it('answers the definitions by id', async () => {
    const client = new SlCmsClient(options(), { fetchImpl: createApiFetch() });
    const composites = await client.fetchCompositeFields();
    assert.equal(composites.get('block')[0].name, 'text');
  });

  it('answers an empty map when the route predates the feature', async () => {
    const warnings = [];
    const client = new SlCmsClient(options(), {
      fetchImpl: async () => jsonResponse({ code: 'not_found' }, 404),
      reporter: { warn: (message) => warnings.push(message) },
    });
    // Building against a CMS older than the route still works, with a warning that says why the
    // composite fields are untyped.
    assert.equal((await client.fetchCompositeFields()).size, 0);
    assert.match(warnings.join('\n'), /composite fields stay untyped/);
  });
});

describe('fetchSchemaSnapshot', () => {
  it('asks each collection for its schema with the smallest possible page', async () => {
    const fetchImpl = createApiFetch();
    const client = new SlCmsClient(options(), { fetchImpl });

    const snapshot = await client.fetchSchemaSnapshot();

    assert.deepEqual(snapshot.collectionNames, ['blog', 'authors']);
    assert.deepEqual(snapshot.pageNames, ['home']);
    assert.equal(snapshot.pages.get('home')[0].name, 'title');
    assert.ok(fetchImpl.requests.some((url) => url.includes('limit=1')));
  });

  it('fetches the schema of a collection a relation names, even with no published items', async () => {
    const fetchImpl = createApiFetch();
    const client = new SlCmsClient(options(), { fetchImpl });

    const snapshot = await client.fetchSchemaSnapshot();

    // `editors` is not in `/content/collections` (it has no published items) but `blog.editors`
    // names it, and the relation field needs its type.
    assert.deepEqual([...snapshot.collections.keys()].sort(), ['authors', 'blog', 'editors']);
    assert.ok(fetchImpl.requests.some((url) => url.includes('/collections/editors')));
  });

  it('reports a single-page target that is not published', async () => {
    const client = new SlCmsClient(options(), { fetchImpl: createApiFetch() });
    const snapshot = await client.fetchSchemaSnapshot();
    // Reached through `editors.homepage`, which is a target of a target.
    assert.deepEqual(snapshot.unpublishedPageTargets, ['contact']);
    assert.equal(snapshot.pages.has('contact'), false);
  });

  it('reads the composite definitions the schemas name', async () => {
    const client = new SlCmsClient(options(), { fetchImpl: createApiFetch() });
    const snapshot = await client.fetchSchemaSnapshot();
    assert.deepEqual([...snapshot.composites.keys()].sort(), ['block', 'cta', 'seo']);
  });
});

describe('mapWithConcurrency', () => {
  it('keeps the order of the input', async () => {
    const result = await mapWithConcurrency([3, 1, 2], 2, async (value) => {
      await new Promise((resolve) => setTimeout(resolve, value));
      return value * 10;
    });
    assert.deepEqual(result, [30, 10, 20]);
  });
});
