'use strict';

/**
 * Reading the CMS's field types, and the value shapes they imply.
 *
 * A value on the wire carries no type tag: the CMS sends `"..."` for both `Text` and `Markdown`,
 * `{"id":3,"url":"..."}` for an `Image`, and `{"target":"authors","item":7}` for a `Relation`. A
 * relation holds one reference; several are an `Array` whose item types are all relations
 * (`Array([Relation(…)])`), and one array may name several targets. The schema returned next to the
 * values is the only thing that says which is which, so every decision in this file takes the
 * field's `field_type` and never the value's own shape.
 *
 * The wire shapes are the contract in `docs/content-api.md` (sections 3 and 3.1) and the Rust
 * `FieldValueResponse` in `backend/crates/core/src/models/values.rs`.
 */

/**
 * The kind of a field type, with its options.
 *
 * The CMS serialises an enum variant either as a bare string (`"Number"`, `"Image"`) or as a
 * single-key object holding the options (`{"Text":{}}`, `{"Array":[{"Image"}]}`), so both spellings
 * have to be understood by the same reader.
 */
function describeFieldType(fieldType) {
  if (typeof fieldType === 'string') {
    return { kind: fieldType, options: undefined };
  }
  if (fieldType === null || typeof fieldType !== 'object') {
    return { kind: 'Unknown', options: undefined };
  }
  for (const key of ['Text', 'Slug', 'Markdown', 'TextEnum', 'CompositeField', 'Relation', 'Array']) {
    if (key in fieldType) {
      return { kind: key, options: fieldType[key] };
    }
  }
  return { kind: 'Unknown', options: undefined };
}

/**
 * Whether a field type is several references: an `Array` whose item types are all `Relation`.
 *
 * That is how the schema says "this field holds many" since `has_many` was replaced by the array
 * (`docs/relations-design.md` §3), and the item types may name different targets. A mixed array is
 * not one: there is no per-element type tag, so its elements cannot all be read as references.
 */
function isRelationArray(fieldType) {
  const { kind, options } = describeFieldType(fieldType);
  return (
    kind === 'Array' &&
    Array.isArray(options) &&
    options.length > 0 &&
    options.every((item) => describeFieldType(item).kind === 'Relation')
  );
}

/**
 * The relation options a schema field declares: its own for a `Relation`, one per item type for an
 * `Array` of relations. Anything else declares nothing.
 *
 * A field may declare several targets this way, each with its own `inverse_name`, so the caller
 * reads one declaration per entry.
 */
function relationOptionsOf(fieldType) {
  const { kind, options } = describeFieldType(fieldType);
  if (kind === 'Relation') {
    return [options === null || typeof options !== 'object' ? {} : options];
  }
  if (isRelationArray(fieldType)) {
    return options.map((item) => describeFieldType(item).options ?? {});
  }
  return [];
}

/**
 * An image value as the site wants it.
 *
 * The API answers `{id, url}` with a path (`/api/images/<file>`), which is not openable from a
 * browser on another origin, so the absolute URL is what a page actually renders. `stableUrl` is
 * the id link the delivery API documents (`/api/images/by-id/{id}`, prefix included): the file name
 * changes when an image is replaced, the id does not, so it is the one to put in Markdown by hand.
 *
 * `resolve` carries the two ways a path is made absolute - one for a path the API returned (which
 * already holds the API prefix) and one for a path this plugin builds under it - because prefixing
 * `/api/images/...` twice is the obvious way to get this wrong. When `imageFiles` is a map, the
 * value also carries `localFile`: the id of the `File` node the bytes were downloaded into, which
 * is what `gatsby-transformer-sharp` needs to see.
 */
function toImageValue(value, resolve) {
  if (value === null || value === undefined) {
    return null;
  }
  const id = typeof value === 'number' ? value : value.id;
  const url = typeof value === 'object' && typeof value.url === 'string' ? value.url : '';
  const imageId = typeof id === 'number' ? id : null;

  const image = {
    id: imageId,
    url,
    absoluteUrl: url === '' ? null : resolve.resolveUrl(url),
    stableUrl: imageId === null ? null : resolve.resolveApiPath(`/images/by-id/${imageId}`),
  };

  if (resolve.imageFiles !== undefined && resolve.imageFiles !== null) {
    const key = imageKey({ id: imageId, url });
    image.localFile = key === null ? null : resolve.imageFiles.get(key) ?? null;
  }
  return image;
}

