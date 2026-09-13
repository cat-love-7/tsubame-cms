import { FieldValue } from './fields';

/** A collection item's values, keyed by the collection schema's field names. */
export type CollectionValue = { [field: string]: FieldValue };

/**
 * Shape of `GET /models/collections/{name}/items`: a list of `[id, values]` pairs.
 */
export type CollectionItemEntry = [number, CollectionValue];
