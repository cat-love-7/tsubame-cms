import { describe, expect, it } from 'vitest';

import { formatFieldValue } from './fields';

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
    expect(formatFieldValue([{ target: 'authors', item: 7 }])).toBe('authors #7');
    expect(
      formatFieldValue([
        { target: 'authors', item: 7 },
        { target: 'authors', item: 9 },
      ]),
    ).toBe('authors #7, authors #9');
    // A single page has no id, because its name is its identity.
    expect(formatFieldValue([{ target: 'home' }])).toBe('home');
  });

  it('falls back to the JSON for a value it has no rendering for', () => {
    expect(formatFieldValue({ id: 'seo', values: { description: 'meta' } })).toBe(
      '{"id":"seo","values":{"description":"meta"}}',
    );
  });
});
