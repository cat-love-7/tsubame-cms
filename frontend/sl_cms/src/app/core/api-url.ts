/**
 * Prefix a backend-relative URL with the API prefix used by the dev proxy
 * (`/api` is stripped before reaching the server).
 *
 * Absolute URLs are returned untouched: an S3 presigned URL on AWS is already complete,
 * and prefixing it would break it.
 */
/**
 * The prefix every endpoint hangs off.
 *
 * One constant, because two copies of a prefix is how a client and a server drift: the
 * repositories build their paths with `apiUrl`, and the interceptor asks this whether an address
 * is one of ours already.
 */
export const API_BASE = '/api';

export function apiUrl(pathOrUrl: string): string {
  if (/^https?:\/\//i.test(pathOrUrl)) {
    return pathOrUrl;
  }
  return pathOrUrl.startsWith(API_BASE) ? pathOrUrl : `${API_BASE}${pathOrUrl}`;
}
