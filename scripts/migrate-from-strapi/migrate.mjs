#!/usr/bin/env node
// Migrate content from a Strapi v3 instance into this CMS.
//
// The shape of the job, in one place:
//
//   1. read Strapi's definitions (project directory, or the admin API)
//   2. create the components as composite-field definitions, in dependency order
//   3. create a collection / single-page schema for each Strapi content type
//   4. upload the media library, remembering Strapi file id -> CMS image id
//   5. read every entry, translate it, create it, publish it
//
// v3 has no `strapi export` (that arrived with v4.6), so there is no archive to read: entries
// come from the content API and definitions from the project or the admin API. Running the
// migration again after it stops is safe - the state file records what has been written.
//
// Usage: node scripts/migrate-from-strapi/migrate.mjs --help

import { mkdir, writeFile } from 'node:fs/promises';
import path from 'node:path';

import { CmsClient } from './lib/cms.mjs';
import { applyOverrides, loadDefinitionsFromBuilder, loadDefinitionsFromProject } from './lib/definitions.mjs';
import {
  FieldKind,
  chooseTitleField,
  convertEntry,
  entryDates,
  planContentType,
  relationOwnerProblems,
  relationReport,
  toDateTime,
} from './lib/mapping.mjs';
import {
  displayFilename,
  fileExtension,
  orderComponents,
  orderForPublish,
  sanitizeName,
  shouldPublish,
  thumbnailFor,
  withoutUnavailableReferences,
} from './lib/plan.mjs';
import { MigrationState } from './lib/state.mjs';
import { StrapiV3Client } from './lib/strapi.mjs';
import { createLogger, pMap } from './lib/util.mjs';

const USAGE = `Migrate content from a Strapi v3 instance into this CMS.

  node migrate.mjs --strapi-url <url> --strapi-project <dir> \\
                   --cms-url <url> --cms-username <name> --cms-password <pass>

Source (Strapi v3)
  --strapi-url <url>          Base URL of the running Strapi (default http://localhost:1337)
  --strapi-prefix <path>      config/middleware.js's settings.router.prefix (stock v3: empty)
  --strapi-jwt <token>        Content-API JWT to use instead of signing in
  --strapi-identifier <id>    Users & Permissions identifier (email or username)
  --strapi-password <pass>    Its password
  --strapi-project <dir>      Checkout to read content-type/component definitions from
  --strapi-admin-token <tok>  Admin token, to read definitions from the running instance instead
  --strapi-admin-email <mail> Admin email, to sign in and read definitions
  --strapi-admin-password <pw>
  --publication-state <state> live | preview (default preview: drafts are carried too)
  --locale <code>             Only this Strapi locale, or \`all\` (this CMS has no content
                              localisation, so \`all\` puts every locale in one collection)
  --route <apiId>=<path>      Override the path a content type is read from (repeatable)
  --populate <a,b>            Force a populate parameter; v3 auto-populates, so this is only for
                              a project that switched autoPopulate off (repeatable)

Target (this CMS)
  --cms-url <url>             Base URL (default http://127.0.0.1:8080)
  --cms-token <token>         Bearer token instead of signing in (needed on a Cognito deployment)
  --cms-username <name>       Administrator's sign-in name
  --cms-password <pass>       Its password

What to do
  --only <apiId,apiId>        Migrate only these content types
  --skip <apiId,apiId>        Skip these content types
  --name <apiId>=<name>       Rename the collection in the CMS (repeatable)
  --relations <mode>          skip (default) | text | relation
                                skip     drop relations, and report every one
                                text     keep the target ids as a text field
                                relation write real references (needs the targets migrated and
                                         runs a second pass, and publishes in dependency order)
  --relation-owner <apiId>.<field>
                              Which side of a mutual relation is migrated (repeatable). The other
                              side is reported and skipped, because the CMS stores a link once and
                              derives the reverse from its index. Without this the side holding
                              the single reference wins (so a oneToMany is written from its "many"
                              end), and a many-to-many is settled by name.
  --no-schema                 Do not create or update composite/collection/page schemas
  --no-images                 Do not migrate the media library
  --no-items                  Migrate no entries
  --no-publish                Leave every migrated entry as a draft
  --publish-all               Publish every entry, even one Strapi never published
                              (default: an entry is published only if Strapi had published it)
  --no-dates                  Do not restore created/updated/published dates
  --no-title-field            Do not mark a field as the one that names an item
  --max-items <n>             Stop after n entries per content type (for a trial run)
  --max-images <n>            Stop after n media files
  --page-size <n>             Entries per Strapi request (default 100)
  --concurrency <n>           Parallel uploads/creates (default 4)

Running it
  --list-relations            Print every relation, which side the run would keep, and the flag
                              that keeps the other one; then stop (touches neither CMS)
  --dry-run                   Read Strapi and report the plan; write nothing, contact no CMS
  --sample <n>                With --dry-run, print n converted entries per content type (default 1)
  --state <file>              Where the resume state lives (default ./.strapi-migration/state.json)
  --report <file>             Where the JSON report is written (default ./.strapi-migration/report.json)
  --force                     Ignore the state file and migrate everything again
  --quiet / --verbose

Environment variables mirror the flags: CMS_URL, CMS_TOKEN, CMS_USERNAME, CMS_PASSWORD,
STRAPI_URL, STRAPI_PREFIX, STRAPI_JWT, STRAPI_IDENTIFIER, STRAPI_PASSWORD, STRAPI_PROJECT,
STRAPI_ADMIN_TOKEN, STRAPI_ADMIN_EMAIL, STRAPI_ADMIN_PASSWORD.
`;

