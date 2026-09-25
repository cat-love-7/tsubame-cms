/**
 * The screenshots the README shows.
 *
 * It seeds a small site - categories, articles, a page and a few images - through the API (driving
 * the interface to create content is the browser suite's job, not this one's), then takes one
 * picture per screen with a real browser over the result. `scripts/screenshots.sh` boots a server
 * with throwaway data first, so this can be run again whenever the interface changes.
 *
 * Usage: scripts/screenshots.sh
 *        BASE_URL=http://localhost:4200 node e2e/screenshots.mjs   (servers you started yourself)
 */
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateSync } from 'node:zlib';

import { chromium } from 'playwright';

const BASE = process.env.BASE_URL ?? 'http://localhost:4200';
const API = `${BASE}/api`;
const USERNAME = process.env.ADMIN_USERNAME ?? 'admin@example.com';
const PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin-password';
const OUT = resolve(dirname(fileURLToPath(import.meta.url)), '../../doc/images');

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

/** A gradient with one soft diagonal sweep through it, so a cover reads as a picture. */
function cover(top, bottom) {
  const width = 1200;
  const height = 800;
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

const COVERS = [
  cover([18, 62, 72], [96, 190, 176]),
  cover([32, 40, 92], [126, 140, 232]),
  cover([92, 60, 24], [232, 190, 120]),
  cover([44, 52, 58], [150, 168, 178]),
];

// ---------------------------------------------------------------------------------------- seeding

/** A request through the browser's own stack, so it goes where the page goes. */
async function api(request, method, path, body, token) {
  const response = await request.fetch(`${API}${path}`, {
    method,
    data: body,
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!response.ok()) {
    throw new Error(`${method} ${path} → ${response.status()} ${await response.text()}`);
  }
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

async function deleteIfPresent(request, path, token) {
  const response = await request.fetch(`${API}${path}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok() && response.status() !== 404) {
    throw new Error(`could not delete ${path}: ${response.status()}`);
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
      Relation: { target: { kind: 'collection', name: 'categories' }, has_many: false },
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

/** Upload the covers through the library screen, which is the one path that always works. */
async function uploadCovers(page, request, token) {
  await deleteIfPresent(request, '/models/collections/articles', token);
  await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
  for (const [index, bytes] of COVERS.entries()) {
    const expected = (await page.locator('.library .image').count()) + 1;
    await page.setInputFiles('input[type=file]', {
      name: `cover-${index + 1}.png`,
      mimeType: 'image/png',
      buffer: bytes,
    });
    await page
      .waitForFunction(
        (count) => document.querySelectorAll('.library .image').length === count,
        expected,
        {
          timeout: 20000,
        },
      )
      .catch(() => {});
  }
  // The library answers newest first, so the newest few, turned round, are the covers in the
  // order they were uploaded.
  const images = await api(request, 'GET', '/models/images', undefined, token);
  return images
    .slice(0, COVERS.length)
    .reverse()
    .map((image) => ({
      id: image.id,
      url: image.url,
    }));
}

async function seedContent(request, token, covers) {
  await deleteIfPresent(request, '/models/collections/categories', token);
  await deleteIfPresent(request, '/models/collections/articles', token);
  await deleteIfPresent(request, '/models/single_pages/home', token);

  await api(request, 'POST', '/models/collections/categories/schema', CATEGORY_SCHEMA, token);
  for (const name of CATEGORIES) {
    await api(request, 'POST', '/models/collections/categories/item', { name }, token);
  }
  const categories = await api(
    request,
    'GET',
    '/models/collections/categories/items?limit=50',
    undefined,
    token,
  );
  // An item list is `[id, values]` pairs, and the id is what a relation stores.
  const categoryId = (name) => categories.find(([, values]) => values.name === name)?.[0];

  await api(request, 'POST', '/models/collections/articles/schema', COLLECTION_SCHEMA, token);
  for (const [index, article] of ARTICLES.entries()) {
    await api(
      request,
      'POST',
      '/models/collections/articles/item',
      {
        title: article.title,
        slug: slugify(article.title),
        summary: article.summary,
        cover: covers[index % covers.length],
        published_at: `2026-0${(index % 9) + 1}-1${index % 9}`,
        category: [{ target: 'categories', item: categoryId(article.category) }],
        body: article.body,
      },
      token,
    );
  }
  const items = await api(
    request,
    'GET',
    '/models/collections/articles/items?limit=50',
    undefined,
    token,
  );
  for (const article of ARTICLES.filter((entry) => entry.published)) {
    const id = items.find(([, values]) => values.title === article.title)?.[0];
    await api(
      request,
      'POST',
      `/models/collections/articles/items/${id}/publish`,
      undefined,
      token,
    );
  }

  await api(request, 'POST', '/models/single_pages/home/schema', PAGE_SCHEMA, token);
  await api(
    request,
    'PUT',
    '/models/single_pages/home/item',
    { heading: HOME.heading, hero: covers[1], intro: HOME.intro },
    token,
  );
  await api(request, 'POST', '/models/single_pages/home/publish', undefined, token);

  return items.find(([, values]) => values.title === ARTICLES[0].title)?.[0];
}

// ------------------------------------------------------------------------------------- pictures

/** Everything a picture wants: the network quiet, the fonts in, the animations over. */
async function settle(page, extra = 400) {
  await page.waitForLoadState('networkidle').catch(() => {});
  await page
    .evaluate(async () => {
      // The icons are a lazily-loaded face: asking for it is what makes it appear before the shot.
      await document.fonts.load("24px 'Material Icons'").catch(() => {});
      await document.fonts.ready;
    })
    .catch(() => {});
  await page.waitForTimeout(extra);
}

/** The window a screen is worth: a list fits a laptop, a form is taller than one. */
const SHAPES = {
  tall: { width: 1440, height: 1300 },
  wide: { width: 1440, height: 900 },
  card: { width: 1280, height: 720 },
};

async function shoot(page, name, shape = 'wide') {
  await page.setViewportSize(SHAPES[shape]);
  await settle(page);
  await page.screenshot({ path: `${OUT}/${name}.png` });
  console.log(`  ${name}.png`);
}

async function main() {
  mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch({ args: ['--no-sandbox'] });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    locale: 'en-US',
  });
  // The interface follows the browser language; the pictures are English.
  await context.addInitScript(() => window.localStorage.setItem('tsubame.language', 'en'));
  const page = await context.newPage();
  const request = context.request;

  try {
    console.log('sign in:');
    await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
    await settle(page, 600);
    await shoot(page, 'sign-in', 'card');

    await page.fill('input[name=username]', USERNAME);
    await page.fill('input[name=password]', PASSWORD);
    await page.click('button:has-text("Sign in")');
    await page.waitForURL((url) => !url.pathname.startsWith('/login'), { timeout: 20000 });
    await settle(page);

    console.log('seed:');
    const login = await api(request, 'POST', '/auth/login', {
      username: USERNAME,
      password: PASSWORD,
    });
    const covers = await uploadCovers(page, request, login.token);
    const featured = await seedContent(request, login.token, covers);
    console.log(`  ${CATEGORIES.length} categories, ${ARTICLES.length} articles, 1 page`);

    console.log('screens:');
    await page.goto(`${BASE}/collections/articles`, { waitUntil: 'networkidle' });
    await page
      .locator('table.items tbody tr:has(app-item-status)')
      .first()
      .waitFor({ timeout: 20000 });
    await shoot(page, 'articles', 'wide');

    await page.goto(`${BASE}/collections/articles/edit/${featured}`, {
      waitUntil: 'networkidle',
    });
    await page.locator('textarea').first().waitFor({ timeout: 20000 });
    await shoot(page, 'editor', 'tall');

    await page.goto(`${BASE}/settings/schemas/collections/edit/articles`, {
      waitUntil: 'networkidle',
    });
    await settle(page, 800);
    await shoot(page, 'schema', 'tall');

    await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
    await page.locator('.library .image').first().waitFor({ timeout: 20000 });
    await shoot(page, 'library', 'wide');

    await page.goto(`${BASE}/single-pages/home`, { waitUntil: 'networkidle' });
    await page.locator('app-value-field').first().waitFor({ timeout: 20000 });
    await shoot(page, 'page', 'tall');

    console.log(`\nwrote the pictures to ${OUT}`);
  } finally {
    await browser.close();
  }
}

await main();
