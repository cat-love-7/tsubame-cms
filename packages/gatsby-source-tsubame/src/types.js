'use strict';

const { graphqlFieldType, relationUnionKey, schemaKey } = require('./model');

/**
 * The GraphQL schema this plugin asks Gatsby to use.
 *
 * The CMS is schema-driven, so the site's schema is too: every collection becomes a node type whose
 * fields are the fields of that collection, every composite definition becomes a value type, and
 * both are typed with what the CMS declared. That is the difference between this plugin and a
 * generic "content as JSON" source - `author { name }` and `blocks { text }` are checked at build
 * time, and a field the CMS renamed is a compile error rather than a null at runtime.
 *
 * The types are declared in `createSchemaCustomization`, which runs before `sourceNodes` (Gatsby's
 * `bootstrap/index.ts`: `customizeSchema` then `sourceNodes`), so the schema is read twice per
 * build: once here to declare the types, once in `sourceNodes` for the values. Both times through
 * `buildContentModel`, which is what keeps the two from drifting.
 */

/**
 * Every declaration, in one SDL string.
 *
 * `createTypes` is given SDL rather than type-builder objects because the whole schema is a string
 * of declarations either way, and SDL is what a reader can paste into GraphiQL to see what the
 * build produced. That includes the relation unions: their members are node types, so Gatsby's own
 * `resolveType` (`node => node.internal.type`) is the right one, and `@link` resolves the ids.
 *
 * The collection type asks for `@dontInfer` and the others must not. Gatsby adds the child fields a
 * consumer uses - `childMarkdownRemark` on a markdown node, `childrenTsubameMarkdown` on its owner -
 * through inference, and `@dontInfer` takes those away with it (verified by building: the query
 * fails with "Cannot query field childMarkdownRemark on type TsubameMarkdown"). The collection type
 * has no children and no inferred field worth having, while its `schema` is the one value that is
 * polymorphic on purpose - a field type is a string (`"Image"`) in one entry and an object
 * (`{"Text":{}}`) in the next - which is what makes Gatsby warn about conflicting field types.
 *
 * @param {import('./model.js').ContentModel} model the model
 * @returns {string} the SDL the plugin declares
 */
function buildTypeDefinitions(model) {
  const { names, plan, fieldNames, relationUnions } = model;
  const parts = [staticTypeDefinitions(names, model)];

  for (const union of relationUnions.values()) {
    parts.push(`union ${union.name} = ${union.members.join(' | ')}`);
  }

  for (const [name, schema] of model.snapshot.collections) {
    const typeName = plan.collections.get(name);
    if (typeName === undefined) {
      continue;
    }
    parts.push(
      contentTypeDefinition({
        typeName,
        kind: 'collection',
        name,
        schema,
        mapping: fieldNames.collections.get(name),
        model,
      }),
    );
  }

  // `plan.pages` also holds single pages a relation points at that are not published: their schema
  // is not public (the page answers 404), so the type is declared with the plugin's own fields
  // only. That is enough for a relation field to name it, and the page's own fields appear the
  // build after it is published.
  for (const [name, typeName] of plan.pages) {
    parts.push(
      contentTypeDefinition({
        typeName,
        kind: 'page',
        name,
        schema: model.snapshot.pages.get(name) ?? [],
        mapping: fieldNames.pages.get(name) ?? new Map(),
        model,
      }),
    );
  }

  for (const [id, typeName] of plan.composites) {
    parts.push(
      compositeTypeDefinition({
        typeName,
        schema: model.snapshot.composites.get(id) ?? [],
        mapping: fieldNames.composites.get(id) ?? new Map(),
        model,
      }),
    );
  }

  return parts.join('\n\n');
}

/**
 * @param {{typeName: string, kind: 'collection' | 'page', name: string, schema: import('./fields.js').SchemaField[], mapping: import('./model.js').FieldMapping | undefined, model: import('./model.js').ContentModel}} content
 *   the type to declare
 * @returns {string} its SDL declaration
 */
function contentTypeDefinition({ typeName, kind, name, schema, mapping, model }) {
  const lines = [`type ${typeName} implements Node {`];
  if (kind === 'collection') {
    lines.push('  remoteId: Int!');
    lines.push('  collection: String!');
  } else {
    lines.push('  name: String!');
  }
  lines.push('  publishedAt: Date');
  lines.push('  lastPublishedAt: Date');
  lines.push('  values: JSON!');
  lines.push('  fieldNames: JSON!');
  lines.push(...fieldLines(schema, mapping, model, typeName));
  lines.push(...inverseFieldLines(kind, name, model));
  lines.push('}');
  return lines.join('\n');
}

/**
 * The fields a target type answers to for the content that references it.
 *
 * `inverse_name` is declared on the referring field (`blog.author` says the other side calls it
 * `articles`), and the CMS keeps it unique per target, so every name here is a list of one concrete
 * type. The value is an array of node ids, which is what `@link` resolves - the same mechanism as a
 * forward relation, read from the other end.
 *
 * @param {'collection' | 'page'} kind which kind of content type it is
 * @param {string} name the collection's or page's name
 * @param {import('./model.js').ContentModel} model the model
 * @returns {string[]} one line per inverse field
 */
