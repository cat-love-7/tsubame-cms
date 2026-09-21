// The CMS sends values without type tags, so the field's `field_type` is the only thing that says
// what a value means. These tests pin that reading, the value shapes it produces, and the walk that
// finds the content a schema's relations point at.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import fields from '../src/fields.js';
import { BLOG_SCHEMA, COMPOSITE_FIELDS, EDITORS_SCHEMA, HOME_SCHEMA } from './fixtures.mjs';

const {
  collectRelationTargets,
  describeFieldType,
  toCompositeValue,
  toImageValue,
  toRelationValue,
} = fields;

const resolveUrl = (path) => `https://cms.example.com${path}`;
const resolveApiPath = (path) => `https://cms.example.com/api${path}`;
const resolve = { resolveUrl, resolveApiPath };
const composites = new Map(Object.entries(COMPOSITE_FIELDS));

describe('describeFieldType', () => {
  it('reads a field type spelled as a bare string', () => {
    assert.deepEqual(describeFieldType('Number'), { kind: 'Number', options: undefined });
    assert.deepEqual(describeFieldType('Image'), { kind: 'Image', options: undefined });
  });

  it('reads a field type spelled as a single-key object', () => {
    assert.deepEqual(describeFieldType({ Markdown: {} }), { kind: 'Markdown', options: {} });
    assert.deepEqual(describeFieldType({ Array: ['Image'] }), { kind: 'Array', options: ['Image'] });
  });

  it('does not mistake an unknown type for a known one', () => {
    assert.equal(describeFieldType({ SomethingNew: 1 }).kind, 'Unknown');
    assert.equal(describeFieldType(null).kind, 'Unknown');
  });
});

describe('toImageValue', () => {
  it('resolves the API path into something a browser can open', () => {
    assert.deepEqual(toImageValue({ id: 3, url: '/api/images/logo.png' }, resolve), {
      id: 3,
      url: '/api/images/logo.png',
      absoluteUrl: 'https://cms.example.com/api/images/logo.png',
      // The id link lives under the API prefix, which the returned `url` already carries and this
      // one has to be given: `/images/by-id/3` alone would be a 404.
      stableUrl: 'https://cms.example.com/api/images/by-id/3',
    });
  });

  it('keeps an empty image empty', () => {
    assert.equal(toImageValue(null, resolve), null);
  });

  it('carries the downloaded file when the images option turned it on', () => {
    const imageFiles = new Map([['id:3', 'file-node-id']]);
    assert.deepEqual(toImageValue({ id: 3, url: '/api/images/logo.png' }, { ...resolve, imageFiles }), {
      id: 3,
      url: '/api/images/logo.png',
      absoluteUrl: 'https://cms.example.com/api/images/logo.png',
      stableUrl: 'https://cms.example.com/api/images/by-id/3',
      localFile: 'file-node-id',
    });
  });

  it('answers a null localFile for an image that was not downloaded', () => {
    const imageFiles = new Map();
    assert.equal(toImageValue({ id: 3, url: '/api/images/logo.png' }, { ...resolve, imageFiles }).localFile, null);
  });
});

describe('toCompositeValue', () => {
  it('keeps the id and the untyped values', () => {
    assert.deepEqual(toCompositeValue({ id: 'seo', values: { description: 'd' } }), {
      id: 'seo',
      values: { description: 'd' },
    });
  });

  it('treats something that is not a composite as empty', () => {
    assert.equal(toCompositeValue(null), null);
    assert.equal(toCompositeValue('text'), null);
  });
});

describe('toRelationValue', () => {
  it('spells out whether the target is a collection item or a single page', () => {
    assert.deepEqual(toRelationValue([{ target: 'authors', item: 7 }, { target: 'home' }]), [
      { target: 'authors', item: 7, kind: 'collection' },
      { target: 'home', item: null, kind: 'single_page' },
    ]);
  });

  it('treats no references as an empty set', () => {
    assert.deepEqual(toRelationValue(null), []);
    assert.deepEqual(toRelationValue([]), []);
  });
});

describe('collectRelationTargets', () => {
  it('finds the collections a schema points at', () => {
    const targets = collectRelationTargets([BLOG_SCHEMA], composites);
    assert.deepEqual(
      targets.map((target) => `${target.kind}:${target.name}`).sort(),
      ['collection:authors', 'collection:editors'],
    );
  });

  it('finds one inside a composite definition', () => {
    // The schema itself names no relation at all: only `block`'s definition does.
    const onlyAComposite = [{ name: 'blocks', field_type: { Array: [{ CompositeField: { id: 'block' } }] } }];
    assert.deepEqual(collectRelationTargets([onlyAComposite], composites), [{ kind: 'collection', name: 'authors' }]);
  });

  it('finds a target of a target', () => {
    // `editors` does not reach `contact` by itself; its own schema does.
    const targets = collectRelationTargets([EDITORS_SCHEMA], composites);
    assert.deepEqual(targets, [{ kind: 'single_page', name: 'contact' }]);
  });

  it('stops walking a definition that reaches itself through an array', () => {
    // `block.children` is an array of blocks: the walk would not terminate without the guard.
    const schema = [{ name: 'blocks', field_type: { CompositeField: { id: 'block' } } }];
    const targets = collectRelationTargets([schema], composites);
    assert.deepEqual(
      targets.map((target) => `${target.kind}:${target.name}`).sort(),
      ['collection:authors'],
    );
  });

  it('finds a single-page target', () => {
    const targets = collectRelationTargets([HOME_SCHEMA], composites);
    assert.deepEqual(
      targets.map((target) => `${target.kind}:${target.name}`).sort(),
      ['collection:authors', 'single_page:home'],
    );
  });
});
