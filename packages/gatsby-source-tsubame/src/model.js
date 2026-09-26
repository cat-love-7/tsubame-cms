'use strict';

const { sanitizeTypeName, sanitizeFieldName, createNameAllocator } = require('./naming');
const { describeFieldType, isRelationArray, relationOptionsOf } = require('./fields');

/**
 * The model one build is read against: which GraphQL type each collection, page and composite
 * definition gets, which GraphQL name each of their fields gets, how a CMS field type maps to a
 * GraphQL type, and which reverse references (`inverse_name`) each type answers to.
 *
 * It is built twice per build - once in `createSchemaCustomization` to declare the schema and once
 * in `sourceNodes` to fill it - from the same `fetchSchemaSnapshot()`, and the two have to agree
 * exactly: a field the declaration renamed but the value did not is a null at runtime, and the
 * other way round is a field that does not exist. Keeping every naming decision in one function
 * over one input is what makes that agreement checkable rather than hopeful.
 */

/** The types that are the same for every project, after the configurable prefix. */
const STATIC_TYPE_SUFFIXES = ['Collection', 'Markdown', 'Image', 'Composite'];

/** The GraphQL names this plugin uses, all built from the configured prefix. */
function graphqlNamesFor(prefix) {
  return {
    collection: `${prefix}Collection`,
    markdown: `${prefix}Markdown`,
    image: `${prefix}Image`,
    composite: `${prefix}Composite`,
  };
}

/** `collection:blog` / `page:home`: the key a schema (or a content type) is looked up by. */
function schemaKey(kind, name) {
  return `${kind}:${name}`;
}

/** `collection:authors` / `page:home`: the key a relation target is looked up by. */
function targetKey(target) {
  return `${target.kind === 'single_page' ? 'page' : 'collection'}:${target.name}`;
}

/**
 * The key a piece of content's node is created with (`tsubame-item:authors:7`, `tsubame-page:home`).
 *
 * `values.js` builds the same key from a reference, which is what lets a reverse reference be
 * answered with `createNodeId` and nothing else.
 */
function ownerNodeKey({ kind, name, item }) {
  return kind === 'collection' ? `tsubame-item:${name}:${item}` : `tsubame-page:${name}`;
}

/** A relation target, with the two kinds spelled the way the rest of the model spells them. */
function normalizeTarget(target) {
  if (target === null || typeof target !== 'object' || typeof target.name !== 'string') {
    return null;
  }
  return { kind: target.kind === 'single_page' ? 'single_page' : 'collection', name: target.name };
}

/**
 * Which GraphQL type each collection, page and composite definition uses.
 *
 * All three are allocated from one namespace, in a fixed order (collections, then pages, then
 * composite definitions; each sorted), because two of them can want the same name and the
 * allocator's answer depends on who asked first. Sorting is what makes it stable: the API does not
 * promise an order, and a name allocated in a different order would be a different name.
 */
function planTypeNames(collectionNames, pageNames, compositeIds, prefix) {
  const allocator = createNameAllocator(STATIC_TYPE_SUFFIXES.map((suffix) => `${prefix}${suffix}`));

  const collections = new Map();
  for (const name of [...collectionNames].sort()) {
    collections.set(name, allocator.take(`${prefix}${sanitizeTypeName(name)}Item`));
  }

  const pages = new Map();
  for (const name of [...pageNames].sort()) {
    pages.set(name, allocator.take(`${prefix}${sanitizeTypeName(name)}Page`));
  }

  const composites = new Map();
  for (const id of [...compositeIds].sort()) {
    composites.set(id, allocator.take(`${prefix}Composite${sanitizeTypeName(id)}`));
  }

  return { prefix, collections, pages, composites };
}

/**
 * The names a content field may not take, because the plugin already uses them.
 *
 * `children<Markdown>` and `childMarkdownRemark` are what Gatsby itself derives from the
 * parent/child links, so a CMS field of that name would collide with a field the consumer did not
 * ask for but gets anyway. A composite is not a node, but its `id` and `values` are still the
 * plugin's own.
 */
