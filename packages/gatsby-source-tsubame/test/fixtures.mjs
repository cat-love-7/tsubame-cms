// A miniature Tsubame delivery API, and the Gatsby API surface the plugin uses.
//
// The payloads are the shapes `doc/content-api.md` promises (`{schema, items, total, limit, offset,
// next_offset}`, the untagged values from the Rust `FieldValueResponse`, and the composite map from
// `GET /api/content/composite-fields`), so a test that passes here is a test against the contract
// rather than against an invention.
//
// Nothing here is imported by the plugin; this file is only for the tests.

import modelModule from '../src/model.js';

const { buildContentModel } = modelModule;

/** `seo` holds a Markdown-less pair; `block` reaches itself through an array and holds both a
 *  Markdown field and a relation; `cta` points back at a single page. */
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
          has_many: false,
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
      field_type: { Relation: { target: { kind: 'single_page', name: 'home' }, has_many: false } },
      required: false,
      width: 12,
      height: 1,
    },
  ],
};

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
      Relation: { target: { kind: 'collection', name: 'authors' }, has_many: false, inverse_name: 'articles' },
    },
    required: false,
    width: 12,
    height: 1,
  },
  // Points at a collection that has no published items, and so is not in `/content/collections`.
  {
    name: 'editors',
    field_type: { Relation: { target: { kind: 'collection', name: 'editors' }, has_many: true } },
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

export const AUTHORS_SCHEMA = [
  { name: 'name', field_type: { Text: {} }, required: true, width: 12, height: 1, is_title: true },
];

/** A collection a relation names that has no published items: its schema is public, its list is empty. */
export const EDITORS_SCHEMA = [
  { name: 'name', field_type: { Text: {} }, required: true, width: 12, height: 1, is_title: true },
  // Points at a single page that is not published, so the target has no public schema at all. The
  // page still answers to the inverse name: the declaration is the referrer's, not the target's.
  {
    name: 'homepage',
    field_type: {
      Relation: { target: { kind: 'single_page', name: 'contact' }, has_many: false, inverse_name: 'editors' },
    },
    required: false,
    width: 12,
    height: 1,
  },
];

export const HOME_SCHEMA = [
  { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
  { name: 'intro', field_type: { Markdown: {} }, required: false, width: 12, height: 1 },
  { name: 'cta', field_type: { CompositeField: { id: 'cta' } }, required: false, width: 12, height: 1 },
  // A page declaring an inverse on a collection target, so the target answers to a page type too.
  {
    name: 'featured_author',
    field_type: {
      Relation: { target: { kind: 'collection', name: 'authors' }, has_many: false, inverse_name: 'features' },
    },
    required: false,
    width: 12,
    height: 1,
  },
];

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
      author: [{ target: 'authors', item: 7 }],
      editors: [{ target: 'editors', item: 2 }],
      seo: { id: 'seo', values: { description: 'about hello', og_image: { id: 9, url: '/api/images/og.png' } } },
      blocks: [
        {
          id: 'block',
          values: {
            text: 'First **block**',
            caption: 'cap',
            link: [{ target: 'authors', item: 7 }],
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
    values: { title: 'Second', body: '', 'body-parts': [], tags: [], author: [] },
  },
  {
    id: 3,
    published_at: '2026-01-05T00:00:00Z',
    last_published_at: '2026-01-05T00:00:00Z',
    values: { title: 'Third', body: 'plain', 'body-parts': ['only one'], tags: ['blog'] },
  },
];

export const AUTHORS_ITEMS = [
  { id: 7, published_at: '2025-12-01T00:00:00Z', last_published_at: '2025-12-01T00:00:00Z', values: { name: 'Ada' } },
];

export const HOME_PAGE = {
  schema: HOME_SCHEMA,
  published_at: '2025-11-01T00:00:00Z',
  last_published_at: '2025-11-02T00:00:00Z',
  values: {
    title: 'Home',
    intro: '## Welcome',
    cta: { id: 'cta', values: { label: 'Read more', body: 'The **cta** body', target: [{ target: 'home' }] } },
    featured_author: [{ target: 'authors', item: 7 }],
  },
};

const COLLECTIONS = {
  blog: { schema: BLOG_SCHEMA, items: BLOG_ITEMS },
  authors: { schema: AUTHORS_SCHEMA, items: AUTHORS_ITEMS },
  // In the API but not in the index: a collection with no published items.
  editors: { schema: EDITORS_SCHEMA, items: [] },
};

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
 */
export function createApiFetch() {
  const requests = [];
  const fetchImpl = async (url) => {
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
  };
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
 */
export function createGatsbyApi({ files = [] } = {}) {
  const nodes = new Map();
  const types = [];
  const links = [];
  const warnings = [];
  const infos = [];
  const touched = [];

  const api = {
    actions: {
      createTypes(sdl) {
        types.push(sdl);
      },
      createNode(node) {
        nodes.set(node.id, node);
      },
      createParentChildLink({ parent, child }) {
        links.push({ parent: parent.id, child: child.id });
        if (!parent.children.includes(child.id)) {
          parent.children.push(child.id);
        }
      },
      touchNode(node) {
        touched.push(node.id);
      },
    },
    createNodeId: (key) => `node:${key}`,
    createContentDigest: (data) => `digest:${typeof data === 'string' ? data : JSON.stringify(data)}`,
    getNodesByType: (type) => (type === 'File' ? files : []),
    reporter: {
      warn: (message) => warnings.push(message),
      info: (message) => infos.push(message),
      verbose: () => {},
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

/** The model a build over the fake API is read against. */
export function createModel(overrides = {}) {
  return buildContentModel(createSnapshot(), {
    typePrefix: 'Tsubame',
    images: { download: false, concurrency: 4, requestHeaders: {} },
    ...overrides,
  });
}

