import {
  FieldSchema,
  isArrayFieldSchema,
  isCompositeFieldSchema,
  isEnumFieldSchema,
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
 */
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

/** Single-line rendering used by tables and summaries. */
export function formatFieldValue(value: FieldValue | undefined): string {
  if (value === undefined || value === null) {
    return '—';
  }
  if (Array.isArray(value)) {
    return value.length > 0 ? value.map((item) => formatFieldValue(item)).join(', ') : '—';
  }
  if (typeof value === 'object') {
    const url = (value as { url?: unknown }).url;
    return typeof url === 'string' ? url : JSON.stringify(value);
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
