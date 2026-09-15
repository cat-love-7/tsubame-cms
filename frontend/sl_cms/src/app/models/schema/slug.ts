/**
 * Slugs, from the browser's side.
 *
 * The rule is the server's (`sl_cms/crates/core/src/models/slug.rs`) and the two are kept in step
 * by the same table of cases, tested on both sides. The browser needs it for two things the server
 * cannot do: telling an editor what a slug is made of while they type, and its "generate from
 * another field" button. What is stored is whatever the server normalises it to.
 */

/** The longest slug a value may normalise to. Mirrors `SLUG_MAX_LENGTH` on the server. */
export const SLUG_MAX_LENGTH = 200;

/**
 * The canonical form of `value`: lower-case ASCII letters and digits, separated by single hyphens.
 *
 * Anything else is a separator - including letters that are not ASCII, which are dropped rather
 * than transliterated. Idempotent: normalising a canonical slug changes nothing.
 */
export function normaliseSlug(value: string): string {
  let out = '';
  let separatorPending = false;
  for (const character of value) {
    if (/[a-zA-Z0-9]/.test(character)) {
      // A separator is only written once there is something to separate, which is what trims a
      // leading hyphen and collapses runs of them.
      if (separatorPending && out !== '') {
        out += '-';
      }
      separatorPending = false;
      out += character.toLowerCase();
    } else {
      separatorPending = true;
    }
  }
  return out;
}

/** Whether anything usable is left after normalisation. */
export function isUsableSlug(value: string): boolean {
  return normaliseSlug(value) !== '';
}
