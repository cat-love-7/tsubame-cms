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
  /**
   * When the item was **first** published, kept even while it is a draft.
   *
   * The publication date belongs to the item; a later release does not move it (that is what
   * `last_published_at` records).
   */
  published_at: string | null;
  /** When the live copy last went out; `null` while it is a draft. */
  last_published_at: string | null;
  /** Who published the version that is live; `null` while it is a draft. */
  published_by: PublishedBy | null;
  /** When the values were first saved; `null` for content saved before this was recorded. */
  created_at: string | null;
  /**
   * When the content last changed: a save, or a publish that released a working copy.
   *
   * Publishing with nothing waiting does not move it, so it still answers "did the content
   * change?" rather than "was the button pressed?".
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

/**
 * A freshly issued password reset link.
 *
 * The token goes in the URL of the reset screen; the CMS mails nothing, so the administrator
 * hands the link on however they like.
 */
export interface PasswordResetLink {
  token: string;
  expires_at: string;
}
