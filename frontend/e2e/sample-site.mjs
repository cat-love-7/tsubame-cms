/**
 * The sample site: four categories, five articles, one page and four covers.
 *
 * Two things put it into an empty CMS. `e2e/screenshots.mjs` photographs it for the README, and
 * `scripts/demo.sh` leaves it running for a reader to click around in. One fixture, so what the
 * README shows is what the demo serves.
 *
 * It writes over HTTP alone - no browser, and nothing to install beyond Node. The pictures are
 * drawn rather than found, so that a CMS demo does not ship somebody else's photograph, and the
 * covers are uploaded with the small copy the library tiles show.
 *
 * Usage: node e2e/sample-site.mjs                (a server on http://localhost:4200)
 *        BASE_URL=... ADMIN_USERNAME=... ADMIN_PASSWORD=... node e2e/sample-site.mjs
 */
import { deflateSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';

/** Where the servers are unless the caller says otherwise: the dev server proxies the API. */
const DEFAULT_BASE = 'http://localhost:4200';

const CATEGORIES = ['Engineering', 'Design', 'Product', 'Field notes'];

/** Enough of a body that the editor screenshot shows a form somebody filled in. */
const ARTICLES = [
  {
    title: 'Editing a CMS without a build step',
    category: 'Engineering',
    summary: 'What changes when the editor talks to the API instead of to a repository.',
    body: `## The short version

A build step is a promise that content and code ship together. That promise is worth keeping for
code, and expensive for prose: every typo waits for a deploy.

Tsubame keeps the two apart:

- the **schema** is data, so an editor can add a field from the browser
- **drafts** are a second copy of an item, so a half-written page is never what a visitor reads
- **preview links** are signed addresses, so a reviewer sees unpublished work without an account

The cost is honest: the site has to fetch its content at runtime, or at build time from the same
API.`,
    published: true,
  },
  {
    title: 'Why drafts and live content are two copies',
    category: 'Engineering',
    summary: 'Unpublishing and undoing are only possible if the working copy is kept apart.',
    body: `## One item, two records

Every item has a working copy and a published copy. Saving writes the first; publishing copies it
across; unpublishing takes the second away again.

That is what makes "undo my unpublished changes" a delete rather than a diff, and it is why an
editor can compare what they have against what is live.`,
    published: true,
  },
  {
    title: 'Drawing a schema in the browser',
    category: 'Design',
    summary: 'Fields with widths and heights, on the same grid the content editor uses.',
    body: `## The grid is the interface

A field carries a width (a colspan of twelve) and a height (a multiple of the row unit). The schema
editor draws exactly what the content editor will draw, because both ask the same function for the
cell's style.

- **width** maps to grid columns
- **height** is a minimum, not a rowspan: a six-row textarea is taller than a checkbox, and the
  layout should say so rather than fight it.`,
    published: true,
  },
  {
    title: 'Composite fields, and when not to',
    category: 'Product',
    summary:
      'A definition reused across collections, and the cases that want a collection instead.',
    body: `## Reuse, not hierarchy

A composite definition is a named group of fields: an SEO block, a call to action, a card. An array
of composites is how a page gets a list of them.

When the same thing needs its own URL, its own publishing, or its own permissions, it wants to be a
collection with a relation - not a composite.`,
    published: false,
  },
  {
    title: 'Notes from the first deployment',
    category: 'Field notes',
    summary: 'What Terraform could not do, and the four things that had to be made by hand.',
    body: `## What an apply does not make

Three things are the operator's: the **secret**, the **domain**, and its **certificate**. They come
first because the rest refers to them.

Everything else - the table, the two buckets, the pool, the distribution, the function - is one
apply away, and the smoke test asks the result the questions a deployment has to answer.`,
    published: true,
  },
];

const HOME = {
  heading: 'Tsubame',
  intro: `A CMS for people who draw their own content model.

- collections with a schema you build in the browser
- drafts, publishing, and a way back
- an API a site builds from`,
};

// ------------------------------------------------------------------------------------ pictures

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(buffer) {
  let crc = 0xffffffff;
  for (const byte of buffer) crc = CRC_TABLE[(crc ^ byte) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const buffer = Buffer.alloc(data.length + 12);
  buffer.writeUInt32BE(data.length, 0);
  buffer.write(type, 4, 'ascii');
  data.copy(buffer, 8);
  buffer.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, 'ascii'), data])), data.length + 8);
  return buffer;
}

