'use strict';

/**
 * gatsby-source-tsubame
 *
 * Sources published content from a Tsubame delivery API (`/api/content/*`) and declares a GraphQL
 * type per collection and per single page. Markdown values become `text/markdown` child nodes, so
 * adding `gatsby-transformer-remark` is all it takes for them to arrive as HTML.
 *
 * The two hooks are split the way Gatsby's own bootstrap is: `createSchemaCustomization` runs
 * before `sourceNodes`, so the schema is read once to declare the types and once to source the
 * values. Neither hook needs a token: the delivery API serves published content to anyone, which is
 * the contract `docs/content-api.md` defines for a site build.
 */

const { normalizeOptions } = require('./src/options');
const { TsubameClient } = require('./src/client');
const { buildContentModel } = require('./src/model');
const { buildTypeDefinitions } = require('./src/types');
const { sourceAll } = require('./src/nodes');
const { loadRemoteFileCreator } = require('./src/images');

exports.pluginOptionsSchema = ({ Joi }) =>
  Joi.object({
    apiUrl: Joi.string()
      .uri({ scheme: ['http', 'https'] })
      .required()
      .description(
        'The root of the CMS, for example `http://127.0.0.1:8000`. The `/api` prefix is added ' +
          'automatically; do not include it here unless the deployment uses a different prefix ' +
          '(`apiPrefix`).',
      ),
    apiPrefix: Joi.string()
      .pattern(/^\//)
      .description('The path the API is mounted under. Defaults to `/api`.'),
    pageSize: Joi.number()
      .integer()
      .min(1)
      .max(200)
      .description('Items per request while walking a collection. The CMS refuses more than 200. Defaults to 50.'),
    typePrefix: Joi.string()
      .pattern(/^[_A-Za-z][_0-9A-Za-z]*$/)
      .description('Prefix of every generated GraphQL type. Defaults to `Tsubame`.'),
    requestTimeout: Joi.number()
      .integer()
      .min(1)
      .description('Milliseconds before a request is abandoned. Defaults to 30000.'),
    retries: Joi.number()
      .integer()
      .min(0)
      .max(10)
      .description('Extra attempts for a network failure or a 5xx/429 answer. Defaults to 2.'),
    concurrency: Joi.number()
      .integer()
      .min(1)
      .max(16)
      .description('Requests in flight while reading schemas. Defaults to 4.'),
    fetchOptions: Joi.object()
      .unknown(true)
      .description('Extra options passed to `fetch`, e.g. `{ headers: { authorization: "..." } }` for a proxied deployment.'),
    images: Joi.object({
      download: Joi.boolean()
        .default(false)
        .description(
          'Download every CMS image and create a `File` node for it, so gatsby-transformer-sharp ' +
            'can process it (`image { localFile { childImageSharp { gatsbyImageData } } }`). ' +
            'Needs gatsby-source-filesystem. Defaults to false.',
        ),
      concurrency: Joi.number()
        .integer()
        .min(1)
        .max(16)
        .description('Images downloaded at once. Defaults to 4.'),
      requestHeaders: Joi.object()
        .unknown(true)
        .description('Headers sent when fetching an image, e.g. for a deployment that gates its files.'),
    }).description('Image files: whether to download them for the sharp plugins.'),
  });

exports.createSchemaCustomization = async ({ actions, reporter }, pluginOptions) => {
  const options = normalizeOptions(pluginOptions, reporter);

  // Declaring `localFile: File` only works if the type is there, which means the package has to be
  // installed. Asking now is what turns "Unknown type File" into a sentence with the install in it.
  if (options.images.download) {
    loadRemoteFileCreator(reporter);
  }

  const client = new TsubameClient(options, { reporter });

  const snapshot = await client.fetchSchemaSnapshot();
  const model = buildContentModel(snapshot, options);

  actions.createTypes(buildTypeDefinitions(model));
  reporter.verbose(
    `[gatsby-source-tsubame] declared ${snapshot.collections.size} collection type(s), ` +
      `${model.plan.pages.size} single-page type(s) and ${snapshot.composites.size} composite type(s)`,
  );
};

exports.sourceNodes = async (gatsbyApi, pluginOptions) => {
  const options = normalizeOptions(pluginOptions, gatsbyApi.reporter);
  const client = new TsubameClient(options, { reporter: gatsbyApi.reporter });

  const counters = await sourceAll(gatsbyApi, client, options);

  gatsbyApi.reporter.info(
    `[gatsby-source-tsubame] sourced ${counters.items} item(s) from ${counters.collections} collection(s), ` +
      `${counters.pages} single page(s), and ${counters.markdown} markdown field(s)`,
  );
};
