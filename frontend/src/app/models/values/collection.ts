import { FieldValues } from './fields';

/** A collection item's values. */
export type CollectionValue = FieldValues;

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
