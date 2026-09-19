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
 * What an administrator has to hand over after resetting an account's password.
 *
 * Two deployments, two shapes, and the answer says which. Where the CMS holds the password it
 * mints a **link** whose owner chooses the password, so nobody else ever knows it; where an
 * identity provider holds it, the provider sets a **temporary password** that the person has to
 * change at their next sign-in. Nothing is mailed either way - the administrator passes it on.
 */
export type PasswordReset =
  { kind: 'link'; token: string; expires_at: string } | { kind: 'temporary'; password: string };
