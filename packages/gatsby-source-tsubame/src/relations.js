'use strict';

/**
 * The GraphQL side of a field that holds more than one kind of node.
 *
 * An `Array` of relations that names several targets is typed as a **union** of those targets (plus
 * the reference type), because GraphQL has no other way to say "one of these node types" - and a
 * union needs two things a plain type does not: a `resolveType` (which member is this value?) and,
 * because the field's value on the node is a node id per linked element, a resolver that turns
 * those ids into nodes.
 *
 * `@link` would do the second half on its own, but only for a field whose every element is an id:
 * an element whose target is not part of the build is stored as a reference (`{target, item, kind}`)
 * so that the target is not lost, and `@link` has nothing to look up for it. Hence one resolver that
 * does both, and one `resolveType` that reads a node's own type or falls back to the reference type.
 *
 * The element shapes are decided by `src/values.js`: an id where the target has a node type, a
 * reference where it does not. The two files agree by asking `relationTargetTypeName` the same
 * question, so a value this resolver cannot place cannot occur - except for a node id whose node is
 * gone (the target was unpublished between the two reads), which is dropped the way `@link` drops
 * it: the list holds what the site can serve.
 */

/**
 * The `resolveType` of a relation union: the node's own type, or the reference type.
 *
 * Gatsby's own default for a union reads `node.internal.type`, which is right for the linked
 * elements and undefined for the references - the case this function exists for.
 */
function relationUnionResolveType(relationRefTypeName) {
  return (value) => {
    const typeName =
      value === null || typeof value !== 'object' || value.internal === undefined
        ? undefined
        : value.internal.type;
    return typeof typeName === 'string' && typeName !== '' ? typeName : relationRefTypeName;
  };
}

/**
 * The field resolver of a relation union: a stored node id becomes the node, a stored reference is
 * served as it is.
 *
 * The value is read the way Gatsby's default resolver reads it (`source[info.fieldName]`), because
 * these fields are declared in SDL and this is the only resolver they get.
 *
 * `{ path: context.path }` is not optional: it is what tells Gatsby this page read that node, so
 * changing the node re-runs the page's query in `gatsby develop` and in an incremental build.
 * `@link` passes it too (`gatsby/src/schema/resolvers.ts`), which is the part of `@link` a
 * hand-written resolver has to remember.
 */
function linkRelationElements(source, _args, context, info) {
  const value = source === null || typeof source !== 'object' ? undefined : source[info.fieldName];
  if (!Array.isArray(value)) {
    return value === undefined ? null : value;
  }
  const linked = value.map((element) =>
    typeof element === 'string'
      ? context.nodeModel.getNodeById({ id: element }, { path: context.path })
      : element,
  );
  return linked.filter((element) => element !== undefined && element !== null);
}

module.exports = { relationUnionResolveType, linkRelationElements };