function inverseFieldLines(kind, name, model) {
  const declared = model.inverseDeclarationsByTarget.get(schemaKey(kind, name)) ?? [];
  const names = model.inverseFieldNames.get(schemaKey(kind, name)) ?? new Map();
  const lines = [];
  for (const { inverseName, declaringTypeName } of declared) {
    const graphqlName = names.get(inverseName);
    if (graphqlName === undefined) {
      continue;
    }
    lines.push(`  ${graphqlName}: [${declaringTypeName}] @link`);
  }
  return lines;
}

/**
 * One composite definition as a value type.
 *
 * `id` is the definition's own id (the same value the schema names), and `values` is the untyped
 * object the API sent, kept beside the typed fields so a field the definition does not declare is
 * still readable.
 *
 * @param {{typeName: string, schema: import('./fields.js').SchemaField[], mapping: import('./model.js').FieldMapping | undefined, model: import('./model.js').ContentModel}} definition
 *   the definition to declare
 * @returns {string} its SDL declaration
 */
function compositeTypeDefinition({ typeName, schema, mapping, model }) {
  const lines = [`type ${typeName} {`];
  lines.push('  id: String!');
  lines.push('  values: JSON!');
  lines.push(...fieldLines(schema, mapping, model, typeName));
  lines.push('}');
  return lines.join('\n');
}

/**
 * One field per line, with the union the model planned for it when it has one.
 *
 * A field the model left out is not in the mapping, so it is not declared here either; that is the
 * one place the "left out" decision has to be read, and `src/nodes.js` reads the same mapping.
 *
 * @param {import('./fields.js').SchemaField[]} schema the owner's schema
 * @param {import('./model.js').FieldMapping | undefined} mapping the owner's name mapping
 * @param {import('./model.js').ContentModel} model the model
 * @param {string} ownerTypeName the owner's GraphQL type
 * @returns {string[]} one line per declared field
 */
function fieldLines(schema, mapping, model, ownerTypeName) {
  const lines = [];
  const emitted = new Set();
  for (const field of Array.isArray(schema) ? schema : []) {
    const graphqlName = mapping === undefined ? undefined : mapping.get(String(field.name));
    if (graphqlName === undefined || emitted.has(graphqlName)) {
      continue;
    }
    emitted.add(graphqlName);
    const union = model.relationUnions.get(relationUnionKey(ownerTypeName, graphqlName));
    const { type, link } = graphqlFieldType(model, field.field_type, union?.name);
    lines.push(`  ${graphqlName}: ${type}${link ? ' @link' : ''}`);
  }
  return lines;
}

/**
 * @param {import('./model.js').GraphqlNames} names the plugin's own GraphQL names
 * @param {import('./model.js').ContentModel} model the model
 * @returns {string} the SDL for the types that are the same for every project
 */
function staticTypeDefinitions(names, model) {
  // `File` is `gatsby-source-filesystem`'s type, so it is only named when the plugin actually
  // creates File nodes. A site that does not download images does not have to install it - and
  // would get "Unknown type File" if this were declared anyway.
  const localFile = model.options.images !== undefined && model.options.images.download ? '\n  localFile: File @link' : '';

  return `# The schema of a collection, and how its fields were named in GraphQL.
type ${names.collection} implements Node @dontInfer {
  name: String!
  itemTypeName: String!
  itemCount: Int!
  schema: JSON!
  fieldNames: JSON!
}

# One Markdown field of one item, carrying the raw source in \`internal.content\`.
#
# \`mediaType: text/markdown\` is what makes gatsby-transformer-remark pick it up and add a
# \`MarkdownRemark\` child with \`html\`, \`excerpt\`, \`headings\` and the rest. The field is linked
# from the item, so the usual query is \`body { childMarkdownRemark { html } }\`; \`field\` and
# \`path\` identify it when read back from the item's children instead. A Markdown field inside a
# composite is a child of the same item, with the field's path as its \`path\`
# (\`blocks.0.text\`).
type ${names.markdown} implements Node {
  raw: String!
  field: String!
  path: String!
  collection: String
  pageName: String
  itemId: Int
}

# An image value. \`url\` is what the API sent; \`absoluteUrl\` opens it from anywhere;
# \`stableUrl\` is the id link that survives the image being replaced; \`localFile\` (with
# \`images.download\`) is the downloaded copy gatsby-transformer-sharp turns into \`childImageSharp\`.
type ${names.image} {
  id: Int
  url: String!
  absoluteUrl: String
  stableUrl: String${localFile}
}

# A composite value whose definition the delivery API did not answer. The normal case is a type per
# definition (\`${names.composite}Block\`), with the definition's fields typed; this is what is left
# when an id has no definition.
type ${names.composite} {
  id: String!
  values: JSON!
}`;
}

module.exports = { buildTypeDefinitions };
