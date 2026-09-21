#!/usr/bin/env node
// A minimal stand-in for a Strapi v3 instance, for exercising the migration without a Strapi.
//
// It answers the shapes v3 actually answers - a **bare JSON array** for a list, a **bare number**
// for `/count`, `{ "jwt", "user" }` for `/auth/local`, `{ "data": [...] }` for the admin
// content-type-builder, auto-populated relations/components/media, and `_publicationState`
// filtering - so the migration's source client is tested against the contract it claims to
// speak. The definitions it corresponds to are in `test/fixtures/strapi-v3/`.
//
// The content-type-builder answers are derived from those fixture files and normalised the way
// v3 normalises them (media to `{ type: "media", multiple }`, relations to
// `{ nature, target, targetAttribute }`), which is the shape the migration has to read when
// there is no checkout at all.
//
// Usage: node test/fake-strapi-v3.mjs [--port 1337]

import { readFile, readdir } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const FIXTURE_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), 'fixtures', 'strapi-v3');
const JWT = 'test-jwt';
const ADMIN_JWT = 'test-admin-jwt';
// A real 1x1 PNG, so the CMS stores decodable bytes whatever name the file has.
const PIXEL = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);

const stamp = (created, updated = created, published = created) => ({
  created_at: created,
  updated_at: updated,
  published_at: published,
});

const files = [
  {
    id: 1,
    name: 'hero',
    hash: 'hero_9f2c1a',
    ext: '.png',
    mime: 'image/png',
    size: 0.068,
    url: '/uploads/hero_9f2c1a.png',
    width: 1,
    height: 1,
    formats: { thumbnail: { url: '/uploads/thumbnail_hero_9f2c1a.png', width: 1, height: 1 } },
    provider: 'local',
    provider_metadata: null,
    ...stamp('2023-01-05T09:00:00.000Z'),
  },
  {
    id: 2,
    name: 'diagram',
    hash: 'diagram_44bd07',
    ext: '.png',
    mime: 'image/png',
    size: 0.068,
    url: '/uploads/diagram_44bd07.png',
    width: 1,
    height: 1,
    formats: {},
    provider: 'local',
    provider_metadata: null,
    ...stamp('2023-01-06T09:00:00.000Z'),
  },
];

const sections = [
  { id: 1, name: 'News', ...stamp('2023-02-01T08:00:00.000Z') },
  { id: 2, name: 'Guides', ...stamp('2023-02-02T08:00:00.000Z') },
];

const categories = [
  {
    id: 1,
    name: 'Engineering',
    slug: 'engineering',
    // Required relation: publishing this category needs its section published first, which is
    // what the migration's dependency-ordered publish is for.
    section: sections[0],
    parent: null,
    ...stamp('2023-03-01T08:00:00.000Z'),
  },
  {
    id: 2,
    name: 'How-to',
    slug: 'how-to',
    section: sections[1],
    // A self-relation, optional, so it never orders anything.
    parent: { id: 1, name: 'Engineering' },
    ...stamp('2023-03-02T08:00:00.000Z'),
  },
];

const tags = [
  { id: 1, name: 'intro', ...stamp('2023-04-01T08:00:00.000Z') },
  // The `tag` content type does not enable Strapi's draft/publish plugin, so `published_at` is
  // not even a field - yet this entry is live, and must arrive published.
  { id: 2, name: 'news', ...stamp('2023-04-02T08:00:00.000Z', '2023-04-02T08:00:00.000Z', null) },
];

const home = {
  id: 1,
  headline: 'Welcome home',
  intro: 'The landing page.',
  hero: files[1],
  seo: { id: 41, title: 'Home', description: 'The landing page' },
  ...stamp('2023-01-10T08:00:00.000Z'),
};

