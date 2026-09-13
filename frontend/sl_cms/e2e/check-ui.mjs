/**
 * End-to-end check for the admin collection list, driven by a real browser.
 *
 * This is deliberately *not* part of `npm test`: the unit suite stays fast, browserless and
 * dependency-free, while this needs two running servers, a browser and a few seconds. What
 * it covers is the part the unit tests cannot see, because they stub the service out — that
 * the screen really works against the API:
 *
 *   - signing in through the form
 *   - one page of 25 rows, ordered by id, with the total in the pager
 *   - the status badge and the Updated column
 *   - the next page and a different page size reaching the server
 *   - publishing an item from the list
 *   - deleting the only row of the last page, which has to fall back a page
 *   - the image library: uploading, serving and deleting an image
 *   - picking that image from the library while editing content, and saving it
 *   - adding several images to an image array at once, and saving them in order
 *   - no console errors while all of that happens
 *
 * It seeds its own data (`e2e_blog`, `e2e_small`, `e2e_images`) and recreates those
 * collections on every run, so it neither depends on nor disturbs whatever else is in the
 * dev database.
 *
 * Prerequisites and usage: see e2e/README.md.
 */
import { chromium } from 'playwright';

const BASE = process.env.BASE_URL ?? 'http://localhost:4200';
const API = `${BASE}/api`;
const EMAIL = process.env.ADMIN_EMAIL ?? 'admin@example.com';
const PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin-password';
/** Enough items for several pages, and for a 50-row page. */
const COLLECTION = process.env.COLLECTION ?? 'e2e_blog';
const TOTAL = Number(process.env.TOTAL ?? 60);
/** One more than a full page, so the last page holds exactly one row. */
const LAST_PAGE_COLLECTION = process.env.LAST_PAGE_COLLECTION ?? 'e2e_small';
const LAST_PAGE_TOTAL = Number(process.env.LAST_PAGE_TOTAL ?? 26);
/** One item whose only field is an image, for the library picker. */
const IMAGE_COLLECTION = process.env.IMAGE_COLLECTION ?? 'e2e_images';

/** A 1x1 PNG: enough for the upload path to be exercised for real. */
const PNG_BASE64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

const TEXT_SCHEMA = [
  { name: 'title', field_type: { Text: {} }, required: true, width: 12, height: 1 },
];
/** One single image, and one array of them: the two ways an image field is used. */
const IMAGE_SCHEMA = [
  { name: 'photo', field_type: 'Image', required: false, width: 12, height: 1 },
  { name: 'gallery', field_type: { Array: ['Image'] }, required: false, width: 12, height: 1 },
];

