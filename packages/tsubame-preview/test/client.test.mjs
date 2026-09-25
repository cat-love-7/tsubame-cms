// Talking to the CMS: the two APIs, the paths each is asked for, and what a refusal looks like.
// `fetch` is injected, so nothing here needs a network.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import preview from '../src/index.js';

const { fetchPreview, createContentClient } = preview;

const CMS = 'https://cms.example.test';

/** A `fetch` that answers from a table of `path -> answer` and remembers what it was asked. */
function fakeFetch(answers) {
  const calls = [];
  const fetchImpl = async (url) => {
    calls.push(url);
    const path = url.replace(CMS, '').split('?')[0];
    const answer = answers[path];
    if (answer === undefined) {
      return { ok: false, status: 404, json: async () => ({ message: 'not found' }) };
    }
    if (typeof answer === 'number') {
      return { ok: false, status: answer, json: async () => ({ message: `refused ${answer}` }) };
    }
    return { ok: true, status: 200, json: async () => answer };
  };
  return { fetchImpl, calls };
}

describe('fetchPreview', () => {
  it('asks for the working copy with the token, and answers its schema and values', async () => {
    const { fetchImpl, calls } = fakeFetch({
      '/api/preview/collections/blog/items/7': { schema: [{ name: 'title' }], values: { title: 'Hi' } },
    });

    const result = await fetchPreview({
      apiUrl: CMS,
      target: { kind: 'collection', collection: 'blog', id: 7 },
      token: '1758000000.abc',
      fetchImpl,
    });

    assert.deepEqual(calls, [
      `${CMS}/api/preview/collections/blog/items/7?token=1758000000.abc`,
    ]);
    assert.deepEqual(result, { schema: [{ name: 'title' }], values: { title: 'Hi' } });
  });

  it('asks for a single page at the preview API spelling', async () => {
    const { fetchImpl, calls } = fakeFetch({
      '/api/preview/single_pages/home': { schema: [], values: {} },
    });

    await fetchPreview({
      apiUrl: CMS,
      target: { kind: 'single_page', page: 'home' },
      token: '1.abc',
      fetchImpl,
    });

    assert.deepEqual(calls, [`${CMS}/api/preview/single_pages/home?token=1.abc`]);
  });

  it('carries the status and the API’s own words when the link is refused', async () => {
    const { fetchImpl } = fakeFetch({ '/api/preview/single_pages/home': 403 });

    await assert.rejects(
      fetchPreview({
        apiUrl: CMS,
        target: { kind: 'single_page', page: 'home' },
        token: '1.abc',
        fetchImpl,
      }),
      (error) => error.status === 403 && /refused 403/.test(error.message),
    );
  });

  it('refuses to ask without a token, because the token is the whole credential', async () => {
    await assert.rejects(
      fetchPreview({ apiUrl: CMS, target: { kind: 'single_page', page: 'home' }, token: '' }),
      /token/,
    );
  });

  it('refuses a target it cannot build a path for', async () => {
    await assert.rejects(
      fetchPreview({ apiUrl: CMS, target: { kind: 'collection', collection: '', id: 1 }, token: 'x' }),
      /collection name/,
    );
  });
});

describe('createContentClient', () => {
  it('reads a published collection item', async () => {
    const { fetchImpl, calls } = fakeFetch({
      '/api/content/collections/authors/items/9': {
        schema: [{ name: 'name', field_type: 'Text' }],
        values: { name: 'Ada' },
      },
    });
    const client = createContentClient({ apiUrl: CMS, fetchImpl });

    const published = await client.loadPublished({ kind: 'collection', name: 'authors', item: 9 });

    assert.deepEqual(calls, [`${CMS}/api/content/collections/authors/items/9`]);
    assert.deepEqual(published, { schema: [{ name: 'name', field_type: 'Text' }], values: { name: 'Ada' } });
  });

  it('reads a published single page at the delivery API spelling, which uses a hyphen', async () => {
    const { fetchImpl, calls } = fakeFetch({
      '/api/content/single-pages/home': { schema: [], values: { title: 'Home' } },
    });
    const client = createContentClient({ apiUrl: CMS, fetchImpl });

    await client.loadPublished({ kind: 'single_page', name: 'home', item: null });

    assert.deepEqual(calls, [`${CMS}/api/content/single-pages/home`]);
  });

  it('answers null for content that is not published, which is a 404 and not a failure', async () => {
    const { fetchImpl } = fakeFetch({});
    const client = createContentClient({ apiUrl: CMS, fetchImpl });

    assert.equal(await client.loadPublished({ kind: 'collection', name: 'authors', item: 9 }), null);
  });

  it('reads the composite definitions once, however many fields ask', async () => {
    const { fetchImpl, calls } = fakeFetch({
      '/api/content/composite-fields': {
        seo: [{ name: 'description', field_type: 'Text' }],
      },
    });
    const client = createContentClient({ apiUrl: CMS, fetchImpl });

    assert.deepEqual(await client.loadCompositeSchema('seo'), [
      { name: 'description', field_type: 'Text' },
    ]);
    assert.equal(await client.loadCompositeSchema('missing'), null);
    assert.deepEqual(calls, [`${CMS}/api/content/composite-fields`]);
  });

  it('treats an older CMS without the definitions endpoint as having none', async () => {
    const { fetchImpl } = fakeFetch({});
    const client = createContentClient({ apiUrl: CMS, fetchImpl });

    assert.equal(await client.loadCompositeSchema('seo'), null);
  });

  it('refuses to pretend a server error is missing content', async () => {
    const { fetchImpl } = fakeFetch({ '/api/content/collections/authors/items/9': 500 });
    const client = createContentClient({ apiUrl: CMS, fetchImpl });

    await assert.rejects(
      client.loadPublished({ kind: 'collection', name: 'authors', item: 9 }),
      (error) => error.status === 500,
    );
  });
});
