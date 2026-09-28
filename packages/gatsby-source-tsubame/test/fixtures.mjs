// A miniature Tsubame delivery API, and the Gatsby API surface the plugin uses.
//
// The payloads are the shapes `docs/content-api.md` promises (`{schema, items, total, limit, offset,
// next_offset}`, the untagged values from the Rust `FieldValueResponse`, and the composite map from
// `GET /api/content/composite-fields`), so a test that passes here is a test against the contract
// rather than against an invention.
//
// Nothing here is imported by the plugin; this file is only for the tests.

import modelModule from '../src/model.js';

const { buildContentModel } = modelModule;

/**
 * The fake `fetch` `createApiFetch` returns: a `FetchLike` that also records the URLs it was asked
 * for.
 *
 * @typedef {import('../src/client.js').FetchLike & {requests: string[]}} ApiFetch
 */

/**
 * What `createGatsbyApi` hands back: the API Gatsby would pass, and the records the tests read.
 *
 * @typedef {object} GatsbyHarness
 * @property {import('../src/nodes.js').GatsbyApi} api the API the plugin is called with
 * @property {Map<string, any>} nodes the created nodes, by id
 * @property {any[]} types what `createTypes` was given
 * @property {Array<{parent: string, child: string}>} links what `createParentChildLink` was given
 * @property {string[]} warnings what `warn` was given
 * @property {string[]} infos what `info` was given
 * @property {string[]} touched what `touchNode` was given
 */

/**
 * The options `createModel` accepts, so a test can override just the type prefix or the images.
 *
 * @typedef {{typePrefix?: string, images?: {download?: boolean, concurrency?: number, requestHeaders?: Record<string, any>}}} ModelOverrides
 */

/** `seo` holds a Markdown-less pair; `block` reaches itself through an array and holds both a
 *  Markdown field and a relation; `cta` points back at a single page.
 *  @type {Record<string, import('../src/fields.js').SchemaField[]>} */
