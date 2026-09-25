// Where a preview lives. Two spellings of "single page" travel through here - the API's preview
// route (`single_pages`) and the delivery API's (`single-pages`) - so the tests pin both.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import preview from '../src/index.js';

const { checkTarget, previewRoutePath, previewApiPath, parsePreviewRoute, previewSiteUrl } = preview;

const item = { kind: 'collection', collection: 'blog', id: 7 };
const page = { kind: 'single_page', page: 'home' };

describe('previewApiPath', () => {
  it('spells the route the API serves, prefix included', () => {
    assert.equal(previewApiPath(item), '/api/preview/collections/blog/items/7');
    assert.equal(previewApiPath(page), '/api/preview/single_pages/home');
  });

  it('follows a deployment that serves the API under another prefix', () => {
    assert.equal(previewApiPath(item, '/cms/api'), '/cms/api/preview/collections/blog/items/7');
  });

  it('escapes a name that cannot travel in a path as it is', () => {
    assert.equal(
      previewApiPath({ kind: 'collection', collection: 'お知らせ', id: 1 }),
      `/api/preview/collections/${encodeURIComponent('お知らせ')}/items/1`,
    );
  });

  it('answers an empty path for a target it cannot build one from', () => {
    assert.equal(previewApiPath({ kind: 'collection', collection: 'blog', id: 0 }), '');
    assert.equal(previewApiPath(null), '');
  });
});

describe('previewRoutePath', () => {
  it('is the API route without the prefix, which is what the preview site serves', () => {
    assert.equal(previewRoutePath(item), '/preview/collections/blog/items/7');
    assert.equal(previewRoutePath(page), '/preview/single_pages/home');
  });
});

describe('checkTarget', () => {
  it('says what is missing rather than letting a broken link be built', () => {
    assert.equal(checkTarget(item), null);
    assert.equal(checkTarget(page), null);
    assert.match(checkTarget({ kind: 'collection', collection: '', id: 1 }), /collection name/);
    assert.match(checkTarget({ kind: 'collection', collection: 'blog', id: 'x' }), /item id/);
    assert.match(checkTarget({ kind: 'single_page', page: 3 }), /page name/);
    assert.match(checkTarget({ kind: 'nonsense' }), /collection item or a single page/);
    assert.match(checkTarget(null), /object/);
  });
});

describe('parsePreviewRoute', () => {
  it('reads a collection item out of the preview site address', () => {
    assert.deepEqual(parsePreviewRoute('/preview/collections/blog/items/7'), {
      kind: 'collection',
      collection: 'blog',
      id: 7,
    });
  });

  it('reads a single page', () => {
    assert.deepEqual(parsePreviewRoute('/preview/single_pages/home'), {
      kind: 'single_page',
      page: 'home',
    });
  });

  it('does not care what base path the site is mounted under', () => {
    assert.deepEqual(parsePreviewRoute('/some/base/preview/single_pages/home/'), {
      kind: 'single_page',
      page: 'home',
    });
  });

  it('undoes the escaping the link applied', () => {
    assert.deepEqual(parsePreviewRoute(`/preview/collections/${encodeURIComponent('お知らせ')}/items/2`), {
      kind: 'collection',
      collection: 'お知らせ',
      id: 2,
    });
  });

  it('answers null for an address that names something else', () => {
    assert.equal(parsePreviewRoute('/blog/hello'), null);
    assert.equal(parsePreviewRoute('/preview/collections/blog/items/'), null);
    assert.equal(parsePreviewRoute(undefined), null);
  });
});

describe('previewSiteUrl', () => {
  it('moves the API path onto the preview site and keeps the token', () => {
    assert.equal(
      previewSiteUrl('/api/preview/collections/blog/items/7?token=1.abc', 'https://preview.example.com'),
      'https://preview.example.com/preview/collections/blog/items/7?token=1.abc',
    );
  });

  it('tolerates a trailing slash on the origin', () => {
    assert.equal(
      previewSiteUrl('/api/preview/single_pages/home?token=1.abc', 'http://localhost:3000/'),
      'http://localhost:3000/preview/single_pages/home?token=1.abc',
    );
  });

  it('follows a deployment that serves the API under another prefix', () => {
    assert.equal(
      previewSiteUrl('/cms/api/preview/single_pages/home', 'https://preview.example.com', '/cms/api'),
      'https://preview.example.com/preview/single_pages/home',
    );
  });

  it('leaves a path that does not name the API alone', () => {
    assert.equal(
      previewSiteUrl('/preview/single_pages/home', 'https://preview.example.com'),
      'https://preview.example.com/preview/single_pages/home',
    );
  });
});
