import {
  FieldSchema,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
  isRelationFieldSchema,
} from '../schema/fields';

/**
 * Content values travel **without type tags**.
 *
 * The collection schema already states each field's type, so a tag would be redundant —
 * and worse, it could contradict the schema. The schema is the single source of truth for
 * interpreting a value.
 *
 * The concrete JSON shape therefore depends on the field's `FieldType`:
 *
 * | FieldType          | JSON on the wire                                                    |
 * |--------------------|---------------------------------------------------------------------|
 * | Text / Markdown    | `string`                                                            |
 * | Number             | `number` or `null`                                                  |
 * | Boolean            | `boolean`                                                           |
 * | Date               | `"YYYY-MM-DD"` or `null`                                            |
 * | DateTime           | RFC 3339 string or `null`                                           |
 * | Image              | `{ id, url }` (read) — a bare `number` id is also accepted on write |
 * | TextEnum           | `string[]`                                                          |
 * | Array              | array of the declared item types                                    |
 * | CompositeField     | object keyed by the composite's own field names                     |
 * | Relation           | `{ target, item }` (a page reference has no `item`), or `null`      |
 * | Array([Relation])  | `[{ target, item }, …]`, or `[]`                                     |
 */
/**
 * A whole form's values, by field name.
 *
 * A collection item and a single page hold the same thing - a value per field - and differ in
 * everything around it (an id, a working copy, a status). This is the shape they share, so the
 * two named aliases below are about which one a screen means rather than about a different type.
 */
export type FieldValues = { [field: string]: FieldValue };

export type FieldValue =
  string | number | boolean | null | FieldValue[] | { [field: string]: FieldValue };

/**
 * An image value as returned by the API. Declared as a type alias (not an interface) so
 * it keeps an implicit index signature and stays assignable to `FieldValue`.
 */
export type ImageValue = { id: number; url: string };

/** The image id inside an image value, whichever accepted shape it uses. */
export function imageIdOf(value: FieldValue | undefined): number | null {
  if (typeof value === 'number') {
    return value;
  }
  if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
    const id = (value as { id?: unknown }).id;
    return typeof id === 'number' ? id : null;
  }
  return null;
}

/**
 * The images a value holds, in order: one image, or the ones an array carries.
 *
 * What a list column needs to draw a thumbnail instead of a url. The read shape is `{ id, url }`;
 * a value that was written but never read back carries a bare id, and has nothing to show, so the
 * id is kept as the fallback a cell can print.
 */
export function imagesOf(
  value: FieldValue | undefined,
): { id: number | null; url: string | null }[] {
  const items = Array.isArray(value) ? value : [value];
  return items
    .filter((item) => item !== null && item !== undefined)
    .map((item) => {
      const url =
        typeof item === 'object' && !Array.isArray(item)
          ? (item as { url?: unknown }).url
          : undefined;
      return {
        id: imageIdOf(item),
        url: typeof url === 'string' ? url : null,
      };
    })
    .filter((image) => image.id !== null || image.url !== null);
}

/**
 * One line of a relation's value: what it points at.
 *
 * An absent `item` is a single page, whose identity is its name.
 */
export type RelationRef = { target: string; item?: number | null };

/** The references a value holds, whether the field is one relation or an array of them. */
export function relationRefsOf(value: FieldValue | undefined): RelationRef[] {
  const entries = Array.isArray(value) ? value : [value];
  return entries.filter(
    (entry): entry is RelationRef =>
      entry !== null &&
      typeof entry === 'object' &&
      !Array.isArray(entry) &&
      typeof (entry as { target?: unknown }).target === 'string',
  );
}

/**
 * The key a reference is looked up by.
 *
 * The same shape the owner of a piece of content has on the server (`collection:<name>:<id>` and
 * `page:<name>`), so two references cannot be confused: a collection named `home` is not the page
 * named `home`.
 */
export function referenceKey(reference: RelationRef): string {
  return typeof reference.item === 'number'
    ? `collection:${reference.target}:${reference.item}`
    : `page:${reference.target}`;
}

/**
 * What a reference is called: the title the target's schema names, and the reference itself when
 * there is none - `categories #3` says which item, which is what is left when nothing names it.
 */
export function referenceName(reference: RelationRef, labels: ReadonlyMap<string, string>): string {
  const label = labels.get(referenceKey(reference));
  if (label !== undefined && label !== '') {
    return label;
  }
  return typeof reference.item === 'number'
    ? `${reference.target} #${reference.item}`
    : reference.target;
}

/** Single-line rendering used by tables and summaries. */
export function formatFieldValue(value: FieldValue | undefined): string {
  if (value === undefined || value === null) {
    return '—';
  }
  if (Array.isArray(value)) {
    return value.length > 0 ? value.map((item) => formatFieldValue(item)).join(', ') : '—';
  }
  if (typeof value === 'object') {
    const entry = value as { url?: unknown; target?: unknown; item?: unknown };
    if (typeof entry.url === 'string') {
      return entry.url;
    }
    // A relation reference: what it points at, and which item. Expanding the referenced content
    // itself is the delivery API's job (`?populate=`), not a cell's.
    if (typeof entry.target === 'string') {
      return typeof entry.item === 'number' ? `${entry.target} #${entry.item}` : entry.target;
    }
    return JSON.stringify(value);
  }
  if (typeof value === 'boolean') {
    return value ? '✓' : '✗';
  }
  return String(value);
}

/**
 * The starting value for a field the user has not filled in yet.
 *
 * Only used when a field is absent; an explicit `null` from the server is preserved.
 */
export function defaultValueForField(field: FieldSchema): FieldValue {
  const type = field.field_type;
  if (type === 'Boolean') {
    return false;
  }
  if (type === 'Number' || type === 'Date' || type === 'DateTime' || type === 'Image') {
    return null;
  }
  if (isEnumFieldSchema(type) || isArrayFieldSchema(type)) {
    return [];
  }
  // One relation is an object or null, so an empty list would be the wrong shape.
  if (isRelationFieldSchema(type)) {
    return null;
  }
  if (isCompositeFieldSchema(type)) {
    return null;
  }
  // Text and Markdown.
  return '';
}

/**
 * Fill in any field the server did not send, so the editor always has something to bind
 * to. Existing values (including an explicit `null`) are left alone.
 */
export function withDefaults(
  schema: FieldSchema[],
  values: { [field: string]: FieldValue },
): { [field: string]: FieldValue } {
  const result: { [field: string]: FieldValue } = {};
  for (const field of schema) {
    const existing = values[field.name];
    result[field.name] = existing === undefined ? defaultValueForField(field) : existing;
  }
  return result;
}
