/**
 * Human-readable text for an error coming out of `HttpClient`.
 *
 * The API answers errors with a plain-text body, so prefer that; fall back to the status
 * line and then to `String(error)` so nothing is ever rendered as `[object Object]`.
 */
export function errorMessage(error: unknown): string {
  const http = error as { error?: unknown; message?: unknown; status?: number };
  if (typeof http?.error === 'string' && http.error) {
    return http.error;
  }
  if (typeof http?.status === 'number' && http.status > 0) {
    const statusText = typeof http.message === 'string' ? http.message : '';
    return `${http.status} ${statusText}`.trim();
  }
  return String(http?.message ?? error);
}
