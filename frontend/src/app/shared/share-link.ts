import { API_BASE, apiUrl } from 'app/core/api-url';

/**
 * Shareable links, from the browser's side.
 *
 * The API hands back either a signed path (a preview link, which already carries the API
 * prefix) or a bare token (a password reset), because only the browser knows which origin it
 * reached the CMS through. One place builds those URLs, so every screen offers the same thing.
 */

/**
 * A path the API returned, as a URL a browser can open: this origin plus, if it is not already
 * there, the API prefix. `apiUrl` decides the second half - it leaves a path that already names
 * the API alone, which is what the API hands out now.
 */
export function absoluteApiUrl(path: string, origin: string = location.origin): string {
  const url = apiUrl(path);
  // An absolute URL is already an address: prefixing the origin to one would produce
  // `https://cms.example.comhttps://…`, which is nobody's intention.
  return /^https?:\/\//i.test(url) ? url : `${origin}${url}`;
}

/** A password reset token as the link an administrator passes on. */
export function passwordResetUrl(token: string, origin: string = location.origin): string {
  return `${origin}/reset-password?token=${encodeURIComponent(token)}`;
}

/**
 * A signed preview link as the page a reviewer opens, on the deployment's preview site.
 *
 * The API's own preview answer is JSON, which is not something to hand someone with no account.
 * The preview site exists to render it, and its routes mirror the API's minus the `/api` prefix
 * the API adds (`tsubame_core::API_PREFIX`): `/api/preview/collections/blog/items/7?token=…`
 * becomes `<origin>/preview/collections/blog/items/7?token=…`. The token is carried through
 * untouched, because it is the whole credential and it is what the site hands back to the API.
 *
 * `siteOrigin` is an origin - scheme, host and port - which is what a deployment reports through
 * `preview_site_url` (`tsubame_core::config::parse_preview_site_url`), so nothing here has to
 * decide whether a base path and a route meet with one slash or two.
 */
export function previewSiteUrl(path: string, siteOrigin: string): string {
  const origin = siteOrigin.replace(/\/+$/, '');
  const route = path.startsWith(API_BASE) ? path.slice(API_BASE.length) : path;
  return `${origin}${route.startsWith('/') ? route : `/${route}`}`;
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
