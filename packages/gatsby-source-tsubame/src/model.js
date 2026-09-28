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

/**
 * The GraphQL names this plugin uses, all built from the configured prefix.
 *
 * @typedef {object} GraphqlNames
 * @property {string} collection the collection metadata type
 * @property {string} markdown the markdown node type
 * @property {string} image the image value type
 * @property {string} composite the opaque composite value type
 */

/**
 * One type's CMS field name -> GraphQL field name mapping.
 *
 * @typedef {Map<string, string>} FieldMapping
 */

/**
 * The mappings every type carries inside its `fieldNames`.
 *
 * @typedef {object} FieldNameMappings
 * @property {Map<string, FieldMapping>} collections by collection name
 * @property {Map<string, FieldMapping>} pages by page name
 * @property {Map<string, FieldMapping>} composites by definition id
 */

/**
 * Which GraphQL type each collection, page and composite definition uses.
 *
 * @typedef {object} ContentPlan
 * @property {string} prefix the configured type prefix
 * @property {Map<string, string>} collections by collection name
 * @property {Map<string, string>} pages by page name
 * @property {Map<string, string>} composites by definition id
 */

/**
 * The options the model reads: the type prefix, and whether images are downloaded.
 *
 * @typedef {object} ModelOptions
 * @property {string} typePrefix prefix of every generated type
 * @property {{download?: boolean}} [images] the image options
 */

/**
 * One content type or definition's identity, keyed the way the node's owner key is built.
 *
 * @typedef {object} OwnerDescriptor
 * @property {'collection' | 'single_page'} kind which kind of content it is
 * @property {string} name the collection's or page's name
 * @property {number | null} item the item's id, null for a single page
 */

/**
 * One reverse reference a referring schema declares.
 *
 * @typedef {object} InverseDeclaration
 * @property {string} inverseName the name the target answers to
 * @property {import('./fields.js').RelationTarget} target the content the declaration is against
 */

/**
 * One reverse reference as the target reads it.
 *
 * @typedef {object} DeclaredInverse
 * @property {string} inverseName the name the target answers to
 * @property {string} declaringTypeName the GraphQL type of the referrers
 */

/**
 * A union planned for one relation field whose elements need not be one node type.
 *
 * @typedef {object} RelationUnion
 * @property {string} name the union's GraphQL name
 * @property {string} ownerType the type the field belongs to
 * @property {string} field the field's GraphQL name
 * @property {string[]} members the member node types
 */

/**
 * A relation target the CMS does not answer, reported for a warning.
 *
 * @typedef {object} MissingRelationTarget
 * @property {string} ownerType the type the field belongs to
 * @property {string} field the field's GraphQL name
 * @property {import('./fields.js').RelationTarget} target the declared target
 * @property {boolean} fieldLeftOut whether the whole field was left out
 */

/**
 * What `TsubameClient.fetchSchemaSnapshot()` read: the input the model is built from.
 *
 * @typedef {object} SchemaSnapshot
 * @property {string[]} collectionNames the collections with published items
 * @property {string[]} pageNames the published single pages
 * @property {Map<string, import('./fields.js').SchemaField[]>} collections by collection name
 * @property {Map<string, import('./fields.js').SchemaField[]>} pages by page name
 * @property {Map<string, import('./fields.js').SchemaField[]>} composites by definition id
 * @property {string[]} unpublishedPageTargets single-page targets with no public schema
 */

/**
 * The GraphQL type of a field, and whether its value is a link to a node.
 *
 * @typedef {object} GraphqlFieldType
 * @property {string} type the GraphQL type
 * @property {boolean} link whether `@link` resolves the value
 */

/**
 * The model one build is read against.
 *
 * @typedef {object} ContentModel
 * @property {ModelOptions} options the options the model was built with
 * @property {GraphqlNames} names the plugin's own GraphQL names
 * @property {ContentPlan} plan which type each collection, page and definition gets
 * @property {FieldNameMappings} fieldNames each type's CMS field name -> GraphQL name
 * @property {Map<string, FieldMapping>} inverseFieldNames each type's inverse name -> GraphQL name
 * @property {Map<string, InverseDeclaration[]>} inverseDeclarationsBySchema what each schema declares
 * @property {Map<string, DeclaredInverse[]>} inverseDeclarationsByTarget what each target answers to
 * @property {Map<string, RelationUnion>} relationUnions by owner type and field
 * @property {MissingRelationTarget[]} missingRelationTargets the targets the CMS does not answer
 * @property {SchemaSnapshot} snapshot the schema the model was built from
 */

