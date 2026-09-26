import { describe, expect, it } from 'vitest';

import { formatFieldValue, imagesOf, referenceKey, referenceName, relationRefsOf } from './fields';

describe('formatFieldValue', () => {
  it('writes the value a table cell can show', () => {
    expect(formatFieldValue(undefined)).toBe('—');
    expect(formatFieldValue(null)).toBe('—');
    expect(formatFieldValue('')).toBe('');
    expect(formatFieldValue('Hello')).toBe('Hello');
    expect(formatFieldValue(3)).toBe('3');
    expect(formatFieldValue(true)).toBe('✓');
    expect(formatFieldValue(false)).toBe('✗');
    expect(formatFieldValue([])).toBe('—');
    expect(formatFieldValue(['a', 'b'])).toBe('a, b');
  });

  it('shows an image by its url, which is what a reader can act on', () => {
    expect(formatFieldValue({ id: 3, url: '/api/images/logo.png' })).toBe('/api/images/logo.png');
    expect(formatFieldValue([{ id: 3, url: '/api/images/logo.png' }])).toBe('/api/images/logo.png');
  });

  // A relation is what the item points at: expanding the content itself is the delivery API's job
  // (`?populate=`), and a cell has no business printing the reference as JSON.
  it('names a relation by its target and item', () => {
    // One relation is one object; several are an array of them.
    expect(formatFieldValue({ target: 'authors', item: 7 })).toBe('authors #7');
    expect(formatFieldValue([{ target: 'authors', item: 7 }])).toBe('authors #7');
    expect(
      formatFieldValue([
        { target: 'authors', item: 7 },
        { target: 'authors', item: 9 },
      ]),
    ).toBe('authors #7, authors #9');
    // A single page has no id, because its name is its identity.
    expect(formatFieldValue([{ target: 'home' }])).toBe('home');
    expect(formatFieldValue({ target: 'home' })).toBe('home');
  });

  it('falls back to the JSON for a value it has no rendering for', () => {
    expect(formatFieldValue({ id: 'seo', values: { description: 'meta' } })).toBe(
      '{"id":"seo","values":{"description":"meta"}}',
    );
  });
});

describe('imagesOf', () => {
  it('reads one image, or the ones an array carries', () => {
    expect(imagesOf({ id: 3, url: '/api/images/logo.png' })).toEqual([
      { id: 3, url: '/api/images/logo.png' },
    ]);
    expect(imagesOf([3, { id: 4, url: '/api/images/photo.png' }])).toEqual([
      { id: 3, url: null },
      { id: 4, url: '/api/images/photo.png' },
    ]);
  });

  it('has nothing to show for a value that holds no image', () => {
    expect(imagesOf(undefined)).toEqual([]);
    expect(imagesOf(null)).toEqual([]);
    expect(imagesOf([])).toEqual([]);
    expect(imagesOf('not an image')).toEqual([]);
  });
});

describe('references', () => {
  it('reads the references out of a relation value', () => {
    // One relation is one object, and several are an array of them.
    expect(relationRefsOf({ target: 'authors', item: 1 })).toEqual([
      { target: 'authors', item: 1 },
    ]);
    expect(relationRefsOf({ target: 'home' })).toEqual([{ target: 'home' }]);
    expect(relationRefsOf([{ target: 'authors', item: 1 }, { target: 'home' }])).toEqual([
      { target: 'authors', item: 1 },
      { target: 'home' },
    ]);
    expect(relationRefsOf([])).toEqual([]);
    expect(relationRefsOf(null)).toEqual([]);
    expect(relationRefsOf('not a relation')).toEqual([]);
    expect(relationRefsOf([{ item: 1 }, 'x'])).toEqual([]);
  });

  // A collection and a page of the same name are different things, and the key says which.
  it('keys a reference the way the server names an item', () => {
    expect(referenceKey({ target: 'authors', item: 7 })).toBe('collection:authors:7');
    expect(referenceKey({ target: 'home' })).toBe('page:home');
  });

  it('calls a reference by its title, and by the reference when there is none', () => {
    const labels = new Map([['collection:authors:7', 'Ada']]);

    expect(referenceName({ target: 'authors', item: 7 }, labels)).toBe('Ada');
    expect(referenceName({ target: 'authors', item: 9 }, labels)).toBe('authors #9');
    // A page's identity is its name.
    expect(referenceName({ target: 'home' }, labels)).toBe('home');
    // A title that is there but empty says nothing, so the reference does the talking.
    expect(
      referenceName({ target: 'authors', item: 7 }, new Map([['collection:authors:7', '']])),
    ).toBe('authors #7');
  });
});
