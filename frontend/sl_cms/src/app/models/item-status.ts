/**
 * Draft/published state of a collection item or a single page.
 *
 * Content starts as a draft: it is published by an explicit action, so saving an item
 * never puts it on the public site.
 */
export type ItemStatus = 'draft' | 'published';

/**
 * Server-side metadata that is deliberately *not* part of the user-defined schema, so a
 * field may be named `status` or `published_at` without colliding with it.
 */
export interface ItemMetadata {
  status: ItemStatus;
  /** When it was last published; `null` while it is a draft. */
  published_at: string | null;
}

/** Status of every item of a collection, keyed by item id (`"1"`, `"2"`, ...). */
export interface ItemMetadataMap {
  [id: string]: ItemMetadata;
}
