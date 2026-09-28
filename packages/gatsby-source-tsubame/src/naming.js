'use strict';

/**
 * GraphQL names for things the CMS named.
 *
 * A CMS field may be called `published-at`, a collection may be `お知らせ`, and neither is a name
 * GraphQL accepts: a GraphQL name has to match `[_A-Za-z][_0-9A-Za-z]*`, and a name starting with
 * `__` belongs to introspection. So a name that does not fit has to be rewritten before it can
 * appear in the schema.
 *
 * Rewriting is deterministic - the same CMS name always becomes the same GraphQL name - because a
 * query is written once and run on every build, and a name that moved between builds would be a
 * query that compiles on one build and not the next. What each name became is reported on the node
 * as `fieldNames` (and as `itemTypeName` on the collection), so a consumer can look the
 * translation up instead of guessing at it.
 */

const FIELD_NAME = /^[_A-Za-z][_0-9A-Za-z]*$/;

/**
 * Hands out GraphQL names, remembering the ones already taken.
 *
 * @typedef {object} NameAllocator
 * @property {(name: string) => boolean} used whether a name is already taken
 * @property {(base: string) => string} take the base, or the first free `base_<n>`
 */

/**
 * Whether a string is already a name GraphQL will accept.
 *
 * @param {unknown} name the candidate name
 * @returns {boolean} true when GraphQL accepts it
 */
function isGraphqlName(name) {
  return typeof name === 'string' && FIELD_NAME.test(name) && !name.startsWith('__');
}

/**
 * A type name for a collection or page: the words of the name, each capitalised, joined.
 *
 * `blog` -> `Blog`, `press-releases` -> `PressReleases`, `お知らせ` -> `Unnamed` (nothing that
 * survives the ASCII filter is left, and an empty name is worse than an obvious placeholder).
 *
 * Splitting on every non-alphanumeric character also removes the underscores, so the result can
 * never start with `__` - which is why there is no check for introspection here and there is one in
 * [`sanitizeFieldName`], where underscores survive.
 *
 * @param {unknown} raw the name the CMS gave
 * @returns {string} a name GraphQL accepts
 */
function sanitizeTypeName(raw) {
  const words = String(raw)
    .split(/[^0-9A-Za-z]+/)
    .filter((word) => word !== '');
  let name = words.map((word) => word.charAt(0).toUpperCase() + word.slice(1)).join('');
  if (name === '') {
    name = 'Unnamed';
  }
  if (/^[0-9]/.test(name)) {
    name = `_${name}`;
  }
  return name;
}

/**
 * A field name for a value the CMS named: every character GraphQL rejects becomes `_`.
 *
 * This keeps the shape of the original name visible (`published-at` -> `published_at`), which is
 * what makes a renamed field findable; two names that collide after rewriting are told apart by
 * [`createNameAllocator`] rather than by dropping one.
 *
 * @param {unknown} raw the name the CMS gave
 * @returns {string} a name GraphQL accepts
 */
function sanitizeFieldName(raw) {
  let name = String(raw).replace(/[^0-9A-Za-z_]/g, '_');
  if (name === '') {
    name = '_';
  }
  if (/^[0-9]/.test(name)) {
    name = `_${name}`;
  }
  if (name.startsWith('__')) {
    name = `x${name}`;
  }
  return name;
}

/**
 * Hands out unique names from a set of reserved ones.
 *
 * A collision is resolved by a numeric suffix (`body`, `body_2`, ...), in the order the caller
 * asks: the allocator is the only place that decides, so two callers that ask in the same order
 * get the same answer. Callers are expected to sort their input, because the CMS does not promise
 * a stable order for a schema or a list of collection names.
 *
 * @param {Iterable<string>} [seed] names that are already taken
 * @returns {NameAllocator} the allocator
 */
function createNameAllocator(seed = []) {
  const used = new Set(seed);
  return {
    /**
     * Whether a name has already been handed out (or was reserved to begin with).
     *
     * @param {string} name the name to check
     * @returns {boolean} true when it is taken
     */
    used(name) {
      return used.has(name);
    },
    /**
     * The name itself, or the first free `name_<n>` after it.
     *
     * @param {string} base the wanted name
     * @returns {string} the name to use
     */
    take(base) {
      let name = base;
      let suffix = 2;
      while (used.has(name)) {
        name = `${base}_${suffix}`;
        suffix += 1;
      }
      used.add(name);
      return name;
    },
  };
}

module.exports = {
  isGraphqlName,
  sanitizeTypeName,
  sanitizeFieldName,
  createNameAllocator,
};
