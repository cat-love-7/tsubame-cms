/**
 * Prefix a backend-relative URL with the API prefix used by the dev proxy
 * (`/api` is stripped before reaching the server).
 *
 * Absolute URLs are returned untouched: an S3 presigned URL on AWS is already complete,
 * and prefixing it would break it.
 */
export function apiUrl(pathOrUrl: string): string {
  if (/^https?:\/\//i.test(pathOrUrl)) {
    return pathOrUrl;
  }
  return pathOrUrl.startsWith('/api') ? pathOrUrl : `/api${pathOrUrl}`;
}