const articles = [
  {
    id: 1,
    title: 'Hello from Strapi',
    slug: 'hello-from-strapi',
    body: '# Heading\n\nSome **markdown** body.',
    excerpt: 'A first post.',
    views: 120,
    rating: 4.5,
    featured: true,
    publishedOn: '2024-03-01',
    reviewedAt: '2024-03-02T10:00:00.000Z',
    status: 'live',
    meta: { tags: ['intro', 'news'], nested: { level: 2 } },
    cover: files[0],
    gallery: [files[0], files[1]],
    seo: { id: 11, title: 'Hello from Strapi', description: 'The first post' },
    sections: [
      { id: 21, text: 'A pull quote.', cite: 'Someone' },
      { id: 22, text: 'Another one.', cite: null },
    ],
    blocks: [
      { __component: 'blog.quote', id: 31, text: 'Quoted.', cite: 'Anon' },
      { __component: 'default.seo', id: 32, title: 'Block title', description: 'Block description' },
    ],
    // Two components that reach each other through arrays, nested one level deep, so the value
    // conversion has to follow the value rather than the schema.
    tree: {
      id: 61,
      line: 'root',
      branches: [{ id: 62, text: 'first branch', back: [{ id: 63, line: 'inner', branches: [] }] }],
    },
    category: categories[0],
    tags: [tags[0], tags[1]],
    featuredOn: home,
    author: { id: 5, username: 'editor' },
    ...stamp('2024-03-01T09:00:00.000Z', '2024-03-02T10:00:00.000Z', '2024-03-01T12:00:00.000Z'),
  },
  {
    // Never published in Strapi, so it must arrive as a draft - with every field populated, to
    // show that an empty optional value is carried as empty rather than dropped.
    id: 2,
    title: 'A draft with nothing attached',
    slug: 'a-draft-with-nothing-attached',
    body: '',
    excerpt: null,
    views: 0,
    rating: null,
    featured: false,
    publishedOn: null,
    reviewedAt: null,
    status: 'draft',
    meta: null,
    cover: null,
    gallery: [],
    seo: null,
    sections: [],
    blocks: [],
    tree: null,
    category: categories[0],
    tags: [tags[0]],
    featuredOn: null,
    author: null,
    ...stamp('2024-02-01T09:00:00.000Z', '2024-02-01T09:00:00.000Z', null),
  },
  {
    // Deliberately broken: `title` is required, and this one is empty, so the migration has to
    // report it as skipped rather than let the CMS refuse the whole batch.
    id: 3,
    title: '',
    slug: 'broken-post',
    body: 'no title',
    views: 3,
    featured: false,
    status: 'review',
    cover: { id: 999 },
    gallery: [],
    seo: null,
    sections: [],
    blocks: [],
    tree: null,
    category: null,
    tags: [],
    featuredOn: null,
    author: null,
    ...stamp('2024-04-01T09:00:00.000Z', '2024-04-01T09:00:00.000Z', null),
  },
];

/** The route each collection type is served on, exactly as its `settings.json` implies. */
const collectionRoutes = new Map([
  ['/blog-posts', articles],
  ['/categories', categories],
  ['/sections', sections],
  ['/tags', tags],
]);

/** v3 hides drafts unless `_publicationState=preview`. */
function visible(entry, url) {
  if (url.searchParams.get('_publicationState') === 'preview') return true;
  return Boolean(entry.published_at);
}

function send(response, status, body, headers = {}) {
  const payload = Buffer.isBuffer(body) ? body : Buffer.from(JSON.stringify(body));
  response.writeHead(status, {
    'Content-Type': Buffer.isBuffer(body) ? 'application/octet-stream' : 'application/json',
    'Content-Length': payload.length,
    ...headers,
  });
  response.end(payload);
}

function authorised(request) {
  return request.headers.authorization === `Bearer ${JWT}`;
}

// === Definitions, the way the content-type-builder normalises them ==========================

function normaliseAttribute(attribute) {
  if (attribute.model === 'file' || attribute.collection === 'file' || attribute.plugin === 'upload') {
    return { ...attribute, type: 'media', multiple: Boolean(attribute.collection) };
  }
  if (!attribute.type && (attribute.model || attribute.collection)) {
    // Note the casing the admin API actually uses: `targetAttribute`, not `via`.
    return {
      nature: attribute.collection ? 'oneToMany' : 'oneToOne',
      target: attribute.model ?? attribute.collection,
      ...(attribute.via ? { targetAttribute: attribute.via } : {}),
      ...(attribute.plugin ? { plugin: attribute.plugin } : {}),
      ...(attribute.required ? { required: true } : {}),
    };
  }
  return attribute;
}

function normaliseSchema(schema) {
  const attributes = {};
  for (const [name, attribute] of Object.entries(schema.attributes ?? {})) {
    attributes[name] = normaliseAttribute(attribute);
  }
  return { ...schema, attributes };
}

