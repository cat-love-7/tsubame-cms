// Loading and normalising Strapi v3's content-type and component definitions.
//
// Two sources, because either can be the only one available:
//
//   * the project directory (`--strapi-project`) - `api/*/models/*.settings.json`,
//     `components/**/*.json` and `api/*/config/routes.json`. This is what the running app was
//     built from, and it is the only place the *routes* are written down.
//   * the admin content-type-builder API - the same definitions, read from a running instance,
//     for someone without the checkout. Routes are not exposed there, so they are derived from
//     the API id, which `--route` can correct.
//
// v3 has no `strapi export` command (that arrived with v4.6), so there is no archive to read.

import { readFile, readdir, stat } from 'node:fs/promises';
import path from 'node:path';

import { pluralize } from './util.mjs';

/** Keep a component uid usable as a URL path segment and as a JSON key. */
export function sanitizeComponentId(uid) {
  return String(uid).replace(/[^A-Za-z0-9_.-]/g, '-');
}

async function readJson(file) {
  try {
    return JSON.parse(await readFile(file, 'utf8'));
  } catch (error) {
    throw new Error(`cannot read ${file}: ${error.message}`);
  }
}

async function listFiles(root, { recursive = true } = {}) {
  const found = [];
  let entries;
  try {
    entries = await readdir(root, { withFileTypes: true, recursive });
  } catch (error) {
    if (error.code === 'ENOENT') return found;
    throw error;
  }
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    // `recursive` entries carry their directory in `parentPath` (Node 20+); the fallback keeps
    // this working on an older runtime.
    const dir = entry.parentPath ?? entry.path ?? root;
    found.push(path.join(dir, entry.name));
  }
  return found;
}

/** The route v3 serves a collection type on, when `config/routes.json` says so. */
async function readCustomRoute(projectDir, apiId) {
  const file = path.join(projectDir, 'api', apiId, 'config', 'routes.json');
  let routes;
  try {
    routes = (await readJson(file)).routes;
  } catch {
    return null;
  }
  if (!Array.isArray(routes)) return null;
  // The list route is the GET whose path takes no parameter and is not the count route.
  const list = routes.find(
    (route) =>
      String(route.method).toUpperCase() === 'GET' &&
      typeof route.path === 'string' &&
      route.path.startsWith('/') &&
      !route.path.includes(':') &&
      !route.path.endsWith('/count'),
  );
  return list?.path ?? null;
}

/**
 * Read the definitions out of a Strapi v3 checkout.
 *
 * @returns {Promise<{contentTypes: object[], components: object[]}>}
 */
export async function loadDefinitionsFromProject(projectDir) {
  const resolved = path.resolve(projectDir);
  const info = await stat(resolved).catch(() => null);
  if (!info?.isDirectory()) throw new Error(`--strapi-project is not a directory: ${resolved}`);

  const components = [];
  for (const file of await listFiles(path.join(resolved, 'components'))) {
    if (!file.endsWith('.json')) continue;
    const relative = path.relative(path.join(resolved, 'components'), file);
    // `components/default/simple.json` is the component uid `default.simple`.
    const uid = relative.replace(/\.json$/, '').split(path.sep).join('.');
    const definition = await readJson(file);
    components.push({
      uid,
      id: sanitizeComponentId(uid),
      attributes: definition.attributes ?? {},
    });
  }

  const contentTypes = [];
  for (const file of await listFiles(path.join(resolved, 'api'))) {
    if (!file.endsWith('.settings.json')) continue;
    const relative = path.relative(path.join(resolved, 'api'), file);
    const [apiId] = relative.split(path.sep);
    const settings = await readJson(file);
    const kind = settings.kind === 'singleType' ? 'singleType' : 'collectionType';
    const name = settings.info?.name ?? path.basename(file, '.settings.json');
    const route =
      (await readCustomRoute(resolved, apiId)) ??
      (kind === 'singleType' ? `/${apiId}` : `/${pluralize(apiId)}`);
    contentTypes.push({
      uid: `${apiId}.${name}`,
      apiId,
      name,
      kind,
      route,
      // Strapi's draft/publish plugin is **off** unless the model asks for it
      // (`_.get(model, 'options.draftAndPublish', false)` in v3), and a content type without it
      // has no `published_at` at all - every entry is live.
      draftAndPublish: settings.options?.draftAndPublish === true,
      attributes: settings.attributes ?? {},
      source: 'project',
    });
  }

  contentTypes.sort((a, b) => a.apiId.localeCompare(b.apiId));
  components.sort((a, b) => a.id.localeCompare(b.id));
  return { contentTypes, components };
}

/** The API id of a content-type-builder entry, across the uid spellings v3 and v4 use. */
function apiIdOf(entry) {
  if (entry.apiID) return entry.apiID;
  const uid = String(entry.uid ?? '');
  const withoutPlugin = uid.includes('::') ? uid.split('::')[1] : uid;
  return withoutPlugin.split('.')[0];
}

/** Whether a content-type-builder entry belongs to the app rather than to a plugin or the core. */
function isApplicationContentType(entry) {
  if (entry.isReserved === true) return false;
  const uid = String(entry.uid ?? '');
  return !uid.startsWith('plugins::') && !uid.startsWith('strapi::');
}

/**
 * Read the same definitions from a running instance's admin API.
 *
 * The admin token is not the content API's JWT; the caller signs in with `adminLogin` first.
 */
export async function loadDefinitionsFromBuilder(client) {
  const components = [];
  for (const entry of await client.fetchComponentsFromBuilder()) {
    const uid = entry.uid ?? entry.id;
    if (!uid) continue;
    components.push({
      uid,
      id: sanitizeComponentId(uid),
      attributes: entry.schema?.attributes ?? entry.attributes ?? {},
    });
  }

  const contentTypes = [];
  for (const entry of await client.fetchContentTypesFromBuilder()) {
    if (!isApplicationContentType(entry)) continue;
    const apiId = apiIdOf(entry);
    if (!apiId) continue;
    const kind = entry.schema?.kind === 'singleType' ? 'singleType' : 'collectionType';
    contentTypes.push({
      uid: entry.uid ?? apiId,
      apiId,
      name: entry.schema?.info?.name ?? entry.schema?.name ?? apiId,
      kind,
      route: kind === 'singleType' ? `/${apiId}` : `/${pluralize(apiId)}`,
      // The content-type-builder reports the flag at the top of the schema it answers with.
      draftAndPublish: entry.schema?.draftAndPublish === true,
      attributes: entry.schema?.attributes ?? entry.attributes ?? {},
      source: 'builder',
    });
  }

  contentTypes.sort((a, b) => a.apiId.localeCompare(b.apiId));
  components.sort((a, b) => a.id.localeCompare(b.id));
  return { contentTypes, components };
}

/**
 * Apply the operator's overrides and return the definitions the migration will use.
 *
 * `nameOverrides` renames the collection in the CMS; `routeOverrides` corrects the path a
 * content type is read from. Both exist because neither the API id nor the pluralised route is
 * guaranteed to be what the project actually serves, and guessing wrong should be a flag rather
 * than an edit.
 */
export function applyOverrides({ contentTypes, components }, { nameOverrides = {}, routeOverrides = {} }) {
  for (const contentType of contentTypes) {
    if (routeOverrides[contentType.apiId]) contentType.route = routeOverrides[contentType.apiId];
    contentType.cmsName = nameOverrides[contentType.apiId] ?? contentType.apiId;
  }
  return { contentTypes, components };
}
