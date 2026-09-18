/**
 * The links the API hands out: a preview of a working copy, and a password reset.
 *
 * Their own file because they are not item state: `item-status.ts` is about where an item sits
 * between draft and published, and these two are the answers to two endpoints.
 */

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
