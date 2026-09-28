// Turning a working copy (`{schema, values}`) into something a page can read. The schema is the
// only thing that says what a value is, so every test here varies the field type, not the value.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import preview from '../src/index.js';

const { resolvePreview } = preview;

const API = 'https://cms.example.test';

/** The shapes the wire and the resolved answer use, so the fixtures below say what they build. */
/** @typedef {import('../src/field-types.js').SchemaField} SchemaField */
/** @typedef {import('../src/field-types.js').PublishedReference} PublishedReference */
/** @typedef {import('../src/resolve.js').PreviewProblem} PreviewProblem */
/** @typedef {import('../src/resolve.js').PreviewOptions} PreviewOptions */

const REFERENCE = { target: 'authors', item: 9 };

/** A relation holds one reference; several are an `Array` whose item types are all relations. */
const RELATION = { Relation: { target: { kind: 'collection', name: 'authors' } } };

function schema() {
  return [
    // The wire spellings, as the CMS serialises them: a unit variant is a bare string, a variant
    // that carries options is a single-key object (`backend/crates/core/src/models/schema.rs`).
    { name: 'title', field_type: { Text: {} } },
    { name: 'slug', field_type: { Slug: {} } },
    { name: 'count', field_type: 'Number' },
    { name: 'draft', field_type: 'Boolean' },
    { name: 'published', field_type: 'Date' },
    { name: 'body', field_type: { Markdown: {} } },
    { name: 'cover', field_type: 'Image' },
    { name: 'tags', field_type: { TextEnum: ['a', 'b'] } },
    { name: 'author', field_type: RELATION },
    { name: 'seo', field_type: { CompositeField: { id: 'seo' } } },
    { name: 'gallery', field_type: { Array: ['Image'] } },
    {
      name: 'mixed',
      field_type: { Array: [{ Text: {} }, 'Number'] },
      // The API has no type tag per entry, so a mixed list stays the JSON it arrived as.
    },
  ];
}

function values() {
  return {
    title: 'Hello',
    slug: 'hello',
    count: 3,
    draft: true,
    published: '2024-01-01T00:00:00Z',
    body: '# Hi',
    cover: { id: 3, url: '/api/images/one.png' },
    tags: ['a', 'b'],
    author: REFERENCE,
    seo: { id: 'seo', values: { description: 'd' } },
    gallery: [{ id: 1, url: '/api/images/g.png' }],
    mixed: ['a', 1],
  };
}

/**
 * The published side of the world: the author is published, item 10 is not.
 *
 * @param {{ nested?: boolean }} [options]
 * @returns {Pick<PreviewOptions, 'loadPublished' | 'loadCompositeSchema'>}
 */
function content({ nested = false } = {}) {
  /** @type {SchemaField[]} */
  const authorSchema = [{ name: 'name', field_type: { Text: {} } }];
  if (nested) {
    authorSchema.push({ name: 'mentor', field_type: RELATION });
  }
  return {
    loadPublished: async (reference) => {
      if (reference.kind === 'collection' && reference.name === 'authors' && reference.item === 9) {
        return {
          schema: authorSchema,
          values: nested ? { name: 'Ada', mentor: { target: 'authors', item: 11 } } : { name: 'Ada' },
        };
      }
      if (reference.kind === 'collection' && reference.name === 'authors' && reference.item === 11) {
        return { schema: [{ name: 'name', field_type: { Text: {} } }], values: { name: 'Grace' } };
      }
      if (reference.kind === 'collection' && reference.name === 'categories' && reference.item === 3) {
        return { schema: [{ name: 'name', field_type: { Text: {} } }], values: { name: 'News' } };
      }
      return null;
    },
    loadCompositeSchema: async (id) =>
      id === 'seo' ? [{ name: 'description', field_type: { Text: {} } }] : null,
  };
}

/**
 * Resolve the stock working copy, with any option overridden.
 *
 * @param {Partial<PreviewOptions>} [overrides]
 * @returns {Promise<Record<string, unknown>>}
 */
function resolve(overrides = {}) {
  return resolvePreview({
    apiUrl: API,
    schema: schema(),
    values: values(),
    renderMarkdown: (raw) => `<p>${raw}</p>`,
    ...content(),
    ...overrides,
  });
}