const results = [];
function check(label, ok, detail = '') {
  results.push({ label, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${label}${detail ? `  (${detail})` : ''}`);
}

// ------------------------------------------------------------------------------ seed data

const browser = await chromium.launch({ args: ['--no-sandbox'] });
const context = await browser.newContext();
// Requests go through the browser's own network stack, not Node's `fetch`: `localhost`
// resolves to IPv4 first for Node, while the dev server may only listen on IPv6.
const request = context.request;

async function api(method, path, body, token) {
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

/** Replace `name` with a fresh collection holding `count` items. */
async function seedCollection(name, count, token, schema = TEXT_SCHEMA, item = null) {
  const reset = await request.fetch(`${API}/models/collections/${name}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${token}` },
  });
  // A missing collection is as good as a deleted one.
  if (!reset.ok() && reset.status() !== 404) {
    throw new Error(`could not reset ${name}: ${reset.status()}`);
  }

  await api('POST', `/models/collections/${name}/schema`, schema, token);
  for (let index = 1; index <= count; index += 1) {
    const body = item ? item(index) : { title: `${name} item ${index}` };
    await api('POST', `/models/collections/${name}/item`, body, token);
  }
}

async function seed() {
  const login = await api('POST', '/auth/login', { email: EMAIL, password: PASSWORD });
  await seedCollection(COLLECTION, TOTAL, login.token);
  await seedCollection(LAST_PAGE_COLLECTION, LAST_PAGE_TOTAL, login.token);
  await seedCollection(IMAGE_COLLECTION, 1, login.token, IMAGE_SCHEMA, () => ({
    photo: null,
    gallery: [],
  }));
  return login.token;
}

// ------------------------------------------------------------------------------- the check

// Fail with a sentence instead of a stack trace when the servers are not up. An
// unauthenticated request is enough: a 401 still proves that the proxy and the API answer.
const probe = await request.fetch(`${API}/models/collections`).catch(() => null);
if (!probe) {
  console.error(
    `Cannot reach ${API}.\n` +
      'Start the backend (cargo run, port 8080) and the dev server (npm start, port 4200) ' +
      'first; see e2e/README.md.',
  );
  await browser.close();
  process.exit(2);
}

const token = await seed();
console.log(
  `seeded: ${COLLECTION} (${TOTAL} items), ${LAST_PAGE_COLLECTION} (${LAST_PAGE_TOTAL} items), ` +
    `${IMAGE_COLLECTION} (1 item)\n`,
);

const page = await context.newPage();

const consoleErrors = [];
page.on('pageerror', (error) => consoleErrors.push(String(error)));
page.on('console', (message) => {
  if (message.type() === 'error') consoleErrors.push(message.text());
});
// Deleting asks for confirmation through window.confirm.
page.on('dialog', (dialog) => dialog.accept());

/** Data rows only: the "No items yet." row has no status badge. */
const dataRows = () => page.locator('table.items tbody tr:has(app-item-status)');
const firstRow = () => dataRows().first();
const badgeOf = (row) => row.locator('app-item-status .badge');

async function waitForRows(count) {
  await page
    .waitForFunction(
      (expected) =>
        document.querySelectorAll('table.items tbody tr app-item-status').length === expected,
      count,
      { timeout: 15000 },
    )
    .catch(() => {});
}

try {
  // -------------------------------------------------------------- sign in through the form
  await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
  await page.fill('input[name=email]', EMAIL);
  await page.fill('input[name=password]', PASSWORD);
  await page.click('button:has-text("Sign in")');
  await page
    .waitForFunction(() => !location.pathname.startsWith('/login'), null, { timeout: 15000 })
    .catch(() => {});
  check('ログインフォームからサインインできる', !page.url().includes('/login'), page.url());

  // -------------------------------------------------------------- first page
  await page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });

  const firstPageRows = await dataRows().count();
  check('1 ページ目は既定の 25 件', firstPageRows === 25, `${firstPageRows} 行`);

  const firstId = (await firstRow().locator('td').first().textContent())?.trim();
  check('id 昇順で 1 から始まる', firstId === '1', `id=${firstId}`);

  const paginator = page.locator('mat-paginator');
  check('ページャが表示される', await paginator.isVisible());

  const rangeLabel = (await paginator.locator('.mat-mdc-paginator-range-label').textContent())?.trim();
  check(`ページャに総件数 ${TOTAL} が出る`, new RegExp(String(TOTAL)).test(rangeLabel ?? ''), rangeLabel);

  // -------------------------------------------------------------- status and updated columns
  const badge = (await badgeOf(firstRow()).textContent())?.trim();
  check('状態バッジが出る', badge === 'Draft' || badge === 'Published', badge);

  const updated = (await firstRow().locator('td.updated').textContent())?.trim();
  check('Updated 列に日時が出る', Boolean(updated) && updated !== '—', updated);

  // -------------------------------------------------------------- next page
  // Material overlays a touch target on the pager buttons, so a plain click never lands.
  await paginator.locator('.mat-mdc-paginator-navigation-next').click({ force: true });
  await waitForRows(25);
  const secondPageFirstId = (await firstRow().locator('td').first().textContent())?.trim();
  check('次ページは id 26 から始まる', secondPageFirstId === '26', `id=${secondPageFirstId}`);

  // -------------------------------------------------------------- page size
  await paginator.locator('mat-select').click({ force: true });
  await page.locator('mat-option', { hasText: '50' }).first().click({ force: true });
  await waitForRows(50);
  const rowsAtFifty = await dataRows().count();
  check('ページサイズ 50 で 50 行', rowsAtFifty === 50, `${rowsAtFifty} 行`);

  // -------------------------------------------------------------- publish a draft from the list
  const draftRow = dataRows()
    .filter({ has: page.locator('button[aria-label^="publish item"]') })
    .first();
  const draftId = (await draftRow.locator('td').first().textContent())?.trim();

  // Identify the row by its id afterwards, not by which button it shows: the label flips
  // as soon as the publish lands.
  const rowById = (id) =>
    dataRows()
      .filter({ has: page.locator(`button[aria-label$="item ${id}"]`) })
      .first();

  await draftRow.locator('button[aria-label^="publish item"]').click();
  await rowById(draftId)
    .locator('app-item-status .badge')
    .filter({ hasText: 'Published' })
    .waitFor({ timeout: 10000 })
    .catch(() => {});
  const badgeAfterPublish = (await badgeOf(rowById(draftId)).textContent())?.trim();
  check('一覧から公開できる', badgeAfterPublish === 'Published', `id=${draftId} → ${badgeAfterPublish}`);

  // -------------------------------------------------------------- delete the last page's only row
  await page.goto(`${BASE}/collections/${LAST_PAGE_COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });
  await page.locator('mat-paginator .mat-mdc-paginator-navigation-last').click({ force: true });
  await waitForRows(1);
  const lastPageRows = await dataRows().count();
  check('最終ページは 1 行', lastPageRows === 1, `${lastPageRows} 行`);

  const doomedId = (await firstRow().locator('td').first().textContent())?.trim();
  await firstRow().locator('button[aria-label^="delete item"]').click();
  await waitForRows(25);
  const rowsAfterDelete = await dataRows().count();
  check(
    '最終ページの最後の 1 件を削除すると前のページへ戻る',
    rowsAfterDelete === 25,
    `${rowsAfterDelete} 行 (削除 id=${doomedId})`,
  );

  // -------------------------------------------------------------- the image library
  await page.goto(`${BASE}/settings/images`, { waitUntil: 'networkidle' });
  const imagesBefore = await page.locator('.library .image').count();

  // Upload twice, so the image array has something to choose between. Each upload goes
  // through the same hidden file input the button drives.
  for (const name of ['e2e-1.png', 'e2e-2.png']) {
    const expected = (await page.locator('.library .image').count()) + 1;
    await page.setInputFiles('input[type=file]', {
      name,
      mimeType: 'image/png',
      buffer: Buffer.from(PNG_BASE64, 'base64'),
    });
    await page
      .waitForFunction(
        (count) => document.querySelectorAll('.library .image').length === count,
        expected,
        { timeout: 15000 },
      )
      .catch(() => {});
  }
  const imagesAfterUpload = await page.locator('.library .image').count();
  check(
    '画像をアップロードできる',
    imagesAfterUpload === imagesBefore + 2,
    `${imagesBefore} → ${imagesAfterUpload}`,
  );

  // The library is newest first, so the last upload is the first card.
  const uploadedCard = page.locator('.library .image').first();
  const uploadedId = (await uploadedCard.locator('.meta').textContent())?.trim();
  check(
    'アップロードした画像が名前つきで並ぶ',
    Boolean((await uploadedCard.locator('.name').textContent())?.includes('e2e-2.png')),
    uploadedId,
  );

  const imageSource = await uploadedCard.locator('img').getAttribute('src');
  const servedStatus = await page.evaluate(async (src) => (await fetch(src)).status, imageSource);
  check('画像の実体が配信される', servedStatus === 200, `${imageSource} → ${servedStatus}`);

  // -------------------------------------------------------------- pick images while editing
  await page.goto(`${BASE}/collections/${IMAGE_COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('button:has-text("Choose existing")').click();
  const thumbs = page.locator('.thumb');
  await thumbs.first().waitFor({ timeout: 15000 });
  const thumbCount = await thumbs.count();
  check('編集中に既存画像を一覧できる', thumbCount >= 2, `${thumbCount} 件`);

  await thumbs.first().click();

  // The image array takes several at once, through the same library.
  await page.locator('button.array-add').click();
  await thumbs.nth(0).click();
  await thumbs.nth(1).click();
  await page.locator('button.array-add-selected').click();
  // Give the re-render a moment: `count()` does not wait for change detection.
  await page
    .waitForFunction(() => document.querySelectorAll('.array-item').length === 2, null, {
      timeout: 10000,
    })
    .catch(() => {});
  const arrayItems = await page.locator('.array-item').count();
  check('画像配列に複数まとめて追加できる', arrayItems === 2, `${arrayItems} 件`);

  await page.click('button:has-text("Save")');
  await page.waitForURL(`${BASE}/collections/${IMAGE_COLLECTION}`, { timeout: 15000 }).catch(() => {});

  const saved = await api(
    'GET',
    `/models/collections/${IMAGE_COLLECTION}/items/1`,
    undefined,
    token,
  );
  check(
    '選んだ画像がアイテムに保存される',
    typeof saved?.photo === 'object' && saved.photo !== null && saved.photo.url === imageSource?.replace('/api', ''),
    JSON.stringify(saved?.photo),
  );
  check(
    '画像配列が複数まとめて保存される',
    Array.isArray(saved?.gallery) &&
      saved.gallery.length === 2 &&
      saved.gallery.every((image) => image?.url?.startsWith('/images/')),
    JSON.stringify(saved?.gallery),
  );

  // -------------------------------------------------------------- delete them again
  await page.goto(`${BASE}/settings/images`, { waitUntil: 'networkidle' });
  for (let remaining = imagesAfterUpload - 1; remaining >= imagesBefore; remaining -= 1) {
    await page.locator('.library .image').first().locator('.remove').click();
    await page
      .waitForFunction(
        (expected) => document.querySelectorAll('.library .image').length === expected,
        remaining,
        { timeout: 15000 },
      )
      .catch(() => {});
  }
  const imagesAfterDelete = await page.locator('.library .image').count();
  check('画像を削除できる', imagesAfterDelete === imagesBefore, `${imagesAfterUpload} → ${imagesAfterDelete}`);

  check('ブラウザのコンソールエラーがない', consoleErrors.length === 0, consoleErrors.slice(0, 2).join(' | '));
} catch (error) {
  check('検証スクリプトが最後まで走る', false, String(error).split('\n')[0]);
} finally {
  await browser.close();
}

const failed = results.filter((result) => !result.ok);
console.log(`\n${results.length - failed.length}/${results.length} 件のチェックに成功`);
process.exit(failed.length === 0 ? 0 : 1);
