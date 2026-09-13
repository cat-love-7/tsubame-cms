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

/**
 * One page of collection items.
 *
 * The server reports the total in the `X-Total-Count` header rather than in the body, so
 * the two are put together at the HTTP boundary and travel as one value from there on.
 */
export interface CollectionItemPage {
  items: CollectionItemEntry[];
  /** Items in the whole collection, not only on this page. */
  total: number;
}
