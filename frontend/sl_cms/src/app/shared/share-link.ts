/**
 * Shareable links, from the browser's side.
 *
 * The API hands back either a signed path relative to itself (a preview link) or a bare token
 * (a password reset), because only the browser knows which origin it reached the CMS through.
 * One place builds those URLs, so every screen offers the same thing.
 */

/** A path the API returned, as a URL a browser can open: this origin plus the API prefix. */
export function absoluteApiUrl(path: string, origin: string = location.origin): string {
  return `${origin}/api${path}`;
}

/** A password reset token as the link an administrator passes on. */
export function passwordResetUrl(token: string, origin: string = location.origin): string {
  return `${origin}/reset-password?token=${encodeURIComponent(token)}`;
}

/**
 * Put `text` on the clipboard, answering whether it worked.
 *
 * Clipboard access needs a secure context and, in some browsers, permission; a refusal is
 * not an error worth showing. The caller displays the URL either way, so it can still be
 * copied by hand.
 */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
