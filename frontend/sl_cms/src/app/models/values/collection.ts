import { FieldValue } from './fields';

/**
 * A map of field name to value.
 *
 * Collection items and single page items have exactly the same shape; only the container
 * differs, so both use this type.
 */
export type ContentValue = { [field: string]: FieldValue };

/** A collection item's values. */
export type CollectionValue = ContentValue;

/** Shape of `GET /models/collections/{name}/items`: a list of `[id, values]` pairs. */
export type CollectionItemEntry = [number, CollectionValue];
