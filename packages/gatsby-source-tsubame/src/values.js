'use strict';

const { describeFieldType, imageKey, isRelationArray, normalizeImageValue, toImageValue, toCompositeValue, toRelationValue } = require('./fields');
const { ownerNodeKey, relationTargetTypeName } = require('./model');

/**
 * A value, shaped for the GraphQL type the model declared for it.
 *
 * The walk is recursive because a value can be: an array of composites, a composite holding a
 * relation and a Markdown field, a composite inside a composite. Every level is resolved against
 * the *schema* of that level - the collection's, then the composite definition's - because the
 * value itself has no type tags.
 *
 * Markdown is the one part with a side effect: a Markdown value becomes a child node of the item
 * (see `planMarkdown`), so what this function puts in the field is that node's id. Markdown inside
 * a composite is what `/api/content/composite-fields` newly makes reachable - without the
 * definition there was no way to know that a string under `blocks[0].text` was Markdown at all.
 *
 * @param {object} fieldTypeDescriptor the field's `field_type`, as the CMS sends it
 * @param {*} value the raw value
 * @param {object} context `{ model, createNodeId, planMarkdown, resolveUrl, topField, path }`
 */
function resolveFieldValue(fieldTypeDescriptor, value, context) {
  const { kind, options } = describeFieldType(fieldTypeDescriptor);
  switch (kind) {
    case 'Markdown':
      // An empty string is a value, not a missing one: a node with empty content keeps
      // `body { childMarkdownRemark { html } }` from failing on a half-written item.
      return typeof value === 'string' ? context.planMarkdown(context.path, value, context.topField) : null;
    case 'Array': {
      if (!Array.isArray(value)) {
        return [];
      }
      // Several references are an `Array` whose item types are all relations. Their elements may
      // name different targets, so they are resolved as references and not through one item type.
      if (isRelationArray(fieldTypeDescriptor)) {
        return resolveRelationArray(options, value, context);
      }
      // A mixed list stays the JSON the API sent; the model declared it `JSON` for the same reason.
      if (!Array.isArray(options) || options.length !== 1) {
        return value;
      }
      return value.map((entry, index) =>
        resolveFieldValue(options[0], entry, { ...context, path: `${context.path}.${index}` }),
      );
    }
    case 'CompositeField':
      return resolveComposite(value, context);
    case 'Relation':
      return resolveRelation(fieldTypeDescriptor, value, context);
    case 'Image':
      return toImageValue(value, context);
    case 'Text':
    case 'Slug':
      return typeof value === 'string' ? value : null;
    case 'Number':
      return typeof value === 'number' ? value : null;
    case 'Boolean':
      return typeof value === 'boolean' ? value : null;
    case 'Date':
    case 'DateTime':
      return typeof value === 'string' ? value : null;
    case 'TextEnum':
      return Array.isArray(value) ? value.filter((entry) => typeof entry === 'string') : [];
    default:
      return value === undefined ? null : value;
  }
}

/**
 * A composite value: the plugin's `id`/`values`, plus a typed field per field the definition
 * declares.
 *
 * A definition that is not part of the build (an id no `/api/content/composite-fields` entry
 * answers) leaves the object as `{id, values}`, which is what the opaque fallback type declares.
 */
function resolveComposite(value, context) {
  const composite = toCompositeValue(value);
  if (composite === null) {
    return null;
  }

  const result = { id: composite.id, values: composite.values };
  const definition = context.model.snapshot.composites.get(composite.id);
  const mapping = context.model.fieldNames.composites.get(composite.id);
  if (definition === undefined || mapping === undefined) {
    return result;
  }

  for (const field of definition) {
    const originalName = String(field.name);
    const graphqlName = mapping.get(originalName);
    if (graphqlName === undefined) {
      continue;
    }
    result[graphqlName] = resolveFieldValue(field.field_type, composite.values[originalName], {
      ...context,
      path: `${context.path}.${originalName}`,
    });
  }
  return result;
}

/**
 * One relation: the id of the node it points at, so `@link` can resolve it.
 *
 * A target that was not published is simply not a node, and `@link` answers null for it rather than
 * failing the build - which is the honest answer to "show me the author of this article" when the
 * author is not on the site yet. The reference itself (target and item id) stays readable under
 * `values`.
 */
function resolveRelation(fieldTypeDescriptor, value, context) {
  const { options } = describeFieldType(fieldTypeDescriptor);
  const relation = options === null || typeof options !== 'object' ? {} : options;

  if (relationTargetTypeName(context.model, relation.target) === null) {
    return toRelationValue(value);
  }
  if (
    value === null ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    typeof value.target !== 'string'
  ) {
    return null;
  }
  return relationNodeId(value, context.createNodeId);
}

/**
 * An `Array` of relations: one node id per element.
 *
 * One item type means one target, which is the one type a GraphQL list can hold. An array that
 * declares several targets has no single node type, so it keeps the reference shape - each element
 * carries its own target, and a consumer reads it rather than following `@link`.
 */