export const COMPOSITE_FIELDS = {
  seo: [
    { name: 'description', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    { name: 'og_image', field_type: 'Image', required: false, width: 12, height: 1 },
  ],
  block: [
    { name: 'text', field_type: { Markdown: {} }, required: false, width: 12, height: 1 },
    { name: 'caption', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    {
      name: 'link',
      field_type: {
        Relation: {
          target: { kind: 'collection', name: 'authors' },
          // An inverse declared inside a composite definition is *not* a declaration: a composite
          // can be embedded by several collections, so "who refers" would have more than one
          // answer. The delivery API reads `inverse_name` from the schema's own fields only, and
          // so does the plugin - this fixture is what proves the plugin ignores it.
          inverse_name: 'blocks',
        },
      },
      required: false,
      width: 12,
      height: 1,
    },
    {
      name: 'children',
      field_type: { Array: [{ CompositeField: { id: 'block' } }] },
      required: false,
      width: 12,
      height: 1,
    },
  ],
  cta: [
    { name: 'label', field_type: { Text: {} }, required: false, width: 12, height: 1 },
    { name: 'body', field_type: { Markdown: {} }, required: false, width: 12, height: 1 },
    {
      name: 'target',
      field_type: { Relation: { target: { kind: 'single_page', name: 'home' } } },
      required: false,
      width: 12,
      height: 1,
    },
  ],
};

/** @type {import('../src/fields.js').SchemaField[]} */
export const BLOG_SCHEMA = [
  { name: 'title', field_type: { Text: {} }, required: true, width: 12, height: 1, is_title: true },
  { name: 'slug', field_type: { Slug: {} }, required: true, width: 12, height: 1 },
  { name: 'body', field_type: { Markdown: {} }, required: false, width: 12, height: 1 },
  { name: 'body-parts', field_type: { Array: [{ Markdown: {} }] }, required: false, width: 12, height: 1 },
  { name: 'cover', field_type: 'Image', required: false, width: 6, height: 1 },
  { name: 'gallery', field_type: { Array: ['Image'] }, required: false, width: 6, height: 1 },
  { name: 'tags', field_type: { TextEnum: ['news', 'blog'] }, required: false, width: 12, height: 1 },
  {
    name: 'author',
    field_type: {
      Relation: { target: { kind: 'collection', name: 'authors' }, inverse_name: 'articles' },
    },
    required: false,
    width: 12,
    height: 1,
  },
  // Several references are an `Array` whose item types are all relations; one item type per target.
  // A single item type is the old `has_many: true`, read from the array wrapper instead of a flag.
  {
    name: 'editors',
    field_type: {
      Array: [{ Relation: { target: { kind: 'collection', name: 'editors' }, inverse_name: 'editor_of' } }],
    },
    required: false,
    width: 12,
    height: 1,
  },
  // One array may declare several targets: its element's own `target` says which one it is.
  {
    name: 'related',
    field_type: {
      Array: [
        { Relation: { target: { kind: 'collection', name: 'authors' } } },
        { Relation: { target: { kind: 'collection', name: 'editors' } } },
      ],
    },
    required: false,
    width: 12,
    height: 1,
  },
  // The same, with a target no list names: the plugin leaves that target out of the field, which is
  // then a plain list of the target that is there (`TsubameAuthorsItem`).
  {
    name: 'mentions',
    field_type: {
      Array: [
        { Relation: { target: { kind: 'collection', name: 'authors' } } },
        { Relation: { target: { kind: 'collection', name: 'nowhere' } } },
      ],
    },
    required: false,
    width: 12,
    height: 1,
  },
  // A field whose only target the CMS does not answer: the plugin leaves the field itself out.
  {
    name: 'ghost',
    field_type: { Relation: { target: { kind: 'collection', name: 'nowhere' } } },
    required: false,
    width: 12,
    height: 1,
  },
  { name: 'seo', field_type: { CompositeField: { id: 'seo' } }, required: false, width: 12, height: 1 },
  {
    name: 'blocks',
    field_type: { Array: [{ CompositeField: { id: 'block' } }] },
    required: false,
    width: 12,
    height: 1,
  },
  { name: 'rating', field_type: 'Number', required: false, width: 12, height: 1 },
  { name: 'featured', field_type: 'Boolean', required: false, width: 12, height: 1 },
  { name: 'published_on', field_type: 'Date', required: false, width: 12, height: 1 },
  { name: 'updated_on', field_type: 'DateTime', required: false, width: 12, height: 1 },
  // A field whose name the plugin already uses for its own bookkeeping: it has to be renamed, not
  // silently dropped.
  { name: 'values', field_type: { Text: {} }, required: false, width: 12, height: 1 },
];

/** @type {import('../src/fields.js').SchemaField[]} */
export const AUTHORS_SCHEMA = [
  { name: 'name', field_type: { Text: {} }, required: true, width: 12, height: 1, is_title: true },
];

/** A collection a relation names that has no published items: its schema is public, its list is empty.
 *  @type {import('../src/fields.js').SchemaField[]} */
export const EDITORS_SCHEMA = [
  { name: 'name', field_type: { Text: {} }, required: true, width: 12, height: 1, is_title: true },
  // Points at a single page that is not published, so the target has no public schema at all. The
  // page still answers to the inverse name: the declaration is the referrer's, not the target's.
  {
    name: 'homepage',
    field_type: {
      Relation: { target: { kind: 'single_page', name: 'contact' }, inverse_name: 'editors' },
    },
    required: false,
    width: 12,
    height: 1,
  },
];

/** @type {import('../src/fields.js').SchemaField[]} */
export const HOME_SCHEMA = [
  { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
  { name: 'intro', field_type: { Markdown: {} }, required: false, width: 12, height: 1 },
  { name: 'cta', field_type: { CompositeField: { id: 'cta' } }, required: false, width: 12, height: 1 },
  // A page declaring an inverse on a collection target, so the target answers to a page type too.
  {
    name: 'featured_author',
    field_type: {
      Relation: { target: { kind: 'collection', name: 'authors' }, inverse_name: 'features' },
    },
    required: false,
    width: 12,
    height: 1,
  },
];

/** @type {import('../src/fields.js').WireContentItem[]} */
export const BLOG_ITEMS = [
  {
    id: 1,
    published_at: '2026-01-01T00:00:00Z',
    last_published_at: '2026-01-03T00:00:00Z',
    values: {
      title: 'Hello',
      slug: 'hello',
      body: '# Hello\n\nworld',
      'body-parts': ['part one', 'part two'],
      cover: { id: 3, url: '/api/images/logo.png' },
      gallery: [{ id: 4, url: '/api/images/a.png' }],
      tags: ['news'],
      author: { target: 'authors', item: 7 },
      editors: [{ target: 'editors', item: 2 }],
      related: [
        { target: 'authors', item: 7 },
        { target: 'editors', item: 2 },
      ],
      // One target of this field is part of the build and one is not (`nowhere` is in no list): the
      // field is a list of the target that is there, and the element naming the other one is not
      // something the delivery API would serve (it drops references it cannot resolve), so it is
      // here only to prove the plugin does not choke on a value left behind by an older schema.
      mentions: [
        { target: 'authors', item: 7 },
        { target: 'nowhere', item: 5 },
      ],
      // `ghost` names a collection the CMS does not answer, so the field is left out of the schema.
      ghost: { target: 'nowhere', item: 5 },
      seo: { id: 'seo', values: { description: 'about hello', og_image: { id: 9, url: '/api/images/og.png' } } },
      blocks: [
        {
          id: 'block',
          values: {
            text: 'First **block**',
            caption: 'cap',
            link: { target: 'authors', item: 7 },
            children: [{ id: 'block', values: { text: 'child text', caption: 'child cap' } }],
          },
        },
      ],
      rating: 4.5,
      featured: true,
      published_on: '2026-01-02',
      updated_on: '2026-01-03T04:05:06+09:00',
      values: 'a text field that happens to be called values',
    },
  },
  {
    id: 2,
    published_at: '2026-01-04T00:00:00Z',
    last_published_at: null,
    values: { title: 'Second', body: '', 'body-parts': [], tags: [], author: null },
  },
  {
    id: 3,
    published_at: '2026-01-05T00:00:00Z',
    last_published_at: '2026-01-05T00:00:00Z',
    values: { title: 'Third', body: 'plain', 'body-parts': ['only one'], tags: ['blog'] },
  },
];

/** @type {import('../src/fields.js').WireContentItem[]} */
export const AUTHORS_ITEMS = [
  { id: 7, published_at: '2025-12-01T00:00:00Z', last_published_at: '2025-12-01T00:00:00Z', values: { name: 'Ada' } },
];

/** @type {import('../src/fields.js').WireContentPage} */
export const HOME_PAGE = {
  schema: HOME_SCHEMA,
  published_at: '2025-11-01T00:00:00Z',
  last_published_at: '2025-11-02T00:00:00Z',
  values: {
    title: 'Home',
    intro: '## Welcome',
    cta: { id: 'cta', values: { label: 'Read more', body: 'The **cta** body', target: { target: 'home' } } },
    featured_author: { target: 'authors', item: 7 },
  },
};

/** @type {Record<string, {schema: import('../src/fields.js').SchemaField[], items: import('../src/fields.js').WireContentItem[]}>} */
const COLLECTIONS = {
  blog: { schema: BLOG_SCHEMA, items: BLOG_ITEMS },
  authors: { schema: AUTHORS_SCHEMA, items: AUTHORS_ITEMS },
  // In the API but not in the index: a collection with no published items.
  editors: { schema: EDITORS_SCHEMA, items: [] },
};

/**
 * @param {any} body the body to answer with
 * @param {number} [status] the HTTP status
 * @returns {import('../src/client.js').FetchResponse} a response the client can read
 */
function jsonResponse(body, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async json() {
      return body;
    },
    async text() {
      return JSON.stringify(body);
    },
  };
}

/**
 * A `fetch` that answers the delivery API routes, counting what was asked for.
 *
 * The page size of the *request* decides how much comes back, exactly as the server does, so a
 * test can make the plugin walk several pages by asking for a small `pageSize`.
 *
 * @returns {ApiFetch} the fake fetch, with the URLs it was asked for on `requests`
 */
export function createApiFetch() {
  /** @type {string[]} */
  const requests = [];
  /** @type {ApiFetch} */
  const fetchImpl = /** @type {ApiFetch} */ (async (/** @type {string} */ url) => {
    requests.push(url);
    const parsed = new URL(url);
    const { pathname } = parsed;

    if (pathname === '/api/content/collections') {
      return jsonResponse(['blog', 'authors']);
    }
    if (pathname === '/api/content/single-pages') {
      return jsonResponse(['home']);
    }
    if (pathname === '/api/content/composite-fields') {
      return jsonResponse(COMPOSITE_FIELDS);
    }
    if (pathname === '/api/content/single-pages/home') {
      return jsonResponse(HOME_PAGE);
    }
    if (pathname.startsWith('/api/content/single-pages/')) {
      // `contact` is named by a relation but never published: the route answers 404, which is the
      // "no public schema" case the plugin has to declare a type for anyway.
      return jsonResponse({ code: 'not_found' }, 404);
    }

    const match = pathname.match(/^\/api\/content\/collections\/([^/]+)$/);
    if (match !== null) {
      const name = decodeURIComponent(match[1]);
      const source = COLLECTIONS[name];
      if (source === undefined) {
        return jsonResponse({ code: 'not_found' }, 404);
      }
      const limitParam = parsed.searchParams.get('limit');
      const limit = limitParam === null ? 50 : Number(limitParam);
      const offset = Number(parsed.searchParams.get('offset') ?? '0');
      const page = source.items.slice(offset, offset + limit);
      const consumed = offset + page.length;
      return jsonResponse({
        schema: source.schema,
        items: page,
        total: source.items.length,
        limit,
        offset,
        next_offset: consumed < source.items.length ? consumed : null,
      });
    }

    return jsonResponse({ code: 'not_found' }, 404);
  });
  fetchImpl.requests = requests;
  return fetchImpl;
}

/**
 * The parts of the Gatsby node API the plugin touches, recording what they were given.
 *
 * `createNode` stores the object as-is and `createParentChildLink` mutates the parent it is handed,
 * which is what the real action does (`gatsby/src/redux/actions/public.js`). `files` is what a
 * previous build left in the store, which the image reuse looks through; `touched` is what the
 * plugin asked Gatsby to keep.
 *
 * @param {{files?: Array<Record<string, any> | null>}} [options] the files a previous build left
 * @returns {GatsbyHarness} the API and the records
 */
export function createGatsbyApi({ files = [] } = {}) {
  /** @type {Map<string, any>} */
  const nodes = new Map();
  /** @type {any[]} */
  const types = [];
  /** @type {Array<{parent: string, child: string}>} */
  const links = [];
  /** @type {string[]} */
  const warnings = [];
  /** @type {string[]} */
  const infos = [];
  /** @type {string[]} */
  const touched = [];

  /** @type {import('../src/nodes.js').GatsbyApi} */
  const api = {
    actions: {
      /** @param {string | object | Array<string | object>} definition the definitions to declare */
      createTypes(definition) {
        // `createTypes` takes SDL, type-builder objects, or an array of both; the fake keeps them in
        // the order they were handed over, so a test can read what the plugin declared.
        for (const entry of Array.isArray(definition) ? definition : [definition]) {
          types.push(entry);
        }
      },
      /** @param {import('../src/nodes.js').GatsbyNode} node the node to store */
      createNode(node) {
        nodes.set(node.id, node);
      },
      /** @param {{parent: import('../src/nodes.js').GatsbyNode, child: import('../src/nodes.js').GatsbyNode}} link the pair to link */
      createParentChildLink({ parent, child }) {
        links.push({ parent: parent.id, child: child.id });
        if (!parent.children.includes(child.id)) {
          parent.children.push(child.id);
        }
      },
      /** @param {import('../src/nodes.js').GatsbyNode} node the node to keep */
      touchNode(node) {
        touched.push(node.id);
      },
    },
    // The type builders Gatsby hands `createSchemaCustomization`, with the shape they really have
    // (`{kind, config}`, `gatsby/dist/schema/types/type-builders.js`). The unit tests read those
    // descriptors instead of Gatsby's schema.
    schema: {
      /** @param {any} config the union's config @returns {{kind: string, config: any}} the descriptor */
      buildUnionType: (config) => ({ kind: 'UNION', config }),
      /** @param {any} config the object's config @returns {{kind: string, config: any}} the descriptor */
      buildObjectType: (config) => ({ kind: 'OBJECT', config }),
    },
    /** @param {string} key the node key @returns {string} the node id */
    createNodeId: (key) => `node:${key}`,
    /** @param {any} data the node data @returns {string} the digest */
    createContentDigest: (data) => `digest:${typeof data === 'string' ? data : JSON.stringify(data)}`,
    /** @param {string} type the node type @returns {Array<Record<string, any> | null>} the stored files */
    getNodesByType: (type) => (type === 'File' ? files : []),
    reporter: {
      /** @param {string} message the warning @returns {number} the new length */
      warn: (message) => warnings.push(message),
      /** @param {string} message the summary @returns {number} the new length */
      info: (message) => infos.push(message),
      verbose: () => {},
      /** @param {string} message why the build stops */
      panic: (message) => {
        throw new Error(message);
      },
    },
  };

  return { api, nodes, types, links, warnings, infos, touched };
}

export const BLOG_ITEM_TYPE = 'TsubameBlogItem';
export const BLOG_COLLECTION_TYPE = 'TsubameCollection';
export const HOME_PAGE_TYPE = 'TsubameHomePage';
export const MARKDOWN_TYPE = 'TsubameMarkdown';

/**
 * What `TsubameClient.fetchSchemaSnapshot()` answers for the fake API above, without the requests.
 *
 * `editors` is in `collections` although it is not in `collectionNames`: it has no published items,
 * so the index does not list it, but a relation names it and its schema is what the relation field
 * was declared against.
 *
 * @returns {import('../src/model.js').SchemaSnapshot} the snapshot
 */
export function createSnapshot() {
  return {
    collectionNames: ['blog', 'authors'],
    pageNames: ['home'],
    collections: new Map([
      ['blog', BLOG_SCHEMA],
      ['authors', AUTHORS_SCHEMA],
      ['editors', EDITORS_SCHEMA],
    ]),
    pages: new Map([['home', HOME_SCHEMA]]),
    composites: new Map(Object.entries(COMPOSITE_FIELDS)),
    unpublishedPageTargets: ['contact'],
  };
}

/**
 * The model a build over the fake API is read against.
 *
 * @param {ModelOverrides} [overrides] options to override the defaults with
 * @returns {import('../src/model.js').ContentModel} the model
 */
export function createModel(overrides = {}) {
  return buildContentModel(
    createSnapshot(),
    /** @type {import('../src/model.js').ModelOptions} */ ({
      typePrefix: 'Tsubame',
      images: { download: false, concurrency: 4, requestHeaders: {} },
      ...overrides,
    }),
  );
}

