'use strict';

const { graphqlNamesFor, mappingToObject, planFieldNames } = require('./model');
const { DEFAULT_TYPE_PREFIX } = require('./options');

/**
 * The GraphQL names a preview's fields take.
 *
 * A site whose presentation components were written against GraphQL data reads
 * `published-at` as `published_at`, and `お知らせ` as something else again. A preview has no GraphQL
 * layer to do that renaming, and `tsubame-preview` deliberately answers the CMS's own snake_case
 * names - so a site that wants the build's names asks for them here, from the same planner the build
 * uses (`planFieldNames`), rather than reimplementing the rule and drifting from it.
 *
 * This is the whole adapter, and it is this small on purpose. What a preview *cannot* reproduce is
 * not naming: there is no node store for a relation to link to, no downloaded image for
 * `gatsbyImageData` to read, and no `gatsby-transformer-remark` node with `excerpt` and `timeToRead`.
 * Those live in the site's own `fromPreview` mapper, which is where the two sources become one
 * view-model - see `docs/preview-site.md`.
 *
 * One known difference from a build: a build also allocates names for a relation's `inverse_name`,
 * which collide with a field's name if an editor chose one that does. The preview's schema carries
 * no inverse declarations, so a field that would have been suffixed in the build keeps its plain
 * name here. Renaming it back is the site's business, and the site knows which fields those are.
 */

/**
 * `{ cmsName: graphqlName }` for one content type's schema.
 *
 * `kind` is `'item'` (a collection item), `'page'` (a single page) or `'composite'`, which is what
 * decides the names reserved for the plugin's own fields (`values`, `fieldNames`, ...) and therefore
 * which CMS names have to be suffixed to make room.
 *
 * @param {string} kind `'item'`, `'page'` or `'composite'`
 * @param {import('./fields.js').SchemaField[]} schema the content type's schema
 * @param {{typePrefix?: string}} [options] the site's type prefix
 * @returns {Record<string, string>} CMS field name -> GraphQL name
 */
function previewFieldNames(kind, schema, { typePrefix = DEFAULT_TYPE_PREFIX } = {}) {
  const names = graphqlNamesFor(typePrefix);
  const plannerKind = kind === 'composite' ? 'composite' : kind === 'page' ? 'page' : 'item';
  return mappingToObject(planFieldNames(plannerKind, schema, names));
}

/**
 * `{ cmsName: graphqlName }` for a composite definition.
 *
 * Composite definitions reserve only `id` and `values`, so this needs no type prefix - which is also
 * why it is a separate call rather than a `kind` of the one above: there is one answer, not a
 * choice.
 *
 * @param {import('./fields.js').SchemaField[]} schema the definition's schema
 * @returns {Record<string, string>} CMS field name -> GraphQL name
 */
function previewCompositeFieldNames(schema) {
  return mappingToObject(planFieldNames('composite', schema, graphqlNamesFor(DEFAULT_TYPE_PREFIX)));
}

module.exports = {
  previewFieldNames,
  previewCompositeFieldNames,
};
