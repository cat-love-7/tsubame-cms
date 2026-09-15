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
 *   - adding several images to an image array at once, and uploading one straight into it
 *   - the same array, inside a composite field
 *   - no console errors while all of that happens
 *
 * It seeds its own data (`e2e_blog`, `e2e_small`, `e2e_images`, `e2e_composite` and a
 * composite definition) and recreates those on every run, so it neither depends on nor
 * disturbs whatever else is in the dev database.
 *
 * Prerequisites and usage: see e2e/README.md.
 */
import { chromium } from 'playwright';

const BASE = process.env.BASE_URL ?? 'http://localhost:4200';
const API = `${BASE}/api`;
const USERNAME = process.env.ADMIN_USERNAME ?? process.env.ADMIN_EMAIL ?? 'admin@example.com';
const PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin-password';
/** Enough items for several pages, and for a 50-row page. */
const COLLECTION = process.env.COLLECTION ?? 'e2e_blog';
const TOTAL = Number(process.env.TOTAL ?? 60);
/** One more than a full page, so the last page holds exactly one row. */
const LAST_PAGE_COLLECTION = process.env.LAST_PAGE_COLLECTION ?? 'e2e_small';
const LAST_PAGE_TOTAL = Number(process.env.LAST_PAGE_TOTAL ?? 26);
/** One item whose only field is an image, for the library picker. */
const IMAGE_COLLECTION = process.env.IMAGE_COLLECTION ?? 'e2e_images';
/** One item whose field is a composite that itself holds an image array. */
const COMPOSITE_COLLECTION = process.env.COMPOSITE_COLLECTION ?? 'e2e_composite';
/** The collection this run builds through the schema editor, then uses as content. */
const SCHEMA_COLLECTION = process.env.SCHEMA_COLLECTION ?? 'e2e_schema_editor';
/** The composite definition its array field holds. */
const SCHEMA_BLOCK = process.env.SCHEMA_BLOCK ?? 'e2e_block';
const COMPOSITE_ID = process.env.COMPOSITE_ID ?? 'e2e_gallery_block';

/** A 1x1 PNG: enough for the upload path to be exercised for real. */
const PNG_BASE64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';

const TEXT_SCHEMA = [
  { name: 'title', field_type: { Text: {} }, required: true, width: 12, height: 1 },
  // A plain array: the JSON box is its whole editor, so the box has to say what it accepts.
  { name: 'scores', field_type: { Array: ['Number'] }, required: false, width: 12, height: 1 },
];
/** One single image, and one array of them: the two ways an image field is used. */
const IMAGE_SCHEMA = [
  { name: 'photo', field_type: 'Image', required: false, width: 12, height: 1 },
  { name: 'gallery', field_type: { Array: ['Image'] }, required: false, width: 12, height: 1 },
];
const IMAGE_ARRAY_FIELD = {
  name: 'images',
  field_type: { Array: ['Image'] },
  required: false,
  width: 12,
  height: 1,
};
const COMPOSITE_SCHEMA = [
  {
    name: 'block',
    field_type: { CompositeField: { id: COMPOSITE_ID } },
    required: false,
    width: 12,
    height: 1,
  },
];