const FLAGS = {
  'strapi-url': { type: 'string', env: 'STRAPI_URL', default: 'http://localhost:1337' },
  'strapi-prefix': { type: 'string', env: 'STRAPI_PREFIX', default: '' },
  'strapi-jwt': { type: 'string', env: 'STRAPI_JWT' },
  'strapi-identifier': { type: 'string', env: 'STRAPI_IDENTIFIER' },
  'strapi-password': { type: 'string', env: 'STRAPI_PASSWORD' },
  'strapi-project': { type: 'string', env: 'STRAPI_PROJECT' },
  'strapi-admin-token': { type: 'string', env: 'STRAPI_ADMIN_TOKEN' },
  'strapi-admin-email': { type: 'string', env: 'STRAPI_ADMIN_EMAIL' },
  'strapi-admin-password': { type: 'string', env: 'STRAPI_ADMIN_PASSWORD' },
  locale: { type: 'string' },
  'publication-state': { type: 'string', default: 'preview' },
  route: { type: 'map', repeatable: true, into: 'routeOverrides' },
  populate: { type: 'list', repeatable: true, into: 'populate' },

  'cms-url': { type: 'string', env: 'CMS_URL', default: 'http://127.0.0.1:8080' },
  'cms-token': { type: 'string', env: 'CMS_TOKEN' },
  'cms-username': { type: 'string', env: 'CMS_USERNAME' },
  'cms-password': { type: 'string', env: 'CMS_PASSWORD' },

  only: { type: 'list' },
  skip: { type: 'list' },
  name: { type: 'map', repeatable: true, into: 'nameOverrides' },
  relations: { type: 'string', default: 'skip' },
  'relation-owner': { type: 'list', repeatable: true, into: 'relationOwners' },
  'no-schema': { type: 'boolean', into: 'noSchema' },
  'no-images': { type: 'boolean', into: 'noImages' },
  'no-items': { type: 'boolean', into: 'noItems' },
  'no-publish': { type: 'boolean', into: 'noPublish' },
  'publish-all': { type: 'boolean', into: 'publishAll' },
  'no-dates': { type: 'boolean', into: 'noDates' },
  'no-title-field': { type: 'boolean', into: 'noTitleField' },
  'max-items': { type: 'number' },
  'max-images': { type: 'number' },
  'page-size': { type: 'number', default: 100 },
  concurrency: { type: 'number', default: 4 },

  'dry-run': { type: 'boolean', into: 'dryRun' },
  'list-relations': { type: 'boolean', into: 'listRelations' },
  sample: { type: 'number', default: 1 },
  state: { type: 'string' },
  report: { type: 'string' },
  force: { type: 'boolean' },
  quiet: { type: 'boolean' },
  verbose: { type: 'boolean' },
  help: { type: 'boolean' },
};

function parseArgs(argv) {
  const options = {};
  for (const [name, spec] of Object.entries(FLAGS)) {
    if (spec.default !== undefined) options[spec.into ?? name] = spec.default;
    if (spec.type === 'map') options[spec.into ?? name] = {};
    if (spec.type === 'list' && spec.repeatable) options[spec.into ?? name] = [];
  }
  for (const [name, spec] of Object.entries(FLAGS)) {
    if (spec.env && process.env[spec.env] !== undefined) {
      options[spec.into ?? name] = spec.type === 'number' ? Number(process.env[spec.env]) : process.env[spec.env];
    }
  }

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (!argument.startsWith('--')) throw new Error(`unexpected argument '${argument}'`);
    const equals = argument.indexOf('=');
    const name = equals === -1 ? argument.slice(2) : argument.slice(2, equals);
    const spec = FLAGS[name];
    if (!spec) throw new Error(`unknown flag '--${name}' (try --help)`);
    let value = equals === -1 ? undefined : argument.slice(equals + 1);
    if (spec.type !== 'boolean' && value === undefined) {
      index += 1;
      value = argv[index];
      if (value === undefined) throw new Error(`--${name} needs a value`);
    }
    const key = spec.into ?? name;
    switch (spec.type) {
      case 'boolean':
        options[key] = value === undefined ? true : value !== 'false';
        break;
      case 'number': {
        const parsed = Number(value);
        if (!Number.isFinite(parsed)) throw new Error(`--${name} needs a number, got '${value}'`);
        options[key] = parsed;
        break;
      }
      case 'list':
        options[key] = spec.repeatable
          ? [...(options[key] ?? []), ...value.split(',').filter(Boolean)]
          : value.split(',').filter(Boolean);
        break;
      case 'map': {
        const separator = value.indexOf('=');
        if (separator === -1) throw new Error(`--${name} needs <key>=<value>, got '${value}'`);
        options[key][value.slice(0, separator)] = value.slice(separator + 1);
        break;
      }
      default:
        options[key] = value;
    }
  }
  return options;
}

/** Whether a planned field type, or an array's item type, is a relation. */
function isRelationType(type) {
  if (typeof type !== 'object' || type === null) return false;
  if ('Relation' in type) return true;
  // Several references are an `Array` whose item types are relations, so the wrapper has to be
  // looked through as well.
  return (
    Array.isArray(type.Array) &&
    type.Array.some((item) => item !== null && typeof item === 'object' && 'Relation' in item)
  );
}

/** Whether a planned field is a relation, whose target has to exist before the schema is saved. */
function isRelationField(field) {
  return isRelationType(field.field_type);
}

/**
 * Whether a field holds a relation anywhere inside it.
 *
 * A relation may sit behind a composite the field embeds, so "does this content type need the
 * relation pass?" is not answered by its own fields alone - a single page whose only field is a
 * dynamic zone has a reference to write if one of those components holds one.
 */
function fieldReachesRelation(field, componentPlans, seen = new Set()) {
  const type = field.field_type;
  if (typeof type !== 'object' || type === null) return false;
  if (isRelationType(type)) return true;

  const embedded = [];
  if (type.CompositeField) embedded.push(type.CompositeField.id);
  if (Array.isArray(type.Array)) {
    for (const item of type.Array) if (item?.CompositeField) embedded.push(item.CompositeField.id);
  }
  for (const id of embedded) {
    if (seen.has(id)) continue;
    seen.add(id);
    const plan = componentPlans.get(id);
    if (plan?.schema.some((nested) => fieldReachesRelation(nested, componentPlans, seen))) return true;
  }
  return false;
}

/** Whether a content type's schema holds a relation of its own or inside what it embeds. */
function planReachesRelation(plan, componentPlans) {
  return plan.schema.some((field) => fieldReachesRelation(field, componentPlans, new Set()));
}

/** How a relation's cardinality reads in `--list-relations`. */
function card(many) {
  return many ? 'many' : 'single';
}