/**
 * The key an image is identified by, for matching a value to the file it was downloaded into.
 *
 * The id when the API gave one - the same image may be referenced from several items, and its URL
 * is a rendering detail - and the URL otherwise (an image written as a bare id has no URL to key
 * on until it is read back).
 */
function imageKey(image) {
  if (image === null || typeof image !== 'object') {
    return null;
  }
  if (typeof image.id === 'number') {
    return `id:${image.id}`;
  }
  if (typeof image.url === 'string' && image.url !== '') {
    return `url:${image.url}`;
  }
  return null;
}

/** An image value reduced to what downloading needs: an id and a URL, either of which may be absent. */
function normalizeImageValue(value) {
  if (typeof value === 'number') {
    return { id: value, url: '' };
  }
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const id = typeof value.id === 'number' ? value.id : null;
  const url = typeof value.url === 'string' ? value.url : '';
  if (id === null && url === '') {
    return null;
  }
  return { id, url };
}

/**
 * A composite value as `{id, values}`.
 *
 * `values` stays the untyped object the API sent even when the definition is known: the typed
 * fields sit beside it, and keeping the source object means a field the definition no longer
 * declares is still readable rather than silently dropped.
 */
function toCompositeValue(value) {
  if (value === null || value === undefined || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  const id = typeof value.id === 'string' ? value.id : '';
  const values = value.values !== null && typeof value.values === 'object' && !Array.isArray(value.values)
    ? value.values
    : {};
  return { id, values };
}

/**
 * Every content target the given schemas point at, directly or through a composite definition.
 *
 * Used before the schema is declared: a relation field is typed after its target, so the target's
 * type has to exist even when the target has no published items (and is therefore not in
 * `/content/collections`). Composite definitions are walked too - a relation may sit inside one
 * (`docs/content-api.md` §3.1) - and a definition is walked once, because a composite may reach
 * itself through an array (a tree).
 *
 * @param {Array<Array<object>>} schemas the field schemas to walk
 * @param {Map<string, Array<object>>} compositeSchemas the definitions an embedded composite names
 */
function collectRelationTargets(schemas, compositeSchemas) {
  const found = new Map();
  const visited = new Set();

  const visitSchema = (schema) => {
    for (const field of Array.isArray(schema) ? schema : []) {
      visitFieldType(field === null || typeof field !== 'object' ? undefined : field.field_type);
    }
  };

  const visitFieldType = (fieldType) => {
    const { kind, options } = describeFieldType(fieldType);
    if (kind === 'Relation') {
      const target = options === null || typeof options !== 'object' ? undefined : options.target;
      if (target !== null && typeof target === 'object' && typeof target.name === 'string') {
        const targetKind = target.kind === 'single_page' ? 'single_page' : 'collection';
        found.set(`${targetKind}:${target.name}`, { kind: targetKind, name: target.name });
      }
      return;
    }
    if (kind === 'Array') {
      if (Array.isArray(options)) {
        options.forEach(visitFieldType);
      }
      return;
    }
    if (kind === 'CompositeField') {
      const id = options === null || typeof options !== 'object' ? undefined : options.id;
      if (typeof id !== 'string' || visited.has(id)) {
        return;
      }
      visited.add(id);
      const definition = compositeSchemas === undefined ? undefined : compositeSchemas.get(id);
      if (definition !== undefined) {
        visitSchema(definition);
      }
    }
  };

  for (const schema of Array.isArray(schemas) ? schemas : []) {
    visitSchema(schema);
  }
  return [...found.values()];
}

module.exports = {
  describeFieldType,
  isRelationArray,
  relationOptionsOf,
  toImageValue,
  imageKey,
  normalizeImageValue,
  toCompositeValue,
  collectRelationTargets,
};