/** The types that are the same for every project, after the configurable prefix. */
const STATIC_TYPE_SUFFIXES = ['Collection', 'Markdown', 'Image', 'Composite'];

/**
 * The GraphQL names this plugin uses, all built from the configured prefix.
 *
 * @param {string} prefix the configured type prefix
 * @returns {GraphqlNames} the plugin's own GraphQL names
 */
function graphqlNamesFor(prefix) {
  return {
    collection: `${prefix}Collection`,
    markdown: `${prefix}Markdown`,
    image: `${prefix}Image`,
    composite: `${prefix}Composite`,
  };
}

/**
 * `collection:blog` / `page:home`: the key a schema (or a content type) is looked up by.
 *
 * @param {string} kind `collection` or `page`
 * @param {string} name the collection's or page's name
 * @returns {string} the lookup key
 */
function schemaKey(kind, name) {
  return `${kind}:${name}`;
}

/**
 * `collection:authors` / `page:home`: the key a relation target is looked up by.
 *
 * @param {import('./fields.js').RelationTarget} target the relation target
 * @returns {string} the lookup key
 */
function targetKey(target) {
  return `${target.kind === 'single_page' ? 'page' : 'collection'}:${target.name}`;
}

/**
 * The key a piece of content's node is created with (`tsubame-item:authors:7`, `tsubame-page:home`).
 *
 * `values.js` builds the same key from a reference, which is what lets a reverse reference be
 * answered with `createNodeId` and nothing else.
 *
 * @param {OwnerDescriptor} owner the collection item or single page
 * @returns {string} the node key
 */
function ownerNodeKey({ kind, name, item }) {
  return kind === 'collection' ? `tsubame-item:${name}:${item}` : `tsubame-page:${name}`;
}

/**
 * A relation target, with the two kinds spelled the way the rest of the model spells them.
 *
 * @param {import('./fields.js').WireRelationTarget | null | undefined} target the unchecked target
 * @returns {import('./fields.js').RelationTarget | null} the normalized target, or null
 */
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
 *
 * @param {Iterable<string>} collectionNames the collections
 * @param {Iterable<string>} pageNames the single pages
 * @param {Iterable<string>} compositeIds the composite definition ids
 * @param {string} prefix the configured type prefix
 * @returns {ContentPlan} which type each thing got
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
 *
 * @param {'item' | 'page' | 'composite'} kind which kind of thing the fields belong to
 * @param {GraphqlNames} names the plugin's own GraphQL names
 * @returns {string[]} the CMS field names the plugin does not let a schema use
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
 *
 * @param {'item' | 'page' | 'composite'} kind which kind of thing the fields belong to
 * @param {import('./fields.js').SchemaField[]} schema the schema's fields
 * @param {Iterable<string>} inverseNames the inverse names declared against this type
 * @param {GraphqlNames} names the plugin's own GraphQL names
 * @returns {{fields: FieldMapping, inverses: FieldMapping}} the two mappings
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
/**
 * @param {'item' | 'page' | 'composite'} kind which kind of thing the fields belong to
 * @param {import('./fields.js').SchemaField[]} schema the schema's fields
 * @param {GraphqlNames} names the plugin's own GraphQL names
 * @returns {FieldMapping} CMS field name -> GraphQL name
 */
function planFieldNames(kind, schema, names) {
  return planTypeFields(kind, schema, [], names).fields;
}

/**
 * @param {FieldMapping} mapping a name mapping
 * @returns {Record<string, string>} the same mapping as a plain object
 */
function mappingToObject(mapping) {
  return Object.fromEntries(mapping);
}

/**
 * The type of one relation's target, or null when the target is not part of this build.
 *
 * @param {ContentPlan} plan which type each thing got
 * @param {import('./fields.js').WireRelationTarget | null | undefined} target the relation target
 * @returns {string | null} the target's GraphQL type, or null
 */
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