/** The relation list `--list-relations` prints: what exists, and which side the run would keep. */
function printRelationReport({ mutual, oneWay, dropped }, log) {
  log.step('relations');

  log.info(`mutual (${mutual.length}) - the CMS keeps one side; the other is derived from its index`);
  if (mutual.length > 0) {
    log.info(
      `  ${'field'.padEnd(30)}${'card'.padEnd(8)}${'field'.padEnd(30)}${'card'.padEnd(8)}kept by default`,
    );
    for (const row of mutual) {
      log.info(
        `  ${row.mine.padEnd(30)}${card(row.mineMany).padEnd(8)}` +
          `${row.theirs.padEnd(30)}${card(row.theirsMany).padEnd(8)}${row.owner}`,
      );
    }
    log.info('  keep the other side with:');
    for (const row of mutual) log.info(`    --relation-owner ${row.other}`);
  }

  if (oneWay.length > 0) {
    log.info(`\none-way (${oneWay.length}) - the only side there is`);
    for (const row of oneWay) {
      log.info(
        `  ${row.field.padEnd(40)}${card(row.hasMany).padEnd(8)}-> ${row.target}` +
          (row.where === 'composite' ? '  (inside a composite)' : ''),
      );
    }
  }

  if (dropped.length > 0) {
    log.info(`\ndropped (${dropped.length}) - the target is not a migrated content type`);
    for (const row of dropped) log.info(`  ${row.field.padEnd(40)}-> ${row.target}`);
  }
}

/**
 * Record every name an upload is reachable by, so a body's `/uploads/…` link can be resolved back
 * to the file it names.
 *
 * Strapi's generated name (`hash` + `ext`) is what a hand-written body link usually carries, and
 * the library also serves the responsive `formats` a body may have been written with.
 */
