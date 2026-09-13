/**
 * Shareable preview links, from the browser's side.
 *
 * The API hands back a path relative to itself (`/preview/...?token=...`), because only the
 * browser knows which origin it reached the CMS through. Compose that here, in one place, so
 * both editors build the same URL.
 */

/** The link as a reviewer will open it: this origin, the API prefix, and the signed path. */
export function absolutePreviewUrl(path: string, origin: string = location.origin): string {
  return `${origin}/api${path}`;
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