describe('resolvePreview', () => {
  it('reads each value by the field type the schema gave it', async () => {
    const result = await resolve();

    assert.equal(result.title, 'Hello');
    assert.equal(result.slug, 'hello');
    assert.equal(result.count, 3);
    assert.equal(result.draft, true);
    // A date stays the ISO string the API sent: formatting is the page's business.
    assert.equal(result.published, '2024-01-01T00:00:00Z');
    assert.deepEqual(result.tags, ['a', 'b']);
  });

  it('renders Markdown with the renderer the site supplied', async () => {
    const result = await resolve();
    assert.deepEqual(result.body, { raw: '# Hi', html: '<p># Hi</p>' });
  });

  it('keeps Markdown raw when no renderer was supplied', async () => {
    const result = await resolve({ renderMarkdown: undefined });
    assert.deepEqual(result.body, { raw: '# Hi', html: null });
  });

  it('keeps the raw text when the renderer throws, and says so', async () => {
    /** @type {PreviewProblem[]} */
    const problems = [];
    const result = await resolve({
      renderMarkdown: () => {
        throw new Error('bad plugin');
      },
      onProblem: (problem) => problems.push(problem),
    });

    assert.deepEqual(result.body, { raw: '# Hi', html: null });
    assert.equal(problems.length, 1);
    assert.match(problems[0].message, /bad plugin/);
  });

  it('makes an image openable and gives it the link that survives a replacement', async () => {
    const result = await resolve();
    assert.deepEqual(result.cover, {
      id: 3,
      url: '/api/images/one.png',
      absoluteUrl: `${API}/api/images/one.png`,
      stableUrl: `${API}/api/images/by-id/3`,
    });
  });

  it('resolves a composite against its definition and keeps the raw values', async () => {
    const result = await resolve();
    assert.deepEqual(result.seo, {
      id: 'seo',
      values: { description: 'd' },
      description: 'd',
    });
  });

  it('leaves a composite opaque when its definition is not known', async () => {
    const result = await resolve({ loadCompositeSchema: async () => null });
    assert.deepEqual(result.seo, { id: 'seo', values: { description: 'd' } });
  });

  it('resolves an array element by element', async () => {
    const result = await resolve();
    assert.deepEqual(result.gallery, [
      {
        id: 1,
        url: '/api/images/g.png',
        absoluteUrl: `${API}/api/images/g.png`,
        stableUrl: `${API}/api/images/by-id/1`,
      },
    ]);
  });

  it('leaves a mixed array as the JSON it arrived as', async () => {
    const result = await resolve();
    assert.deepEqual(result.mixed, ['a', 1]);
  });

  it('answers an empty list for a list field with no value', async () => {
    const result = await resolve({ values: { ...values(), gallery: undefined } });
    assert.deepEqual(result.gallery, []);
  });
});

describe('resolvePreview relations', () => {
  it('puts the published target in the field rather than pointing at a node', async () => {
    const result = await resolve();
    assert.deepEqual(result.author, { name: 'Ada' });
  });

  it('answers null for a relation with no reference', async () => {
    const result = await resolve({ values: { ...values(), author: null } });
    assert.equal(result.author, null);
  });

  it('answers null for a target that is not published', async () => {
    const result = await resolve({
      values: { ...values(), author: { target: 'authors', item: 10 } },
    });
    assert.equal(result.author, null);
  });

  it('follows one hop by default, and no further', async () => {
    const result = await resolve({
      values: { ...values(), author: REFERENCE },
      ...content({ nested: true }),
    });
    assert.deepEqual(result.author, { name: 'Ada', mentor: null });
  });

  it('follows as far as the budget allows', async () => {
    const result = await resolve({
      values: { ...values(), author: REFERENCE },
      relationDepth: 2,
      ...content({ nested: true }),
    });
    assert.deepEqual(result.author, { name: 'Ada', mentor: { name: 'Grace' } });
  });

  it('answers null with no budget at all', async () => {
    const result = await resolve({ relationDepth: 0 });
    assert.equal(result.author, null);
  });

  it('keeps an array of references in order and null in the place of an unpublished entry', async () => {
    const result = await resolve({
      schema: [{ name: 'editors', field_type: { Array: [RELATION] } }],
      values: {
        editors: [
          { target: 'authors', item: 9 },
          { target: 'authors', item: 10 },
          { target: 'authors', item: 9 },
        ],
      },
    });
    assert.deepEqual(result.editors, [{ name: 'Ada' }, null, { name: 'Ada' }]);
  });

  it('resolves the elements of one array whose targets differ', async () => {
    const result = await resolve({
      schema: [
        {
          name: 'related',
          field_type: {
            Array: [RELATION, { Relation: { target: { kind: 'collection', name: 'categories' } } }],
          },
        },
      ],
      values: {
        related: [
          { target: 'authors', item: 9 },
          { target: 'categories', item: 3 },
        ],
      },
    });
    assert.deepEqual(result.related, [{ name: 'Ada' }, { name: 'News' }]);
  });

  it('answers an empty array for an array of relations with no value', async () => {
    const result = await resolve({
      schema: [{ name: 'editors', field_type: { Array: [RELATION] } }],
      values: {},
    });
    assert.deepEqual(result.editors, []);
  });

  it('leaves an array that mixes a relation with another type as raw JSON', async () => {
    // The whole array is only several references when every item type is a relation; a mixed list
    // has no per-element type tag, so it stays as the API sent it.
    const value = [{ target: 'authors', item: 9 }, 'x'];
    const result = await resolve({
      schema: [{ name: 'mixed_related', field_type: { Array: [RELATION, { Text: {} }] } }],
      values: { mixed_related: value },
    });
    assert.deepEqual(result.mixed_related, value);
  });

  it('reads a single-page reference as a page, not an item', async () => {
    /** @type {PublishedReference[]} */
    const asked = [];
    const result = await resolve({
      schema: [
        {
          name: 'home',
          field_type: { Relation: { target: { kind: 'single_page', name: 'home' } } },
        },
      ],
      values: { home: { target: 'home' } },
      loadPublished: async (reference) => {
        asked.push(reference);
        return { schema: [{ name: 'title', field_type: { Text: {} } }], values: { title: 'Home' } };
      },
    });

    assert.deepEqual(asked, [{ kind: 'single_page', name: 'home', item: null }]);
    assert.deepEqual(result.home, { title: 'Home' });
  });
});