function recordUploadNames(refs, file) {
  const urls = [file.url, ...Object.values(file.formats ?? {}).map((format) => format?.url)];
  for (const url of urls) {
    if (typeof url !== 'string') continue;
    const match = /\/uploads\/([^/?#]+)/.exec(url);
    if (match) refs.set(match[1], file.id);
  }
  if (typeof file.hash === 'string' && typeof file.ext === 'string') {
    refs.set(`${file.hash}${file.ext}`, file.id);
  }
}

/** Create a collection or page when it is new, replace its schema when it is not. */
async function writeSchema({ cms, state }, contentType, schema) {
  const name = contentType.cmsName;
  const outcome =
    contentType.kind === 'singleType'
      ? await cms.ensureSinglePageSchema(name, schema)
      : await cms.ensureCollectionSchema(name, schema);
  state.markSchema(name, outcome);
  return outcome;
}

/**
 * Add the relation fields to the schemas, once the collections exist and the items are created.
 *
 * Both halves of that "once" matter. The target collection has to exist or the schema is refused,
 * and a **required** relation cannot be saved empty - so if the field were in the schema while
 * the creating pass ran, every item of that collection would be refused before the second pass
 * ever got the chance to fill it in.
 */
/**
 * Add every relation field, now that the targets exist and the items are created.
 *
 * Both halves of "now" matter. A target collection or page has to exist or the schema is refused,
 * and a **required** relation cannot be saved empty - so while the creating pass ran, the field
 * had to be absent from the schema, and from a composite the items embed just as much as from a
 * collection of their own.
 */
async function addRelationFields({ cms, state, plans, componentPlans, order, bootstrap, options, log }) {
  if (options.noSchema || options.relations !== 'relation') return;

  let composites = 0;
  for (const id of [...order, ...bootstrap]) {
    const plan = componentPlans.get(id);
    if (!plan.schema.some(isRelationField)) continue;
    await cms.ensureCompositeField(id, plan.schema);
    composites += 1;
  }

  let contentTypes = 0;
  for (const { contentType, plan } of plans) {
    if (!plan.schema.some(isRelationField)) continue;
    await writeSchema({ cms, state }, contentType, plan.schema);
    log.debug(`${contentType.cmsName}: relation fields written`);
    contentTypes += 1;
  }

  if (composites + contentTypes > 0) {
    log.info(`${contentTypes} schema(s) and ${composites} composite(s) gained their relation fields`);
  }
  await state.save();
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    process.stdout.write(USAGE);
    return;
  }
  const log = createLogger({ quiet: options.quiet, verbose: options.verbose });
  if (!['skip', 'text', 'relation'].includes(options.relations)) {
    throw new Error(`--relations must be 'skip', 'text' or 'relation', got '${options.relations}'`);
  }
  if (!['live', 'preview'].includes(options['publication-state'])) {
    throw new Error(`--publication-state must be 'live' or 'preview', got '${options['publication-state']}'`);
  }

  const stateDir = path.resolve('.strapi-migration');
  options.state ??= path.join(stateDir, 'state.json');
  options.report ??= path.join(stateDir, 'report.json');

  // === 1. Definitions ======================================================================

  const strapi = new StrapiV3Client({
    baseUrl: options['strapi-url'],
    prefix: options['strapi-prefix'],
    jwt: options['strapi-jwt'],
  });
  if (!strapi.jwt && options['strapi-identifier']) {
    await strapi.login(options['strapi-identifier'], options['strapi-password']);
    log.info(`signed in to Strapi as ${options['strapi-identifier']}`);
  }

  let definitions;
  if (options['strapi-project']) {
    definitions = await loadDefinitionsFromProject(options['strapi-project']);
    log.info(`read definitions from ${path.resolve(options['strapi-project'])}`);
  } else {
    if (options['strapi-admin-token']) {
      strapi.adminToken = options['strapi-admin-token'];
    } else if (options['strapi-admin-email']) {
      await strapi.adminLogin(options['strapi-admin-email'], options['strapi-admin-password']);
    } else {
      throw new Error(
        'no definition source: pass --strapi-project <checkout>, or --strapi-admin-token / ' +
          '--strapi-admin-email to read them from the running instance',
      );
    }
    definitions = await loadDefinitionsFromBuilder(strapi);
    log.info('read definitions from the admin content-type-builder');
  }
  definitions = applyOverrides(definitions, options);
  // Settled once, here, so every later use - the schema, the state key, and a relation's
  // `target.name` - is the same string.
  for (const contentType of definitions.contentTypes) {
    contentType.cmsName = sanitizeName(contentType.cmsName);
  }

  const selected = definitions.contentTypes.filter((contentType) => {
    if (options.only?.length && !options.only.includes(contentType.apiId)) return false;
    if (options.skip?.includes(contentType.apiId)) return false;
    return true;
  });
  if (selected.length === 0) throw new Error('no content types selected');

  const { order, bootstrap, unmigratable } = orderComponents(definitions.components);
  const componentByUid = new Map(definitions.components.map((component) => [component.uid, component]));
  const idToComponent = new Map(definitions.components.map((component) => [component.id, component]));
  const usableComponentIds = new Set([...order, ...bootstrap]);
  for (const id of unmigratable) {
    log.warn(
      `component '${id}' is a direct cycle, which this CMS refuses because it cannot be rendered; ` +
        'fields naming it are dropped. (Strapi cannot serve one either: its populate recurses ' +
        'through components without a visited set, so the model files load but the API never ' +
        'returns.)',
    );
  }

  // === 2. Plans ============================================================================

  // A relation names its target by Strapi's uid, which is not what the CMS calls things: this
  // lookup answers with the migrated content type (and its CMS name) so a reference can be
  // written the way the CMS expects. Both spellings a v3 project uses - the API id and the
  // model's own name - are keys.
  const contentTypesByUid = new Map();
  for (const contentType of selected) {
    contentTypesByUid.set(contentType.apiId.toLowerCase(), contentType);
    if (contentType.name) contentTypesByUid.set(String(contentType.name).toLowerCase(), contentType);
  }
  const migratedNames = new Set(selected.map((contentType) => contentType.cmsName));

  // Which side of a mutual relation is migrated is a question about the site, not about Strapi, so
  // an operator can answer it (`--relation-owner`). Checked before anything is written, because a
  // name that matches nothing would otherwise leave the automatic choice in place unnoticed.
  const relationOwners = new Set(options.relationOwners ?? []);
  const ownerProblems = relationOwnerProblems(selected, relationOwners, {
    contentTypeFor: (uid) =>
      uid ? (contentTypesByUid.get(String(uid).toLowerCase()) ?? null) : null,
  });
  if (ownerProblems.length > 0) {
    throw new Error(`${ownerProblems.join('\n')}\n(use <apiId>.<field>, e.g. category.posts)`);
  }

  const componentPlans = new Map();
  const ctx = {
    relationsMode: options.relations,
    relationOwners,
    dryRun: options.dryRun,
    state: null,
    // Flipped for the relation pass: references can only be resolved once every target item has
    // an id, so the creating pass writes them empty instead of reporting every one as missing.
    relationsReady: false,
    contentTypeFor(uid) {
      if (!uid) return null;
      return contentTypesByUid.get(String(uid).toLowerCase()) ?? null;
    },
    itemIdOf(collection, strapiId) {
      // A dry run has created nothing, so the Strapi id stands in for the CMS id: the point is
      // to show the shape the value will take, not to invent an id.
      if (options.dryRun) return strapiId;
      return ctx.state.itemId(collection, strapiId);
    },
    pageMigrated(name) {
      return migratedNames.has(name);
    },
    componentIdOf(uid) {
      if (!uid) return null;
      const component = componentByUid.get(uid);
      if (!component || !usableComponentIds.has(component.id)) return null;
      return component.id;
    },
    componentPlan(id) {
      return componentPlans.get(id);
    },
    componentDeclaresValues(id) {
      return Object.prototype.hasOwnProperty.call(idToComponent.get(id)?.attributes ?? {}, 'values');
    },
    imageIdOf(reference) {
      if (reference === null || reference === undefined) return null;
      const strapiId = typeof reference === 'object' ? reference.id : reference;
      if (strapiId === null || strapiId === undefined) return null;
      if (options.dryRun) return strapiId;
      return ctx.state.imageId(strapiId);
    },
    // Every name an upload answers to, filled in from the media listing. A body that links its
    // images by hand refers to them by these names.
    uploadRefs: new Map(),
    warn(message) {
      log.debug(`warning: ${message}`);
    },
  };

  // Left undefined when the images are not being migrated: rewriting a body's links to addresses
  // that would not resolve is worse than leaving them pointing at where they came from.
  if (!options.noImages) {
    ctx.uploadImageId = (fileName) => {
      const strapiId = ctx.uploadRefs.get(fileName);
      return strapiId === undefined ? null : ctx.imageIdOf(strapiId);
    };
  }

  for (const component of definitions.components) {
    if (!usableComponentIds.has(component.id)) continue;
    componentPlans.set(component.id, planContentType(component.attributes, ctx));
  }
  for (const [id, plan] of componentPlans) {
    for (const entry of plan.unmapped) {
      log.warn(`component ${id}.${entry.name}: ${entry.reason}`);
    }
  }

  const plans = selected.map((contentType) => {
    const owner = { apiId: contentType.apiId, name: contentType.name };
    const plan = planContentType(contentType.attributes, ctx, owner);
    // A reference to an item shows the field that names it, so a schema with no title shows ids.
    // Strapi has no such marker, so one is chosen by name; a reference with no title is still
    // correct, just less readable.
    if (!options.noTitleField && contentType.kind !== 'singleType') {
      const title = chooseTitleField(plan.schema);
      const field = plan.fields.find((candidate) => candidate.name === title);
      if (field) {
        field.schemaField.is_title = true;
        field.schemaField.show_in_list = true;
      }
    }
    return { contentType, plan };
  });

  log.step('plan');
  for (const { contentType, plan } of plans) {
    const kind = contentType.kind === 'singleType' ? 'single page' : 'collection';
    log.info(
      `${kind} ${contentType.apiId} -> ${contentType.cmsName}  (GET ${contentType.route})  ` +
        `${plan.fields.length} field(s)`,
    );
    for (const entry of plan.unmapped) log.warn(`  ${contentType.apiId}.${entry.name}: ${entry.reason}`);
  }
  log.info(
    `${definitions.components.length} component(s)` +
      (bootstrap.length > 0 ? ` (${bootstrap.length} mutually recursive through an array)` : '') +
      (unmigratable.length > 0 ? ` (${unmigratable.length} in a direct cycle, skipped)` : '') +
      `; ${selected.filter((c) => c.kind !== 'singleType').length} collection(s), ` +
      `${selected.filter((c) => c.kind === 'singleType').length} single page(s)`,
  );
  if (relationOwners.size > 0) {
    log.info(`relation owner chosen for: ${[...relationOwners].sort().join(', ')}`);
  }

  // `--list-relations` answers "which names can I pass to --relation-owner?" without writing
  // anything, so it stops here - before the CMS is contacted at all.
  if (options.listRelations) {
    const migratable = definitions.components.filter((component) => usableComponentIds.has(component.id));
    printRelationReport(relationReport(selected, migratable, ctx), log);
    return;
  }

  // v3 auto-populates relations, media and components, so nothing is asked for unless the
  // operator says a project turned `autoPopulate` off (`--populate`). `?_populate=...` is not a
  // v3 REST parameter: the default controller would read it as a *filter*, which is why it is
  // never sent on a guess.
  const populate = options.populate ?? [];

  // === 3. Dry run ==========================================================================

  if (options.dryRun) {
    log.step('dry run: reading samples from Strapi (nothing is written)');
    // A reference is one `{ target, item }` object, or an array of them when the field holds
    // several. In a dry run the Strapi id stands in for the CMS id, so the sample shows the shape
    // the value will really take.
    ctx.relationsReady = options.relations === 'relation';
    if (!options.noImages) {
      // The upload list is what turns a body's `/uploads/…` link into the CMS's by-id address, so
      // the sample shows the link it will really have. A library the public role cannot read is
      // not a reason to fail a dry run.
      try {
        const uploads = await strapi.listUploadFiles({
          pageSize: options['page-size'],
          max: options['max-images'] ?? Number.POSITIVE_INFINITY,
        });
        for (const file of uploads) recordUploadNames(ctx.uploadRefs, file);
        log.info(`${uploads.length} media file(s) in Strapi`);
      } catch (error) {
        log.warn(`could not list the media library, so body links are shown as they are: ${error.message}`);
      }
    }
    for (const { contentType, plan } of plans) {
      if (contentType.kind === 'singleType') {
        const entry = await strapi.getSingle(contentType.route, {
          locale: options.locale,
          populate,
          publicationState: options['publication-state'],
        });
        log.info(`\n-- ${contentType.cmsName} (single page)`);
        printSample(entry, plan, ctx, contentType.cmsName);
        continue;
      }
      const page = await strapi.listPage(contentType.route, {
        start: 0,
        limit: Math.max(1, options.sample),
        locale: options.locale,
        populate,
        publicationState: options['publication-state'],
      });
      const total = await strapi.count(contentType.route, {
        locale: options.locale,
        publicationState: options['publication-state'],
      });
      log.info(`\n-- ${contentType.cmsName} (${total ?? '?'} entries)`);
      for (const entry of page.slice(0, options.sample)) {
        printSample(entry, plan, ctx, contentType.cmsName);
      }
    }
    log.step('dry run finished');
    return;
  }

  // === 4. The CMS ==========================================================================

  const cms = new CmsClient({
    baseUrl: options['cms-url'],
    token: options['cms-token'],
  });
  if (!cms.token) {
    if (!options['cms-username']) {
      throw new Error('no CMS credentials: pass --cms-token, or --cms-username and --cms-password');
    }
    const user = await cms.login(options['cms-username'], options['cms-password']);
    log.info(`signed in to the CMS as ${user?.username ?? options['cms-username']}`);
  }

  const state = options.force
    ? new MigrationState(options.state, {})
    : await MigrationState.load(options.state);
  state.start(options['cms-url'], options['strapi-url']);
  ctx.state = state;
  const recordWarning = (entry) => {
    if (state.data.warnings.length < 2000) state.warn(entry);
  };

  // === 5. Definitions that must exist before anything references them =======================
  //
  // Three steps, because the dependencies run both ways. A composite that holds a relation names
  // a collection or a page, so those **names** have to exist before the composite is saved; a
  // collection's schema embeds composites, so the composites have to exist before it is saved.
  // Creating the names first breaks that knot. The relation **fields** - in a composite or in a
  // collection - come later still: an item that embedded a `required` relation while it was
  // empty could not be saved at all (see `addRelationFields`).

  if (!options.noSchema) {
    log.step('collection and page names');
    for (const { contentType } of plans) {
      if (contentType.kind === 'singleType') await cms.ensureSinglePageExists(contentType.cmsName);
      else await cms.ensureCollectionExists(contentType.cmsName);
    }
    log.info(`${plans.length} name(s) present`);

    // The composites, **without** their relation fields: a relation inside one is added later for
    // the same reason a collection's is.
    log.step('composite field definitions');
    const compositeBase = new Map(
      [...order, ...bootstrap].map((id) => [
        id,
        componentPlans.get(id).schema.filter((field) => !isRelationField(field)),
      ]),
    );
    const written = new Set();
    for (const id of order) {
      const outcome = await cms.ensureCompositeField(id, compositeBase.get(id));
      state.markComposite(id, outcome);
      written.add(id);
    }
    // Two components that reach each other through an array cannot be written in full in either
    // order, but the pair is storable: write one with the missing reference left out, which lets
    // the other be written in full, then write the first in full now that it exists.
    for (const id of bootstrap) {
      const partial = withoutUnavailableReferences(compositeBase.get(id), written);
      const outcome = await cms.ensureCompositeField(id, partial);
      state.markComposite(id, outcome);
      written.add(id);
    }
    for (const id of bootstrap) {
      await cms.ensureCompositeField(id, compositeBase.get(id));
    }
    log.info(
      `${order.length + bootstrap.length} composite field(s) present` +
        (bootstrap.length > 0 ? `, ${bootstrap.length} bootstrapped through their array` : ''),
    );

    // The collections' and pages' own fields, still without relations.
    log.step('schemas');
    for (const { contentType, plan } of plans) {
      await writeSchema({ cms, state }, contentType, plan.schema.filter((field) => !isRelationField(field)));
    }
    log.info(`${plans.length} schema(s) present`);
    await state.save();
  } else {
    log.info('--no-schema: assuming every composite and schema already exists');
  }

  // === 6. Media ============================================================================

  if (!options.noImages) {
    log.step('media library');
    const files = await strapi.listUploadFiles({
      pageSize: options['page-size'],
      max: options['max-images'] ?? Number.POSITIVE_INFINITY,
    });
    for (const file of files) recordUploadNames(ctx.uploadRefs, file);
    log.info(`${files.length} file(s) in Strapi`);
    const results = await pMap(files, options.concurrency, async (file) => {
      if (state.imageId(file.id) !== null) return 'already';
      const { buffer } = await strapi.downloadUploadFile(file);
      if (file.mime && !String(file.mime).startsWith('image/')) {
        log.warn(`media ${file.id} (${file.mime}) is not an image; uploading it anyway`);
      }
      const ext = fileExtension(file);
      const { id } = await cms.uploadImage({
        filename: displayFilename(file, ext),
        ext,
        bytes: buffer,
      });
      state.markImage(file.id, id);

      // The library and the pickers show a small copy; this CMS makes one in the browser that
      // uploads a picture, so a migrated library has to bring Strapi's own. Without it every tile
      // downloads the original, which is what an image uploaded through the API looks like too -
      // so a copy that cannot be carried is a warning, not a failed image.
      const thumbnail = thumbnailFor(file);
      if (thumbnail !== null) {
        try {
          const { buffer: thumbnailBytes } = await strapi.downloadMedia(thumbnail.url);
          await cms.setImageThumbnail(id, thumbnail.ext, thumbnailBytes);
        } catch (error) {
          log.warn(`media ${file.id}: uploaded, but its thumbnail was not: ${error.message}`);
        }
      }

      if (!options.noDates) {
        const uploadedAt = toDateTime(file.created_at ?? file.createdAt);
        if (uploadedAt) {
          try {
            await cms.setImageUploadedAt(id, uploadedAt);
          } catch (error) {
            // The bytes are in; only the display date is missing, so this is a warning rather
            // than a failed image.
            log.warn(`media ${file.id}: uploaded, but its date was not set: ${error.message}`);
          }
        }
      }
      return id;
    });
    let migrated = 0;
    results.forEach((result, index) => {
      const file = files[index];
      if (result.error) {
        state.fail({ scope: 'image', strapiId: file.id, name: file.name, reason: result.error.message });
        log.error(`media ${file.id} (${file.name}): ${result.error.message}`);
      } else if (result.value !== 'already') {
        migrated += 1;
      }
    });
    log.info(`${migrated} uploaded, ${results.filter((r) => r.error).length} failed`);
    await state.save();
  } else {
    log.info('--no-images: content references will lose their media');
  }

  // === 7. Entries ==========================================================================
  //
  // Three passes, because a reference points at an item by the id *this* CMS assigned:
  //
  //   1. create every item, with the relation fields empty
  //   2. write the values again, now that every target has an id (only with --relations=relation)
  //   3. publish in dependency order, then state the dates the content had in Strapi
  //
  // Pass 1 could not publish as it went: the CMS refuses to publish an item whose `required`
  // relation would end up with no published target, and the target may be created later.

  if (!options.noItems) {
    const records = [];
    const passes = plans.map(({ contentType, plan }) => ({ contentType, plan }));

    for (const { contentType, plan } of passes) {
      log.step(`${contentType.apiId} -> ${contentType.cmsName}`);
      const collected =
        contentType.kind === 'singleType'
          ? await createSinglePage({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning })
          : await createCollection({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning });
      records.push(...collected);
      await state.save();
    }

    // Now that no further item has to be created against a schema without them, the relation
    // fields can be added.
    await addRelationFields({ cms, state, plans, componentPlans, order, bootstrap, options, log });

    // Which items have their values written again. Everything that holds a relation, and - on a
    // re-run - everything already migrated, whether it holds one or not: a plan can change between
    // runs (a different `--relation-owner`, or `--relations=text`), and a field the schema no
    // longer declares has to be taken back **out of the stored values**. A save replaces the whole
    // value set, so writing the current plan is what removes it - and with it the index entry that
    // would otherwise keep a deleted item from being deleted.
    const secondPass = passes.filter(
      ({ contentType, plan }) =>
        planReachesRelation(plan, componentPlans) ||
        Object.keys(state.data.items[contentType.cmsName] ?? {}).length > 0 ||
        state.singlePages[contentType.cmsName] !== undefined,
    );
    if (options.relations === 'relation' && secondPass.length > 0) {
      log.step('references (second pass)');
      ctx.relationsReady = true;
      for (const { contentType, plan } of secondPass) {
        const deps =
          contentType.kind === 'singleType'
            ? await writeSinglePageRelations({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning, componentPlans })
            : await writeCollectionRelations({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning, componentPlans });
        for (const record of records) {
          if (record.kind === 'single_page' && record.name === contentType.cmsName) record.requiredDeps = deps;
          else if (record.kind === 'collection' && record.name === contentType.cmsName) record.requiredDeps = deps.get(record.cmsId) ?? [];
        }
        await state.save();
      }
    }

    if (!options.noPublish) {
      log.step('publishing');
      await publishRecords({ cms, state, records, options, log });
      await state.save();
    }

    if (!options.noDates) {
      log.step('dates');
      await restoreDates({ cms, state, records, log });
      await state.save();
    }
  } else {
    log.info('--no-items: no entries were written');
    // Nothing had to be created against a relation-less schema, so the fields can go in now.
    await addRelationFields({ cms, state, plans, componentPlans, order, bootstrap, options, log });
  }

  // === 8. Report ===========================================================================

  const summary = state.summary();
  const report = {
    generatedAt: new Date().toISOString(),
    strapiUrl: options['strapi-url'],
    cmsUrl: options['cms-url'],
    locale: options.locale ?? '(default)',
    plan: plans.map(({ contentType, plan }) => ({
      apiId: contentType.apiId,
      cmsName: sanitizeName(contentType.cmsName),
      kind: contentType.kind,
      route: contentType.route,
      fields: plan.schema,
      unmapped: plan.unmapped,
    })),
    components: [...order, ...bootstrap].map((id) => ({
      id,
      schema: componentPlans.get(id).schema,
      unmapped: componentPlans.get(id).unmapped,
    })),
    bootstrappedComponents: bootstrap,
    unmigratableComponents: unmigratable,
    relationOwners: [...relationOwners].sort(),
    summary,
    failures: state.data.failures,
    warnings: state.data.warnings,
  };
  await state.save();
  await mkdir(path.dirname(path.resolve(options.report)), { recursive: true });
  await writeFile(options.report, `${JSON.stringify(report, null, 2)}\n`, 'utf8');

  log.step('done');
  log.info(
    `${summary.items} item(s), ${summary.singlePages} single page(s), ${summary.images} image(s), ` +
      `${summary.schemas} schema(s), ${summary.failures} failure(s)`,
  );
  log.info(`state:  ${options.state}`);
  log.info(`report: ${options.report}`);
  if (summary.failures > 0) process.exitCode = 1;
}

function printSample(entry, plan, ctx, label) {
  if (entry === null || entry === undefined) {
    process.stdout.write(`   ${label}: no entry\n`);
    return;
  }
  const { values, problems, missingRequired } = convertEntry(entry, plan, ctx);
  process.stdout.write(`${JSON.stringify(values, null, 2)}\n`);
  for (const problem of problems) process.stdout.write(`   problem: ${problem}\n`);
  for (const missing of missingRequired) process.stdout.write(`   required but empty: ${missing}\n`);
}

/** The identity of one migrated item, in the form the publish order is built from. */
function recordKey(kind, name, id) {
  return kind === 'page' ? `page:${name}` : `collection:${name}:${id}`;
}

/**
 * Iterate a Strapi collection type, page by page.
 *
 * `--max-items` is applied here, so the creating and the relation pass stop at the same place,
 * and the second pass re-reads the same source rather than keeping every converted item in the
 * process's memory.
 */
async function* pages(strapi, contentType, options) {
  const total = await strapi.count(contentType.route, {
    locale: options.locale,
    publicationState: options['publication-state'],
  });
  const max = options['max-items'] ?? Number.POSITIVE_INFINITY;
  let seen = 0;
  for (let start = 0; seen < max; start += options['page-size']) {
    const page = await strapi.listPage(contentType.route, {
      start,
      limit: options['page-size'],
      locale: options.locale,
      populate: options.populate ?? [],
      publicationState: options['publication-state'],
    });
    if (page.length === 0) return;
    const entries = page.slice(0, Math.max(0, max - seen));
    if (entries.length === 0) return;
    seen += entries.length;
    yield entries;
    if (page.length < options['page-size']) return;
    if (total !== null && start + page.length >= total) return;
  }
}

/**
 * The references a record must wait for before it can be published.
 *
 * Only a **required** relation can refuse a publish (an optional one is simply dropped from the
 * delivered value if its target is not published), so only those become ordering edges - and a
 * required relation may sit inside a composite the item embeds, so the walk follows the plan and
 * the value together.
 */
function requiredDepsOf(plan, values, componentPlans) {
  const deps = [];
  collectRequiredDeps(plan, values, componentPlans, deps, new Set());
  return deps;
}

function recordKeyOf(reference) {
  return reference.item === undefined
    ? recordKey('page', reference.target)
    : recordKey('collection', reference.target, reference.item);
}

function collectRequiredDeps(plan, values, componentPlans, into, seen) {
  for (const field of plan.fields) {
    if (field.kind === FieldKind.Relation) {
      if (field.text || !field.schemaField.required) continue;
      // One relation is a single reference object; several are an array of them. The value's own
      // shape says which, so both orders are handled without consulting the field type.
      const raw = values[field.name];
      const references = Array.isArray(raw) ? raw : raw ? [raw] : [];
      for (const reference of references) into.push(recordKeyOf(reference));
      continue;
    }
    if (field.kind !== FieldKind.Component && field.kind !== FieldKind.DynamicZone) continue;

    // A composite value is `{ id, values }` for an array element and the same for a single one;
    // the element's own `id` says which definition it is, which matters for a dynamic zone.
    const raw = values[field.name];
    const elements = Array.isArray(raw) ? raw : raw ? [raw] : [];
    for (const element of elements) {
      if (!element || typeof element !== 'object') continue;
      const componentId = element.id ?? field.componentId;
      if (!componentId || seen.has(componentId)) continue;
      const nested = componentPlans.get(componentId);
      if (!nested) continue;
      collectRequiredDeps(nested, element.values ?? {}, componentPlans, into, seen);
    }
  }
}

/**
 * The first pass over a collection: create every item, with its relation fields empty.
 *
 * An item a previous run already created is not created again - its CMS id is in the state - but
 * it is still recorded, so the publish and date passes reach it and a run that stopped halfway
 * converges instead of leaving a draft behind.
 */
async function createCollection({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning }) {
  const name = contentType.cmsName;
  const records = [];
  let created = 0;
  let existing = 0;
  let failed = 0;

  for await (const entries of pages(strapi, contentType, options)) {
    const results = await pMap(entries, options.concurrency, async (entry) => {
      const live = shouldPublish(entry, options, contentType.draftAndPublish);
      const dates = options.noDates ? {} : entryDates(entry, { published: live });
      const known = state.itemId(name, entry.id);
      if (known !== null) return { id: known, isNew: false, live, dates };

      const { values, problems, missingRequired } = convertEntry(entry, plan, ctx);
      for (const problem of problems) recordWarning({ contentType: contentType.apiId, strapiId: entry.id, problem });
      if (missingRequired.length > 0) {
        throw new Error(`required and empty: ${missingRequired.join(', ')}`);
      }
      const id = await cms.createCollectionItem(name, values);
      state.markItem(name, entry.id, id);
      return { id, isNew: true, live, dates };
    });

    results.forEach((result, index) => {
      const entry = entries[index];
      if (result.error) {
        failed += 1;
        state.fail({ scope: 'item', contentType: contentType.apiId, strapiId: entry.id, reason: result.error.message });
        log.error(`${contentType.apiId} ${entry.id}: ${result.error.message}`);
        return;
      }
      const { id, isNew, live, dates } = result.value;
      if (isNew) created += 1;
      else existing += 1;
      records.push({
        key: recordKey('collection', name, id),
        kind: 'collection',
        name,
        cmsId: id,
        strapiId: entry.id,
        publish: live,
        dates,
        requiredDeps: [],
      });
    });
    await state.save();
  }
  log.info(`${created} created, ${existing} already migrated${failed ? `, ${failed} failed` : ''}`);
  return records;
}

async function createSinglePage({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning }) {
  const name = contentType.cmsName;
  const entry = await strapi.getSingle(contentType.route, {
    locale: options.locale,
    populate: options.populate ?? [],
    publicationState: options['publication-state'],
  });
  if (entry === null || entry === undefined) {
    log.warn(`${contentType.apiId}: Strapi answered no entry; nothing written`);
    return [];
  }
  const live = shouldPublish(entry, options, contentType.draftAndPublish);
  const dates = options.noDates ? {} : entryDates(entry, { published: live });
  const { values, problems, missingRequired } = convertEntry(entry, plan, ctx);
  for (const problem of problems) recordWarning({ contentType: contentType.apiId, problem });
  if (missingRequired.length > 0) {
    state.fail({ scope: 'single-page', contentType: contentType.apiId, reason: `required and empty: ${missingRequired.join(', ')}` });
    log.error(`${contentType.apiId}: required and empty: ${missingRequired.join(', ')}`);
    return [];
  }
  // A single page is one item, so it is written every run: that is idempotent, and it repairs a
  // previous run that stopped between the write and the publish.
  await cms.updateSinglePageItem(name, values);
  state.markSinglePage(name, 'written');
  log.info('written');
  return [
    {
      key: recordKey('page', name),
      kind: 'single_page',
      name,
      cmsId: null,
      strapiId: null,
      publish: live,
      dates,
      requiredDeps: [],
    },
  ];
}

/** The second pass over a collection: write the references now that every target has an id. */
async function writeCollectionRelations({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning, componentPlans }) {
  const name = contentType.cmsName;
  const dependencies = new Map();
  let updated = 0;
  let failed = 0;

  for await (const entries of pages(strapi, contentType, options)) {
    const results = await pMap(entries, options.concurrency, async (entry) => {
      const id = state.itemId(name, entry.id);
      // An item that never made it into the CMS has nothing to update; the first pass reported it.
      if (id === null) return null;
      const { values, problems, missingRequired } = convertEntry(entry, plan, ctx);
      for (const problem of problems) recordWarning({ contentType: contentType.apiId, strapiId: entry.id, problem });
      if (missingRequired.length > 0) {
        throw new Error(`required and empty: ${missingRequired.join(', ')}`);
      }
      await cms.updateCollectionItem(name, id, values);
      return { id, deps: requiredDepsOf(plan, values, componentPlans) };
    });

    results.forEach((result, index) => {
      const entry = entries[index];
      if (result.error) {
        failed += 1;
        state.fail({ scope: 'relation', contentType: contentType.apiId, strapiId: entry.id, reason: result.error.message });
        log.error(`references ${contentType.apiId} ${entry.id}: ${result.error.message}`);
        return;
      }
      if (result.value === null) return;
      dependencies.set(result.value.id, result.value.deps);
      updated += 1;
    });
  }
  log.info(`${updated} item(s) updated${failed ? `, ${failed} failed` : ''}`);
  return dependencies;
}

async function writeSinglePageRelations({ strapi, cms, state, contentType, plan, ctx, options, log, recordWarning, componentPlans }) {
  const name = contentType.cmsName;
  const entry = await strapi.getSingle(contentType.route, {
    locale: options.locale,
    populate: options.populate ?? [],
    publicationState: options['publication-state'],
  });
  if (entry === null || entry === undefined) return [];
  const { values, problems, missingRequired } = convertEntry(entry, plan, ctx);
  for (const problem of problems) recordWarning({ contentType: contentType.apiId, problem });
  if (missingRequired.length > 0) {
    state.fail({ scope: 'relation', contentType: contentType.apiId, reason: `required and empty: ${missingRequired.join(', ')}` });
    log.error(`${contentType.apiId}: required and empty: ${missingRequired.join(', ')}`);
    return [];
  }
  await cms.updateSinglePageItem(name, values);
  log.info('updated');
  return requiredDepsOf(plan, values, componentPlans);
}

/**
 * Publish what Strapi had published, referenced content first.
 *
 * The CMS refuses to publish an item whose `required` relation would end up with no published
 * target, and in a resumed run that is exactly the situation a target created moments ago is in.
 * Items that require each other cannot be ordered at all, so they are attempted anyway and the
 * refusal is reported.
 */
async function publishRecords({ cms, state, records, options, log }) {
  const toPublish = records.filter((record) => record.publish);
  const dependencies = new Map(toPublish.map((record) => [record.key, new Set(record.requiredDeps ?? [])]));
  const { order, cyclic } = orderForPublish(toPublish.map((record) => record.key), dependencies);
  if (cyclic.length > 0) {
    log.warn(
      `${cyclic.length} item(s) require each other, so no order can publish them: ` +
        `${cyclic.slice(0, 3).join(', ')}${cyclic.length > 3 ? ' …' : ''}`,
    );
  }

  const byKey = new Map(records.map((record) => [record.key, record]));
  let published = 0;
  let failed = 0;
  for (const key of [...order, ...cyclic]) {
    const record = byKey.get(key);
    try {
      if (record.kind === 'single_page') {
        await cms.publishSinglePage(record.name);
        state.markSinglePage(record.name, 'published');
      } else {
        await cms.publishCollectionItem(record.name, record.cmsId);
      }
      record.published = true;
      published += 1;
    } catch (error) {
      record.published = false;
      failed += 1;
      const label = record.cmsId === null ? record.name : `${record.name} #${record.cmsId}`;
      state.fail({ scope: 'publish', contentType: record.name, strapiId: record.strapiId, reason: error.message });
      log.error(`publish ${label}: ${error.message}`);
    }
  }
  log.info(`${published} published${failed ? `, ${failed} failed` : ''} (of ${toPublish.length})`);
}

/**
 * State the dates the content had in Strapi.
 *
 * Last, because publishing stamps `published_at` with "now", and only for what was actually
 * published: a date on an item that stayed a draft would say it had been live.
 */
async function restoreDates({ cms, state, records, log }) {
  let written = 0;
  let failed = 0;
  for (const record of records) {
    const dates = { ...(record.dates ?? {}) };
    if (record.published === false) delete dates.published_at;
    if (Object.keys(dates).length === 0) continue;
    try {
      if (record.kind === 'single_page') await cms.setSinglePageDates(record.name, dates);
      else await cms.setCollectionItemDates(record.name, record.cmsId, dates);
      written += 1;
    } catch (error) {
      failed += 1;
      const label = record.cmsId === null ? record.name : `${record.name} #${record.cmsId}`;
      state.fail({ scope: 'dates', contentType: record.name, strapiId: record.strapiId, reason: error.message });
      log.error(`dates ${label}: ${error.message}`);
    }
  }
  log.info(`${written} item(s) dated${failed ? `, ${failed} failed` : ''}`);
}

main().catch((error) => {
  process.stderr.write(`error: ${error?.stack ?? error}\n`);
  process.exitCode = 1;
});
