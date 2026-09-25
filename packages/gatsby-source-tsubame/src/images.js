'use strict';

const { mapWithConcurrency } = require('./client');
const { imageKey } = require('./fields');

/**
 * Downloading the CMS's images into Gatsby's cache, so the sharp plugins can process them.
 *
 * `gatsby-transformer-sharp` only looks at `File` nodes whose `internal.mediaType` is an image
 * type, and it reads the bytes from disk - a remote URL is nothing it can use. So an image the CMS
 * serves has to become a local file first, and the source plugin is the only one that knows the
 * URL. `createRemoteFileNode` from `gatsby-source-filesystem` is the documented way to do that: it
 * fetches the URL into the cache, creates the `File` node (owned by `gatsby-source-filesystem`,
 * because the type is its), and returns it so the value can link to it.
 *
 * `gatsby-source-filesystem` is required lazily and only when the option is on. The plugin's whole
 * point is that a site which does not use sharp installs nothing extra, and a build that never
 * downloads an image should not need the package that fetches them.
 */

/**
 * `createRemoteFileNode`, or a refusal that says what to install.
 *
 * Resolved from the site root first, not from this file's own directory: a plugin installed by path
 * (a checkout, a workspace) sits outside the site's `node_modules`, and a plain `require` there
 * would look up the plugin's own tree and miss the package the site installed. Resolving from the
 * site also returns the same module instance Gatsby loaded as a plugin, which keeps the download
 * cache shared.
 *
 * `reporter.panic` rather than a thrown error: this is a build that cannot proceed, not a page that
 * cannot render, and the message is the whole value.
 */
function loadRemoteFileCreator(reporter) {
  let filesystem = null;
  for (const base of [process.cwd(), __dirname]) {
    try {
      filesystem = require(require.resolve('gatsby-source-filesystem', { paths: [base] }));
      break;
    } catch (error) {
      filesystem = null;
    }
  }

  if (filesystem === null || typeof filesystem.createRemoteFileNode !== 'function') {
    reporter.panic(
      '[gatsby-source-tsubame] `images.download` needs gatsby-source-filesystem. Install and ' +
        'configure it (with gatsby-plugin-sharp and gatsby-transformer-sharp): ' +
        '`npm install gatsby-source-filesystem gatsby-plugin-sharp gatsby-transformer-sharp`.',
    );
  }
  return filesystem.createRemoteFileNode;
}

/**
 * Fetch every image and answer the `File` node id for each, by image key.
 *
 * An image whose bytes are already in the cache is **reused** rather than fetched again: the file
 * this CMS serves for a given object key never changes, so a `File` node from an earlier build can
 * be kept as long as the identity matches (see [`sourceKey`]). That matters most for a deployment
 * that serves **presigned** URLs: the signature is in the query, so the URL differs on every read
 * while the object is the same, and re-fetching on every build would also give the `File` a new id
 * and make sharp reprocess every image. A reused node is touched so Gatsby's stale-node cleanup
 * keeps it - it is a source node of `gatsby-source-filesystem`, and it was not created this run.
 *
 * A download that fails is a warning and a missing `localFile`, not a failed build: an image the
 * CMS no longer serves should cost one picture, not the whole site. The image's own `url`/`id`
 * fields still carry the remote location either way.
 *
 * @param {object} params `createRemoteFileNode` is injectable for tests; otherwise it is loaded.
 * @returns {Promise<Map<string, string>>} image key -> `File` node id
 */
