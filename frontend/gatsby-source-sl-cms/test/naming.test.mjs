// The names the plugin invents have to be GraphQL names, and the same every build: a query is
// written once and run many times, so a name that moved would be a query that compiles on one build
// and not the next.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import naming from '../src/naming.js';

const { createNameAllocator, isGraphqlName, sanitizeFieldName, sanitizeTypeName } = naming;

describe('sanitizeTypeName', () => {
  it('capitalises the words of a collection name', () => {
    assert.equal(sanitizeTypeName('blog'), 'Blog');
    assert.equal(sanitizeTypeName('press-releases'), 'PressReleases');
    assert.equal(sanitizeTypeName('press_releases'), 'PressReleases');
    assert.equal(sanitizeTypeName('SEO pages'), 'SEOPages');
  });

  it('keeps a name GraphQL accepts as-is', () => {
    assert.ok(isGraphqlName(sanitizeTypeName('blog')));
  });

  it('replaces a name with nothing left in it', () => {
    // Nothing survives the ASCII filter, and an empty type name would be a schema error rather
    // than the obvious placeholder this is.
    assert.equal(sanitizeTypeName('お知らせ'), 'Unnamed');
    assert.equal(sanitizeTypeName('---'), 'Unnamed');
  });

  it('does not start a type name with a digit', () => {
    assert.equal(sanitizeTypeName('2nd'), '_2nd');
    assert.equal(sanitizeTypeName('__proto'), 'Proto');
    assert.ok(isGraphqlName(sanitizeTypeName('2nd')));
    assert.ok(isGraphqlName(sanitizeTypeName('__proto')));
  });
});

describe('sanitizeFieldName', () => {
  it('keeps the shape of the original name', () => {
    assert.equal(sanitizeFieldName('published-at'), 'published_at');
    assert.equal(sanitizeFieldName('body.parts'), 'body_parts');
    assert.equal(sanitizeFieldName('field name'), 'field_name');
  });

  it('only produces names GraphQL accepts', () => {
    for (const raw of ['2nd', '__typename', '', 'お知らせ', 'a-b', 'a b']) {
      assert.ok(isGraphqlName(sanitizeFieldName(raw)), `${raw} -> ${sanitizeFieldName(raw)}`);
    }
  });
});

describe('createNameAllocator', () => {
  it('hands out the first free name', () => {
    const allocator = createNameAllocator(['body']);
    assert.equal(allocator.take('title'), 'title');
    assert.equal(allocator.take('body'), 'body_2');
  });

  it('keeps going past the first collision', () => {
    const allocator = createNameAllocator();
    assert.equal(allocator.take('x'), 'x');
    assert.equal(allocator.take('x'), 'x_2');
    assert.equal(allocator.take('x'), 'x_3');
  });

  it('is deterministic for the same order of asks', () => {
    const first = createNameAllocator();
    const second = createNameAllocator();
    assert.deepEqual(
      ['a-b', 'a_b', 'a b'].map((name) => first.take(name)),
      ['a-b', 'a_b', 'a b'].map((name) => second.take(name)),
    );
  });
});