function reservedFieldNames(kind, names) {
  if (kind === 'composite') {
    return ['id', 'values'];
  }
  const reserved = [
    'id',
    'parent',
    'children',
    'internal',
    'fields',
    'values',
    'fieldNames',
    'childMarkdownRemark',
    `child${names.markdown}`,
    `children${names.markdown}`,
  ];
  if (kind === 'item') {
    reserved.push('remoteId', 'collection', 'publishedAt', 'lastPublishedAt');
  } else {
    reserved.push('name', 'publishedAt', 'lastPublishedAt');
  }
  return reserved;
}

/**
 * The GraphQL names of a type's CMS fields and of the reverse references it answers to.
 *
 * The mappings travel with the node inside `fieldNames`, because a query has to be written against
 * the GraphQL name, and `published-at` -> `published_at` is guessable while `お知らせ` -> `Unnamed`
 * is not. An inverse name is a label the CMS lets an editor choose, so it is rewritten the same
 * way; the CMS already guarantees one inverse name per target, and the allocator is what keeps a
 * name from colliding with the target's own fields.
 */
function planTypeFields(kind, schema, inverseNames, names) {
  const allocator = createNameAllocator(reservedFieldNames(kind, names));

  const fields = new Map();
  for (const field of Array.isArray(schema) ? schema : []) {
    const original = String(field.name);
    if (fields.has(original)) {
      continue;
    }
    fields.set(original, allocator.take(sanitizeFieldName(original)));
  }

  const inverses = new Map();
  for (const inverseName of [...inverseNames].sort()) {
    if (inverses.has(inverseName)) {
      continue;
    }
    inverses.set(inverseName, allocator.take(sanitizeFieldName(inverseName)));
  }

  return { fields, inverses };
}

/** The fields of a schema, without any inverse names: what a composite definition needs. */
function planFieldNames(kind, schema, names) {
  return planTypeFields(kind, schema, [], names).fields;
}

function mappingToObject(mapping) {
  return Object.fromEntries(mapping);
}

/** The type of one relation's target, or null when the target is not part of this build. */
function relationTargetTypeName(plan, target) {
  const normalized = normalizeTarget(target);
  if (normalized === null) {
    return null;
  }
  if (normalized.kind === 'single_page') {
    return plan.pages.get(normalized.name) ?? null;
  }
  return plan.collections.get(normalized.name) ?? null;
}

/** The key a planned relation union is looked up by: one field of one type. */
function relationUnionKey(ownerTypeName, fieldName) {
  return `${ownerTypeName}\u0000${fieldName}`;
}

/**
 * The targets a relation field declares, split by whether this build has a node type for them.
 *
 * `typed` is the type name per target (deduped, in the order the field declares them) and
 * `missing` is the targets the CMS does not answer. A collection answers 404 only when the CMS does
 * not have it at all - a collection with no published items still has a schema - and a single page a
 * relation names always gets a type, published or not, so `missing` is a **dangling declaration**:
 * the schema still names a collection somebody deleted, and the delivery API therefore has no
 * reference to serve either (it drops references whose target is not published content).
 */
function relationTargetTypes(plan, fieldType) {
  const typed = [];
  const missing = [];
  for (const options of relationOptionsOf(fieldType)) {
    const target = normalizeTarget(options.target);
    if (target === null) {
      continue;
    }
    const typeName = relationTargetTypeName(plan, target);
    if (typeName === null) {
      missing.push(target);
    } else if (!typed.includes(typeName)) {
      typed.push(typeName);
    }
  }
  return { typed, missing };
}

/**
 * How each relation field is declared, once the targets the CMS does not answer are known.
 *
 * A field with no typed target is **left out** - it cannot be typed, and pretending otherwise (a
 * reference type) would make a site's query fail with a message about the wrong thing. It is
 * reported in `missing` for the caller to warn about, and the raw `values` still hold whatever the
 * delivery API sent.
 *
 * An array that declares several targets is a union of the targets that have a type; with only one
 * left it is a plain list of that type, and the missing target is left out of the field. The union
 * is allocated from the same namespace as the type names (seeded with every name already taken, in
 * sorted order) so it can never take a name a collection, page or composite wants.
 *
 * The mappings handed in are **trimmed in place**, so a field that is left out is in none of the
 * three places that have to agree: the GraphQL type, the node's own fields, and `fieldNames`.
 *
 * @returns {{ unions: Map<string, {name: string, ownerType: string, field: string, members: string[]}>,
 *   missing: Array<{ownerType: string, field: string, target: object, fieldLeftOut: boolean}> }}
 */
