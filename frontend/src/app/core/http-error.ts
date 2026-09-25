import errorCodes from '../../assets/error-codes.json';

/**
 * Human-readable text for an error coming out of `HttpClient`.
 *
 * The API answers errors with `{"code": …, "message": …}`, where the message is English: it
 * is what a client that does not recognise the code can honestly show, and what an operator
 * sees in a log. A screen that knows the code shows its own wording instead
 * (`errorKey` below), leaving this as the fallback.
 */
export function errorMessage(error: unknown): string {
  const http = error as { error?: unknown; message?: unknown; status?: number };
  if (typeof http?.error === 'string' && http.error) {
    return http.error;
  }
  const body = http?.error as { message?: unknown } | null | undefined;
  if (body && typeof body === 'object' && typeof body.message === 'string' && body.message) {
    return body.message;
  }
  if (typeof http?.status === 'number' && http.status > 0) {
    const statusText = typeof http.message === 'string' ? http.message : '';
    return `${http.status} ${statusText}`.trim();
  }
  return String(http?.message ?? error);
}

/** Every code this client can put a name to. The server publishes both lists (see below). */
const KNOWN_CODES: readonly string[] = [...errorCodes.situational, ...errorCodes.status];

/** The codes that only stand for a status, where `message` is the part that says what happened. */
const STATUS_CODES: readonly string[] = errorCodes.status;

/** The code the server named, when it named one at all. */
export function errorCode(error: unknown): string | null {
  const body = (error as { error?: unknown })?.error as { code?: unknown } | null | undefined;
  const code = body && typeof body === 'object' ? body.code : undefined;
  return typeof code === 'string' && code ? code : null;
}

/**
 * The translation key for an error, when the deployment named a code this client knows.
 *
 * `null` means "show the server's message": a code added since this client was built is exactly
 * the case the English message exists for. Which codes this client knows is the list the server
 * publishes as a file — a Rust test keeps the server's copy equal to it, and a catalog test
 * keeps the translations covering it, so the three cannot drift apart.
 */
export function errorKey(error: unknown): string | null {
  const code = errorCode(error);
  return code !== null && KNOWN_CODES.includes(code) ? `errors.${code}` : null;
}

/**
 * Whether the code says no more than the status did.
 *
 * `bad_request` is answered for a missing field, a duplicate name and a dozen other things, so a
 * screen that replaced the server's sentence with "the request was not accepted" would throw away
 * the only part that helps. The situational codes are the opposite: they *are* the reason, and
 * wording them here is what makes them readable without English.
 */
export function isStatusError(error: unknown): boolean {
  const code = errorCode(error);
  return code !== null && STATUS_CODES.includes(code);
}
