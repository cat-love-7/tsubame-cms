/**
 * Prefix a backend-relative URL with the API prefix.
 *
 * Nothing strips it: the CMS serves its API under `/api` (`tsubame_core::API_PREFIX` nests every
 * route), so the path a client asks for is the path the server has - the dev proxy, nginx, and
 * CloudFront all pass it through unchanged.
 *
 * A path that already names the API is returned as it came (the API hands out some of those
 * itself, like a preview link), and so is an absolute URL: an S3 presigned URL on AWS is already
 * complete, and prefixing it would break it.
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
