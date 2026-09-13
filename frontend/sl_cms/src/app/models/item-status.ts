/**
 * Draft/published state of a collection item or a single page.
 *
 * Content starts as a draft: it is published by an explicit action, so saving an item
 * never puts it on the public site.
 */
export type ItemStatus = 'draft' | 'published';

/**
 * Who published an item, as recorded at the moment it was published.
 *
 * The id links back to the account while it still exists; the username is the name as it
 * was then, so the record still reads after a rename or a deletion.
 */
export interface PublishedBy {
  id: string;
  /** The identifier of the account that published it, as it was then. */
  username: string;
}

/**
 * Server-side metadata that is deliberately *not* part of the user-defined schema, so a
 * field may be named `status` or `published_at` without colliding with it.
 */
export interface ItemMetadata {
  status: ItemStatus;
  /** When it was last published; `null` while it is a draft. */
  published_at: string | null;
  /** Who published it last; `null` while it is a draft, like `published_at`. */
  published_by: PublishedBy | null;
  /** When the values were first saved; `null` for content saved before this was recorded. */
  created_at: string | null;
  /**
   * When the values were last saved. Publishing does not change it, so it answers "did the
   * content itself change?" rather than "was it released?".
   */
  updated_at: string | null;
  /**
   * True while an editor has saved something that has not been published.
   *
   * Editing never touches the live site: the working copy is what an editor saves into and
   * what publishing copies across.
   */
  has_draft: boolean;
}

/** Status of every item of a collection, keyed by item id (`"1"`, `"2"`, ...). */
export interface ItemMetadataMap {
  [id: string]: ItemMetadata;
}

/**
 * A signed, expiring link that shows one working copy to someone without an account.
 *
 * `path` is relative to the API base the CMS is reached through, and the token in it is the
 * whole credential: it opens that one item and nothing else, until `expires_at`.
 */
export interface PreviewLink {
  path: string;
  expires_at: string;
}
