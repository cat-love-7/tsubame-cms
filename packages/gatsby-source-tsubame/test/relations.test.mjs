// The GraphQL side of a relation union: which member a value is, and how a stored element becomes
// the node it names. Both are handed to Gatsby by `gatsby-node.js`; these tests drive them the way
// GraphQL would, with a stand-in for the node model.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import relationsModule from '../src/relations.js';

const { relationUnionResolveType, linkRelationElements } = relationsModule;

const REF = 'TsubameRelationRef';
const resolveType = relationUnionResolveType(REF);

/** A node as the node model hands it back: a plain object with its own type. */
function node(type, id) {
  return { id, internal: { type } };
}

describe('relationUnionResolveType', () => {
  it('answers the node type of a linked element', () => {
    assert.equal(resolveType(node('TsubameAuthorsItem', 'node:authors:7')), 'TsubameAuthorsItem');
    assert.equal(resolveType(node('TsubameHomePage', 'node:home')), 'TsubameHomePage');
  });

  it('answers the reference type for an element whose target is not part of the build', () => {
    // Gatsby's own default reads `node.internal.type`, which is undefined here.
    assert.equal(resolveType({ target: 'nowhere', item: 5, kind: 'collection' }), REF);
    assert.equal(resolveType({ target: 'home', item: null, kind: 'single_page' }), REF);
  });

  it('answers the reference type for a value that is not an object', () => {
    assert.equal(resolveType(null), REF);
    assert.equal(resolveType(undefined), REF);
    assert.equal(resolveType('node:tsubame-item:authors:7'), REF);
  });
});

describe('linkRelationElements', () => {
  const item = {
    related: ['node:tsubame-item:authors:7', { target: 'nowhere', item: 5, kind: 'collection' }],
  };
  const context = {
    nodeModel: {
      getNodeById: ({ id }) => (id === 'node:tsubame-item:authors:7' ? node('TsubameAuthorsItem', id) : undefined),
    },
  };
  const info = { fieldName: 'related' };

  it('answers the node an id names, in the order the value was written in', () => {
    const source = { related: ['node:a', 'node:b'] };
    const withBoth = {
      nodeModel: {
        getNodeById: ({ id }) => node(id === 'node:b' ? 'TsubameEditorsItem' : 'TsubameAuthorsItem', id),
      },
    };
    assert.deepEqual(linkRelationElements(source, undefined, withBoth, info), [
      node('TsubameAuthorsItem', 'node:a'),
      node('TsubameEditorsItem', 'node:b'),
    ]);
  });

  it('serves a reference as it is, beside the linked elements', () => {
    assert.deepEqual(linkRelationElements(item, undefined, context, info), [
      node('TsubameAuthorsItem', 'node:tsubame-item:authors:7'),
      { target: 'nowhere', item: 5, kind: 'collection' },
    ]);
  });

  it('drops an id whose node is gone, the way @link drops it', () => {
    // The target was unpublished between reading the schema and reading the values: the element
    // cannot be served as a node, and the object it named is not there to fall back to.
    const source = { related: ['node:tsubame-item:authors:7', 'node:tsubame-item:authors:8'] };
    assert.deepEqual(linkRelationElements(source, undefined, context, info), [
      node('TsubameAuthorsItem', 'node:tsubame-item:authors:7'),
    ]);
  });

  it('answers an empty list for an empty value, and null for a missing one', () => {
    assert.deepEqual(linkRelationElements({ related: [] }, undefined, context, info), []);
    assert.equal(linkRelationElements({}, undefined, context, info), null);
  });

  it('says which page read the node, so a change to it re-runs that page', () => {
    // `@link` passes the path for the same reason (`gatsby/src/schema/resolvers.ts`): without it
    // Gatsby does not know the page depends on the node, and `gatsby develop` keeps serving the
    // old data after the node changes.
    const calls = [];
    const recording = {
      path: '/blog/hello/',
      nodeModel: {
        getNodeById: (args, pageDependencies) => {
          calls.push([args, pageDependencies]);
          return node('TsubameAuthorsItem', args.id);
        },
      },
    };
    linkRelationElements({ related: ['node:tsubame-item:authors:7'] }, undefined, recording, info);
    assert.deepEqual(calls, [[{ id: 'node:tsubame-item:authors:7' }, { path: '/blog/hello/' }]]);
  });

  it('leaves a value that is not a list alone, which is what the default resolver would do', () => {
    const reference = { target: 'nowhere', item: 5, kind: 'collection' };
    assert.deepEqual(linkRelationElements({ related: reference }, undefined, context, info), reference);
  });
});