const results = [];
function check(label, ok, detail = '') {
  results.push({ label, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${label}${detail ? `  (${detail})` : ''}`);
}

// ------------------------------------------------------------------------------ seed data

const browser = await chromium.launch({ args: ['--no-sandbox'] });
/**
 * Pin the interface language.
 *
 * The interface follows the browser until someone chooses otherwise, so without this every
 * check would depend on the language the test browser happens to ask for - and half of them
 * would start failing the moment a screen is translated. The check about switching sets its own
 * value on top of this.
 */
async function newContext() {
  const context = await browser.newContext();
  await context.addInitScript(() => {
    try {
      window.localStorage.setItem('sl_cms.language', 'en');
    } catch {
      // A context that refuses storage simply keeps the browser's own language.
    }
  });
  return context;
}

const context = await newContext();
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

/** Delete `path` if it is there; a 404 is as good as a deletion. */
async function deleteIfPresent(path, token) {
  const response = await request.fetch(`${API}${path}`, {
    method: 'DELETE',
    headers: { Authorization: `Bearer ${token}` },
  });
  if (!response.ok() && response.status() !== 404) {
    throw new Error(`could not delete ${path}: ${response.status()}`);
  }
}

/** Replace `name` with a fresh collection holding `count` items. */
async function seedCollection(name, count, token, schema = TEXT_SCHEMA, item = null) {
  await deleteIfPresent(`/models/collections/${name}`, token);

  await api('POST', `/models/collections/${name}/schema`, schema, token);
  for (let index = 1; index <= count; index += 1) {
    const body = item ? item(index) : { title: `${name} item ${index}` };
    await api('POST', `/models/collections/${name}/item`, body, token);
  }
}

/**
 * A collection whose field is a composite holding an image array.
 *
 * Order matters: the definition has to exist before a schema may reference it, and the
 * collection referencing it has to be gone before the definition can be replaced.
 */
async function seedComposite(token) {
  await deleteIfPresent(`/models/collections/${COMPOSITE_COLLECTION}`, token);
  await deleteIfPresent(`/models/composite_fields/${COMPOSITE_ID}`, token);
  await api('POST', `/models/composite_fields/${COMPOSITE_ID}`, [IMAGE_ARRAY_FIELD], token);
  await seedCollection(COMPOSITE_COLLECTION, 1, token, COMPOSITE_SCHEMA, () => ({
    block: { images: [] },
  }));
}

async function seed() {
  const login = await api('POST', '/auth/login', { username: USERNAME, password: PASSWORD });
  // A composite the schema editor's array field can hold. Nothing references it yet, so it can
  // be replaced; `e2e_schema_editor` is deleted before it is used again.
  await deleteIfPresent(`/models/collections/${SCHEMA_COLLECTION}`, login.token);
  await deleteIfPresent(`/models/composite_fields/${SCHEMA_BLOCK}`, login.token);
  await api(
    'POST',
    `/models/composite_fields/${SCHEMA_BLOCK}`,
    [
      { name: 'line', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      // The block holds a list of blocks: the definition reaches itself through an array, which
      // is allowed because the editor draws elements from the value.
      {
        name: 'children',
        field_type: { Array: [{ CompositeField: { id: SCHEMA_BLOCK } }] },
        required: false,
        width: 12,
        height: 1,
      },
    ],
    login.token,
  );
  await seedComposite(login.token);
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
/**
 * Console errors this run provokes on purpose.
 *
 * A screen that has to show a refusal makes a request that fails, and the browser logs it. The
 * check at the end is about the *unexpected* ones, so the expected ones are claimed here.
 */
const expectedConsoleErrors = [];
function expectConsoleError(pattern) {
  expectedConsoleErrors.push(pattern);
}
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

/** Sign in on a fresh context, so a second role can be looked at beside the admin one. */
async function openAs(username, password) {
  const roleContext = await newContext();
  const rolePage = await roleContext.newPage();
  rolePage.on('pageerror', (error) => consoleErrors.push(`${username}: ${error}`));
  rolePage.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(`${username}: ${message.text()}`);
  });

  await rolePage.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
  await rolePage.fill('input[name=username]', username);
  await rolePage.fill('input[name=password]', password);
  await rolePage.click('button:has-text("Sign in")');
  await rolePage
    .waitForFunction(() => !location.pathname.startsWith('/login'), null, { timeout: 15000 })
    .catch(() => {});

  return { context: roleContext, page: rolePage };
}

/**
 * Open the Settings branch of the sidebar.
 *
 * The tree is collapsed by default, so its links are not in the DOM until it is expanded —
 * a count of zero would otherwise prove nothing.
 */
async function expandSettings(rolePage) {
  for (const label of ['toggle Settings', 'toggle Schemas']) {
    await rolePage
      .locator(`button[aria-label="${label}"]`)
      .click({ force: true })
      .catch(() => {});
    await rolePage.waitForTimeout(250);
  }
}

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
  // -------------------------------------------------------------- the language switch
  // The harness pins the interface to English; this is the one check that leaves it, and puts it
  // back, so every other check reads the language it expects.
  await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
  // The catalog arrives through an observable, so the title changes a moment after the click:
  // wait for the text rather than reading it once.
  const titleSays = (expected) =>
    page
      .waitForFunction(
        (text) => document.querySelector('mat-card-title')?.textContent?.includes(text) ?? false,
        expected,
        { timeout: 10000 },
      )
      .then(() => true)
      .catch(() => false);

  await page.locator('app-language-switcher button', { hasText: '日本語' }).click();
  check('日本語に切り替えられる', await titleSays('サインイン'));
  await page.locator('app-language-switcher button', { hasText: 'English' }).click();
  check('英語に戻せる', await titleSays('Sign in to'));

  // -------------------------------------------------------------- sign in through the form
  await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
  await page.fill('input[name=username]', USERNAME);
  await page.fill('input[name=password]', PASSWORD);
  await page.click('button:has-text("Sign in")');
  await page
    .waitForFunction(() => !location.pathname.startsWith('/login'), null, { timeout: 15000 })
    .catch(() => {});
  check('ログインフォームからサインインできる', !page.url().includes('/login'), page.url());

  // ------------------------------------------------- a plain array, edited as JSON
  // An array's item types are decided in the schema editor and were invisible in the content
  // editor, which left the JSON box looking like it accepted anything.
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  const scores = page.locator('app-value-field textarea[name=scores]');
  await scores.waitFor({ timeout: 15000 });
  check(
    '配列の要素型が画面に出る',
    ((await scores.locator('xpath=ancestor::mat-form-field').textContent()) ?? '').includes(
      'Item types: Number',
    ),
  );

  await scores.fill('[1, "two"]');
  await page.click('button:has-text("Save")');
  await page.waitForTimeout(600);
  const stillEditing = page.url().includes('/edit/1');
  const scoresProblem = await page.locator('.field-cell.problem').count();
  const scoresError = ((await page.locator('.error').first().textContent()) ?? '').trim();
  check(
    '要素型に合わない値は保存前に止まる',
    stillEditing && scoresProblem === 1 && scoresError.includes('scores[1]'),
    `${stillEditing} / ${scoresProblem} / ${scoresError}`,
  );

  // Put it back so the rest of the run sees a valid item.
  await scores.fill('[1, 2]');
  await page.click('button:has-text("Save")');
  await page
    .waitForURL(`${BASE}/collections/${COLLECTION}`, { timeout: 15000 })
    .catch(() => {});
  const savedScores = await api('GET', `/models/collections/${COLLECTION}/items/1`, undefined, token);
  check('配列の値がそのまま保存される', JSON.stringify(savedScores?.scores) === '[1,2]', JSON.stringify(savedScores?.scores));


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
  // Waiting for 25 rows would prove nothing (page one has 25 too), so wait for the content that
  // tells the pages apart. Without this the check raced the reload and sometimes read page one.
  await page
    .waitForFunction(
      () => document.querySelector('table.items tbody tr td')?.textContent?.trim() === '26',
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
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

  // The audit trail: the row now names the account that published it.
  const publisherNote = (await rowById(draftId).locator('.publisher').textContent())?.trim();
  check('誰が公開したかが一覧に出る', publisherNote === USERNAME, `${publisherNote}`);

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
  await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
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

  // The name is a label: renaming it leaves the id, the URL and the bytes alone, so content that
  // references the image is unaffected.
  const renameLabel = `renamed-${Date.now()}.png`;
  await uploadedCard.locator('button[aria-label^="rename image"]').click();
  await uploadedCard.locator('input[name=imageName]').fill(renameLabel);
  await uploadedCard.locator('button[aria-label="Save"]').click();
  await page
    .waitForFunction(
      (name) => document.querySelector('.library .image .name')?.textContent === name,
      renameLabel,
      { timeout: 10000 },
    )
    .catch(() => {});
  const renamedCard = page.locator('.library .image').first();
  const renamedSource = await renamedCard.locator('img').getAttribute('src');
  check(
    '画像の名前を変更できる(URL は変わらない)',
    (await renamedCard.locator('.name').textContent()) === renameLabel &&
      renamedSource === imageSource,
    `${await renamedCard.locator('.name').textContent()} / ${renamedSource}`,
  );

  // It is the stored name that changed, not just what is on screen.
  await page.reload({ waitUntil: 'networkidle' });
  await page.locator('.library .image').first().waitFor({ timeout: 15000 });
  check(
    '変更した名前が残る',
    (await page.locator('.library .image').first().locator('.name').textContent()) === renameLabel,
  );

  // Replacing the bytes: same image (id and name), new picture. The id keeps working, which is
  // what makes it the right thing to write into a Markdown body.
  const cardBeforeReplace = page.locator('.library .image').first();
  const idBeforeReplace = (await cardBeforeReplace.locator('.meta').textContent())?.trim();
  const urlBeforeReplace = await cardBeforeReplace.locator('img').getAttribute('src');
  const replacedImageId = Number(idBeforeReplace?.match(/id (\d+)/)?.[1]);
  const durableLink = `${BASE}/api/images/by-id/${replacedImageId}`;
  const linkBefore = await page.evaluate(async (src) => {
    const response = await fetch(src, { redirect: 'follow' });
    return { status: response.status, body: await response.text() };
  }, durableLink);
  check(
    'id のリンクで実体を取れる',
    linkBefore.status === 200 && linkBefore.body.length > 0,
    `status=${linkBefore.status}`,
  );

  await cardBeforeReplace
    .locator('button[aria-label^="replace image"]')
    .click()
    .catch(() => {});
  // The hidden input inside the card takes the file, as the button's click would open it.
  await cardBeforeReplace.locator('input[type=file]').setInputFiles({
    name: 'replacement.png',
    mimeType: 'image/png',
    buffer: Buffer.from(PNG_BASE64, 'base64'),
  });
  await page
    .waitForFunction(
      (previous) => {
        const source = document.querySelector('.library .image img')?.getAttribute('src');
        return Boolean(source) && source !== previous;
      },
      urlBeforeReplace,
      { timeout: 15000 },
    )
    .catch(() => {});
  const cardAfterReplace = page.locator('.library .image').first();
  const urlAfterReplace = await cardAfterReplace.locator('img').getAttribute('src');
  check(
    '画像を差し替えても id と名前は変わらない',
    (await cardAfterReplace.locator('.meta').textContent())?.trim() === idBeforeReplace &&
      (await cardAfterReplace.locator('.name').textContent()) === renameLabel &&
      urlAfterReplace !== urlBeforeReplace,
    `${urlBeforeReplace} → ${urlAfterReplace}`,
  );
  check(
    '差し替え後の id リンクは新しい実体を指す',
    (await page.evaluate(async (src) => (await fetch(src, { redirect: 'follow' })).status, durableLink)) ===
      200,
  );

  // The record of what happened arrives where the link is handed out.
  await cardAfterReplace.locator('button[aria-label^="copy the link of image"]').click();
  await page.waitForTimeout(400);
  const linkNotice = ((await page.locator('.notice').first().textContent()) ?? '').trim();
  check('リンクをコピーできる', linkNotice.includes(`/api/images/by-id/`), linkNotice);

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

  // ...and takes an upload directly, without a detour through the library.
  await page.setInputFiles('.array-actions input[type=file]', {
    name: 'e2e-array.png',
    mimeType: 'image/png',
    buffer: Buffer.from(PNG_BASE64, 'base64'),
  });
  await page
    .waitForFunction(() => document.querySelectorAll('.array-item').length === 3, null, {
      timeout: 15000,
    })
    .catch(() => {});
  const arrayAfterUpload = await page.locator('.array-item').count();
  check('画像配列にその場でアップロードできる', arrayAfterUpload === 3, `${arrayItems} → ${arrayAfterUpload}`);

  await page.click('button:has-text("Save")');
  await page.waitForURL(`${BASE}/collections/${IMAGE_COLLECTION}`, { timeout: 15000 }).catch(() => {});

  const saved = await api(
    'GET',
    `/models/collections/${IMAGE_COLLECTION}/items/1`,
    undefined,
    token,
  );
  const libraryNow = await api('GET', '/models/images', undefined, token);
  const expectedPhotoUrl = libraryNow.find((image) => image.id === replacedImageId)?.url;
  check(
    '選んだ画像がアイテムに保存される',
    typeof saved?.photo === 'object' && saved.photo !== null && saved.photo.url === expectedPhotoUrl,
    `${JSON.stringify(saved?.photo)} (expected ${expectedPhotoUrl})`,
  );
  check(
    '画像配列がまとめて保存される',
    Array.isArray(saved?.gallery) &&
      saved.gallery.length === 3 &&
      saved.gallery.every((image) => image?.url?.startsWith('/images/')),
    JSON.stringify(saved?.gallery),
  );

  // -------------------------------------------------------------- saving is not publishing
  // The delivery API only ever sees the published copy.
  const draftsOnly = await api('GET', `/content/collections/${IMAGE_COLLECTION}`, undefined, token);
  check(
    '保存しただけでは公開されない',
    draftsOnly.items.length === 0,
    `${draftsOnly.items.length} 件が公開中`,
  );

  await page.goto(`${BASE}/collections/${IMAGE_COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });
  await firstRow().locator('button[aria-label^="publish item"]').click();
  await page
    .waitForFunction(
      () => {
        const badge = document.querySelector('table.items tbody tr app-item-status .badge');
        return badge && badge.textContent.trim() === 'Published';
      },
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  const publishedItems = await api('GET', `/content/collections/${IMAGE_COLLECTION}`, undefined, token);
  check(
    '公開すると配信 API に反映される',
    publishedItems.items.length === 1,
    `${publishedItems.items.length} 件`,
  );

  // -------------------------------------------------------------- an image array inside a composite
  await page.goto(`${BASE}/collections/${COMPOSITE_COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  const composite = page.locator('fieldset.composite');
  await composite.locator('button.array-add').click();
  await thumbs.first().waitFor({ timeout: 15000 });
  await thumbs.nth(0).click();
  await thumbs.nth(1).click();
  await page.locator('button.array-add-selected').click();
  await page
    .waitForFunction(
      () => document.querySelectorAll('fieldset.composite .array-item').length === 2,
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  const compositeItems = await composite.locator('.array-item').count();
  check('複合フィールド内の画像配列にも追加できる', compositeItems === 2, `${compositeItems} 件`);

  await page.click('button:has-text("Save")');
  await page.waitForURL(`${BASE}/collections/${COMPOSITE_COLLECTION}`, { timeout: 15000 }).catch(() => {});
  const compositeSaved = await api(
    'GET',
    `/models/collections/${COMPOSITE_COLLECTION}/items/1`,
    undefined,
    token,
  );
  // A composite is read back wrapped as `{id, values}`.
  const compositeImages = compositeSaved?.block?.values?.images ?? compositeSaved?.block?.images;
  check(
    '複合フィールド内の画像配列が保存される',
    Array.isArray(compositeImages) && compositeImages.length === 2,
    JSON.stringify(compositeImages),
  );

  // -------------------------------------------------------------- delete them again
  // Everything uploaded during this run goes, whether it came from the library screen or
  // straight into an array, so the next run starts from the same place.
  await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
  for (let guard = 0; guard < 10; guard += 1) {
    const count = await page.locator('.library .image').count();
    if (count <= imagesBefore) {
      break;
    }
    await page.locator('.library .image').first().locator('.remove').click();
    await page
      .waitForFunction(
        (expected) => document.querySelectorAll('.library .image').length === expected,
        count - 1,
        { timeout: 15000 },
      )
      .catch(() => {});
  }
  const imagesAfterDelete = await page.locator('.library .image').count();
  check('画像を削除できる', imagesAfterDelete === imagesBefore, `${imagesBefore} に戻る (現在 ${imagesAfterDelete})`);

  // ------------------------------------------------- a link shows unpublished work to a guest
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  const titleField = page.locator('app-value-field input').first();
  await titleField.waitFor({ timeout: 15000 });
  const previewWording = `preview wording ${Date.now()}`;
  await titleField.fill(previewWording);
  await page.click('button:has-text("Save")');
  await page.waitForURL(`${BASE}/collections/${COLLECTION}`, { timeout: 15000 }).catch(() => {});

  // Saving was not publishing: the delivery API still serves the older wording...
  const publishedCopy = await api('GET', `/content/collections/${COLLECTION}/items/1`);
  check(
    '公開コピーは保存では変わらない',
    publishedCopy.values.title !== previewWording,
    `${publishedCopy.values.title}`,
  );

  // ...while the preview link shows the working copy.
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('button:has-text("Preview link")').click();
  const previewAnchor = page.locator('.preview-link a');
  await previewAnchor.waitFor({ timeout: 10000 });
  const previewUrl = await previewAnchor.getAttribute('href');
  check(
    'プレビュー URL が発行される',
    /\/api\/preview\/collections\/e2e_blog\/items\/1\?token=/.test(previewUrl ?? ''),
    String(previewUrl).slice(0, 70),
  );

  const previewResponse = await request.fetch(previewUrl);
  check('プレビュー URL はトークン無しで開ける', previewResponse.status() === 200, `status=${previewResponse.status()}`);
  const previewBody = await previewResponse.json();
  check(
    'プレビューは作業コピーを見せる',
    previewBody?.values?.title === previewWording,
    JSON.stringify(previewBody?.values?.title),
  );

  // The link is signed for one item: pointing it at another one is refused.
  const tamperedResponse = await request.fetch(previewUrl.replace('/items/1?', '/items/2?'));
  check(
    'リンクの宛先は書き換えられない',
    tamperedResponse.status() === 401,
    `status=${tamperedResponse.status()}`,
  );

  // The item is published with changes waiting, so both acts are offered: release them, or take
  // the item down. Before this the screen only offered "Unpublish", which took the item off the
  // site on the way to releasing them.
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  const toolbar = page.locator('.toolbar');
  check(
    '公開済みで変更があると「変更を公開」が出る',
    (await toolbar.locator('button:has-text("Publish changes")').count()) === 1 &&
      (await toolbar.locator('button:has-text("Unpublish")').count()) === 1,
  );

  // The list offers the same release beside the row's state.
  await page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });
  const releaseInList = page.locator(
    `button[aria-label="publish the changes of item ${'1'}"]`,
  );
  check('一覧にも「変更を公開」が出る', (await releaseInList.count()) === 1);

  const beforeRelease = await api('GET', `/content/collections/${COLLECTION}/items/1`);
  await releaseInList.click();
  await page
    .waitForFunction(
      () => !document.querySelector(`button[aria-label="publish the changes of item 1"]`),
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  const releasedCopy = await api('GET', `/content/collections/${COLLECTION}/items/1`);
  check(
    '一覧から変更を公開できる',
    releasedCopy.values.title === previewWording,
    `${releasedCopy.values.title}`,
  );
  // 公開日は初回のまま。公開物が変わった時刻だけが進む(差分ビルドが見るのはこちら)。
  check(
    '再公開しても公開日は初回のまま',
    releasedCopy.published_at === beforeRelease.published_at,
    `${beforeRelease.published_at} → ${releasedCopy.published_at}`,
  );
  check(
    '公開物が変わった時刻は進む',
    Date.parse(releasedCopy.last_published_at) > Date.parse(beforeRelease.last_published_at),
    `${beforeRelease.last_published_at} → ${releasedCopy.last_published_at}`,
  );
  check(
    '公開したまま変更が反映される（配信は止まらない）',
    releasedCopy.id === 1 && (await api('GET', `/models/collections/${COLLECTION}/items/1/metadata`, undefined, token)).has_draft === false,
  );


  // -------------------------------------------------------------- roles decide what is offered
  // Two extra accounts, recreated each run, so the screens can be looked at as each role.
  const roleAccounts = [
    {
      username: 'e2e-editor@example.com',
      permission: { can_view: true, can_edit: true, can_publish: false },
    },
    {
      username: 'e2e-viewer@example.com',
      permission: { can_view: true, can_edit: false, can_publish: false },
    },
  ];
  const existing = await api('GET', '/auth/users', undefined, token);
  for (const account of roleAccounts) {
    const already = existing.find((user) => user.username === account.username);
    if (already) {
      await api('DELETE', `/auth/users/${already.id}`, undefined, token);
    }
    await api(
      'POST',
      '/auth/users',
      { ...account, password: 'role-password', is_admin: false },
      token,
    );
  }

  await expandSettings(page);
  check(
    '管理者にはアカウント管理が見える',
    (await page.locator('a[href="/settings/users"]').count()) === 1,
  );

  const editor = await openAs('e2e-editor@example.com', 'role-password');
  await editor.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await editor.page.locator('table.items tbody tr:has(app-item-status)').first().waitFor({ timeout: 15000 });
  check(
    '編集ロール: 新規作成はできる',
    (await editor.page.locator('button:has-text("New item")').count()) === 1,
  );
  check(
    '編集ロール: 公開も削除も出ない',
    (await editor.page.locator('button[aria-label^="publish item"]').count()) === 0 &&
      (await editor.page.locator('button[aria-label^="delete item"]').count()) === 0,
  );
  await expandSettings(editor.page);
  // The image library lives with the documents, so that branch has to be open to see its link.
  await editor.page
    .locator('button[aria-label="toggle Documents"]')
    .click({ force: true })
    .catch(() => {});
  await editor.page.waitForTimeout(200);
  check(
    '編集ロール: 画像は見えるがアカウント管理は出ない',
    (await editor.page.locator('a[href="/images"]').count()) === 1 &&
      (await editor.page.locator('a[href="/settings/users"]').count()) === 0,
  );

  await editor.page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await editor.page.locator('app-value-field').first().waitFor({ timeout: 15000 });
  check(
    '編集ロール: 保存はできるが公開はできない',
    (await editor.page.locator('button:has-text("Save")').count()) === 1 &&
      (await editor.page.locator('button:has-text("Publish")').count()) === 0,
  );

  const viewer = await openAs('e2e-viewer@example.com', 'role-password');
  await viewer.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await viewer.page.locator('table.items tbody tr:has(app-item-status)').first().waitFor({ timeout: 15000 });
  check(
    '閲覧ロール: 新規作成も出ない',
    (await viewer.page.locator('button:has-text("New item")').count()) === 0,
  );
  await viewer.page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await viewer.page.locator('app-value-field').first().waitFor({ timeout: 15000 });
  check(
    '閲覧ロール: 保存も公開も出ない',
    (await viewer.page.locator('button:has-text("Save")').count()) === 0 &&
      (await viewer.page.locator('button:has-text("Publish")').count()) === 0,
  );
  check(
    '閲覧ロール: 閲覧のみと案内される',
    (await viewer.page.locator('.note', { hasText: 'may not edit' }).count()) === 1,
  );
  check(
    '閲覧ロール: 自分のパスワードは変更できる',
    (await viewer.page.locator('a[href="/account"], button[aria-label="Change password"]').count()) === 1,
  );

  // ------------------------------------------- a password change ends the old sessions
  await viewer.page.goto(`${BASE}/account`, { waitUntil: 'networkidle' });
  const stolenToken = await viewer.page.evaluate(() => localStorage.getItem('sl_cms.token'));
  await viewer.page.fill('input[name=current]', 'role-password');
  await viewer.page.fill('input[name=next]', 'role-password-2');
  await viewer.page.fill('input[name=repeated]', 'role-password-2');
  await viewer.page.click('button:has-text("Change password")');
  await viewer.page.locator('.status').waitFor({ timeout: 10000 }).catch(() => {});
  check(
    'パスワード変更が完了と表示される',
    (await viewer.page.locator('.status').count()) === 1,
  );

  // The token from before the change is refused by the API. The call is made from here
  // rather than from the page: a refused request is a console error in the browser, and the
  // check at the end rightly treats those as failures.
  const oldTokenStatus = (
    await request.fetch(`${API}/auth/me`, { headers: { Authorization: `Bearer ${stolenToken}` } })
  ).status();
  check('変更前のトークンは失効する', oldTokenStatus === 401, `status=${oldTokenStatus}`);

  // ...while the session that changed it carries on, because the server handed back a
  // token for the new generation and the screen adopted it.
  await viewer.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await viewer.page.locator('table.items tbody tr:has(app-item-status)').first().waitFor({ timeout: 15000 });
  check('変更後も自分のセッションは続く', viewer.page.url().includes('/collections/'), viewer.page.url());

  // And the new password is the one that works from a fresh browser.
  const afterChange = await openAs('e2e-viewer@example.com', 'role-password-2');
  await afterChange.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await afterChange.page.locator('table.items tbody tr:has(app-item-status)').first().waitFor({ timeout: 15000 });
  check('新しいパスワードでサインインできる', afterChange.page.url().includes('/collections/'));
  await afterChange.context.close();

  await editor.context.close();
  await viewer.context.close();

  // ------------------------------------ per-resource permissions, granted from the screen
  const scopedEmail = 'e2e-scoped@example.com';
  for (const account of await api('GET', '/auth/users', undefined, token)) {
    if (account.username === scopedEmail) {
      await api('DELETE', `/auth/users/${account.id}`, undefined, token);
    }
  }
  const scoped = await api(
    'POST',
    '/auth/users',
    {
      username: scopedEmail,
      password: 'scoped-password',
      is_admin: false,
      permission: { can_view: true, can_edit: false, can_publish: false },
    },
    token,
  );
  await api(
    'PATCH',
    `/auth/users/${scoped.id}`,
    {
      collection_permissions: {
        [COLLECTION]: { can_view: true, can_edit: true, can_publish: false },
        [LAST_PAGE_COLLECTION]: { can_view: false, can_edit: false, can_publish: false },
      },
    },
    token,
  );

  // The account screen edits them: open the panel and raise the grant to publisher.
  await page.goto(`${BASE}/settings/users`, { waitUntil: 'networkidle' });
  await page.locator(`button[aria-label="resource permissions of ${scopedEmail}"]`).click();
  // The selects are found by the resource they belong to: Material applies `aria-label` to its
  // own inner element, so the host attribute is not something to rely on.
  const grant = page
    .locator('.resource', { hasText: `collection: ${COLLECTION}` })
    .locator('mat-select');
  await grant.waitFor({ timeout: 10000 });
  check('アカウント画面にリソース権限の一覧が出る', (await page.locator('.resource').count()) >= 2);

  await grant.click();
  await page.locator('mat-option', { hasText: 'Publisher' }).click();
  await page.click('button:has-text("Save permissions")');
  await page.locator('.status').waitFor({ timeout: 10000 }).catch(() => {});
  const scopedAfterSave = (await api('GET', '/auth/users', undefined, token)).find(
    (account) => account.username === scopedEmail,
  );
  check(
    '保存した権限がサーバに届く',
    scopedAfterSave.collection_permissions[COLLECTION].can_publish === true,
    JSON.stringify(scopedAfterSave.collection_permissions),
  );

  // What that account is offered follows the grant: the collection it was given, and not the
  // one it was denied.
  const scopedLogin = await request.fetch(`${API}/auth/login`, {
    method: 'POST',
    data: { username: scopedEmail, password: 'scoped-password' },
  });
  const scopedToken = (await scopedLogin.json()).token;
  const offered = await api('GET', '/models/collections', undefined, scopedToken);
  check(
    '許可したコレクションだけが一覧に出る',
    offered.includes(COLLECTION) && !offered.includes(LAST_PAGE_COLLECTION),
    offered.join(','),
  );

  const scopedSession = await openAs(scopedEmail, 'scoped-password');
  await scopedSession.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await scopedSession.page
    .locator('table.items tbody tr')
    .first()
    .waitFor({ timeout: 15000 });
  check(
    'grant したコレクションは編集できる',
    (await scopedSession.page.locator('button:has-text("New item")').count()) === 1,
  );
  // The denied collection is refused by the server as well. Checked over the API rather than by
  // opening it: a 403 in the browser is a console error, and the check at the end rightly
  // treats those as failures.
  const denied = await request.fetch(`${API}/models/collections/${LAST_PAGE_COLLECTION}/items/1`, {
    headers: { Authorization: `Bearer ${scopedToken}` },
  });
  check('拒否したコレクションは API でも 403', denied.status() === 403, `status=${denied.status()}`);
  await scopedSession.context.close();

  // ------------------------------------------- the schema editor, driven from the screen
  // Every collection above is created through the API, so this screen - where an author
  // builds the content model - would otherwise never be exercised in a browser.
  await deleteIfPresent(`/models/collections/${SCHEMA_COLLECTION}`, token);
  await page.goto(`${BASE}/settings/schemas/collections/create`, { waitUntil: 'networkidle' });
  await page.fill('input[name=name]', SCHEMA_COLLECTION);
  await page.click('button:has-text("Create")');
  await page
    .waitForURL(`**/settings/schemas/collections/edit/${SCHEMA_COLLECTION}`, { timeout: 15000 })
    .catch(() => {});

  // A text field, which starts full width.
  await page.click('button:has-text("Add field")');
  const textField = page.locator('.schema-field').first();
  await textField.locator('input[name=fieldName]').fill('title');
  check(
    'フィールド編集のラベルが訳される',
    ((await textField.locator('mat-label').first().textContent()) ?? '').includes('Field Name'),
  );

  // The title has to be unique: the value is compared across the collection when saved.
  await textField.locator('input[name="fieldUnique"]').check({ force: true });
  check(
    '一意チェックが画面にある',
    await textField.locator('input[name="fieldUnique"]').isChecked(),
  );

  // A second field of another type: an enum, whose values are chips.
  await page.click('button:has-text("Add field")');
  const enumField = page.locator('.schema-field').nth(1);
  await enumField.locator('input[name=fieldName]').fill('state');
  await enumField.locator('mat-select[name=fieldType]').click();
  await page.locator('mat-option', { hasText: 'TextEnum' }).click();

  const chipInput = enumField.locator('mat-chip-grid input');
  await chipInput.fill('draft');
  await chipInput.press('Enter');
  await chipInput.fill('published');
  await chipInput.press('Enter');

  // The chip list is rendered from the model, so the chips appear a moment after the input
  // event: wait for the count rather than reading it once.
  const enumHasChips = (expected) =>
    page
      .waitForFunction(
        (count) => {
          const tiles = document.querySelectorAll('.field-grid .schema-field');
          return tiles.length >= 2 && tiles[1].querySelectorAll('mat-chip-row').length === count;
        },
        expected,
        { timeout: 10000 },
      )
      .then(() => true)
      .catch(() => false);

  check('Enum の値を画面から追加できる', await enumHasChips(2));

  // The chip's own label carries the value; a message that lost its placeholder would read
  // "remove {{value}}".
  const removeLabel =
    (await enumField
      .locator('mat-chip-row')
      .first()
      .locator('button[matChipRemove]')
      .getAttribute('aria-label')) ?? '';
  check(
    'Enum の値を消すボタンに値が入る',
    removeLabel.includes('draft') && !removeLabel.includes('{{'),
    removeLabel,
  );

  await enumField.locator('mat-chip-row').first().locator('button[matChipRemove]').click();
  check('Enum の値を画面から削除できる', await enumHasChips(1));

  // A third field: an array whose items are composites. The definitions are chosen in their
  // own control; the declared order is what the elements are read back against.
  await page.click('button:has-text("Add field")');
  const arrayField = page.locator('.schema-field').nth(2);
  await arrayField.locator('input[name=fieldName]').fill('blocks');
  await arrayField.locator('mat-select[name=fieldType]').click();
  await page.locator('mat-option', { hasText: 'Array' }).click();
  await arrayField.locator('mat-select[name=arrayCompositeTypes]').click();
  await page.locator('mat-option', { hasText: SCHEMA_BLOCK }).click();
  await page.keyboard.press('Escape');
  check(
    '複合を配列の要素型として選べる',
    (await arrayField.locator('mat-select[name=arrayCompositeTypes]').textContent())?.includes(
      SCHEMA_BLOCK,
    ) ?? false,
  );

  // A fourth field carrying the schema's own text limits, which the content editor has to
  // take as seriously as the schema editor does.
  await page.click('button:has-text("Add field")');
  const summaryField = page.locator('.schema-field').nth(3);
  await summaryField.locator('input[name=fieldName]').fill('summary');
  await summaryField.locator('input[name=maxLength]').fill('8');
  await summaryField.locator('input[name=minLength]').fill('5');

  // Half of the 12-column grid, from the presets rather than the number input.
  await textField.locator('.width-presets button', { hasText: '1/2' }).click();

  await page.click('button:has-text("Save schema")');
  await page.locator('.status').waitFor({ timeout: 10000 }).catch(() => {});
  const builtSchema = await api('GET', `/models/collections/${SCHEMA_COLLECTION}/schema`, undefined, token);
  const builtText = builtSchema.find((field) => field.name === 'title');
  const builtEnum = builtSchema.find((field) => field.name === 'state');
  const builtArray = builtSchema.find((field) => field.name === 'blocks');
  check(
    '画面で組んだスキーマが保存される',
    builtText?.width === 6 &&
      builtText?.unique === true &&
      builtText?.field_type?.Text !== undefined &&
      builtEnum?.field_type?.TextEnum?.join() === 'published' &&
      builtArray?.field_type?.Array?.[0]?.CompositeField?.id === SCHEMA_BLOCK,
    JSON.stringify(builtSchema),
  );

  await page.goto(`${BASE}/settings/schemas/collections`, { waitUntil: 'networkidle' });
  check(
    'スキーマ一覧に新しいコレクションが出る',
    (await page.locator('table td', { hasText: SCHEMA_COLLECTION }).count()) === 1,
  );

  // The fields the author defined are usable straight away: content, not just a definition.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}/create`, { waitUntil: 'networkidle' });
  const titleInput = page.locator('app-value-field input').first();
  await titleInput.waitFor({ timeout: 15000 });
  await titleInput.fill('first item');
  await page.locator('app-value-field input[name=summary]').fill('summary1');
  await page.locator('app-value-field mat-select').click();
  await page.locator('mat-option', { hasText: 'published' }).click();
  // A multiple select keeps its panel open; the Save button is behind it until it closes.
  await page.keyboard.press('Escape');

  // The composite array is edited element by element, each by the definition it holds.
  await page.click('button:has-text("Add element")');
  await page.click('button:has-text("Add element")');
  const elements = page.locator('.composite-element');
  await elements.nth(0).locator('input').first().fill('first block');
  await elements.nth(1).locator('input').first().fill('second block');
  check('複合配列の要素が 2 つ出る', (await elements.count()) === 2, `${await elements.count()} 件`);
  const typedFirst = await elements.nth(0).locator('input').first().inputValue();
  const typedSecond = await elements.nth(1).locator('input').first().inputValue();
  check(
    '複合配列の要素に入力できる',
    typedFirst === 'first block' && typedSecond === 'second block',
    `${typedFirst} / ${typedSecond}`,
  );

  // Order is what the array means, so it can be changed from the element header.
  await elements.nth(1).locator('button[aria-label="move element 1 earlier"]').click();
  // The view follows a moment after the click (no zone to flush it synchronously), so wait for
  // the value to move rather than reading it once.
  const reordered = await page
    .waitForFunction(
      () => document.querySelectorAll('.composite-element input')[0]?.value === 'second block',
      null,
      { timeout: 10000 },
    )
    .then(() => true)
    .catch(() => false);
  check('複合配列の要素を並べ替えられる', reordered);

  await page.click('button:has-text("Save")');
  await page
    .waitForURL(`${BASE}/collections/${SCHEMA_COLLECTION}`, { timeout: 15000 })
    .catch(() => {});
  const builtItem = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/items/1`,
    undefined,
    token,
  );
  check(
    'Enum フィールドをコンテンツで選べる',
    JSON.stringify(builtItem?.state) === '["published"]',
    JSON.stringify(builtItem?.state),
  );
  // The definition's own fields come back filled in with their defaults, so compare what this
  // scenario set: which definition each element is, and what was typed into it.
  const savedElements = (builtItem?.blocks ?? []).map(
    (element) => `${element.id}:${element.values?.line}`,
  );
  check(
    '複合配列が要素ごとに保存される',
    JSON.stringify(savedElements) ===
      JSON.stringify([`${SCHEMA_BLOCK}:second block`, `${SCHEMA_BLOCK}:first block`]),
    JSON.stringify(savedElements),
  );

  // Opening it again shows what was stored: the round trip through the form, not just the API.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('.composite-element').first().waitFor({ timeout: 15000 });
  check(
    '保存した複合配列がフォームに出る',
    (await page.locator('.composite-element').nth(0).locator('input').first().inputValue()) ===
      'second block',
    await page.locator('.composite-element').nth(0).locator('input').first().inputValue(),
  );

  // A block that holds blocks. The element's own array is empty, so the form stops there; the
  // one element added inside it is one more level and nothing below that.
  const firstBlock = page.locator('.composite-element').first();
  await firstBlock.locator('button:has-text("Add element")').first().click();
  const nestedBlocks = firstBlock.locator('.composite-element');
  await nestedBlocks.first().locator('input').first().fill('nested block');
  check('複合の中に複合を追加できる', (await nestedBlocks.count()) === 1);
  check(
    '入れ子の配列は空のところで止まる',
    await nestedBlocks
      .first()
      .locator('.note')
      .first()
      .textContent()
      .then((text) => text?.includes('No elements yet') ?? false)
      .catch(() => false),
  );

  await page.click('button:has-text("Save")');
  await page
    .waitForURL(`${BASE}/collections/${SCHEMA_COLLECTION}`, { timeout: 15000 })
    .catch(() => {});
  const nestedItem = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/items/1`,
    undefined,
    token,
  );
  check(
    '入れ子の複合配列が保存される',
    nestedItem?.blocks?.[0]?.values?.children?.[0]?.values?.line === 'nested block',
    JSON.stringify(nestedItem?.blocks?.[0]?.values?.children),
  );

  // A value another item holds is refused, and the form says which field it was about.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}/create`, { waitUntil: 'networkidle' });
  const duplicateTitle = page.locator('app-value-field input').first();
  await duplicateTitle.waitFor({ timeout: 15000 });

  // The schema's limits are the content editor's business too, not just the server's: the input
  // carries them, the hint states them, and a value outside them stops the save before it is
  // sent (the minimum cannot be enforced by the browser, which is the case this covers).
  const summary = page.locator('app-value-field input[name=summary]');
  await summary.waitFor({ timeout: 15000 });
  // The unique field says so on the form: only the server can check it, but the reader is told
  // before they find out from a refusal.
  check(
    '一意なフィールドだと画面に出る',
    (await page.locator('app-value-field .unique-mark').count()) === 1,
    `${await page.locator('app-value-field .unique-mark').count()} 件`,
  );
  check('スキーマの文字数が入力欄に効く', (await summary.getAttribute('maxlength')) === '8', 'maxlength');
  check(
    'スキーマの文字数がヒントに出る',
    ((await summary.locator('xpath=ancestor::mat-form-field').textContent()) ?? '').includes(
      '5–8 characters',
    ),
  );

  await summary.fill('ab');
  await page.click('button:has-text("Save")');
  await page.waitForTimeout(500);
  const stillCreating = page.url().includes('/create');
  const summaryProblem = await page.locator('.field-cell.problem').count();
  check('短すぎる値は保存前に止まる', stillCreating && summaryProblem === 1, `${stillCreating} / ${summaryProblem}`);

  // A value inside the limits is accepted again, so only the duplicate is left to refuse.
  await summary.fill('summary1');
  await duplicateTitle.fill('first item');
  // The refusal is the point of this step, so its 409 is expected.
  expectConsoleError(/409 \(Conflict\)/);
  await page.click('button:has-text("Save")');
  // The length check above left its own message in the banner, so this waits for the banner to
  // say something new rather than for it to appear.
  await page
    .waitForFunction(
      () => document.querySelector('.error')?.textContent?.includes('title') ?? false,
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  const refusalText = ((await page.locator('.error').textContent()) ?? '').trim();
  check('一意な値の重複はフォームで止まる', refusalText.includes('title'), refusalText);
  check(
    '拒否されたフィールドが強調される',
    (await page.locator('.field-cell.problem').count()) === 1,
    `${await page.locator('.field-cell.problem').count()} 件`,
  );

  const afterRefusal = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/items/metadata`,
    undefined,
    token,
  );
  check(
    '重複したアイテムは作られない',
    Object.keys(afterRefusal).length === 1,
    `${Object.keys(afterRefusal).length} 件`,
  );

  // Through the API rather than the screen: this collection exists only for this scenario.
  await deleteIfPresent(`/models/collections/${SCHEMA_COLLECTION}`, token);

  // ------------------------------------------- single pages: switching follows the URL
  // Two pages, so the sidebar can be used to switch from one to the other. The router reuses the
  // component when only the parameter changes, which used to leave the first page on screen.
  const stamp = Date.now();
  const pageA = `e2e-page-${stamp}-a`;
  const pageB = `e2e-page-${stamp}-b`;
  for (const name of [pageA, pageB]) {
    await api(
      'POST',
      `/models/single_pages/${name}/schema`,
      [{ name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 }],
      token,
    );
    await api('PUT', `/models/single_pages/${name}/item`, { title: `content of ${name}` }, token);
  }

  await page.goto(`${BASE}/single-pages/${pageA}`, { waitUntil: 'networkidle' });
  const pageTitle = page.locator('app-value-field input[name=title]');
  await pageTitle.waitFor({ timeout: 15000 });
  check(
    '単一ページの内容が開く',
    (await pageTitle.inputValue()) === `content of ${pageA}`,
    await pageTitle.inputValue(),
  );

  for (const label of ['toggle Documents', 'toggle Single pages']) {
    await page
      .locator(`button[aria-label="${label}"]`)
      .click({ force: true })
      .catch(() => {});
    await page.waitForTimeout(200);
  }
  await page.locator(`app-sidebar a[href="/single-pages/${pageB}"]`).click({ force: true });
  // Wait for the *content* to change: the heading is set first, so waiting on it could pass while
  // the form still held the previous page's values - which is the bug this check is about.
  const switched = await page
    .waitForFunction(
      (expected) =>
        document.querySelector('app-value-field input[name=title]')?.value === expected,
      `content of ${pageB}`,
      { timeout: 15000 },
    )
    .then(() => true)
    .catch(() => false);
  check(
    '単一ページを切り替えると画面も切り替わる',
    switched &&
      ((await page.locator('h2').first().textContent()) ?? '').includes(pageB) &&
      (await pageTitle.inputValue()) === `content of ${pageB}`,
    `url=${page.url()} / ${await pageTitle.inputValue()}`,
  );

  // Saving stays on the page, with the status appearing and the two acts in one click. It used to
  // navigate to the schema list, which has no publish control at all.
  await pageTitle.fill(`edited ${pageB}`);
  await page.click('button:has-text("Save")');
  await page.waitForTimeout(800);
  const stayedOnPage = page.url().includes(`/single-pages/${pageB}`);
  const savedNotice = ((await page.locator('.notice').first().textContent()) ?? '').trim();
  const badgeAppeared = await page.locator('app-item-status .badge').count();
  check(
    '単一ページの保存後も画面に留まる',
    stayedOnPage && savedNotice.length > 0 && badgeAppeared === 1,
    `${stayedOnPage} / ${savedNotice} / ${badgeAppeared}`,
  );

  await page.click('button:has-text("Save and publish")');
  const published = await page
    .waitForFunction(
      () => document.querySelector('app-item-status .badge')?.textContent?.includes('Published') ?? false,
      null,
      { timeout: 10000 },
    )
    .then(() => true)
    .catch(() => false);
  check('保存して公開が 1 クリックでできる', published, await page.locator('app-item-status .badge').first().textContent());

  const servedPage = await request.fetch(`${API}/content/single-pages/${pageB}`).catch(() => null);
  const servedBody = servedPage?.ok() ? JSON.stringify(await servedPage.json()) : '';
  check(
    '保存して公開した内容が配信 API に出る',
    servedPage?.ok() === true && servedBody.includes(`edited ${pageB}`),
    servedBody.slice(0, 120),
  );

  // The overview: every page, what state it is in, and publishing from the list.
  await page.goto(`${BASE}/single-pages`, { waitUntil: 'networkidle' });
  const pageRows = page.locator('table.items tbody tr');
  await pageRows.first().waitFor({ timeout: 15000 });
  const rowForPage = (name) => page.locator('table.items tbody tr', { hasText: name });
  check(
    '単一ページの一覧に状態が出る',
    (await pageRows.count()) >= 2 &&
      (await rowForPage(pageB).textContent())?.includes('Published') === true,
    `${await pageRows.count()} 行 / ${(await rowForPage(pageB).textContent())?.trim()}`,
  );

  // A page that has never been published is a draft, and can be released from here.
  await api(
    'POST',
    `/models/single_pages/${pageA}/publish`,
    undefined,
    token,
  ).catch(() => {});
  await page.reload({ waitUntil: 'networkidle' });
  await rowForPage(pageA).locator('button[aria-label^="unpublish page"]').click();
  const unpublished = await page
    .waitForFunction(
      (name) =>
        Array.from(document.querySelectorAll('table.items tbody tr'))
          .find((row) => row.textContent?.includes(name))
          ?.textContent?.includes('Draft') ?? false,
      pageA,
      { timeout: 10000 },
    )
    .then(() => true)
    .catch(() => false);
  check('一覧から非公開にできる', unpublished);

  for (const name of [pageA, pageB]) {
    await deleteIfPresent(`/models/single_pages/${name}`, token);
  }

  // ------------------------------- an administrator hands out a password reset link
  const resetUsername = `e2e-reset-${Date.now()}`;
  for (const account of await api('GET', '/auth/users', undefined, token)) {
    if (account.username.startsWith('e2e-reset-')) {
      await api('DELETE', `/auth/users/${account.id}`, undefined, token);
    }
  }
  await api(
    'POST',
    '/auth/users',
    {
      username: resetUsername,
      password: 'reset-password',
      is_admin: false,
      permission: { can_view: true, can_edit: false, can_publish: false },
    },
    token,
  );

  // A session that exists before the reset, to watch it die.
  const openSession = await request.fetch(`${API}/auth/login`, {
    method: 'POST',
    data: { username: resetUsername, password: 'reset-password' },
  });
  const sessionBeforeReset = (await openSession.json()).token;

  // The administrator issues the link from the account screen...
  await page.goto(`${BASE}/settings/users`, { waitUntil: 'networkidle' });
  await page
    .locator(`button[aria-label="issue a password reset link for ${resetUsername}"]`)
    .click();
  const resetAnchor = page.locator('.reset-link a');
  await resetAnchor.waitFor({ timeout: 10000 });
  const resetUrl = await resetAnchor.getAttribute('href');
  check(
    'リセット URL が発行される',
    /\/reset-password\?token=/.test(resetUrl ?? ''),
    String(resetUrl).slice(0, 70),
  );

  // ...the owner opens it with no session at all and chooses a password.
  const resetContext = await newContext();
  const resetPage = await resetContext.newPage();
  resetPage.on('pageerror', (error) => consoleErrors.push(`reset: ${error}`));
  resetPage.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(`reset: ${message.text()}`);
  });
  await resetPage.goto(resetUrl, { waitUntil: 'networkidle' });
  await resetPage.fill('input[name=next]', 'chosen-by-the-owner');
  await resetPage.fill('input[name=repeated]', 'chosen-by-the-owner');
  await resetPage.click('button:has-text("Set password")');
  await resetPage
    .waitForFunction(() => !location.pathname.includes('/reset-password'), null, { timeout: 15000 })
    .catch(() => {});
  check('リセット後はサインイン状態になる', resetPage.url().startsWith(`${BASE}/`), resetPage.url());

  // The new password works, the session from before is gone, and the link is spent.
  const afterReset = await request.fetch(`${API}/auth/login`, {
    method: 'POST',
    data: { username: resetUsername, password: 'chosen-by-the-owner' },
  });
  check('新しいパスワードでサインインできる', afterReset.status() === 200, `status=${afterReset.status()}`);
  const stale = await request.fetch(`${API}/auth/me`, {
    headers: { Authorization: `Bearer ${sessionBeforeReset}` },
  });
  check('リセット前のセッションは切れる', stale.status() === 401, `status=${stale.status()}`);
  const reused = await request.fetch(`${API}/auth/password-reset`, {
    method: 'POST',
    data: { token: (resetUrl ?? '').split('token=')[1], new_password: 'someone-elses-choice' },
  });
  check('同じリンクは二度使えない', reused.status() === 403, `status=${reused.status()}`);
  await resetContext.close();

  // ------------------------------------------- guessing a password is not free
  // A throwaway account, so the lock this leaves behind touches nothing else. The API is
  // called from here rather than through the form: what matters is the status codes, and a
  // refused sign-in is a console error the check below would rightly report.
  // A fresh address per run: the lock lives in the server's memory and deleting the account
  // does not clear it, so a fixed address would make the second run against the same server
  // fail. Older ones from previous runs are cleaned up here.
  const throttleEmail = `e2e-throttle-${Date.now()}@example.com`;
  for (const account of await api('GET', '/auth/users', undefined, token)) {
    if (account.username.startsWith('e2e-throttle-')) {
      await api('DELETE', `/auth/users/${account.id}`, undefined, token);
    }
  }
  await api(
    'POST',
    '/auth/users',
    {
      username: throttleEmail,
      password: 'throttle-password',
      is_admin: false,
      permission: { can_view: true, can_edit: false, can_publish: false },
    },
    token,
  );

  const attempt = (password) =>
    request
      .fetch(`${API}/auth/login`, { method: 'POST', data: { username: throttleEmail, password } })
      .then((response) => response.status());

  const statuses = [];
  for (let index = 0; index < 6; index += 1) {
    statuses.push(await attempt('not-the-password'));
  }
  check(
    '繰り返しの失敗は 429 で止まる',
    statuses.slice(0, 5).every((status) => status === 401) && statuses[5] === 429,
    statuses.join(','),
  );

  // While the lock is in force, even the right password waits.
  const locked = await request.fetch(`${API}/auth/login`, {
    method: 'POST',
    data: { username: throttleEmail, password: 'throttle-password' },
  });
  check('ロック中は正しいパスワードでも 429', locked.status() === 429, `status=${locked.status()}`);
  const retryAfter = Number(locked.headers()['retry-after']);
  check(
    'Retry-After で再試行までの秒数が分かる',
    retryAfter > 0 && retryAfter <= 15 * 60,
    `retry-after=${retryAfter}`,
  );

  // Someone else's failures do not lock anybody else out.
  const otherAccount = await request.fetch(`${API}/auth/login`, {
    method: 'POST',
    data: { username: USERNAME, password: PASSWORD },
  });
  check('他のアカウントは影響を受けない', otherAccount.status() === 200, `status=${otherAccount.status()}`);

  const unexpectedErrors = consoleErrors.filter((text) => {
    const claimed = expectedConsoleErrors.findIndex((pattern) => pattern.test(text));
    if (claimed === -1) {
      return true;
    }
    expectedConsoleErrors.splice(claimed, 1);
    return false;
  });
  check(
    'ブラウザのコンソールエラーがない',
    unexpectedErrors.length === 0,
    unexpectedErrors.slice(0, 2).join(' | '),
  );
} catch (error) {
  check('検証スクリプトが最後まで走る', false, String(error).split('\n')[0]);
} finally {
  await browser.close();
}

const failed = results.filter((result) => !result.ok);
console.log(`\n${results.length - failed.length}/${results.length} 件のチェックに成功`);
process.exit(failed.length === 0 ? 0 : 1);
