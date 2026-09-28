// Resolving a value into what the declared GraphQL type expects: scalars, arrays, images,
// composites (now that their definitions are public), and the references that are links.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import valuesModule from '../src/values.js';
import modelModule from '../src/model.js';
import {
  BLOG_ITEMS,
  BLOG_SCHEMA,
  COMPOSITE_FIELDS,
  HOME_PAGE,
  HOME_SCHEMA,
  createModel,
} from './fixtures.mjs';

const { collectImageValues, collectReferenceKeys, relationNodeId, resolveFieldValue } = valuesModule;
const { ownerNodeKey } = modelModule;

const model = createModel();
const composites = new Map(Object.entries(COMPOSITE_FIELDS));

/**
 * The markdown nodes `planMarkdown` was asked to create, in order.
 *
 * @typedef {{id: string, path: string, raw: string, field: string}} MarkdownPlan
 */

/**
 * A resolve context over the fixture model, recording the markdown nodes it is asked to create.
 *
 * @param {Partial<import('../src/values.js').ResolveContext>} [overrides] context fields to replace
 * @returns {{context: import('../src/values.js').ResolveContext, markdown: MarkdownPlan[]}} the context and the record
 */
function makeContext(overrides = {}) {
  /** @type {MarkdownPlan[]} */
  const markdown = [];
  const context = /** @type {import('../src/values.js').ResolveContext} */ ({
    model,
    /** @param {string} key the node key @returns {string} the node id */
    createNodeId: (key) => `node:${key}`,
    /**
     * @param {string} path the value's full path
     * @param {string} raw the markdown source
     * @param {string} field the top-level CMS field's GraphQL name
     * @returns {string} the markdown node's id
     */
    planMarkdown: (path, raw, field) => {
      const id = `node:tsubame-markdown:owner:${path}`;
      markdown.push({ id, path, raw, field });
      return id;
    },
    /** @param {string} path a path the API returned @returns {string} its absolute URL */
    resolveUrl: (path) => `https://cms.example.com${path}`,
    /** @param {string} path a path under the API prefix @returns {string} its absolute URL */
    resolveApiPath: (path) => `https://cms.example.com/api${path}`,
    topField: 'body',
    path: 'body',
    ...overrides,
  });
  return { context, markdown };
}

const image = {
  id: 3,
  url: '/api/images/logo.png',
  absoluteUrl: 'https://cms.example.com/api/images/logo.png',
  stableUrl: 'https://cms.example.com/api/images/by-id/3',
};

describe('scalars', () => {
  it('answers an empty value with the type of empty value, not undefined', () => {
    const { context } = makeContext();
    assert.equal(resolveFieldValue({ Text: {} }, undefined, context), null);
    assert.equal(resolveFieldValue('Number', undefined, context), null);
    assert.deepEqual(resolveFieldValue({ TextEnum: ['a'] }, undefined, context), []);
    assert.deepEqual(resolveFieldValue({ Array: ['Image'] }, undefined, context), []);
  });

  it('keeps enum choices and drops anything that is not one', () => {
    const { context } = makeContext();
    assert.deepEqual(resolveFieldValue({ TextEnum: ['a', 'b'] }, ['a', 3], context), ['a']);
  });

  it('does not invent a value of the wrong type', () => {
    const { context } = makeContext();
    assert.equal(resolveFieldValue('Number', '4.5', context), null);
    assert.equal(resolveFieldValue('Boolean', 'true', context), null);
  });
});

describe('images', () => {
  it('resolves the API path and keeps the stable link', () => {
    const { context } = makeContext();
    assert.deepEqual(resolveFieldValue('Image', { id: 3, url: '/api/images/logo.png' }, context), image);
  });

  it('maps an array of images through the element type', () => {
    const { context } = makeContext();
    assert.deepEqual(resolveFieldValue({ Array: ['Image'] }, [{ id: 3, url: '/api/images/logo.png' }], context), [
      image,
    ]);
  });
});

