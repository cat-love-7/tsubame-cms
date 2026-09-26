// The model both hooks are read against: which type each thing gets, which name each field gets,
// and how a CMS field type becomes a GraphQL type - including the links that make references
// traversable.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import modelModule from '../src/model.js';
import { BLOG_SCHEMA, createModel, createSnapshot } from './fixtures.mjs';

const {
  buildContentModel,
  contentFieldNames,
  graphqlFieldType,
  planTypeNames,
  relationTargetTypeName,
  relationUnionKey,
  reservedFieldNames,
  schemaKey,
} = modelModule;

const model = createModel();

describe('planTypeNames', () => {
  it('names an item type after its collection, a page type after its page, and a composite after its id', () => {
    assert.equal(model.plan.collections.get('blog'), 'TsubameBlogItem');
    assert.equal(model.plan.collections.get('authors'), 'TsubameAuthorsItem');
    assert.equal(model.plan.collections.get('editors'), 'TsubameEditorsItem');
    assert.equal(model.plan.pages.get('home'), 'TsubameHomePage');
    assert.equal(model.plan.pages.get('contact'), 'TsubameContactPage');
    assert.equal(model.plan.composites.get('block'), 'TsubameCompositeBlock');
    assert.equal(model.plan.composites.get('seo'), 'TsubameCompositeSeo');
  });

  it('does not depend on the order the API listed things in', () => {
    const first = planTypeNames(['blog', 'authors'], ['home'], ['block', 'seo'], 'Tsubame');
    const second = planTypeNames(['authors', 'blog'], ['home'], ['seo', 'block'], 'Tsubame');
    assert.deepEqual([...first.collections], [...second.collections]);
    assert.deepEqual([...first.composites], [...second.composites]);
  });

  it('tells apart two names that become the same type', () => {
    const plan = planTypeNames(['a-b', 'a_b'], [], [], 'Tsubame');
    assert.equal(plan.collections.get('a-b'), 'TsubameABItem');
    assert.equal(plan.collections.get('a_b'), 'TsubameABItem_2');
  });

  it('does not hand anything the name of a static type', () => {
    assert.notEqual(planTypeNames(['collection'], [], [], 'Tsubame').collections.get('collection'), 'TsubameCollection');
    assert.notEqual(planTypeNames([], [], ['image'], 'Tsubame').composites.get('image'), 'TsubameImage');
  });
});

describe('reservedFieldNames', () => {
  it('keeps the composite plugin fields out of a definition', () => {
    assert.deepEqual(reservedFieldNames('composite', model.names), ['id', 'values']);
  });
});

describe('buildContentModel', () => {
  it('renames a field that collides with one of the plugin\'s own', () => {
    assert.equal(model.fieldNames.collections.get('blog').get('values'), 'values_2');
  });

  it('rewrites a name GraphQL cannot take', () => {
    assert.equal(model.fieldNames.collections.get('blog').get('body-parts'), 'body_parts');
  });

  it('maps the fields of a composite definition', () => {
    assert.equal(model.fieldNames.composites.get('block').get('text'), 'text');
    assert.equal(model.fieldNames.composites.get('block').get('link'), 'link');
  });

  it('plans a type for a page a relation names but the index does not list', () => {
    assert.equal(model.plan.pages.get('contact'), 'TsubameContactPage');
    // No public schema, so no CMS fields - but it still answers to the inverse declared against it.
    assert.equal(model.fieldNames.pages.get('contact').size, 0);
    assert.equal(model.inverseFieldNames.get(schemaKey('page', 'contact')).get('editors'), 'editors');
  });

  it('accepts a snapshot without the new composite map', () => {
    // The plugin has to survive a CMS that predates the route: an absent map, not an exception.
    const snapshot = { ...createSnapshot(), composites: new Map() };
    const plain = buildContentModel(snapshot, { typePrefix: 'Tsubame' });
    assert.equal(plain.snapshot.composites.size, 0);
  });
});

