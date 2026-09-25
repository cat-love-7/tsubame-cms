// The one thing a preview needs from the Gatsby side: the names the build would have given the
// fields, produced by the same planner so the two cannot drift.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import adapter from '../src/preview-adapter.js';

const { previewFieldNames, previewCompositeFieldNames } = adapter;

describe('previewFieldNames', () => {
  it('names a field the way the build does', () => {
    const names = previewFieldNames('item', [
      { name: 'title', field_type: { Text: {} } },
      { name: 'published-at', field_type: 'Date' },
      { name: 'お知らせ', field_type: { Text: {} } },
    ]);

    assert.deepEqual(names, {
      title: 'title',
      'published-at': 'published_at',
      お知らせ: 'x____',
    });
  });

  it('makes room for the plugin’s own fields', () => {
    // `values` and `fieldNames` belong to the node, so a CMS field with either name is suffixed -
    // the same answer the build gives.
    const names = previewFieldNames('item', [
      { name: 'values', field_type: { Text: {} } },
      { name: 'id', field_type: 'Number' },
    ]);

    assert.deepEqual(names, { values: 'values_2', id: 'id_2' });
  });

  it('tells two names that rewrite the same way apart, in schema order', () => {
    const names = previewFieldNames('item', [
      { name: 'published-at', field_type: 'Date' },
      { name: 'published.at', field_type: 'Date' },
    ]);

    assert.deepEqual(names, { 'published-at': 'published_at', 'published.at': 'published_at_2' });
  });

  it('follows the type prefix a site configured', () => {
    // The reserved list includes `child<Prefix>Markdown`, so the prefix decides which CMS name has
    // to be suffixed.
    const schema = [{ name: 'childTsubameMarkdown', field_type: { Text: {} } }];
    assert.deepEqual(previewFieldNames('item', schema), {
      childTsubameMarkdown: 'childTsubameMarkdown_2',
    });
    assert.deepEqual(previewFieldNames('item', schema, { typePrefix: 'X' }), {
      childTsubameMarkdown: 'childTsubameMarkdown',
    });
  });
});

describe('previewCompositeFieldNames', () => {
  it('reserves only the composite’s own two fields', () => {
    const names = previewCompositeFieldNames([
      { name: 'id', field_type: { Text: {} } },
      { name: 'values', field_type: { Text: {} } },
      { name: 'caption', field_type: { Text: {} } },
    ]);

    assert.deepEqual(names, { id: 'id_2', values: 'values_2', caption: 'caption' });
  });
});
