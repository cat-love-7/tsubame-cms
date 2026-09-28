'use strict';

const { sanitizeTypeName } = require('./naming');

/**
 * Plugin options as the rest of the plugin uses them.
 *
 * Gatsby validates plugin options against `pluginOptionsSchema`, but a plugin is also loaded by
 * `gatsby develop` with the options object as written in `gatsby-config`, and a defensive plugin
 * should not depend on the validator having run. Everything that is optional gets a default here,
 * everything that is unusable is either refused (`apiUrl`) or clamped with a warning, so no later
 * stage has to wonder whether `pageSize` is a number.
 */

/**
 * Raw plugin options as `gatsby-config` writes them, before any default is applied.
 *
 * @typedef {object} PluginOptions
 * @property {string} [apiUrl] the root of the CMS
 * @property {string} [apiPrefix] the path the API is mounted under
 * @property {number} [pageSize] items per request
 * @property {number} [requestTimeout] milliseconds before a request is abandoned
 * @property {number} [retries] extra attempts
 * @property {number} [concurrency] requests in flight
 * @property {string} [typePrefix] prefix of every generated type
 * @property {Record<string, any>} [fetchOptions] extra options for `fetch`
 * @property {PluginImageOptions} [images] the image options
 */

/**
 * The `images` plugin option as written in `gatsby-config`.
 *
 * @typedef {object} PluginImageOptions
 * @property {boolean} [download] whether to download images
 * @property {number} [concurrency] images downloaded at once
 * @property {Record<string, any>} [requestHeaders] headers sent when fetching an image
 */

/**
 * The `images` option after normalization.
 *
 * @typedef {object} NormalizedImageOptions
 * @property {boolean} download whether to download images
 * @property {number} concurrency images downloaded at once
 * @property {Record<string, any>} requestHeaders headers sent when fetching an image
 */

/**
 * Plugin options as the rest of the plugin uses them: every value present and usable.
 *
 * @typedef {object} NormalizedOptions
 * @property {string} apiUrl the root of the CMS
 * @property {string} apiPrefix the path the API is mounted under
 * @property {number} pageSize items per request
 * @property {number} requestTimeout milliseconds before a request is abandoned
 * @property {number} retries extra attempts
 * @property {number} concurrency requests in flight
 * @property {string} typePrefix prefix of every generated type
 * @property {Record<string, any>} fetchOptions extra options for `fetch`
 * @property {NormalizedImageOptions} images the image options
 */

/**
 * The one reporter method normalization needs. A fuller reporter is assignable to it.
 *
 * @typedef {object} WarningReporter
 * @property {(message: string) => void} warn reports a warning
 */

/** The delivery API lives under `/api`; see `docs/content-api.md`. */
const DEFAULT_API_PREFIX = '/api';

/** The API's own default page size; it refuses anything over 200. */
const DEFAULT_PAGE_SIZE = 50;
const MAX_PAGE_SIZE = 200;

const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_RETRIES = 2;
const DEFAULT_CONCURRENCY = 4;
const DEFAULT_TYPE_PREFIX = 'Tsubame';
const DEFAULT_IMAGE_CONCURRENCY = 4;

const noopReporter = { warn() {}, info() {}, verbose() {} };

/**
 * Read the options once, at the edge.
 *
 * `apiUrl` is the root of the CMS (`http://127.0.0.1:8000`), not the API base: the `/api` prefix
 * is added from `apiPrefix`. Requiring the prefix to be passed twice is how a deployment ends up
 * with `/api/api/content/...` in it.
 *
 * @param {PluginOptions} [pluginOptions] the options as Gatsby or `gatsby-config` wrote them
 * @param {WarningReporter} [reporter] where to report a clamped or ignored option
 * @returns {NormalizedOptions} the options the plugin uses
 */
function normalizeOptions(pluginOptions = {}, reporter = noopReporter) {
  const apiUrl = normalizeApiUrl(pluginOptions.apiUrl, reporter);

  return {
    apiUrl,
    apiPrefix: normalizeApiPrefix(pluginOptions.apiPrefix, reporter),
    pageSize: clampInteger(pluginOptions.pageSize, DEFAULT_PAGE_SIZE, 1, MAX_PAGE_SIZE, 'pageSize', reporter),
    requestTimeout: clampInteger(
      pluginOptions.requestTimeout,
      DEFAULT_TIMEOUT_MS,
      1,
      Number.MAX_SAFE_INTEGER,
      'requestTimeout',
      reporter,
    ),
    retries: clampInteger(pluginOptions.retries, DEFAULT_RETRIES, 0, 10, 'retries', reporter),
    concurrency: clampInteger(pluginOptions.concurrency, DEFAULT_CONCURRENCY, 1, 16, 'concurrency', reporter),
    typePrefix: normalizeTypePrefix(pluginOptions.typePrefix, reporter),
    fetchOptions: normalizeFetchOptions(pluginOptions.fetchOptions, reporter, 'fetchOptions'),
    images: normalizeImages(pluginOptions.images, reporter),
  };
}

/**
 * @param {unknown} value the `apiUrl` option
 * @param {WarningReporter} reporter where to report a problem
 * @returns {string} the root of the CMS, without a trailing slash
 */