describe('markdown', () => {
  it('becomes a child node and the field holds its id', () => {
    const { context, markdown } = makeContext();
    assert.equal(resolveFieldValue({ Markdown: {} }, '# Hello', context), 'node:tsubame-markdown:owner:body');
    assert.deepEqual(markdown, [{ id: 'node:tsubame-markdown:owner:body', path: 'body', raw: '# Hello', field: 'body' }]);
  });

  it('keeps an empty string as an empty node', () => {
    const { context } = makeContext();
    assert.equal(resolveFieldValue({ Markdown: {} }, '', context), 'node:tsubame-markdown:owner:body');
  });

  it('gives each element of an array its own node and path', () => {
    const { context, markdown } = makeContext({ path: 'body-parts', topField: 'body-parts' });
    assert.deepEqual(resolveFieldValue({ Array: [{ Markdown: {} }] }, ['one', 'two'], context), [
      'node:tsubame-markdown:owner:body-parts.0',
      'node:tsubame-markdown:owner:body-parts.1',
    ]);
    assert.deepEqual(
      markdown.map((entry) => entry.path),
      ['body-parts.0', 'body-parts.1'],
    );
    assert.deepEqual(
      markdown.map((entry) => entry.field),
      ['body-parts', 'body-parts'],
    );
  });

  it('leaves a mixed array as JSON', () => {
    const { context } = makeContext();
    assert.deepEqual(resolveFieldValue({ Array: ['Number', 'Boolean'] }, [1, true], context), [1, true]);
  });
});

describe('relations', () => {
  const AUTHOR = { Relation: { target: { kind: 'collection', name: 'authors' } } };

  it('answers the id of the target node for one reference', () => {
    const { context } = makeContext();
    assert.equal(
      resolveFieldValue(AUTHOR, { target: 'authors', item: 7 }, context),
      'node:tsubame-item:authors:7',
    );
  });

  it('answers a list of ids for an Array of relations', () => {
    const { context } = makeContext();
    assert.deepEqual(resolveFieldValue({ Array: [AUTHOR] }, [{ target: 'authors', item: 7 }], context), [
      'node:tsubame-item:authors:7',
    ]);
  });

  it('keeps the order an array of references was written in', () => {
    // The order is the value since 2026-09: the CMS stores the editor's order and serves it, so the
    // ids have to come out in that order - sorting by id here would silently turn "featured
    // articles, in this order" into "articles, by id".
    const { context } = makeContext();
    assert.deepEqual(
      resolveFieldValue({ Array: [AUTHOR] }, [{ target: 'authors', item: 9 }, { target: 'authors', item: 7 }], context),
      ['node:tsubame-item:authors:9', 'node:tsubame-item:authors:7'],
    );
  });

  it('answers a page node id for a page reference, which has no item id', () => {
    const { context } = makeContext();
    const fieldType = { Relation: { target: { kind: 'single_page', name: 'home' } } };
    assert.equal(resolveFieldValue(fieldType, { target: 'home' }, context), 'node:tsubame-page:home');
  });

  it('answers null for one relation with no reference, and an empty list for an array of them', () => {
    const { context } = makeContext();
    assert.equal(resolveFieldValue(AUTHOR, null, context), null);
    assert.deepEqual(resolveFieldValue({ Array: [AUTHOR] }, [], context), []);
  });

  it('answers the id of a reference even when its target has no type in this build', () => {
    // A field like this is left out of the schema (`src/model.js`), so no value is ever resolved for
    // it; the resolver stays total rather than guessing, and the raw value is what a reader uses.
    const { context } = makeContext();
    const single = { Relation: { target: { kind: 'collection', name: 'nowhere' } } };
    assert.equal(
      resolveFieldValue(single, { target: 'nowhere', item: 2 }, context),
      'node:tsubame-item:nowhere:2',
    );
    assert.deepEqual(resolveFieldValue({ Array: [single] }, [{ target: 'nowhere', item: 2 }], context), [
      'node:tsubame-item:nowhere:2',
    ]);
  });

  it('links each element of an array that declares several targets, by its own target', () => {
    // The field is a union of the two node types and every element is the id of the node it names:
    // `@link` resolves a union of node types by id, one element at a time.
    const { context } = makeContext();
    const fieldType = {
      Array: [AUTHOR, { Relation: { target: { kind: 'collection', name: 'editors' } } }],
    };
    assert.deepEqual(
      resolveFieldValue(
        fieldType,
        [{ target: 'authors', item: 7 }, { target: 'editors', item: 2 }],
        context,
      ),
      ['node:tsubame-item:authors:7', 'node:tsubame-item:editors:2'],
    );
  });

  it('answers an id for an element the delivery API would not have served at all', () => {
    // A reference to a target the CMS does not answer is dropped by the delivery API before the
    // plugin sees it; a value left behind by an older schema is still read the same way, so a stale
    // element cannot make the build fail. The field is a list of the target that does have a type.
    const { context } = makeContext();
    const fieldType = {
      Array: [AUTHOR, { Relation: { target: { kind: 'collection', name: 'nowhere' } } }],
    };
    assert.deepEqual(
      resolveFieldValue(
        fieldType,
        [{ target: 'authors', item: 7 }, { target: 'nowhere', item: 5 }],
        context,
      ),
      ['node:tsubame-item:authors:7', 'node:tsubame-item:nowhere:5'],
    );
  });
});

