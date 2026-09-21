// Shared helpers: an HTTP core with retries, a small worker pool, and logging.
//
// Deliberately dependency-free (Node's global fetch/AbortSignal only), so the migration can be
// run from a checkout with nothing installed.

/**
 * A non-2xx answer, carrying enough of the body to explain it.
 *
 * The CMS answers a refusal as `{ "code", "message", "field"? }`, and the migration report
 * quotes `message` rather than a bare status, because an operator reading the report needs to
 * know *what* the server disliked.
 */
export class HttpError extends Error {
  constructor(status, method, url, body) {
    super(`${method} ${url} answered ${status}: ${describeBody(body)}`);
    this.name = 'HttpError';
    this.status = status;
    this.method = method;
    this.url = url;
    this.body = body;
  }
}

function describeBody(body) {
  if (body === undefined || body === null) return '(empty body)';
  if (typeof body === 'string') return body.slice(0, 500);
  try {
    return JSON.stringify(body).slice(0, 500);
  } catch {
    return String(body);
  }
}

const RETRYABLE = new Set([408, 425, 429, 500, 502, 503, 504]);

/** Whether a failure is worth trying again: a network drop, a throttle, or a 5xx. */
function isRetryable(error) {
  if (error instanceof HttpError) return RETRYABLE.has(error.status);
  // A DNS failure, a reset connection, or an aborted request all arrive as plain Errors.
  return true;
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * One HTTP call with bounded retries.
 *
 * Returns `{ status, headers, buffer }`; decoding is left to the callers so one core serves
 * JSON requests and image bytes alike. Any non-2xx answer becomes an `HttpError` (after the
 * retries a transient status deserves).
 */
export async function fetchWithRetry(url, options = {}) {
  const {
    method = 'GET',
    token,
    headers = {},
    body,
    retries = 3,
    // A whole image can be slow to upload on a thin link, and a Strapi list page can be slow to
    // build; 30s is generous for a JSON call and short enough to notice a hung server.
    timeoutMs = 30_000,
    expect = 'json',
  } = options;

  const requestHeaders = { ...headers };
  if (token) requestHeaders.Authorization = `Bearer ${token}`;

  let lastError;
  for (let attempt = 0; attempt <= retries; attempt += 1) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const response = await fetch(url, {
        method,
        headers: requestHeaders,
        body,
        signal: controller.signal,
        redirect: 'follow',
      });
      const buffer = Buffer.from(await response.arrayBuffer());
      if (!response.ok) {
        throw new HttpError(response.status, method, url, decodeBody(buffer));
      }
      if (expect === 'none') return { status: response.status, headers: response.headers };
      return { status: response.status, headers: response.headers, buffer };
    } catch (error) {
      lastError = error;
      if (attempt === retries || !isRetryable(error)) break;
      // 1s, 2s, 4s - long enough for a throttled endpoint to recover, short enough to keep a
      // migration moving.
      await sleep(1000 * 2 ** attempt);
    } finally {
      clearTimeout(timer);
    }
  }
  throw lastError;
}

function decodeBody(buffer) {
  const text = buffer.toString('utf8');
  if (text.length === 0) return undefined;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/** A JSON request: parse the answer, or `undefined` for `204`/an empty body. */
export async function requestJson(url, options = {}) {
  const { buffer, status } = await fetchWithRetry(url, options);
  if (!buffer || buffer.length === 0) return { status, json: undefined };
  const text = buffer.toString('utf8');
  try {
    return { status, json: JSON.parse(text) };
  } catch {
    throw new Error(`${options.method ?? 'GET'} ${url} did not answer JSON: ${text.slice(0, 200)}`);
  }
}

/** A request whose body does not matter (create, publish, delete). */
export async function requestVoid(url, options = {}) {
  await fetchWithRetry(url, { ...options, expect: 'none' });
}

/**
 * Run `worker` over `items` with at most `limit` in flight, preserving input order.
 *
 * A rejected worker does not stop the run: the migration has to report *which* entries failed
 * and keep going, so failures come back as `{ error }` beside the successes.
 */
export async function pMap(items, limit, worker) {
  const results = new Array(items.length);
  let next = 0;
  const size = Math.max(1, Math.min(limit, items.length));
  const runners = Array.from({ length: size }, async () => {
    for (;;) {
      const index = next;
      next += 1;
      if (index >= items.length) return;
      try {
        results[index] = { value: await worker(items[index], index) };
      } catch (error) {
        results[index] = { error };
      }
    }
  });
  await Promise.all(runners);
  return results;
}

/**
 * The plural Strapi v3 derives a collection route from, for projects with no `config/routes.json`.
 *
 * This is a small approximation of the `pluralize` package Strapi uses, covering the regular
 * English cases. A project that names things irregularly (or renames its routes) is what the
 * `--route` override is for; guessing wrong is visible in `--dry-run` rather than silent.
 */
export function pluralize(word) {
  const lower = word.toLowerCase();
  if (/(s|x|z|ch|sh)$/.test(lower)) return `${word}es`;
  if (/[^aeiou]y$/.test(lower)) return `${word.slice(0, -1)}ies`;
  if (/(f)$/.test(lower)) return `${word.slice(0, -1)}ves`;
  if (/(fe)$/.test(lower)) return `${word.slice(0, -2)}ves`;
  return `${word}s`;
}

/** Whether a value counts as "nothing" for the CMS's `required` rule. */
export function isEmptyValue(fieldType, value) {
  if (value === undefined || value === null) return true;
  if (typeof fieldType === 'string') {
    switch (fieldType) {
      case 'Text':
      case 'Slug':
      case 'Markdown':
        return typeof value !== 'string' || value.length === 0;
      case 'Number':
        return typeof value !== 'number' || Number.isNaN(value);
      case 'Boolean':
        // `false` is a value the editor chose; only absence is missing.
        return typeof value !== 'boolean';
      case 'Date':
      case 'DateTime':
      case 'Image':
        return value === null;
      default:
        return true;
    }
  }
  const [variant, argument] = Object.entries(fieldType)[0];
  switch (variant) {
    case 'Array':
    case 'TextEnum':
      return !Array.isArray(value) || value.length === 0;
    case 'CompositeField':
      return value === null || value === undefined;
    default:
      return isEmptyValue(variant, value);
  }
}

/** A logging facade so `--quiet` and `--verbose` are decided in one place. */
export function createLogger({ quiet = false, verbose = false } = {}) {
  const write = (stream, prefix, message) => {
    stream.write(`${prefix}${message}\n`);
  };
  return {
    info: (message) => {
      if (!quiet) write(process.stdout, '', message);
    },
    step: (message) => {
      if (!quiet) write(process.stdout, '', `== ${message} ==`);
    },
    warn: (message) => write(process.stderr, 'warning: ', message),
    error: (message) => write(process.stderr, 'error: ', message),
    debug: (message) => {
      if (verbose) write(process.stdout, '    ', message);
    },
  };
}