describe('graphqlFieldType', () => {
  it('maps the scalar types the CMS declares', () => {
    assert.deepEqual(graphqlFieldType(model, { Text: {} }), { type: 'String', link: false });
    assert.deepEqual(graphqlFieldType(model, { Slug: {} }), { type: 'String', link: false });
    assert.deepEqual(graphqlFieldType(model, 'Number'), { type: 'Float', link: false });
    assert.deepEqual(graphqlFieldType(model, 'Boolean'), { type: 'Boolean', link: false });
    assert.deepEqual(graphqlFieldType(model, 'Date'), { type: 'Date', link: false });
    assert.deepEqual(graphqlFieldType(model, 'DateTime'), { type: 'Date', link: false });
    assert.deepEqual(graphqlFieldType(model, 'Image'), { type: 'TsubameImage', link: false });
    assert.deepEqual(graphqlFieldType(model, { TextEnum: ['a'] }), { type: '[String]', link: false });
  });

  it('links a markdown field to its node', () => {
    assert.deepEqual(graphqlFieldType(model, { Markdown: {} }), { type: 'TsubameMarkdown', link: true });
    assert.deepEqual(graphqlFieldType(model, { Array: [{ Markdown: {} }] }), {
      type: '[TsubameMarkdown]',
      link: true,
    });
  });

  it('types a relation after its target, and links it', () => {
    const single = { Relation: { target: { kind: 'collection', name: 'authors' } } };
    const many = { Array: [{ Relation: { target: { kind: 'collection', name: 'authors' } } }] };
    assert.deepEqual(graphqlFieldType(model, single), { type: 'TsubameAuthorsItem', link: true });
    assert.deepEqual(graphqlFieldType(model, many), { type: '[TsubameAuthorsItem]', link: true });
  });

  it('types a page relation, one or several, after the page', () => {
    const single = { Relation: { target: { kind: 'single_page', name: 'home' } } };
    const many = { Array: [{ Relation: { target: { kind: 'single_page', name: 'home' } } }] };
    assert.deepEqual(graphqlFieldType(model, single), { type: 'TsubameHomePage', link: true });
    assert.deepEqual(graphqlFieldType(model, many), { type: '[TsubameHomePage]', link: true });
  });

  it('falls back to the reference type when the target is not part of the build', () => {
    const single = { Relation: { target: { kind: 'collection', name: 'nowhere' } } };
    assert.deepEqual(graphqlFieldType(model, single), { type: 'TsubameRelationRef', link: false });
    assert.deepEqual(graphqlFieldType(model, { Array: [single] }), {
      type: '[TsubameRelationRef]',
      link: false,
    });
  });

  it('types an array that declares several targets as the union planned for the field', () => {
    const relation = {
      Array: [
        { Relation: { target: { kind: 'collection', name: 'authors' } } },
        { Relation: { target: { kind: 'collection', name: 'editors' } } },
      ],
    };
    // With the union the model planned for this field: its elements need not be the same node type.
    assert.deepEqual(graphqlFieldType(model, relation, 'TsubameBlogItemRelated'), {
      type: '[TsubameBlogItemRelated]',
      link: false,
    });
    // Without one - no owner planned a field of this shape - the references themselves are what is
    // left to serve.
    assert.deepEqual(graphqlFieldType(model, relation), { type: '[TsubameRelationRef]', link: false });
  });

  it('types a composite after its definition', () => {
    assert.deepEqual(graphqlFieldType(model, { CompositeField: { id: 'seo' } }), {
      type: 'TsubameCompositeSeo',
      link: false,
    });
    assert.deepEqual(graphqlFieldType(model, { Array: [{ CompositeField: { id: 'block' } }] }), {
      type: '[TsubameCompositeBlock]',
      link: false,
    });
  });

  it('falls back to the opaque composite type for an undefined id', () => {
    assert.deepEqual(graphqlFieldType(model, { CompositeField: { id: 'gone' } }), {
      type: 'TsubameComposite',
      link: false,
    });
  });

  it('falls back to JSON when the elements are not one type', () => {
    assert.deepEqual(graphqlFieldType(model, { Array: ['Number', 'Boolean'] }), { type: 'JSON', link: false });
    assert.deepEqual(graphqlFieldType(model, { Array: [] }), { type: 'JSON', link: false });
    assert.deepEqual(graphqlFieldType(model, { SomethingNew: 1 }), { type: 'JSON', link: false });
  });
});

describe('relationTargetTypeName', () => {
  it('answers the type a relation points at', () => {
    assert.equal(relationTargetTypeName(model.plan, { kind: 'collection', name: 'authors' }), 'TsubameAuthorsItem');
    assert.equal(relationTargetTypeName(model.plan, { kind: 'single_page', name: 'contact' }), 'TsubameContactPage');
    assert.equal(relationTargetTypeName(model.plan, { kind: 'collection', name: 'nowhere' }), null);
  });
});