describe('relationNodeId', () => {
  it('uses the same key the node was created with', () => {
    /** @param {string} key the node key @returns {string} the node id */
    const createNodeId = (key) => `node:${key}`;
    assert.equal(relationNodeId({ target: 'authors', item: 7 }, createNodeId), 'node:tsubame-item:authors:7');
    assert.equal(relationNodeId({ target: 'home' }, createNodeId), 'node:tsubame-page:home');
  });
});

describe('collectReferenceKeys', () => {
  it('finds the content a set of values points at, composites included', () => {
    const found = collectReferenceKeys(BLOG_SCHEMA, BLOG_ITEMS[0].values, composites);
    // `mentions` names a target the build has no node type for; the reference is still a target the
    // index has to know about.
    assert.deepEqual(found.map(ownerNodeKey).sort(), [
      'tsubame-item:authors:7',
      'tsubame-item:editors:2',
      'tsubame-item:nowhere:5',
    ]);
  });

  it('answers each target once, however many fields point at it', () => {
    // The same author is named by `author` and by a block's `link`.
    const found = collectReferenceKeys(BLOG_SCHEMA, BLOG_ITEMS[0].values, composites);
    assert.equal(found.filter((entry) => ownerNodeKey(entry) === 'tsubame-item:authors:7').length, 1);
  });

  it('names a page by its name, without an item id', () => {
    const found = collectReferenceKeys(HOME_SCHEMA, HOME_PAGE.values, composites);
    assert.deepEqual(found.map(ownerNodeKey).sort(), ['tsubame-item:authors:7', 'tsubame-page:home']);
  });

  it('answers nothing for content with no references', () => {
    assert.deepEqual(collectReferenceKeys(BLOG_SCHEMA, BLOG_ITEMS[1].values, composites), []);
  });
});

describe('collectImageValues', () => {
  it('finds images wherever they sit, deduped by id', () => {
    // A top-level image, one in an array, and one inside a composite.
    const found = collectImageValues(BLOG_SCHEMA, BLOG_ITEMS[0].values, composites);
    assert.deepEqual(found.map((image) => image.id).sort(), [3, 4, 9]);
  });

  it('answers nothing for content with no images', () => {
    assert.deepEqual(collectImageValues(BLOG_SCHEMA, BLOG_ITEMS[2].values, composites), []);
  });
});

describe('composites', () => {
  it('types the definition\'s fields beside the raw values', () => {
    const { context } = makeContext({ path: 'seo', topField: 'seo' });
    const value = { id: 'seo', values: { description: 'about hello', og_image: { id: 9, url: '/api/images/og.png' } } };
    assert.deepEqual(resolveFieldValue({ CompositeField: { id: 'seo' } }, value, context), {
      id: 'seo',
      values: { description: 'about hello', og_image: { id: 9, url: '/api/images/og.png' } },
      description: 'about hello',
      og_image: {
        id: 9,
        url: '/api/images/og.png',
        absoluteUrl: 'https://cms.example.com/api/images/og.png',
        stableUrl: 'https://cms.example.com/api/images/by-id/9',
      },
    });
  });

  it('reaches a markdown field and a relation inside a composite', () => {
    const { context, markdown } = makeContext({ path: 'blocks.0', topField: 'blocks' });
    const value = {
      id: 'block',
      values: {
        text: 'First **block**',
        caption: 'cap',
        link: { target: 'authors', item: 7 },
        children: [{ id: 'block', values: { text: 'child text', caption: 'child cap' } }],
      },
    };

    const resolved = resolveFieldValue({ CompositeField: { id: 'block' } }, value, context);

    assert.equal(resolved.text, 'node:tsubame-markdown:owner:blocks.0.text');
    assert.equal(resolved.caption, 'cap');
    assert.equal(resolved.link, 'node:tsubame-item:authors:7');
    assert.equal(resolved.children.length, 1);
    assert.equal(resolved.children[0].text, 'node:tsubame-markdown:owner:blocks.0.children.0.text');
    assert.equal(resolved.children[0].caption, 'child cap');
    assert.equal(resolved.children[0].link, null);
    assert.deepEqual(resolved.children[0].children, []);
    assert.deepEqual(
      markdown.map((entry) => entry.path),
      ['blocks.0.text', 'blocks.0.children.0.text'],
    );
  });

  it('keeps an unknown definition as the untyped value', () => {
    const { context } = makeContext({ path: 'gone', topField: 'gone' });
    assert.deepEqual(resolveFieldValue({ CompositeField: { id: 'gone' } }, { id: 'gone', values: { a: 1 } }, context), {
      id: 'gone',
      values: { a: 1 },
    });
  });
});
