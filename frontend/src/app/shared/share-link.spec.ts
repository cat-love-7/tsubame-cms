import { absoluteApiUrl, previewSiteUrl } from './share-link';

describe('shareable links', () => {
  // The API hands out paths that already carry its prefix (a preview link) as well as ones that
  // do not, and a screen turns either into something a browser can open.
  it('builds an address for a path that already names the API', () => {
    expect(absoluteApiUrl('/api/preview/collections/blog/items/7', 'https://cms.example.com')).toBe(
      'https://cms.example.com/api/preview/collections/blog/items/7',
    );
  });

  it('adds the prefix to a backend-relative path', () => {
    expect(absoluteApiUrl('/preview/single_pages/home', 'https://cms.example.com')).toBe(
      'https://cms.example.com/api/preview/single_pages/home',
    );
  });

  it('leaves an absolute URL alone', () => {
    // An object-storage URL is already complete, and the origin in front of it would be a
    // different address entirely.
    expect(absoluteApiUrl('https://images.example.com/one.png', 'https://cms.example.com')).toBe(
      'https://images.example.com/one.png',
    );
  });
});

describe('preview links on the preview site', () => {
  // The API mints a path that already names the API; the preview site serves the same route
  // without that prefix, so a reviewer opens a page instead of the API's JSON.
  it('moves the API path onto the preview site and keeps the token', () => {
    expect(
      previewSiteUrl(
        '/api/preview/collections/blog/items/7?token=1758000000.abc',
        'https://preview.example.com',
      ),
    ).toBe('https://preview.example.com/preview/collections/blog/items/7?token=1758000000.abc');
  });

  it('handles a single page the same way', () => {
    expect(
      previewSiteUrl('/api/preview/single_pages/home?token=1.def', 'https://preview.example.com'),
    ).toBe('https://preview.example.com/preview/single_pages/home?token=1.def');
  });

  it('tolerates a trailing slash on the origin and a path that does not name the API', () => {
    expect(previewSiteUrl('/preview/single_pages/home', 'http://localhost:3000/')).toBe(
      'http://localhost:3000/preview/single_pages/home',
    );
  });
});