function planRelationFields(plan, names, fieldNames, snapshot) {
  const allocator = createNameAllocator([
    ...STATIC_TYPE_SUFFIXES.map((suffix) => `${plan.prefix}${suffix}`),
    ...plan.collections.values(),
    ...plan.pages.values(),
    ...plan.composites.values(),
  ]);
  const unions = new Map();
  const missing = [];

  const consider = (ownerTypeName, schema, mapping) => {
    if (ownerTypeName === undefined || mapping === undefined) {
      return;
    }
    for (const field of Array.isArray(schema) ? schema : []) {
      const originalName = String(field.name);
      const graphqlName = mapping.get(originalName);
      if (graphqlName === undefined) {
        continue;
      }
      const { typed, missing: untyped } = relationTargetTypes(plan, field.field_type);
      if (typed.length === 0 && untyped.length === 0) {
        continue;
      }
      const fieldLeftOut = typed.length === 0;
      if (fieldLeftOut) {
        mapping.delete(originalName);
      }
      for (const target of untyped) {
        missing.push({ ownerType: ownerTypeName, field: graphqlName, target, fieldLeftOut });
      }
      if (typed.length >= 2) {
        unions.set(relationUnionKey(ownerTypeName, graphqlName), {
          name: allocator.take(`${ownerTypeName}${sanitizeTypeName(graphqlName)}`),
          ownerType: ownerTypeName,
          field: graphqlName,
          members: typed,
        });
      }
    }
  };

  // Sorted, for the same reason `planTypeNames` sorts: the API does not promise an order, and the
  // allocator's answer depends on who asked first.
  for (const name of [...snapshot.collections.keys()].sort()) {
    consider(plan.collections.get(name), snapshot.collections.get(name), fieldNames.collections.get(name));
  }
  for (const name of [...plan.pages.keys()].sort()) {
    consider(plan.pages.get(name), snapshot.pages.get(name) ?? [], fieldNames.pages.get(name));
  }
  for (const id of [...snapshot.composites.keys()].sort()) {
    consider(plan.composites.get(id), snapshot.composites.get(id), fieldNames.composites.get(id));
  }

  return { unions, missing };
}

/**
 * The GraphQL type of a field, and whether its value is a link to a node.
 *
 * Five choices are worth stating:
 *
 * - `Number` is `Float`, not `Int`: the CMS stores `f64`, and a GraphQL `Int` would make a value
 *   like `1.5` a query error rather than a value.
 * - `Date` and `DateTime` are the `Date` scalar Gatsby already has, so `formatString` works and the
 *   consumer does not have to parse strings by hand.
 * - A `Relation` field is a link to the target's type, and an `Array` of relations to a list of it
 *   when the item types name one target. A relation field with no typed target is left out of the
 *   schema altogether (`planRelationFields`), so the `JSON` fallback below is unreachable for a
 *   field that was declared - it is what a caller that asks about such a field in isolation gets.
 * - An `Array` of relations that names several targets is a **union** of those targets' types,
 *   because its elements need not be the same node type; `unionName` is what the model planned for
 *   this field, and the field is still `@link`ed, because every element is a node id and Gatsby
 *   resolves a union of node types by id (it expands the union to its node members). A union
 *   without a planned name is a programming error, and answers `JSON` rather than a wrong type.
 * - A `CompositeField` is typed after its definition once `/api/content/composite-fields` answers
 *   it, and by the opaque `TsubameComposite` otherwise.
 *
 * `JSON` is the last fallback that keeps an unknown or ambiguous field queryable: the whole value
 * is still there, under `values`.
 */