function normalizeApiUrl(value, reporter) {
  const apiUrl = typeof value === 'string' ? value.trim().replace(/\/+$/, '') : '';
  if (apiUrl === '') {
    throw new Error(
      '[gatsby-source-tsubame] `apiUrl` is required and must be the root of the CMS, for example ' +
        '`http://127.0.0.1:8000` (the `/api` prefix is added automatically).',
    );
  }
  return apiUrl;
}

/**
 * @param {unknown} value the `apiPrefix` option
 * @param {WarningReporter} reporter where to report a problem
 * @returns {string} the path the API is mounted under, always starting with `/`
 */
function normalizeApiPrefix(value, reporter) {
  if (value === undefined || value === null || value === '') {
    return DEFAULT_API_PREFIX;
  }
  if (typeof value !== 'string') {
    reporter.warn(`[gatsby-source-tsubame] \`apiPrefix\` should be a string; using "${DEFAULT_API_PREFIX}".`);
    return DEFAULT_API_PREFIX;
  }
  let prefix = value.trim();
  if (!prefix.startsWith('/')) {
    prefix = `/${prefix}`;
  }
  prefix = prefix.replace(/\/+$/, '');
  return prefix === '' ? DEFAULT_API_PREFIX : prefix;
}

/**
 * @param {unknown} value the `typePrefix` option
 * @param {WarningReporter} reporter where to report a rewritten prefix
 * @returns {string} a prefix GraphQL accepts
 */
function normalizeTypePrefix(value, reporter) {
  const raw = value === undefined || value === null || value === '' ? DEFAULT_TYPE_PREFIX : String(value);
  const sanitized = sanitizeTypeName(raw);
  if (sanitized !== raw) {
    reporter.warn(
      `[gatsby-source-tsubame] \`typePrefix\` "${raw}" is not usable as a GraphQL name; using "${sanitized}".`,
    );
  }
  return sanitized;
}

/**
 * @param {unknown} value the option to read
 * @param {WarningReporter} reporter where to report the wrong type
 * @param {string} name the option's name, for the warning
 * @returns {Record<string, any>} the option when it is a plain object, `{}` otherwise
 */
function normalizeFetchOptions(value, reporter, name) {
  if (value === undefined || value === null) {
    return {};
  }
  if (typeof value !== 'object' || Array.isArray(value)) {
    reporter.warn(`[gatsby-source-tsubame] \`${name}\` should be an object; ignoring it.`);
    return {};
  }
  return /** @type {Record<string, any>} */ (value);
}

/**
 * Whether to download the CMS's images, and how.
 *
 * Off by default: downloading every image is real work and needs `gatsby-source-filesystem`, and a
 * site that renders the API's URLs directly (or uses `gatsby-plugin-image` on its own files) does
 * not want either. On, the plugin creates a `File` node per image and links it, which is what
 * `gatsby-transformer-sharp` reads.
 *
 * @param {unknown} value the `images` option
 * @param {WarningReporter} reporter where to report a wrong type
 * @returns {NormalizedImageOptions} the image options the plugin uses
 */
function normalizeImages(value, reporter) {
  const raw = /** @type {PluginImageOptions} */ (value === undefined || value === null ? {} : value);
  if (typeof raw !== 'object' || Array.isArray(raw)) {
    reporter.warn('[gatsby-source-tsubame] `images` should be an object; ignoring it.');
    return { download: false, concurrency: DEFAULT_IMAGE_CONCURRENCY, requestHeaders: {} };
  }
  return {
    download: raw.download === true,
    concurrency: clampInteger(raw.concurrency, DEFAULT_IMAGE_CONCURRENCY, 1, 16, 'images.concurrency', reporter),
    requestHeaders: normalizeFetchOptions(raw.requestHeaders, reporter, 'images.requestHeaders'),
  };
}

/**
 * A whole number inside a range, or the default with a warning.
 *
 * Clamping rather than refusing: a page size over the API's maximum is a request the CMS would
 * answer with a 400, and "50 instead of 500" is a build that works, which is a better outcome than
 * a build that stops over an option that has an obvious nearest usable value.
 *
 * @param {unknown} value the option to read
 * @param {number} fallback the value to use when it is missing or not a whole number
 * @param {number} min the smallest usable value
 * @param {number} max the largest usable value
 * @param {string} name the option's name, for the warning
 * @param {WarningReporter} reporter where to report a clamped value
 * @returns {number} the whole number to use
 */
function clampInteger(value, fallback, min, max, name, reporter) {
  if (value === undefined || value === null) {
    return fallback;
  }
  const parsed = typeof value === 'number' ? value : Number(value);
  if (!Number.isInteger(parsed)) {
    reporter.warn(`[gatsby-source-tsubame] \`${name}\` should be an integer; using ${fallback}.`);
    return fallback;
  }
  if (parsed < min || parsed > max) {
    const clamped = Math.min(Math.max(parsed, min), max);
    reporter.warn(`[gatsby-source-tsubame] \`${name}\` ${parsed} is outside ${min}..${max}; using ${clamped}.`);
    return clamped;
  }
  return parsed;
}

module.exports = {
  normalizeOptions,
  DEFAULT_API_PREFIX,
  DEFAULT_PAGE_SIZE,
  MAX_PAGE_SIZE,
  DEFAULT_TYPE_PREFIX,
};