/** A PNG built byte by byte: a picture here is a cover, and a CMS demo should not ship one grey. */
function png(width, height, pixel) {
  const raw = Buffer.alloc((width * 3 + 1) * height);
  let at = 0;
  for (let y = 0; y < height; y += 1) {
    raw[at] = 0;
    at += 1;
    for (let x = 0; x < width; x += 1) {
      const [r, g, b] = pixel(x, y);
      raw[at] = r;
      raw[at + 1] = g;
      raw[at + 2] = b;
      at += 3;
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bits per channel
  header[9] = 2; // truecolour
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/**
 * A gradient with one soft diagonal sweep through it, so a cover reads as a picture.
 *
 * The coordinates are fractions of the frame, so the same painter draws the cover and the small
 * copy of it - which is why the tiles show the cover rather than a resized original: there is no
 * image decoder here, and there does not need to be one for a picture drawn to order.
 */
function cover(top, bottom, width, height) {
  return png(width, height, (x, y) => {
    const down = y / (height - 1);
    const diagonal = Math.min(1, Math.max(0, x / (width - 1) - down + 0.35));
    const sheen = 1 - Math.min(1, Math.abs(diagonal - 0.5) * 4);
    return top.map((channel, index) => {
      const value = channel + (bottom[index] - channel) * down;
      return Math.round(value + (255 - value) * 0.16 * sheen);
    });
  });
}

/** The size a cover is stored at, and the widest tile the library draws one in. */
const COVER_SIZE = [1200, 800];
const THUMBNAIL_SIZE = [360, 240];

const COVER_PAINT = [
  [
    [18, 62, 72],
    [96, 190, 176],
  ],
  [
    [32, 40, 92],
    [126, 140, 232],
  ],
  [
    [92, 60, 24],
    [232, 190, 120],
  ],
  [
    [44, 52, 58],
    [150, 168, 178],
  ],
];

const COVERS = COVER_PAINT.map(([top, bottom]) => ({
  cover: cover(top, bottom, ...COVER_SIZE),
  thumbnail: cover(top, bottom, ...THUMBNAIL_SIZE),
}));

// ------------------------------------------------------------------------------------- seeding

/** One JSON call, in the shape the admin app sends it. */
async function api(base, method, path, body, token) {
  const response = await fetch(`${base}/api${path}`, {
    method,
    headers: {
      ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`${method} ${path} → ${response.status} ${text}`);
  }
  return text ? JSON.parse(text) : null;
}

/**
 * Remove something the fixture is about to write again.
 *
 * A fresh data directory has none of it, and a re-run against a used CMS would otherwise fail on a
 * name that is already taken.
 */
async function deleteIfPresent(base, path, token) {
  const response = await fetch(`${base}/api${path}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok && response.status !== 404) {
    throw new Error(`could not delete ${path}: ${response.status}`);
  }
}

/**
 * Where a handed-out upload URL goes: on-premises it is a path the API already prefixes, on AWS it
 * is absolute and complete. The same rule the app's own `apiUrl` follows.
 */
function uploadAddress(base, pathOrUrl) {
  if (/^https?:\/\//i.test(pathOrUrl)) {
    return pathOrUrl;
  }
  return pathOrUrl.startsWith('/api') ? `${base}${pathOrUrl}` : `${base}/api${pathOrUrl}`;
}

/** Put bytes where the server asked for them: the one route that is not JSON in, JSON out. */
async function putBytes(base, pathOrUrl, bytes, token, contentType) {
  const response = await fetch(uploadAddress(base, pathOrUrl), {
    method: 'PUT',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': contentType },
    body: bytes,
  });
  if (!response.ok) {
    throw new Error(`PUT ${pathOrUrl} → ${response.status} ${await response.text()}`);
  }
}

function slugify(title) {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
}

const COLLECTION_SCHEMA = [
  {
    name: 'title',
    field_type: { Text: {} },
    required: true,
    unique: true,
    width: 8,
    height: 1,
    is_title: true,
    show_in_list: true,
  },
  {
    name: 'slug',
    field_type: { Slug: { generate_from: 'title' } },
    required: false,
    width: 4,
    height: 1,
  },
  { name: 'summary', field_type: { Text: {} }, required: false, width: 12, height: 1 },
  { name: 'cover', field_type: 'Image', required: false, width: 4, height: 2, show_in_list: true },
  { name: 'published_at', field_type: 'Date', required: false, width: 4, height: 1 },
  {
    name: 'category',
    field_type: {
      Relation: { target: { kind: 'collection', name: 'categories' } },
    },
    required: false,
    width: 4,
    height: 1,
    show_in_list: true,
  },
  { name: 'body', field_type: { Markdown: {} }, required: false, width: 12, height: 9 },
];

const CATEGORY_SCHEMA = [
  {
    name: 'name',
    field_type: { Text: {} },
    required: true,
    unique: true,
    width: 12,
    height: 1,
    is_title: true,
  },
];

const PAGE_SCHEMA = [
  { name: 'heading', field_type: { Text: {} }, required: true, width: 12, height: 1 },
  { name: 'hero', field_type: 'Image', required: false, width: 12, height: 3 },
  { name: 'intro', field_type: { Markdown: {} }, required: false, width: 12, height: 6 },
];

/** Upload the covers, each with the small copy a tile shows. */
async function uploadCovers(base, token) {
  const covers = [];
  for (const [index, { cover: bytes, thumbnail }] of COVERS.entries()) {
    const info = await api(
      base,
      'POST',
      '/models/images/get_upload_url',
      { original_filename: `cover-${index + 1}.png`, ext: 'png', size: bytes.length },
      token,
    );
    await putBytes(base, info.upload_url, bytes, token, 'image/png');
    await putBytes(
      base,
      `/models/images/${info.id}/thumbnail?ext=png`,
      thumbnail,
      token,
      'image/png',
    );
    covers.push({ id: info.id, url: info.url });
  }
  return covers;
}

async function seedContent(base, token, covers) {
  await deleteIfPresent(base, '/models/collections/articles', token);
  await deleteIfPresent(base, '/models/collections/categories', token);
  await deleteIfPresent(base, '/models/single_pages/home', token);

  await api(base, 'POST', '/models/collections/categories/schema', CATEGORY_SCHEMA, token);
  for (const name of CATEGORIES) {
    await api(base, 'POST', '/models/collections/categories/item', { name }, token);
  }
  const categories = await api(
    base,
    'GET',
    '/models/collections/categories/items?limit=50',
    undefined,
    token,
  );
  // An item list is `[id, values]` pairs, and the id is what a relation stores.
  const categoryId = (name) => categories.find(([, values]) => values.name === name)?.[0];

  await api(base, 'POST', '/models/collections/articles/schema', COLLECTION_SCHEMA, token);
  for (const [index, article] of ARTICLES.entries()) {
    await api(
      base,
      'POST',
      '/models/collections/articles/item',
      {
        title: article.title,
        slug: slugify(article.title),
        summary: article.summary,
        cover: covers[index % covers.length],
        published_at: `2026-0${(index % 9) + 1}-1${index % 9}`,
        category: { target: 'categories', item: categoryId(article.category) },
        body: article.body,
      },
      token,
    );
  }
  const items = await api(
    base,
    'GET',
    '/models/collections/articles/items?limit=50',
    undefined,
    token,
  );
  for (const article of ARTICLES.filter((entry) => entry.published)) {
    const id = items.find(([, values]) => values.title === article.title)?.[0];
    await api(base, 'POST', `/models/collections/articles/items/${id}/publish`, undefined, token);
  }

  await api(base, 'POST', '/models/single_pages/home/schema', PAGE_SCHEMA, token);
  await api(
    base,
    'PUT',
    '/models/single_pages/home/item',
    { heading: HOME.heading, hero: covers[1], intro: HOME.intro },
    token,
  );
  await api(base, 'POST', '/models/single_pages/home/publish', undefined, token);

  return items.find(([, values]) => values.title === ARTICLES[0].title)?.[0];
}

/**
 * Sign in and write the whole sample site, replacing what it is named after.
 *
 * Returns the id of the first article (the one the README's editor picture shows) and the covers,
 * so a caller can photograph a screen without looking anything up again.
 */
export async function seedSampleSite({ base = DEFAULT_BASE, username, password, log = () => {} }) {
  const login = await api(base, 'POST', '/auth/login', { username, password });
  log(`signed in as ${username}`);
  const covers = await uploadCovers(base, login.token);
  const featured = await seedContent(base, login.token, covers);
  log(
    `${CATEGORIES.length} categories, ${ARTICLES.length} articles, 1 page, ${covers.length} images`,
  );
  return { featured, covers };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const base = process.env.BASE_URL ?? DEFAULT_BASE;
  console.log(`== writing the sample site into ${base} ==`);
  await seedSampleSite({
    base,
    username: process.env.ADMIN_USERNAME ?? 'admin@example.com',
    password: process.env.ADMIN_PASSWORD ?? 'admin-password',
    log: (line) => console.log(`  ${line}`),
  });
}