function graphqlFieldType(model, fieldType, unionName) {
  const { kind, options } = describeFieldType(fieldType);
  switch (kind) {
    case 'Text':
    case 'Slug':
      return { type: 'String', link: false };
    case 'Markdown':
      return { type: model.names.markdown, link: true };
    case 'Number':
      return { type: 'Float', link: false };
    case 'Boolean':
      return { type: 'Boolean', link: false };
    case 'Date':
    case 'DateTime':
      return { type: 'Date', link: false };
    case 'Image':
      return { type: model.names.image, link: false };
    case 'TextEnum':
      return { type: '[String]', link: false };
    case 'CompositeField': {
      const id = options === null || typeof options !== 'object' ? undefined : options.id;
      const typeName = typeof id === 'string' ? model.plan.composites.get(id) : undefined;
      return { type: typeName ?? model.names.composite, link: false };
    }
    case 'Relation': {
      const [targetType] = relationTargetTypes(model.plan, fieldType).typed;
      return targetType === undefined
        ? { type: 'JSON', link: false }
        : { type: targetType, link: true };
    }
    case 'Array': {
      if (!Array.isArray(options) || options.length === 0) {
        return { type: 'JSON', link: false };
      }
      // Several references are an `Array` whose item types are all relations. One typed target means
      // one node type, which a list of it can hold; several are the union the model planned, which
      // `@link` resolves element by element.
      if (isRelationArray(fieldType)) {
        const { typed } = relationTargetTypes(model.plan, fieldType);
        if (typed.length === 0) {
          return { type: 'JSON', link: false };
        }
        if (typed.length === 1) {
          return { type: `[${typed[0]}]`, link: true };
        }
        return unionName === undefined
          ? { type: 'JSON', link: false }
          : { type: `[${unionName}]`, link: true };
      }
      // An array with more than one declared item type is left untyped: the server reads each
      // element with the first type that fits, so the elements need not be the same type.
      if (options.length !== 1) {
        return { type: 'JSON', link: false };
      }
      const item = graphqlFieldType(model, options[0]);
      if (item.type === 'JSON') {
        return { type: 'JSON', link: false };
      }
      return { type: `[${item.type}]`, link: item.link };
    }
    default:
      return { type: 'JSON', link: false };
  }
}

/**
 * The reverse references the schemas declare, from both ends.
 *
 * `inverse_name` is written on the *referring* field - `blog.author` says the other side calls it
 * `articles` - and the CMS keeps it unique per target, so a target answers to one name from one
 * schema. That is what makes an inverse field a list of one concrete type rather than a union.
 *
 * Only a **top-level** relation field declares an inverse, which is the rule the delivery API's
 * `?populate=<inverse_name>` follows too (`ContentReader::declared_inverses` reads the schema's own
 * fields, and `inverse_references` matches them). An `Array` of relations is one of them: each item
 * type declares its own target, so it contributes one declaration. A relation inside a composite
 * definition is not a declaration: a composite can be embedded by several collections, so "who is
 * referring" would have more than one answer.
 *
 * - `bySchema`: what a content type declares, for the walk that builds the reverse index.
 * - `byTarget`: what a target answers to, for the declared fields and their types.
 */
function collectInverseDeclarations(snapshot, plan) {
  const bySchema = new Map();
  const byTarget = new Map();

  const consider = (schema, declaringSchemaKey, declaringTypeName) => {
    if (declaringTypeName === undefined) {
      return;
    }
    const declarations = [];
    for (const field of Array.isArray(schema) ? schema : []) {
      for (const options of relationOptionsOf(field.field_type)) {
        const inverseName = typeof options.inverse_name === 'string' ? options.inverse_name.trim() : '';
        const target = normalizeTarget(options.target);
        if (inverseName === '' || target === null) {
          continue;
        }
        declarations.push({ inverseName, target });
        const key = targetKey(target);
        const declared = byTarget.get(key) ?? [];
        declared.push({ inverseName, declaringTypeName });
        byTarget.set(key, declared);
      }
    }
    if (declarations.length > 0) {
      bySchema.set(declaringSchemaKey, declarations);
    }
  };

  for (const [name, schema] of snapshot.collections) {
    consider(schema, schemaKey('collection', name), plan.collections.get(name));
  }
  for (const [name, schema] of snapshot.pages) {
    consider(schema, schemaKey('page', name), plan.pages.get(name));
  }

  for (const [key, declared] of byTarget) {
    const seen = new Set();
    byTarget.set(
      key,
      declared
        .filter((entry) => {
          const identity = `${entry.inverseName}\u0000${entry.declaringTypeName}`;
          if (seen.has(identity)) {
            return false;
          }
          seen.add(identity);
          return true;
        })
        .sort((left, right) =>
          left.inverseName === right.inverseName
            ? left.declaringTypeName.localeCompare(right.declaringTypeName)
            : left.inverseName.localeCompare(right.inverseName),
        ),
    );
  }

  return { bySchema, byTarget };
}