async function readBuilderContentTypes() {
  const apiRoot = path.join(FIXTURE_DIR, 'api');
  const contentTypes = [];
  for (const apiId of await readdir(apiRoot)) {
    const models = path.join(apiRoot, apiId, 'models');
    for (const file of await readdir(models)) {
      if (!file.endsWith('.settings.json')) continue;
      const schema = JSON.parse(await readFile(path.join(models, file), 'utf8'));
      contentTypes.push({ uid: apiId, apiID: apiId, schema: normaliseSchema(schema) });
    }
  }
  return contentTypes;
}

async function readBuilderComponents() {
  const root = path.join(FIXTURE_DIR, 'components');
  const components = [];
  for (const category of await readdir(root)) {
    for (const file of await readdir(path.join(root, category))) {
      if (!file.endsWith('.json')) continue;
      const uid = `${category}.${file.replace(/\.json$/, '')}`;
      const schema = JSON.parse(await readFile(path.join(root, category, file), 'utf8'));
      components.push({ uid, category, apiId: file.replace(/\.json$/, ''), schema: normaliseSchema(schema) });
    }
  }
  return components;
}

const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  const pathname = url.pathname;

  if (request.method === 'POST' && pathname === '/auth/local') {
    let body = '';
    request.on('data', (chunk) => {
      body += chunk;
    });
    request.on('end', () => {
      try {
        const { identifier, password } = JSON.parse(body || '{}');
        if (identifier === 'strapi@example.com' && password === 'strapi-password') {
          return send(response, 200, { jwt: JWT, user: { id: 1, username: 'strapi', email: identifier } });
        }
        return send(response, 400, { statusCode: 400, error: 'Bad Request', message: 'Invalid identifier or password' });
      } catch {
        return send(response, 400, { statusCode: 400, message: 'Bad Request' });
      }
    });
    return;
  }

  if (request.method === 'POST' && pathname === '/admin/login') {
    return send(response, 200, { data: { token: ADMIN_JWT, user: { id: 1, email: 'admin@example.com' } } });
  }

  // The admin content-type-builder, which needs an admin token rather than the content JWT.
  if (pathname.startsWith('/content-type-builder/')) {
    if (request.headers.authorization !== `Bearer ${ADMIN_JWT}`) {
      return send(response, 401, { statusCode: 401, error: 'Unauthorized' });
    }
    if (pathname === '/content-type-builder/content-types') {
      return send(response, 200, { data: await readBuilderContentTypes() });
    }
    if (pathname === '/content-type-builder/components') {
      return send(response, 200, { data: await readBuilderComponents() });
    }
    return send(response, 404, { statusCode: 404, message: `No route for ${pathname}` });
  }

  if (pathname.startsWith('/uploads/')) {
    return send(response, 200, PIXEL);
  }

  // Everything below is the content API, which v3 protects with the Users & Permissions JWT.
  if (!authorised(request)) {
    return send(response, 401, { statusCode: 401, error: 'Unauthorized', message: 'Missing or invalid credentials' });
  }

  if (pathname.endsWith('/count')) {
    const entries = collectionRoutes.get(pathname.slice(0, -'/count'.length));
    if (entries) {
      // A bare number, exactly as v3's `/count` answers.
      return send(response, 200, entries.filter((entry) => visible(entry, url)).length);
    }
  }

  const entries = collectionRoutes.get(pathname);
  if (entries) {
    const live = entries.filter((entry) => visible(entry, url));
    const start = Number(url.searchParams.get('_start') ?? 0);
    const limit = Number(url.searchParams.get('_limit') ?? 100);
    return send(response, 200, live.slice(start, start + limit));
  }

  if (pathname === '/home') {
    return send(response, 200, home);
  }
  if (pathname === '/upload/files') {
    const start = Number(url.searchParams.get('_start') ?? 0);
    const limit = Number(url.searchParams.get('_limit') ?? 100);
    return send(response, 200, files.slice(start, start + limit));
  }

  return send(response, 404, { statusCode: 404, error: 'Not Found', message: `No route for ${pathname}` });
});

const portFlag = process.argv.indexOf('--port');
const port = portFlag === -1 ? 1337 : Number(process.argv[portFlag + 1]);
server.listen(port, '127.0.0.1', () => {
  process.stdout.write(`fake Strapi v3 listening on http://127.0.0.1:${port}\n`);
});