describe('relationUnions', () => {
  it('plans one per field whose elements need not be the same node type', () => {
    const related = model.relationUnions.get(relationUnionKey('TsubameBlogItem', 'related'));
    // The reference type is always the last member, so an element whose target is not part of the
    // build still has a type to resolve to.
    assert.deepEqual(related, {
      name: 'TsubameBlogItemRelated',
      ownerType: 'TsubameBlogItem',
      field: 'related',
      members: ['TsubameAuthorsItem', 'TsubameEditorsItem', 'TsubameRelationRef'],
    });

    // One target that has no node type leaves the other one and the reference type.
    assert.deepEqual(model.relationUnions.get(relationUnionKey('TsubameBlogItem', 'mentions')).members, [
      'TsubameAuthorsItem',
      'TsubameRelationRef',
    ]);
  });

  it('plans none for a field that is one node type, or for one whose targets are all absent', () => {
    // One item type is a list of that type; nothing to choose between.
    assert.equal(model.relationUnions.has(relationUnionKey('TsubameBlogItem', 'editors')), false);
    assert.equal(model.relationUnions.has(relationUnionKey('TsubameBlogItem', 'author')), false);

    const snapshot = createSnapshot();
    snapshot.collections.set('blog', [
      {
        name: 'related',
        field_type: {
          Array: [
            { Relation: { target: { kind: 'collection', name: 'nowhere' } } },
            { Relation: { target: { kind: 'collection', name: 'gone' } } },
          ],
        },
      },
    ]);
    const plain = buildContentModel(snapshot, { typePrefix: 'Tsubame' });
    assert.equal(plain.relationUnions.size, 0);
  });

  it('plans one for a field inside a composite definition too', () => {
    const snapshot = createSnapshot();
    snapshot.composites.set('block', [
      {
        name: 'related',
        field_type: {
          Array: [
            { Relation: { target: { kind: 'collection', name: 'authors' } } },
            { Relation: { target: { kind: 'single_page', name: 'home' } } },
          ],
        },
      },
    ]);
    const withComposite = buildContentModel(snapshot, { typePrefix: 'Tsubame' });
    assert.deepEqual(withComposite.relationUnions.get(relationUnionKey('TsubameCompositeBlock', 'related')), {
      name: 'TsubameCompositeBlockRelated',
      ownerType: 'TsubameCompositeBlock',
      field: 'related',
      members: ['TsubameAuthorsItem', 'TsubameHomePage', 'TsubameRelationRef'],
    });
  });

  it('does not depend on the order the API listed things in', () => {
    const first = buildContentModel(createSnapshot(), { typePrefix: 'Tsubame' });
    const reversed = createSnapshot();
    reversed.collections = new Map([...reversed.collections].reverse());
    reversed.composites = new Map([...reversed.composites].reverse());
    const second = buildContentModel(reversed, { typePrefix: 'Tsubame' });
    assert.deepEqual([...first.relationUnions], [...second.relationUnions]);
  });
});

describe('inverse declarations', () => {
  it('reads the other side\'s name off the referring schema', () => {
    // `blog.author` says the other side calls it `articles`; `home.featured_author` says `features`.
    assert.deepEqual(model.inverseDeclarationsByTarget.get('collection:authors'), [
      { inverseName: 'articles', declaringTypeName: 'TsubameBlogItem' },
      { inverseName: 'features', declaringTypeName: 'TsubameHomePage' },
    ]);
  });

  it('reads a declaration made by a single page', () => {
    // `editors.homepage` names the page's side `editors`, and the page type is what the referrers
    // are typed as.
    assert.deepEqual(model.inverseDeclarationsByTarget.get('page:contact'), [
      { inverseName: 'editors', declaringTypeName: 'TsubameEditorsItem' },
    ]);
  });

  it('reads a declaration made by an Array of relations, one per declared target', () => {
    // `blog.editors` is an `Array([Relation(editors)])`: each item type declares against its own
    // target, so the array is read the same way one relation is.
    assert.deepEqual(model.inverseDeclarationsByTarget.get('collection:editors'), [
      { inverseName: 'editor_of', declaringTypeName: 'TsubameBlogItem' },
    ]);
    assert.equal(model.inverseFieldNames.get(schemaKey('collection', 'editors')).get('editor_of'), 'editor_of');
  });

  it('ignores a name declared inside a composite definition', () => {
    // `block.link` carries `inverse_name: "blocks"`, but a composite can be embedded by several
    // collections, so it is not a declaration - the same rule the delivery API follows.
    const declared = model.inverseDeclarationsByTarget.get('collection:authors') ?? [];
    assert.equal(declared.some((entry) => entry.inverseName === 'blocks'), false);
    assert.equal(model.inverseFieldNames.get(schemaKey('collection', 'authors')).has('blocks'), false);
  });

  it('plans a GraphQL name for each inverse', () => {
    assert.equal(model.inverseFieldNames.get(schemaKey('collection', 'authors')).get('articles'), 'articles');
    assert.equal(model.inverseFieldNames.get(schemaKey('collection', 'authors')).get('features'), 'features');
    assert.equal(model.inverseFieldNames.get(schemaKey('page', 'contact')).get('editors'), 'editors');
  });

  it('reports them in the node\'s fieldNames beside the CMS fields', () => {
    const names = contentFieldNames(model, 'collection', 'authors');
    assert.equal(names.articles, 'articles');
    assert.equal(names.features, 'features');
    assert.equal(names.name, 'name');
  });
});

describe('the model over the fixture blog schema', () => {
  it('has a mapping for every collection it declares a type for', () => {
    for (const name of model.plan.collections.keys()) {
      assert.ok(model.fieldNames.collections.has(name), name);
    }
    assert.equal(BLOG_SCHEMA.length, model.fieldNames.collections.get('blog').size);
  });
});