/**
 * The key a planned relation union is looked up by: one field of one type.
 *
 * @param {string} ownerTypeName the type the field belongs to
 * @param {string} fieldName the field's GraphQL name
 * @returns {string} the lookup key
 */
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
 *
 * @param {ContentPlan} plan which type each thing got
 * @param {import('./fields.js').FieldType} fieldType the field's `field_type`
 * @returns {{typed: string[], missing: import('./fields.js').RelationTarget[]}} the split targets
 */
function relationTargetTypes(plan, fieldType) {
  /** @type {string[]} */
  const typed = [];
  /** @type {import('./fields.js').RelationTarget[]} */
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
 * @param {ContentPlan} plan which type each thing got
 * @param {GraphqlNames} names the plugin's own GraphQL names
 * @param {FieldNameMappings} fieldNames the mappings to trim in place
 * @param {SchemaSnapshot} snapshot the schema the model is built from
 * @returns {{unions: Map<string, RelationUnion>, missing: MissingRelationTarget[]}} the planned
 *   unions and the missing targets to warn about
 */
function planRelationFields(plan, names, fieldNames, snapshot) {
  const allocator = createNameAllocator([
    ...STATIC_TYPE_SUFFIXES.map((suffix) => `${plan.prefix}${suffix}`),
    ...plan.collections.values(),
    ...plan.pages.values(),
    ...plan.composites.values(),
  ]);
  /** @type {Map<string, RelationUnion>} */
  const unions = new Map();
  /** @type {MissingRelationTarget[]} */
  const missing = [];

  /**
   * @param {string | undefined} ownerTypeName the owner's GraphQL type
   * @param {import('./fields.js').SchemaField[] | undefined} schema the owner's schema
   * @param {FieldMapping | undefined} mapping the owner's mapping
   */
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
 *
 * @param {ContentModel} model the model
 * @param {import('./fields.js').FieldType} fieldType the field's `field_type`
 * @param {string} [unionName] the union the model planned for this field, when it planned one
 * @returns {GraphqlFieldType} the field's GraphQL type, and whether it is a link
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
 *
 * @param {SchemaSnapshot} snapshot the schema the model is built from
 * @param {ContentPlan} plan which type each thing got
 * @returns {{bySchema: Map<string, InverseDeclaration[]>, byTarget: Map<string, DeclaredInverse[]>}}
 *   the declarations from the referring side and from the target's side
 */
function collectInverseDeclarations(snapshot, plan) {
  /** @type {Map<string, InverseDeclaration[]>} */
  const bySchema = new Map();
  /** @type {Map<string, DeclaredInverse[]>} */
  const byTarget = new Map();

  /**
   * @param {import('./fields.js').SchemaField[] | undefined} schema the declaring schema
   * @param {string} declaringSchemaKey the declaring schema's lookup key
   * @param {string | undefined} declaringTypeName the declaring type's GraphQL name
   */
  const consider = (schema, declaringSchemaKey, declaringTypeName) => {
    if (declaringTypeName === undefined) {
      return;
    }
    /** @type {InverseDeclaration[]} */
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
 *
 * @param {ContentModel} model the model
 * @param {'collection' | 'page'} kind which kind of content type it is
 * @param {string} name the collection's or page's name
 * @returns {Record<string, string>} CMS field name (and inverse name) -> GraphQL name
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
 *
 * @param {SchemaSnapshot} snapshot what the client read from the delivery API
 * @param {ModelOptions} options the options the model reads
 * @returns {ContentModel} the model
 */
function buildContentModel(snapshot, options) {
  const names = graphqlNamesFor(options.typePrefix);
  const pageNames = [...new Set([...snapshot.pages.keys(), ...(snapshot.unpublishedPageTargets || [])])];
  const plan = planTypeNames([...snapshot.collections.keys()], pageNames, [...snapshot.composites.keys()], options.typePrefix);
  const { bySchema, byTarget } = collectInverseDeclarations(snapshot, plan);

  /** @type {FieldNameMappings} */
  const fieldNames = { collections: new Map(), pages: new Map(), composites: new Map() };
  /** @type {Map<string, FieldMapping>} */
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