async function downloadImages({ gatsbyApi, client, images, options, createRemoteFileNode }) {
  const { reporter, actions, createNodeId, cache, getCache, getNodesByType } = gatsbyApi;
  const creator = createRemoteFileNode ?? loadRemoteFileCreator(reporter);
  const existing = reusableFiles(getNodesByType);

  const downloaded = await mapWithConcurrency(images, options.images.concurrency, async (image) => {
    const source = imageSource(image, client);
    if (source === null) {
      return null;
    }

    const reusable = source.key === null ? undefined : existing.get(source.key);
    if (reusable !== undefined) {
      actions.touchNode(reusable);
      return [imageKey(image), reusable.id];
    }

    try {
      const fileNode = await creator({
        url: source.url,
        cache,
        getCache,
        createNode: actions.createNode,
        createNodeId,
        httpHeaders: options.images.requestHeaders,
        // `create-file-node.js` reads the media type off the file's extension, and sharp only
        // looks at images, so the URL's extension is what makes the File usable.
        ext: extensionOf(image.url),
      });
      return [imageKey(image), fileNode.id];
    } catch (error) {
      reporter.warn(`[gatsby-source-tsubame] could not download image ${source.url}: ${error.message}`);
      return null;
    }
  });

  return new Map(downloaded.filter((entry) => entry !== null));
}

/**
 * The `File` nodes a build could reuse, by the identity of the bytes they hold.
 *
 * `File` nodes carry the `url` they were fetched from (`createRemoteFileNode` sets it), which is the
 * only thing that says which of them are this plugin's remote images: a locally sourced file has no
 * `url`, and another plugin's would have a different one.
 */
function reusableFiles(getNodesByType) {
  const byKey = new Map();
  if (typeof getNodesByType !== 'function') {
    return byKey;
  }
  for (const node of getNodesByType('File')) {
    if (node === null || typeof node !== 'object' || typeof node.url !== 'string' || node.url === '') {
      continue;
    }
    const key = sourceKey(node.url);
    if (key !== null && !byKey.has(key)) {
      byKey.set(key, node);
    }
  }
  return byKey;
}

/**
 * Where an image's bytes are fetched from, and the identity those bytes are reused by.
 *
 * `key` is null for the id link - see [`sourceKey`] for why that one can never be reused.
 */
function imageSource(image, client) {
  if (typeof image.url === 'string' && image.url !== '') {
    const url = client.absoluteUrl(image.url);
    return url === null ? null : { url, key: sourceKey(url) };
  }
  if (typeof image.id === 'number') {
    const url = client.apiPathUrl(`/images/by-id/${image.id}`);
    return url === null ? null : { url, key: null };
  }
  return null;
}

/**
 * The identity of the bytes a URL points at: the URL without its query.
 *
 * This is what makes reuse work for a deployment that serves **presigned** URLs. The signature lives
 * in the query and is different on every read, so the URL is not an identity - but the path is the
 * object key, and on this CMS the object key *is* the identity: an upload gets a fresh random file
 * name, and a replacement writes a new one and deletes the old
 * (`backend/crates/on-premises/src/repository/images.rs`, and the same shape on S3). So
 * `origin + pathname` is stable while the image is the same and changes exactly when it is
 * replaced.
 *
 * The query has to be dropped through the URL parser rather than by splitting on `/`: the CMS itself
 * learned that a signature looks like part of the file name when a URL is read as a string
 * (`docs/content-api.md`, "差し替えの適用…"), and it is the same mistake here. The id link is
 * deliberately given no key: it resolves to whatever the object is now, so its path survives a
 * replacement and reusing it would keep serving the picture that was replaced.
 */
function sourceKey(url) {
  try {
    const parsed = new URL(url);
    return `${parsed.origin}${parsed.pathname}`;
  } catch {
    return null;
  }
}

/** The URL's extension, dot included, when it looks like one; null lets the caller derive it. */
function extensionOf(url) {
  if (typeof url !== 'string') {
    return null;
  }
  const match = /\.([0-9A-Za-z]+)(?:[?#]|$)/.exec(url);
  if (match === null) {
    return null;
  }
  const extension = match[1].toLowerCase();
  return /^[a-z0-9]{1,5}$/.test(extension) ? `.${extension}` : null;
}

module.exports = { downloadImages, loadRemoteFileCreator, imageSource, sourceKey, reusableFiles, extensionOf };
