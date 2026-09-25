import { FieldValues } from './fields';

/**
 * The values of one single page, by field name.
 *
 * Its own file rather than `collection.ts`: a single page has no id, no working copy and no
 * status of its own, and the only thing it shares with a collection item is what a field holds.
 */
/**
 * A map of field name to value.
 *
 * Collection items and single page items have exactly the same shape; only the container
 * differs, so both use this type.
 */
export type ContentValue = FieldValues;