function resolveRelationArray(options, value, context) {
  if (options.length !== 1) {
    return toRelationValue(value);
  }
  const relation = describeFieldType(options[0]).options ?? {};
  if (relationTargetTypeName(context.model, relation.target) === null) {
    return toRelationValue(value);
  }
  return value
    .filter((reference) => reference !== null && typeof reference === 'object' && typeof reference.target === 'string')
    .map((reference) => relationNodeId(reference, context.createNodeId));
}

/**
 * The Gatsby node id of the content a reference points at.
 *
 * The same key `nodes.js` creates the node with: the two have to be written from one place in
 * spirit, and this is that place's mirror - a reference with an `item` is a collection item, one
 * without is a single page, whose identity is its name.
 */
function relationNodeId(reference, createNodeId) {
  return createNodeId(ownerNodeKey(referenceDescriptor(reference)));
}

/** A reference as the content it points at: an item id, or a page name. */
function referenceDescriptor(reference) {
  return {
    kind: typeof reference.item === 'number' ? 'collection' : 'single_page',
    name: reference.target,
    item: typeof reference.item === 'number' ? reference.item : null,
  };
}

/**
 * Walk a content type's values, through composites and arrays, handing each relation and image
 * value to `onValue`.
 *
 * Two things are read out of a value tree before the nodes are built - the content it references
 * (for the reverse index) and the images it holds (for downloading them) - and both need the same
 * walk. Going through composites and arrays is not optional for either: the delivery API's own
 * reverse lookup walks the whole value, and a Markdown body can show an image that sits inside a
 * block.
 */
function walkValues(schema, values, composites, onValue) {
  const visitType = (fieldType, value) => {
    const { kind, options } = describeFieldType(fieldType);
    if (kind === 'Relation' || kind === 'Image') {
      onValue(kind, value);
      return;
    }
    if (kind === 'CompositeField') {
      const composite = toCompositeValue(value);
      if (composite === null || composites === undefined) {
        return;
      }
      const definition = composites.get(composite.id);
      if (definition === undefined) {
        return;
      }
      for (const field of definition) {
        visitType(field.field_type, composite.values[String(field.name)]);
      }
      return;
    }
    if (kind === 'Array') {
      if (!Array.isArray(value) || !Array.isArray(options)) {
        return;
      }
      // An array of relations *is* the relation value: its elements carry their own targets, so the
      // whole array goes to `onValue` rather than being read as one item type per entry.
      if (isRelationArray(fieldType)) {
        onValue('Relation', value);
        return;
      }
      if (options.length !== 1) {
        return;
      }
      for (const entry of value) {
        visitType(options[0], entry);
      }
    }
  };

  const typedValues = values !== null && typeof values === 'object' ? values : {};
  for (const field of Array.isArray(schema) ? schema : []) {
    visitType(field.field_type, typedValues[String(field.name)]);
  }
}

/**
 * Every piece of content the values point at, wherever the reference sits.
 *
 * The reverse index is built from this: a content type declares a name for a relation
 * (`inverse_name`), and this says which targets the content actually holds. The walk goes through
 * composites and arrays because the delivery API's own reverse lookup does
 * (`DeliveryMaps::holds_reference` walks the whole value), so a reference inside a composite still
 * puts the item on the target's list.
 *
 * @returns {Array<{kind: string, name: string, item: number|null}>} one entry per target, deduped
 */
function collectReferenceKeys(schema, values, composites) {
  const found = new Map();
  walkValues(schema, values, composites, (kind, value) => {
    if (kind !== 'Relation') {
      return;
    }
    // One relation is one object; an array of relations is an array of them.
    for (const reference of Array.isArray(value) ? value : [value]) {
      if (reference !== null && typeof reference === 'object' && typeof reference.target === 'string') {
        const descriptor = referenceDescriptor(reference);
        found.set(ownerNodeKey(descriptor), descriptor);
      }
    }
  });
  return [...found.values()];
}

/**
 * Every image the values hold, wherever it sits, deduped by id.
 *
 * This is what gets downloaded when `images.download` is on: the bytes are not in the API response,
 * and `gatsby-transformer-sharp` reads a local file, so each image has to be fetched and given a
 * `File` node before the content that points at it is created.
 *
 * @returns {Array<{id: number|null, url: string}>}
 */
function collectImageValues(schema, values, composites) {
  const found = new Map();
  walkValues(schema, values, composites, (kind, value) => {
    if (kind !== 'Image') {
      return;
    }
    const image = normalizeImageValue(value);
    if (image === null) {
      return;
    }
    const key = imageKey(image);
    if (key !== null) {
      found.set(key, image);
    }
  });
  return [...found.values()];
}

module.exports = {
  resolveFieldValue,
  resolveComposite,
  resolveRelation,
  relationNodeId,
  referenceDescriptor,
  collectReferenceKeys,
  collectImageValues,
  walkValues,
};