/**
 * The `fieldNames` a content node carries: its CMS fields, plus the reverse references it answers
 * to. A name already taken by a CMS field keeps that field's entry (the allocator gave the inverse
 * a suffix, so the two are still separate GraphQL fields; only the lookup table is shadowed, and
 * only when a schema names an inverse exactly like one of the target's own fields).
 */
function contentFieldNames(model, kind, name) {
  const mapping = kind === 'collection' ? model.fieldNames.collections.get(name) : model.fieldNames.pages.get(name);
  const result = Object.fromEntries(mapping ?? new Map());
  const inverses = model.inverseFieldNames.get(schemaKey(kind, name)) ?? new Map();
  for (const [inverseName, graphqlName] of inverses) {
    if (!(inverseName in result)) {
      result[inverseName] = graphqlName;
    }
  }
  return result;
}

/**
 * The model for one build.
 *
 * `snapshot` is what `TsubameClient.fetchSchemaSnapshot()` read: the collections with published items
 * (the index), every collection and page a relation names (whose schema is public whether or not
 * they have published items), the published pages, the composite definitions, and the single-page
 * targets that are not published and therefore have no public schema.
 */
function buildContentModel(snapshot, options) {
  const names = graphqlNamesFor(options.typePrefix);
  const pageNames = [...new Set([...snapshot.pages.keys(), ...(snapshot.unpublishedPageTargets || [])])];
  const plan = planTypeNames([...snapshot.collections.keys()], pageNames, [...snapshot.composites.keys()], options.typePrefix);
  const { bySchema, byTarget } = collectInverseDeclarations(snapshot, plan);

  const fieldNames = { collections: new Map(), pages: new Map(), composites: new Map() };
  const inverseFieldNames = new Map();

  for (const [name, schema] of snapshot.collections) {
    const declared = byTarget.get(schemaKey('collection', name)) ?? [];
    const planned = planTypeFields(
      'item',
      schema,
      declared.map((entry) => entry.inverseName),
      names,
    );
    fieldNames.collections.set(name, planned.fields);
    inverseFieldNames.set(schemaKey('collection', name), planned.inverses);
  }

  for (const name of plan.pages.keys()) {
    // A page a relation names but the index does not list has no public schema; its type is
    // declared with the plugin fields only, but it still answers to the inverse names declared
    // against it, so those are planned from the target key rather than from a schema.
    const schema = snapshot.pages.get(name) ?? [];
    const declared = byTarget.get(schemaKey('page', name)) ?? [];
    const planned = planTypeFields(
      'page',
      schema,
      declared.map((entry) => entry.inverseName),
      names,
    );
    fieldNames.pages.set(name, planned.fields);
    inverseFieldNames.set(schemaKey('page', name), planned.inverses);
  }

  for (const [id, schema] of snapshot.composites) {
    fieldNames.composites.set(id, planFieldNames('composite', schema, names));
  }

  // Last, because it trims the mappings it is handed: a field whose target the CMS does not answer
  // is left out of the type, the node and `fieldNames`, and the union names are allocated after
  // every type name is known.
  const relationFields = planRelationFields(plan, names, fieldNames, snapshot);

  return {
    options,
    names,
    plan,
    fieldNames,
    inverseFieldNames,
    inverseDeclarationsBySchema: bySchema,
    inverseDeclarationsByTarget: byTarget,
    relationUnions: relationFields.unions,
    missingRelationTargets: relationFields.missing,
    snapshot,
  };
}

module.exports = {
  STATIC_TYPE_SUFFIXES,
  graphqlNamesFor,
  schemaKey,
  targetKey,
  ownerNodeKey,
  normalizeTarget,
  planTypeNames,
  reservedFieldNames,
  planTypeFields,
  planFieldNames,
  mappingToObject,
  relationTargetTypeName,
  relationTargetTypes,
  relationUnionKey,
  planRelationFields,
  graphqlFieldType,
  collectInverseDeclarations,
  contentFieldNames,
  buildContentModel,
};
