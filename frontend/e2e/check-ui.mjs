/**
 * End-to-end check for the admin collection list, driven by a real browser.
 *
 * This is deliberately *not* part of `npm test`: the unit suite stays fast, browserless and
 * dependency-free, while this needs two running servers, a browser and a few seconds. What
 * it covers is the part the unit tests cannot see, because they stub the service out — that
 * the screen really works against the API:
 *
 *   - signing in through the form
 *   - the deployment's own name on the sign-in card, in the app bar and in the tab title
 *   - the tab icon reading on a light tab strip and on a dark one
 *   - one page of 25 rows, ordered by id, with the total in the pager
 *   - the status badge and the Updated column
 *   - the next page and a different page size reaching the server
 *   - publishing an item from the list
 *   - opening an item by its row, with the actions column pinned to the right edge
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
/** Where a preview link is opened: the site's origin, which the API is told separately. */
const PREVIEW_SITE = process.env.PREVIEW_SITE_URL ?? BASE;
const API = `${BASE}/api`;
const USERNAME = process.env.ADMIN_USERNAME ?? process.env.ADMIN_EMAIL ?? 'admin@example.com';
const PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin-password';
/** What the harness's backend calls itself (`SITE_NAME`), which the screens have to show. */
const SITE_NAME = process.env.SITE_NAME ?? 'e2e content site';
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
  // The field a reference to one of these items shows as its name.
  { name: 'title', field_type: { Text: {} }, required: true, width: 12, height: 1, is_title: true },
  // A plain array: the JSON box is its whole editor, so the box has to say what it accepts.
  { name: 'scores', field_type: { Array: ['Number'] }, required: false, width: 12, height: 1 },
];
/** One single image, and one array of them: the two ways an image field is used. */
const IMAGE_SCHEMA = [
  // Marked for the list: an image column is a column of pictures, which is the one thing a reader
  // recognises an item by here.
  { name: 'photo', field_type: 'Image', required: false, width: 12, height: 1, show_in_list: true },
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
/** The scenario being run, so a result (and a failure) says which part of the suite it is from. */
let scenario = 'start-up';
function check(label, ok, detail = '') {
  results.push({ label, ok, scenario });
  console.log(`${ok ? 'PASS' : 'FAIL'}  [${scenario}] ${label}${detail ? `  (${detail})` : ''}`);
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
      window.localStorage.setItem('tsubame.language', 'en');
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
  await seedComposite(login.token);
  await seedCollection(COLLECTION, TOTAL, login.token);
  await seedCollection(LAST_PAGE_COLLECTION, LAST_PAGE_TOTAL, login.token);
  await seedCollection(IMAGE_COLLECTION, 1, login.token, IMAGE_SCHEMA, () => ({
    photo: null,
    gallery: [],
  }));
  // After the collections it points at: a relation inside a definition names a target of the site,
  // and that target has to exist when the definition is saved.
  await api(
    'POST',
    `/models/composite_fields/${SCHEMA_BLOCK}`,
    [
      { name: 'line', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      // A relation inside a composite definition: its target is a collection of the site, and the
      // item that holds the value is what the reference belongs to.
      {
        name: 'author',
        field_type: {
          Relation: { target: { kind: 'collection', name: COLLECTION } },
        },
        required: false,
        width: 12,
        height: 1,
      },
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
/**
 * What to answer a browser prompt with.
 *
 * The editors ask before leaving unsaved work (`unsavedChangesGuard`), so a navigation that used
 * to be silent now raises one; the checks that are *about* that ask for `dismiss`.
 */
let dialogAnswer = 'accept';
/**
 * What a prompt is answered with, when the check is about one.
 *
 * A `confirm` ignores it; a `prompt` is answered with it, which is how the Markdown toolbar's
 * "where should the link go?" is driven.
 */
let dialogText;
const answerDialog = (dialog) =>
  dialogAnswer === 'accept' ? dialog.accept(dialogText) : dialog.dismiss();
page.on('dialog', answerDialog);

/**
 * The form's own save button.
 *
 * By accessible name and exact: the toolbar offers "Save and publish" whenever the form holds
 * unsaved edits, and a substring match would press that instead - which publishes. The toolbar
 * carries a plain "Save" of its own now, so this takes the last match, which is the one at the end
 * of the form - the one that is there whatever the form holds.
 */
const save = () => page.getByRole('button', { name: 'Save', exact: true }).last();

/**
 * Save the open form and wait for the save to land.
 *
 * Saving keeps the reader on the item now, so the navigation that used to announce "the save
 * finished" is gone; the save's own answer is what says so, and waiting for it is what keeps the
 * API reads below from racing the write.
 */
async function saveAndWait() {
  const answered = page
    .waitForResponse(
      (response) =>
        /\/items(\/\d+)?$/.test(new URL(response.url()).pathname) &&
        ['POST', 'PUT'].includes(response.request().method()),
      { timeout: 15000 },
    )
    .catch(() => {});
  await save().click();
  await answered;
}
/** Data rows only: the "No items yet." row has no status badge. */
const dataRows = () => page.locator('table.items tbody tr:has(app-item-status)');
const firstRow = () => dataRows().first();
const badgeOf = (row) => row.locator('app-item-status .badge');

/** Sign in on a fresh context, so a second role can be looked at beside the admin one. */
/**
 * Create an account and give it a password, the way a deployment ends up with one.
 *
 * `POST /auth/users` chooses no credential (an administrator cannot decide someone else's
 * password), so this creates the account and then completes a reset link for it - the same two
 * steps the account screen takes.
 */
async function createAccount(username, password, extra = {}, token) {
  const created = await api(
    'POST',
    '/auth/users',
    {
      username,
      is_admin: false,
      permission: { can_view: true, can_edit: false, can_publish: false },
      ...extra,
    },
    token,
  );
  const link = await api('POST', `/auth/users/${created.id}/password-reset`, undefined, token);
  // This suite drives the on-premises deployment, whose answer is a link. A deployment where the
  // identity provider holds the password answers a temporary password instead, and there is
  // nothing here to complete it with - that is what `hosted-signin.mjs` is for.
  if (link.kind !== 'link') {
    throw new Error(`this suite expects a reset link, got ${JSON.stringify(link)}`);
  }
  await api(
    'POST',
    '/auth/password-reset',
    { token: link.token, new_password: password },
    undefined,
  );
  return created;
}

async function openAs(username, password) {
  const roleContext = await newContext();
  const rolePage = await roleContext.newPage();
  rolePage.on('pageerror', (error) => consoleErrors.push(`${username}: ${error}`));
  rolePage.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(`${username}: ${message.text()}`);
  });
  rolePage.on('dialog', answerDialog);

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
async function expandBranch(rolePage, label) {
  await rolePage
    .locator(`button[aria-label="toggle ${label}"]`)
    .click({ force: true })
    .catch(() => {});
  await rolePage.waitForTimeout(250);
}

async function expandSettings(rolePage) {
  for (const label of ['Settings', 'Schemas']) {
    await expandBranch(rolePage, label);
  }
}

/**
 * Put the whole collection on one page.
 *
 * The list is newest first, so the oldest item (the one scenarios name) is on the last page; a
 * hundred rows to a page is simpler than paging to the end and makes the row visible by name.
 */
async function showWholeCollection(count) {
  await page.locator('mat-paginator mat-select').click({ force: true });
  await page.locator('mat-option', { hasText: '100' }).first().click({ force: true });
  await waitForRows(count);
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

// ------------------------------------------------------------------------- the scenarios

// The suite is an ordered list of scenarios, each a function: a failure then says which part
// of the suite it came from, and a scenario can be read (or run) on its own. They share the
// page, the signed-in token and the values just below, in the order they are listed - the
// suite is one story told in chapters, and the order is what makes it cheap to set up.

// Values one scenario leaves for the next: assigned where they are produced, read by whichever
// scenario needs them.
let paginator;
let badge;
let imagesBefore;
let replacedImageId;
let thumbs;
let titleField;
let publishWording;
let editor;
let viewer;

/**
 * The tab icon, on a tab strip of either colour.
 *
 * A tab strip belongs to the browser rather than to the page, so this is the one picture the suite
 * can judge only by drawing it: the icon is painted over each strip's own background in a canvas and
 * read back as pixels. An icon that reads on a light strip only is what a dark-mode reader reported
 * (`prefers-color-scheme` inside an SVG icon is not an answer every browser gives), and nothing else
 * in these suites would have seen it.
 */
async function theTabIconReadsOnAnyTabStrip() {
  await page.goto(`${BASE}/`, { waitUntil: 'networkidle' });

  for (const [href, label] of [
    ['/favicon.svg', 'SVG'],
    ['/favicon.ico', 'ICO'],
  ]) {
    const measured = await page.evaluate(async (source) => {
      const image = new Image();
      image.src = source;
      await image.decode();

      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 64;
      const context = canvas.getContext('2d');
      /** The icon over one tab-strip colour, as its brightest and darkest pixel. */
      const over = (background) => {
        context.clearRect(0, 0, 64, 64);
        context.fillStyle = background;
        context.fillRect(0, 0, 64, 64);
        context.drawImage(image, 0, 0, 64, 64);
        const pixels = context.getImageData(0, 0, 64, 64).data;
        let brightest = 0;
        let darkest = 255;
        for (let i = 0; i < pixels.length; i += 4) {
          const luminance = 0.2126 * pixels[i] + 0.7152 * pixels[i + 1] + 0.0722 * pixels[i + 2];
          brightest = Math.max(brightest, luminance);
          darkest = Math.min(darkest, luminance);
        }
        return { brightest: Math.round(brightest), darkest: Math.round(darkest) };
      };

      // A light strip and a dark one, as a browser paints them.
      return { light: over('#f1f3f4'), dark: over('#202124') };
    }, href);

    // Something dark has to be in the icon for a light strip, and something light for a dark one.
    check(
      `タブアイコン(${label})は明暗どちらのタブでも見える`,
      measured.light.darkest <= 60 && measured.dark.brightest >= 180,
      `明るいタブ: 最暗 ${measured.light.darkest} / 暗いタブ: 最明 ${measured.dark.brightest}`,
    );
  }
}

/**
 * The deployment's own name where a reader meets it before signing in.
 *
 * Which site a CMS administers is otherwise answered by the product's name, which is the same
 * wherever it is deployed: someone with two of these open has nothing telling the screens apart.
 * The name arrives as a setting (`SITE_NAME`), so this is the whole path - the environment, the
 * capabilities answer, and the screen.
 */
async function theDeploymentNamesItself() {
  await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });

  // The sign-in screen has no shell around it, so the card is the one place that can say it - and
  // the tab is what is left of the screen while a form is being filled in.
  const card = (await page.locator('mat-card-title').textContent())?.trim();
  check('サインイン画面に配備の名前が出る', card === `Sign in to ${SITE_NAME}`, `${card}`);
  check(
    'タブのタイトルに配備の名前が出る',
    (await page.title()) === `${SITE_NAME} — Tsubame`,
    await page.title(),
  );
}

/** The same name in the app bar, which exists only with a session behind it. */
async function theDeploymentNamesItselfInTheShell() {
  await page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });

  // The name being administered first, the product under it: neither is lost, and the bar keeps
  // the account, the language switch and the buttons it had.
  const site = (await page.locator('.brand .site-name').textContent())?.trim();
  const product = (await page.locator('.brand .product').textContent())?.trim();
  check(
    'アプリバーに配備の名前と製品名が出る',
    site === SITE_NAME && product === 'Tsubame',
    `${site} / ${product}`,
  );

  // The landing screen is the first thing a session sees, so its heading says the same two lines -
  // and it is the largest text anywhere for "which site is this?".
  await page.goto(`${BASE}/`, { waitUntil: 'networkidle' });
  const heading = (await page.locator('.welcome h3').textContent())?.trim();
  const under = (await page.locator('.welcome .product').textContent())?.trim();
  check(
    'トップページの見出しに配備の名前が出る',
    heading === SITE_NAME && under === 'Tsubame',
    `${heading} / ${under}`,
  );
}

/** The language switch. */
async function theLanguageSwitch() {
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
}

/** Sign in through the form. */
async function signInThroughTheForm() {
  await page.goto(`${BASE}/login`, { waitUntil: 'networkidle' });
  await page.fill('input[name=username]', USERNAME);
  await page.fill('input[name=password]', PASSWORD);
  await page.click('button:has-text("Sign in")');
  await page
    .waitForFunction(() => !location.pathname.startsWith('/login'), null, { timeout: 15000 })
    .catch(() => {});
  check('ログインフォームからサインインできる', !page.url().includes('/login'), page.url());

  // The icons are ligatures, so a font that never loads draws their *names* as text - and every
  // assertion about text and aria labels still passes. `mat-icon` sets the box and leaves the
  // family to the application, so this is the check that notices `styles.scss` no longer naming it.
  const iconFont = await page.evaluate(async () => {
    const icon = document.querySelector('mat-icon');
    if (!icon) return null;
    // Asking for the face is what makes a lazily-loaded font load at all; `check` on its own can
    // answer "not loaded" for a font that nothing has needed yet, which is a race this check lost
    // once. A face that cannot be fetched leaves `check` false, which is the failure it is for.
    await document.fonts.load("24px 'Material Icons'").catch(() => {});
    return {
      family: getComputedStyle(icon).fontFamily,
      loaded: document.fonts.check("24px 'Material Icons'"),
    };
  });
  check(
    'アイコンがフォントで描かれる(リガチャ名が文字にならない)',
    iconFont !== null && iconFont.family.includes('Material Icons') && iconFont.loaded,
    JSON.stringify(iconFont),
  );
}

/** A plain array, edited as JSON. */
async function aPlainArrayEditedAsJSON() {
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
  await save().click();
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
  await saveAndWait();
  const savedScores = await api(
    'GET',
    `/models/collections/${COLLECTION}/items/1`,
    undefined,
    token,
  );
  check(
    '配列の値がそのまま保存される',
    JSON.stringify(savedScores?.scores) === '[1,2]',
    JSON.stringify(savedScores?.scores),
  );
}

/** First page. */
async function firstPage() {
  await page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });

  const firstPageRows = await dataRows().count();
  check('1 ページ目は既定の 25 件', firstPageRows === 25, `${firstPageRows} 行`);

  // The first cell is the batch checkbox; the id is the one after it.
  // Nothing is marked in this collection's schema, so it has no field columns at all: id, state and
  // when it changed are what every row has in common.
  const unconfiguredHeaders = (await page.locator('table.items thead th').allTextContents()).map(
    (cell) => cell.trim(),
  );
  check(
    '一覧に表示を選んでいないコレクションは id・状態・更新日時だけ',
    unconfiguredHeaders.join(',') === ',ID,Status (publisher),Updated,',
    unconfiguredHeaders.join(','),
  );

  // Newest first: the id an item got when it was created is its age, so the largest is on top.
  const firstId = (await firstRow().locator('td').nth(1).textContent())?.trim();
  check(`新しい順で id ${TOTAL} から始まる`, firstId === String(TOTAL), `id=${firstId}`);

  paginator = page.locator('mat-paginator');
  check('ページャが表示される', await paginator.isVisible());

  const rangeLabel = (
    await paginator.locator('.mat-mdc-paginator-range-label').textContent()
  )?.trim();
  check(
    `ページャに総件数 ${TOTAL} が出る`,
    new RegExp(String(TOTAL)).test(rangeLabel ?? ''),
    rangeLabel,
  );
}

/** Status and updated columns. */
async function statusAndUpdatedColumns() {
  badge = (await badgeOf(firstRow()).textContent())?.trim();
  check('状態バッジが出る', badge === 'Draft' || badge === 'Published', badge);

  const updated = (await firstRow().locator('td.updated').textContent())?.trim();
  check('Updated 列に日時が出る', Boolean(updated) && updated !== '—', updated);
}

/** Next page. */
async function nextPage() {
  // Material overlays a touch target on the pager buttons, so a plain click never lands.
  await paginator.locator('.mat-mdc-paginator-navigation-next').click({ force: true });
  // Waiting for 25 rows would prove nothing (page one has 25 too), so wait for the content that
  // tells the pages apart. Without this the check raced the reload and sometimes read page one.
  // One page holds 25, so the next page starts 25 ids below the top of this one.
  const secondPageFirst = String(TOTAL - 25);
  await page
    .waitForFunction(
      (expected) =>
        document.querySelector('table.items tbody tr td')?.textContent?.trim() === expected,
      secondPageFirst,
      { timeout: 10000 },
    )
    .catch(() => {});
  await waitForRows(25);
  const secondPageFirstId = (await firstRow().locator('td').nth(1).textContent())?.trim();
  check(
    `次ページは id ${secondPageFirst} から始まる`,
    secondPageFirstId === secondPageFirst,
    `id=${secondPageFirstId}`,
  );
}

/** Page size. */
async function pageSize() {
  await paginator.locator('mat-select').click({ force: true });
  await page.locator('mat-option', { hasText: '50' }).first().click({ force: true });
  await waitForRows(50);
  const rowsAtFifty = await dataRows().count();
  check('ページサイズ 50 で 50 行', rowsAtFifty === 50, `${rowsAtFifty} 行`);
}

/** Ordering from the headers. */
async function orderingFromTheHeaders() {
  // The order belongs to the screen: the id column's header asks for the other way round, and the
  // header says which way it is (`aria-sort`, and an arrow drawn in CSS).
  const idHeading = page.locator('thead th').nth(1);
  const idHeaderButton = idHeading.locator('.sort-header');
  check(
    '既定の並び順が見出しに出る',
    (await idHeading.getAttribute('aria-sort')) === 'descending',
    (await idHeading.getAttribute('aria-sort')) ?? 'none',
  );

  const firstIdNow = async () => (await firstRow().locator('td').nth(1).textContent())?.trim();
  await idHeaderButton.click();
  await page
    .waitForFunction(
      () =>
        document.querySelector('table.items tbody tr td:nth-child(2)')?.textContent?.trim() === '1',
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  check(
    '見出しを押すと昇順になる',
    (await firstIdNow()) === '1' && (await idHeading.getAttribute('aria-sort')) === 'ascending',
    `id=${await firstIdNow()} / ${(await idHeading.getAttribute('aria-sort')) ?? 'none'}`,
  );

  await idHeaderButton.click();
  await page
    .waitForFunction(
      (expected) =>
        document.querySelector('table.items tbody tr td:nth-child(2)')?.textContent?.trim() ===
        expected,
      String(TOTAL),
      { timeout: 10000 },
    )
    .catch(() => {});
  check(
    'もう一度押すと降順に戻る',
    (await firstIdNow()) === String(TOTAL) &&
      (await idHeading.getAttribute('aria-sort')) === 'descending',
    `id=${await firstIdNow()} / ${(await idHeading.getAttribute('aria-sort')) ?? 'none'}`,
  );
}

/** Publish a draft from the list. */
async function publishADraftFromTheList() {
  // Item 1 is what the rest of the scenario works with (the delivery checks and the relation
  // target), and the list is newest first: the oldest item lives on the last page, so show the
  // whole collection and name the row rather than taking the top of the list.
  await showWholeCollection(TOTAL);
  const draftRow = dataRows()
    .filter({ has: page.locator('button[aria-label="publish item 1"]') })
    .first();
  const draftId = (await draftRow.locator('td').nth(1).textContent())?.trim();

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
  check(
    '一覧から公開できる',
    badgeAfterPublish === 'Published',
    `id=${draftId} → ${badgeAfterPublish}`,
  );

  // The audit trail: the row now names the account that published it.
  const publisherNote = (await rowById(draftId).locator('.publisher').textContent())?.trim();
  check('誰が公開したかが一覧に出る', publisherNote === USERNAME, `${publisherNote}`);
}

/** Opening an item from the list, and the row's controls staying within reach. */
async function openingAnItemFromTheList() {
  await page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });

  // The row-actions column is pinned to the right edge of the list, so the buttons no longer travel
  // off the screen when the schema shows enough columns to make the table wider than the window.
  const actionsPosition = await page.evaluate(() => {
    const cell = document.querySelector('table.items tbody td.actions');
    return cell === null ? 'no cell' : getComputedStyle(cell).position;
  });
  check('操作の列は右端に固定される', actionsPosition === 'sticky', actionsPosition);

  // The screen around the table belongs to the screen. The toolbar and the pager used to sit in the
  // same scroll box as the table, so scrolling sideways carried "new item" off with it; the table
  // has a box of its own now. Both halves are read here: the toolbar button has to stay where it is,
  // the table has to really scroll, and the pin has to come to rest where the table ends rather than
  // hiding the last column's tail (which is what the gutter between them used to do).
  const windowSize = page.viewportSize();
  await page.setViewportSize({ width: 800, height: 800 });
  const geometry = await page.evaluate(() => {
    const content = document.querySelector('.content');
    const box = document.querySelector('.table-scroll');
    const last = document.querySelector('table.items tbody tr td.updated');
    const pinned = document.querySelector('table.items tbody td.actions');
    const newItem = [...document.querySelectorAll('button')].find((button) =>
      button.textContent.includes('New item'),
    );
    const before = newItem.getBoundingClientRect().left;
    box.scrollLeft = box.scrollWidth;
    return {
      screenScrollsSideways: content.scrollWidth > content.clientWidth,
      newItemMoved: Math.round(newItem.getBoundingClientRect().left - before),
      tableScrollRange: Math.round(box.scrollWidth - box.clientWidth),
      hidden: Math.round(last.getBoundingClientRect().right - pinned.getBoundingClientRect().left),
    };
  });
  await page.setViewportSize(windowSize);
  check(
    '横にスクロールするのは表だけで、新しいアイテムは動かない',
    !geometry.screenScrollsSideways && geometry.newItemMoved === 0 && geometry.tableScrollRange > 0,
    `画面=${geometry.screenScrollsSideways} / ボタンの移動=${geometry.newItemMoved}px / 表の可動範囲=${geometry.tableScrollRange}px`,
  );
  check(
    '右端までスクロールすると最後の列まで見える',
    geometry.hidden <= 0,
    `${geometry.hidden}px 隠れている`,
  );

  // The row opens the item, the way a page's name does in the single-page list.
  await firstRow().locator('td').nth(1).click();
  const opened = await page
    .waitForURL(`**/collections/${COLLECTION}/edit/**`, { timeout: 15000 })
    .then(() => true)
    .catch(() => false);
  check('行を押すと編集画面が開く', opened, page.url());

  // The checkbox is the row's own control: a click there selects, and does not navigate away from
  // the list the reader is working in.
  await page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });
  await firstRow().locator('td.select input[type=checkbox]').check();
  await page.waitForTimeout(200);
  check(
    'チェックボックスを押しても遷移しない',
    !page.url().includes('/edit/') && (await page.locator('.toolbar .selection').count()) === 1,
    page.url(),
  );
}

/** Delete the last page's only row. */
async function deleteTheLastPagesOnlyRow() {
  await page.goto(`${BASE}/collections/${LAST_PAGE_COLLECTION}`, { waitUntil: 'networkidle' });
  await dataRows().first().waitFor({ timeout: 15000 });
  await page.locator('mat-paginator .mat-mdc-paginator-navigation-last').click({ force: true });
  await waitForRows(1);
  const lastPageRows = await dataRows().count();
  check('最終ページは 1 行', lastPageRows === 1, `${lastPageRows} 行`);

  const doomedId = (await firstRow().locator('td').nth(1).textContent())?.trim();
  // Deleting lives in the row's menu, behind the button that keeps the row's controls narrow.
  await firstRow().locator('button[aria-label^="more actions"]').click();
  await page.getByRole('menuitem', { name: 'Delete item' }).click();
  await waitForRows(25);
  const rowsAfterDelete = await dataRows().count();
  check(
    '最終ページの最後の 1 件を削除すると前のページへ戻る',
    rowsAfterDelete === 25,
    `${rowsAfterDelete} 行 (削除 id=${doomedId})`,
  );
}

/** The image library. */
async function theImageLibrary() {
  await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
  imagesBefore = await page.locator('.library .image').count();

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

  // A tile shows the small copy the browser made from the file it just uploaded, not the original:
  // 180 pixels of tile for a megabyte of photograph is what made a library cost hundreds of
  // megabytes (see `docs/content-api.md`).
  check(
    'タイルはブラウザが作った小さなコピーを出す',
    /\/api\/images\/thumb-.*\.webp$/.test(imageSource ?? ''),
    String(imageSource).slice(0, 80),
  );
  const thumbnailStatus = await page.evaluate(
    async (src) => (await fetch(src)).status,
    imageSource,
  );
  check('小さなコピーが配信される', thumbnailStatus === 200, `status=${thumbnailStatus}`);

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
  replacedImageId = Number(idBeforeReplace?.match(/id (\d+)/)?.[1]);
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
    (await page.evaluate(
      async (src) => (await fetch(src, { redirect: 'follow' })).status,
      durableLink,
    )) === 200,
  );

  // The record of what happened arrives where the link is handed out.
  await cardAfterReplace.locator('button[aria-label^="copy the link of image"]').click();
  await page.waitForTimeout(400);
  const linkNotice = (
    (await page.locator('.notice-toast .text').first().textContent()) ?? ''
  ).trim();
  check('リンクをコピーできる', linkNotice.includes(`/api/images/by-id/`), linkNotice);
}

/** Pick images while editing. */
async function pickImagesWhileEditing() {
  await page.goto(`${BASE}/collections/${IMAGE_COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('button:has-text("Choose existing")').click();
  thumbs = page.locator('.thumb');
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
  check(
    '画像配列にその場でアップロードできる',
    arrayAfterUpload === 3,
    `${arrayItems} → ${arrayAfterUpload}`,
  );

  await saveAndWait();

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
    typeof saved?.photo === 'object' &&
      saved.photo !== null &&
      saved.photo.url === expectedPhotoUrl,
    `${JSON.stringify(saved?.photo)} (expected ${expectedPhotoUrl})`,
  );

  // The list's image column draws the picture rather than the url the value holds.
  await page.goto(`${BASE}/collections/${IMAGE_COLLECTION}`, { waitUntil: 'networkidle' });
  await page.locator('table.items tbody tr').first().waitFor({ timeout: 15000 });
  const thumbnail = page.locator('table.items tbody tr').first().locator('td.value img').first();
  const thumbnailSrc = (await thumbnail.getAttribute('src').catch(() => null)) ?? '';
  check(
    '一覧の画像列はサムネイルで出る',
    (await thumbnail.count()) === 1 && thumbnailSrc.includes('/api/images/'),
    thumbnailSrc || 'no thumbnail',
  );
  check(
    '画像配列がまとめて保存される',
    Array.isArray(saved?.gallery) &&
      saved.gallery.length === 3 &&
      // The stored URL carries the API prefix: the CMS serves its images under `/api`.
      saved.gallery.every((image) => image?.url?.startsWith('/api/images/')),
    JSON.stringify(saved?.gallery),
  );
}

/** Saving is not publishing. */
async function savingIsNotPublishing() {
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
  const publishedItems = await api(
    'GET',
    `/content/collections/${IMAGE_COLLECTION}`,
    undefined,
    token,
  );
  check(
    '公開すると配信 API に反映される',
    publishedItems.items.length === 1,
    `${publishedItems.items.length} 件`,
  );
}

/** An image array inside a composite. */
async function anImageArrayInsideAComposite() {
  await page.goto(`${BASE}/collections/${COMPOSITE_COLLECTION}/edit/1`, {
    waitUntil: 'networkidle',
  });
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

  await saveAndWait();
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
}

/** Delete them again. */
async function deleteThemAgain() {
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
  check(
    '画像をゴミ箱へ移動できる',
    imagesAfterDelete === imagesBefore,
    `${imagesBefore} に戻る (現在 ${imagesAfterDelete})`,
  );

  // An image the content still shows is named in the question before it leaves the library.
  // The item saved earlier references the images in its array, so one of them has references.
  await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
  const referenced = await api('GET', '/models/images/2/references', undefined, token);
  check(
    '画像を使っているコンテンツを参照索引から引ける',
    referenced.length === 0 || referenced.every((owner) => owner.kind && owner.name),
    JSON.stringify(referenced),
  );

  // Trash is the undoable half: the images are out of the library, and can be put back.
  await page.goto(`${BASE}/images`, { waitUntil: 'networkidle' });
  await page.locator('button:has-text("Trash")').first().click();
  await page
    .locator('.library .image')
    .first()
    .waitFor({ timeout: 10000 })
    .catch(() => {});
  const trashedBefore = await page.locator('.library .image').count();
  check('ゴミ箱に入れた画像が並ぶ', trashedBefore > 0, `${trashedBefore} 件`);
  check(
    'ゴミ箱の画像はライブラリに戻せる',
    (await page.locator('button[aria-label$="back in the library"]').count()) === trashedBefore,
  );

  await page.locator('button[aria-label$="back in the library"]').first().click();
  await page
    .waitForFunction(
      (expected) => document.querySelectorAll('.library .image').length === expected,
      trashedBefore - 1,
      { timeout: 10000 },
    )
    .catch(() => {});
  check(
    'ゴミ箱から戻すとゴミ箱が 1 つ減る',
    (await page.locator('.library .image').count()) === trashedBefore - 1,
    `${await page.locator('.library .image').count()} 件`,
  );

  // The restored image is back in the library, and can be taken out again.
  await page.locator('button:has-text("Library")').first().click();
  await page.waitForTimeout(300);
  check('戻した画像はライブラリにある', (await page.locator('.library .image').count()) > 0);
  await page.locator('.library .image .remove').first().click();
  await page.waitForTimeout(300);
  await page.locator('button:has-text("Trash")').first().click();
  await page.waitForTimeout(300);
  check(
    'もう一度ゴミ箱へ入れられる',
    (await page.locator('.library .image').count()) === trashedBefore,
    `${await page.locator('.library .image').count()} 件`,
  );

  // Deleting for good, from the trash, is the half that cannot be undone.
  await page.locator('.library .image .remove').first().click();
  await page
    .waitForFunction(
      (expected) => document.querySelectorAll('.library .image').length === expected,
      trashedBefore - 1,
      { timeout: 10000 },
    )
    .catch(() => {});
  check(
    '完全削除でゴミ箱から消える',
    (await page.locator('.library .image').count()) === trashedBefore - 1,
    `${await page.locator('.library .image').count()} 件`,
  );
}

/** Preview links are a schema's choice. */
async function previewLinksAreASchemasChoice() {
  // A schema that was never asked for previews offers no link at all: the button is not there, so
  // nobody is invited to mint something the server would refuse.
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('app-value-field input').first().waitFor({ timeout: 15000 });
  check(
    'プレビュー無効のスキーマではボタンを出さない',
    (await page.locator('button:has-text("Preview link")').count()) === 0,
    `buttons=${await page.locator('button:has-text("Preview link")').count()}`,
  );

  // Turning it on happens where the rest of the schema is saved, and on its own it says the
  // collection may be previewed - the fields did not change.
  await page.goto(`${BASE}/settings/collections/${COLLECTION}/schema`, {
    waitUntil: 'networkidle',
  });
  const previewToggle = page.locator('.preview-setting input[type="checkbox"]');
  await previewToggle.waitFor({ timeout: 15000 });
  check('既定ではプレビューは許可されていない', !(await previewToggle.isChecked()));
  await previewToggle.check();
  // Save from the bottom of the page, which is where the button is on a schema of any length: the
  // message used to be printed at the top of the page and left there, 1900 pixels above the
  // reader's eyes (measured on a schema of four fields).
  await page.evaluate(() => {
    const pane = document.querySelector('main.content');
    if (pane) pane.scrollTop = pane.scrollHeight;
  });
  await page.waitForTimeout(200);
  await page.getByRole('button', { name: 'Save schema' }).click();
  await page.locator('.notice-toast .text').first().waitFor({ timeout: 15000 });
  check('スキーマの保存でプレビューが許可される', await previewToggle.isChecked(), 'checked');

  const scrolled = await page.evaluate(
    () => (document.querySelector('main.content')?.scrollTop ?? 0) > 100,
  );
  const toastBox = await page.locator('.notice-toast .text').first().boundingBox();
  const viewport = page.viewportSize();
  check(
    '下端から保存してもメッセージが画面の上に出る',
    scrolled && Boolean(toastBox) && toastBox.y >= 0 && toastBox.y < (viewport?.height ?? 0),
    `scrolled=${scrolled} y=${Math.round(toastBox?.y ?? -1)} viewport=${viewport?.height}`,
  );
}

/** A link shows unpublished work to a guest. */
async function aLinkShowsUnpublishedWorkToAGuest() {
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  titleField = page.locator('app-value-field input').first();
  await titleField.waitFor({ timeout: 15000 });
  const previewWording = `preview wording ${Date.now()}`;
  await titleField.fill(previewWording);
  await saveAndWait();

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
  // What is handed over is an address on the *preview site*, not the API's own JSON: the site is
  // what a reviewer can read, and the route mirrors the API's minus the `/api` prefix.
  check(
    'プレビュー URL が発行される',
    new RegExp(`^${PREVIEW_SITE}/preview/collections/${COLLECTION}/items/1\\?token=`).test(
      previewUrl ?? '',
    ),
    String(previewUrl).slice(0, 70),
  );

  // The site asks the API behind it, which is the answer a check can read here: the harness has
  // no preview site to render (that package has its own suite), only the CMS.
  const copied = new URL(previewUrl);
  const apiPreviewUrl = `${API}${copied.pathname}${copied.search}`;
  const previewResponse = await request.fetch(apiPreviewUrl);
  check(
    'プレビュー URL はトークン無しで開ける',
    previewResponse.status() === 200,
    `status=${previewResponse.status()}`,
  );
  const previewBody = await previewResponse.json();
  check(
    'プレビューは作業コピーを見せる',
    previewBody?.values?.title === previewWording,
    JSON.stringify(previewBody?.values?.title),
  );

  // The link is signed for one item: pointing it at another one is refused.
  const tamperedResponse = await request.fetch(apiPreviewUrl.replace('/items/1?', '/items/2?'));
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
  // The row has to be on screen: the list is newest first and item 1 is the oldest, so order the
  // list by id ascending - the oldest item is then the first row.
  await page.locator('thead th').nth(1).locator('.sort-header').click();
  await page
    .waitForFunction(
      () =>
        document.querySelector('table.items tbody tr td:nth-child(2)')?.textContent?.trim() === '1',
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  const releaseInList = page.locator(`button[aria-label="publish the changes of item ${'1'}"]`);
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
    releasedCopy.id === 1 &&
      (await api('GET', `/models/collections/${COLLECTION}/items/1/metadata`, undefined, token))
        .has_draft === false,
  );

  // Publishing copies what the server holds, so an editor with the form half-changed used to put
  // the *previous* version live while the screen showed the new one.
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await titleField.waitFor({ timeout: 15000 });
  publishWording = `publish wording ${Date.now()}`;
  await titleField.fill(publishWording);
  check(
    '未保存の変更があると画面にそう出る',
    (await page.locator('.draft-note', { hasText: 'Not saved yet' }).count()) === 1,
  );
  check(
    '未保存なら「保存して公開」になる',
    (await page.locator('button:has-text("Save and publish")').count()) === 1 &&
      (await page.locator('button:has-text("Publish changes")').count()) === 0,
  );
  await page.click('button:has-text("Save and publish")');
  await page
    .waitForFunction(
      // Plain DOM: `:has-text` is a Playwright selector and the page cannot parse it.
      () =>
        !Array.from(document.querySelectorAll('button')).some((button) =>
          button.textContent?.includes('Save and publish'),
        ),
      null,
      { timeout: 10000 },
    )
    .catch(() => {});
  const afterSaveAndPublish = await api('GET', `/content/collections/${COLLECTION}/items/1`);
  check(
    '「保存して公開」は保存と公開をまとめて行う',
    afterSaveAndPublish.values.title === publishWording,
    `${afterSaveAndPublish.values.title}`,
  );
}

/** What is live, and taking changes back. */
async function whatIsLiveAndTakingChangesBack() {
  // The form holds the working copy, so the content the changes would replace is not visible
  // anywhere else in the CMS: it is read on demand, and the changes can be thrown away.
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await titleField.waitFor({ timeout: 15000 });
  const discardWording = `discard wording ${Date.now()}`;
  await titleField.fill(discardWording);
  await save().click();
  await page.waitForTimeout(500);
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await titleField.waitFor({ timeout: 15000 });

  await page.locator('button:has-text("Compare with what is published")').click();
  const comparison = page.locator('.comparison');
  await comparison.waitFor({ timeout: 10000 });
  const sides = await comparison
    .locator('input')
    .evaluateAll((inputs) => inputs.map((input) => input.value));
  check(
    '公開中と保存した変更を並べて見られる',
    sides.length === 2 && sides[0] !== discardWording && sides[1] === discardWording,
    sides.join(' / '),
  );

  await page.locator('button:has-text("Discard the changes")').click();
  await page
    .waitForFunction(
      // Plain DOM again: the wording is what the form holds, so the form has been re-read.
      (wording) =>
        Array.from(document.querySelectorAll('app-value-field input')).every(
          (input) => input.value !== wording,
        ),
      discardWording,
      { timeout: 15000 },
    )
    .catch(() => {});
  const afterDiscard = await api(
    'GET',
    `/models/collections/${COLLECTION}/items/1`,
    undefined,
    token,
  );
  check(
    '取り消すと公開中の内容に戻る',
    afterDiscard.title !== discardWording && afterDiscard.title === publishWording,
    `${afterDiscard.title}`,
  );
  check(
    '取り消してもサイトは変わらない',
    (await api('GET', `/content/collections/${COLLECTION}/items/1`)).values.title ===
      publishWording,
  );
  check(
    '取り消したので保留中の変更は無い',
    (await api('GET', `/models/collections/${COLLECTION}/items/1/metadata`, undefined, token))
      .has_draft === false,
  );

  // Leaving with unsaved edits asks first: the answer is the person's.
  await titleField.fill(`left behind ${Date.now()}`);
  dialogAnswer = 'dismiss';
  await page.click('button:has-text("Back to the list without saving")');
  await page.waitForTimeout(500);
  check('未保存で離れようとすると確認が出る', page.url().includes('/edit/1'), page.url());
  check(
    '取り消すと編集内容が残っている',
    (await titleField.inputValue()).startsWith('left behind'),
  );

  dialogAnswer = 'accept';
  await page.click('button:has-text("Back to the list without saving")');
  await page.waitForURL(`${BASE}/collections/${COLLECTION}`, { timeout: 15000 }).catch(() => {});
  check('確認に同意すると移動する', page.url().endsWith(`/collections/${COLLECTION}`), page.url());
}

/** Roles decide what is offered. */
async function rolesDecideWhatIsOffered() {
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
    // The account's own permission travels with it: a role test needs the role.
    await createAccount(account.username, 'role-password', account, token);
  }

  // Accounts are beside the schema branch, not inside it, so opening Settings is enough to reach
  // them: a link that needed "Schemas" opened first would be one the reader has to know is there.
  await expandBranch(page, 'Settings');
  check(
    '管理者にはアカウント管理が見える',
    (await page.locator('a[href="/settings/users"]').count()) === 1,
  );
  await expandBranch(page, 'Schemas');

  editor = await openAs('e2e-editor@example.com', 'role-password');
  await editor.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await editor.page
    .locator('table.items tbody tr:has(app-item-status)')
    .first()
    .waitFor({ timeout: 15000 });
  check(
    '編集ロール: 新規作成はできる',
    (await editor.page.locator('button:has-text("New item")').count()) === 1,
  );
  // Deleting lives in the row's menu, and a closed menu holds nothing: it is opened to read what
  // this role is offered, or "no delete button on screen" would be true for everybody.
  await editor.page
    .locator('table.items tbody tr:has(app-item-status)')
    .first()
    .locator('button[aria-label^="more actions"]')
    .click();
  await editor.page.locator('.mat-mdc-menu-panel').waitFor({ timeout: 10000 });
  const editorMenu = await editor.page.getByRole('menuitem').allTextContents();
  await editor.page.keyboard.press('Escape');
  check(
    '編集ロール: 公開も削除も出ない',
    (await editor.page.locator('button[aria-label^="publish item"]').count()) === 0 &&
      editorMenu.some((text) => text.includes('Copy item')) &&
      !editorMenu.some((text) => text.includes('Delete item')),
    editorMenu.join(' | '),
  );
  await expandSettings(editor.page);
  // The image library lives with the documents, so that branch has to be open to see its link.
  await editor.page
    .locator('button[aria-label="toggle Documents"]')
    .click({ force: true })
    .catch(() => {});
  await editor.page.waitForTimeout(200);
  check(
    '編集ロール: 画像は見えるが、設定は何も出ない',
    (await editor.page.locator('a[href="/images"]').count()) === 1 &&
      (await editor.page.locator('a[href^="/settings/"]').count()) === 0,
  );

  // A link is not a rule: the schema screens are administrator-only addresses, so going to one
  // directly lands back on the landing screen rather than on a screen whose save is refused.
  await editor.page.goto(`${BASE}/settings/collections`, { waitUntil: 'networkidle' });
  check(
    '編集ロール: スキーマ画面のアドレスにも入れない',
    new URL(editor.page.url()).pathname === '/',
    editor.page.url(),
  );

  await editor.page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await editor.page.locator('app-value-field').first().waitFor({ timeout: 15000 });
  // Two: the toolbar's and the form's. An editor is offered both acts of saving and no publishing.
  check(
    '編集ロール: 保存はできるが公開はできない',
    (await editor.page.getByRole('button', { name: 'Save', exact: true }).count()) === 2 &&
      (await editor.page.locator('button:has-text("Publish")').count()) === 0,
  );

  viewer = await openAs('e2e-viewer@example.com', 'role-password');
  await viewer.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await viewer.page
    .locator('table.items tbody tr:has(app-item-status)')
    .first()
    .waitFor({ timeout: 15000 });
  check(
    '閲覧ロール: 新規作成も出ない',
    (await viewer.page.locator('button:has-text("New item")').count()) === 0,
  );
  await viewer.page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await viewer.page.locator('app-value-field').first().waitFor({ timeout: 15000 });
  check(
    '閲覧ロール: 保存も公開も出ない',
    (await viewer.page.getByRole('button', { name: 'Save', exact: true }).count()) === 0 &&
      (await viewer.page.locator('button:has-text("Publish")').count()) === 0,
  );
  check(
    '閲覧ロール: 閲覧のみと案内される',
    (await viewer.page.locator('.note', { hasText: 'may not edit' }).count()) === 1,
  );
  check(
    '閲覧ロール: 自分のパスワードは変更できる',
    (await viewer.page
      .locator('a[href="/account"], button[aria-label="Change password"]')
      .count()) === 1,
  );
}

/** A password change ends the old sessions. */
async function aPasswordChangeEndsTheOldSessions() {
  await viewer.page.goto(`${BASE}/account`, { waitUntil: 'networkidle' });
  const stolenToken = await viewer.page.evaluate(() => localStorage.getItem('tsubame.token'));
  await viewer.page.fill('input[name=current]', 'role-password');
  await viewer.page.fill('input[name=next]', 'role-password-2');
  await viewer.page.fill('input[name=repeated]', 'role-password-2');
  await viewer.page.click('button:has-text("Change password")');
  await viewer.page
    .locator('.notice-toast .text')
    .waitFor({ timeout: 10000 })
    .catch(() => {});
  check(
    'パスワード変更が完了と表示される',
    (await viewer.page.locator('.notice-toast .text').count()) === 1,
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
  await viewer.page
    .locator('table.items tbody tr:has(app-item-status)')
    .first()
    .waitFor({ timeout: 15000 });
  check(
    '変更後も自分のセッションは続く',
    viewer.page.url().includes('/collections/'),
    viewer.page.url(),
  );

  // And the new password is the one that works from a fresh browser.
  const afterChange = await openAs('e2e-viewer@example.com', 'role-password-2');
  await afterChange.page.goto(`${BASE}/collections/${COLLECTION}`, { waitUntil: 'networkidle' });
  await afterChange.page
    .locator('table.items tbody tr:has(app-item-status)')
    .first()
    .waitFor({ timeout: 15000 });
  check('新しいパスワードでサインインできる', afterChange.page.url().includes('/collections/'));
  await afterChange.context.close();

  await editor.context.close();
  await viewer.context.close();
}

/** Per-resource permissions, granted from the screen. */
async function perResourcePermissionsGrantedFromTheScreen() {
  const scopedEmail = 'e2e-scoped@example.com';
  for (const account of await api('GET', '/auth/users', undefined, token)) {
    if (account.username === scopedEmail) {
      await api('DELETE', `/auth/users/${account.id}`, undefined, token);
    }
  }
  const scoped = await createAccount(scopedEmail, 'scoped-password', {}, token);
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
  await page
    .locator('.notice-toast .text')
    .waitFor({ timeout: 10000 })
    .catch(() => {});
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
  await scopedSession.page.locator('table.items tbody tr').first().waitFor({ timeout: 15000 });
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
  check(
    '拒否したコレクションは API でも 403',
    denied.status() === 403,
    `status=${denied.status()}`,
  );
  await scopedSession.context.close();
}

/** The schema editor, driven from the screen. */
async function theSchemaEditorDrivenFromTheScreen() {
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

  // Which fields the collection's list shows: an editor scanning the list needs what identifies an
  // item, not every field the content holds.
  await summaryField.locator('input[name=fieldInList]').check();
  check(
    '一覧に出す項目をスキーマで選べる',
    await summaryField.locator('input[name=fieldInList]').isChecked(),
  );

  // A slug: the type owns the rule, and the schema says which field the editor may fill it from.
  await page.click('button:has-text("Add field")');
  const slugField = page.locator('.schema-field').nth(4);
  await slugField.locator('input[name=fieldName]').fill('address');
  await slugField.locator('mat-select[name=fieldType]').click();
  await page.locator('mat-option', { hasText: 'Slug' }).click();
  // The select draws its own value through `FieldTypeString`: a type that pipe does not name has
  // no option to match, and the field it was chosen in reads as blank ("I chose Slug and the type
  // went empty"). The chosen text is what tells one from the other - the element's `textContent`
  // also holds every option, chosen or not.
  // The chosen text, waited for: the value element is redrawn a beat after the option is picked,
  // and reading it too early answers what it held before.
  const chosen = async (select, expected) => {
    const value = select.locator('.mat-mdc-select-value-text');
    await value
      .filter({ hasText: expected })
      .waitFor({ timeout: 5000 })
      .catch(() => {});
    return ((await value.textContent()) ?? '').trim();
  };
  const typeSelect = slugField.locator('mat-select[name=fieldType]');
  check(
    '型に Slug と出る',
    (await chosen(typeSelect, 'Slug')) === 'Slug',
    await chosen(typeSelect, 'Slug'),
  );
  await slugField.locator('mat-select[name=slugGenerateFrom]').click();
  await page.locator('mat-option', { hasText: 'title' }).click();
  await page.keyboard.press('Escape');
  const sourceSelect = slugField.locator('mat-select[name=slugGenerateFrom]');
  check(
    'Slug を画面で選び、生成元を指定できる',
    (await chosen(sourceSelect, 'title')) === 'title',
    await chosen(sourceSelect, 'title'),
  );

  // A relation points at another collection's items - or at a single page. Several references are
  // an array of relations now, so "how many it may hold" is the array wrapper and each item type
  // names one target. The target list is what the server says exists.
  await page.click('button:has-text("Add field")');
  const relationField = page.locator('.schema-field').nth(5);
  await relationField.locator('input[name=fieldName]').fill('related');
  // A relation is a column too: which author an article points at is exactly what identifies it.
  await relationField.locator('input[name=fieldInList]').check();
  await relationField.locator('mat-select[name=fieldType]').click();
  await page.locator('mat-option', { hasText: 'Array' }).click();
  // An array whose item types are relations is what several references are.
  await relationField.locator('mat-select[name=arrayItemTypes]').click();
  await page.locator('mat-option', { hasText: 'Relation' }).click();
  await page.keyboard.press('Escape');
  const relationItemType = relationField.locator('.array-relation-item').first();
  await relationItemType.locator('mat-select[name=arrayRelationTarget]').click();
  await page.locator('mat-option', { hasText: COLLECTION }).click();
  await page.keyboard.press('Escape');
  await relationItemType.locator('input[name=arrayRelationInverseName]').fill('posts');
  check(
    'リレーションの対象を画面で選べる',
    ((await relationField.locator('mat-select[name=arrayItemTypes]').textContent()) ?? '').includes(
      'Relation',
    ) &&
      (
        (await relationItemType.locator('mat-select[name=arrayRelationTarget]').textContent()) ?? ''
      ).includes(COLLECTION) &&
      (await relationItemType.locator('input[name=arrayRelationInverseName]').inputValue()) ===
        'posts',
  );

  // Half of the 12-column grid, from the presets rather than the number input.
  await textField.locator('.width-presets button', { hasText: '1/2' }).click();

  await page.click('button:has-text("Save schema")');
  await page
    .locator('.notice-toast .text')
    .waitFor({ timeout: 10000 })
    .catch(() => {});
  const builtSchema = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/schema`,
    undefined,
    token,
  );
  const builtText = builtSchema.find((field) => field.name === 'title');
  const builtEnum = builtSchema.find((field) => field.name === 'state');
  const builtArray = builtSchema.find((field) => field.name === 'blocks');
  const builtSlug = builtSchema.find((field) => field.name === 'address');
  const builtRelation = builtSchema.find((field) => field.name === 'related');
  check(
    '画面で組んだスキーマが保存される',
    builtText?.width === 6 &&
      builtText?.unique === true &&
      builtText?.field_type?.Text !== undefined &&
      builtEnum?.field_type?.TextEnum?.join() === 'published' &&
      builtArray?.field_type?.Array?.[0]?.CompositeField?.id === SCHEMA_BLOCK &&
      builtSlug?.field_type?.Slug?.generate_from === 'title' &&
      builtRelation?.field_type?.Array?.[0]?.Relation?.target?.kind === 'collection' &&
      builtRelation?.field_type?.Array?.[0]?.Relation?.target?.name === COLLECTION &&
      builtRelation?.field_type?.Array?.[0]?.Relation?.inverse_name === 'posts',
    JSON.stringify(builtSchema),
  );

  // The columns follow the schema: the marked field is there, and the rest are not.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}`, { waitUntil: 'networkidle' });
  const listedHeaders = (await page.locator('table.items thead th').allTextContents()).map((cell) =>
    cell.trim(),
  );
  check(
    '一覧には「一覧に表示」の項目だけが並ぶ',
    listedHeaders.join(',') === ',ID,summary,related,Status (publisher),Updated,' ||
      (listedHeaders.includes('summary') &&
        listedHeaders.includes('related') &&
        !listedHeaders.includes('title') &&
        !listedHeaders.includes('blocks')),
    listedHeaders.join(','),
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

  // The slug is filled from the title when asked, and what it holds is already canonical.
  const addressInput = page.locator('app-value-field input[name=address]');
  await addressInput.waitFor({ timeout: 15000 });
  check(
    'Slug に生成ボタンが出る',
    (await page.locator('button:has-text("Generate from title")').count()) === 1,
  );
  await page.click('button:has-text("Generate from title")');
  // The click sets the value; the input follows on the next check of the view, so wait for it
  // rather than reading it once.
  const generatedSlug = await page
    .waitForFunction(
      () => document.querySelector('app-value-field input[name=address]')?.value === 'first-item',
      null,
      { timeout: 10000 },
    )
    .then(() => true)
    .catch(() => false);
  check('タイトルから slug が生成される', generatedSlug, await addressInput.inputValue());
  await page.locator('app-value-field mat-select').click();
  await page.locator('mat-option', { hasText: 'published' }).click();
  // A multiple select keeps its panel open; the Save button is behind it until it closes.
  await page.keyboard.press('Escape');

  // The composite array is edited element by element, each by the definition it holds. An element
  // holds an array of the same definition, so it grows an "Add element" of its own the moment the
  // first element appears - and that one comes *before* the array's own button in the document.
  // `.last()` is the outer array's, which is the one this scenario means, whatever has rendered
  // by the time the click is dispatched.
  const addElement = page.locator('button.array-add:has-text("Add element")').last();
  await addElement.click();
  await addElement.click();
  const elements = page.locator('.composite-element');
  // The elements follow the value, which the next render writes out: the second fill is what
  // needs the second element on screen, not the click that added it.
  await elements.nth(1).waitFor({ timeout: 15000 });
  await elements.nth(0).locator('input').first().fill('first block');
  await elements.nth(1).locator('input').first().fill('second block');
  check(
    '複合配列の要素が 2 つ出る',
    (await elements.count()) === 2,
    `${await elements.count()} 件`,
  );
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

  // What the target calls its item 1 is a title the checks above have been editing, so ask instead
  // of assuming it: the names offered below should be that one. A label is matched whole, because
  // one title being a prefix of another is exactly the mistake these checks are here to notice.
  const referencedItem = await api(
    'GET',
    `/models/collections/${COLLECTION}/items/1`,
    undefined,
    token,
  );
  const referencedTitle = String(referencedItem?.title ?? '');
  const wholeLabel = (text) => new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`);

  // The form holds more than one relation field now, because a relation may sit inside a composite
  // definition: the checks name the field they mean (the top-level `related`) instead of taking the
  // first button or chip on the page.
  const relationCell = page
    .locator('.field-cell')
    .filter({ has: page.locator('.field-label', { hasText: 'related' }) });

  // The block elements hold a relation of their own, and the picker works the same way there: what
  // it offers comes from the field's target, wherever the field sits.
  const firstElement = elements.nth(0);
  await firstElement.locator('button.relation-add').click();
  const nestedCandidates = firstElement.locator('app-relation-picker .candidate .label');
  await nestedCandidates.first().waitFor({ timeout: 15000 });
  const nestedNames = (await nestedCandidates.allTextContents()).map((name) => name.trim());
  check(
    '複合の中のリレーションもピッカーで選べる',
    nestedNames.includes(referencedTitle),
    nestedNames.slice(0, 3).join(' / '),
  );
  await nestedCandidates
    .filter({ hasText: wholeLabel(referencedTitle) })
    .first()
    .click();
  // What the chip reads is a request of its own, and until it answers the chip falls back to the
  // reference: wait for the name rather than reading the fallback.
  const nestedChip = firstElement.locator('mat-chip-row').first();
  await nestedChip
    .filter({ hasText: referencedTitle })
    .waitFor({ timeout: 15000 })
    .catch(() => {});
  const nestedChipText = ((await nestedChip.textContent()) ?? 'none').replace('cancel', '').trim();
  check(
    '複合の中の参照がチップで出る',
    nestedChipText === referencedTitle,
    `${nestedChipText} (expected ${referencedTitle})`,
  );
  await firstElement.locator('button.relation-add').click();

  // A relation is picked, not typed: the picker lists the target's items by the name their schema
  // gives them, which is the whole reason a collection says which field names an item.
  await relationCell.locator('button.relation-add').click();
  const candidateLabels = relationCell.locator('app-relation-picker .candidate .label');
  // The items arrive first and their names are a second request: the check below is about the
  // names, so wait for one of them rather than reading the fallbacks it started with.
  await candidateLabels
    .filter({ hasText: wholeLabel(referencedTitle) })
    .first()
    .waitFor({ timeout: 15000 })
    .catch(() => {});
  const candidates = (await candidateLabels.allTextContents()).map((name) => name.trim());
  check(
    'ピッカーが参照先をタイトルで並べる',
    candidates.length > 0 && candidates.every((name) => !name.includes(' #')),
    candidates.slice(0, 3).join(' / '),
  );
  await relationCell
    .locator('app-relation-picker .candidate .label')
    .filter({ hasText: wholeLabel(referencedTitle) })
    .first()
    .click();
  // The chip's own name is looked up after the pick, the way the row's is looked up after the row
  // is drawn: read once and this catches the fallback the chip starts with - which is how this
  // check failed with `e2e_blog #1` on a run whose earlier picker checks were green.
  const pickedLabel = relationCell.locator('mat-chip-row .reference-label').first();
  await pickedLabel
    .filter({ hasText: wholeLabel(referencedTitle) })
    .waitFor({ timeout: 15000 })
    .catch(() => {});
  const pickedChip = ((await pickedLabel.textContent()) ?? 'none').trim();
  check(
    'ピッカーで参照先を選べる',
    (await relationCell.locator('mat-chip-row').count()) === 1 && pickedChip === referencedTitle,
    `${pickedChip} (expected ${referencedTitle})`,
  );
  // The picker closes the way it opened, from the button beside it (the panel's own Close is the
  // other way out, and the component's spec is where that one is checked).
  await relationCell.locator('button.relation-add').click();
  await page.locator('app-relation-picker').waitFor({ state: 'detached', timeout: 15000 });

  // A relation is a list, so the order it was picked in is the order it is served in - and the
  // chips can change it. A second reference is picked, moved in front of the first, and taken
  // away again, which leaves this scenario's reference where the checks below expect it.
  await relationCell.locator('button.relation-add').click();
  const secondCandidate = relationCell.locator('app-relation-picker .candidate .label');
  await secondCandidate.first().waitFor({ timeout: 15000 });
  await secondCandidate
    .filter({ hasText: wholeLabel(`${COLLECTION} item 2`) })
    .first()
    .click();
  await relationCell
    .locator('mat-chip-row .reference-label')
    .nth(1)
    .filter({ hasText: `${COLLECTION} item 2` })
    .waitFor({ timeout: 15000 });
  await relationCell.locator('button[aria-label="move reference 1 earlier"]').click();
  // The value travels back through the form before the chips follow it, so wait for the moved
  // reference to be first rather than reading the order the click started from.
  await relationCell
    .locator('mat-chip-row .reference-label')
    .first()
    .filter({ hasText: wholeLabel(`${COLLECTION} item 2`) })
    .waitFor({ timeout: 15000 })
    .catch(() => {});
  const moved = (await relationCell.locator('mat-chip-row .reference-label').allTextContents()).map(
    (name) => name.trim(),
  );
  check(
    '参照の順序を画面で入れ替えられる',
    moved[0] === `${COLLECTION} item 2` && moved[1] === referencedTitle,
    moved.join(' / '),
  );
  await relationCell.locator('mat-chip-row').first().locator('button[matChipRemove]').click();
  await page
    .waitForFunction(
      (selector) => document.querySelectorAll(selector).length === 1,
      'mat-chip-row .reference-label',
      { timeout: 10000 },
    )
    .catch(() => {});
  const left = (await relationCell.locator('mat-chip-row .reference-label').allTextContents()).map(
    (name) => name.trim(),
  );
  check('外した参照は残らない', left.length === 1 && left[0] === referencedTitle, left.join(' / '));

  // The picker closes the way it opened, from the button beside it (the panel's own Close is the
  // other way out, and the component's spec is where that one is checked).
  await relationCell.locator('button.relation-add').click();
  await page.locator('app-relation-picker').waitFor({ state: 'detached', timeout: 15000 });

  // The JSON box stays for what the picker does not express: a shape the server refuses, or a
  // value written by a migration.
  await relationCell.locator('button:has-text("Edit as JSON")').click();
  const related = relationCell.locator('textarea[name=related]');
  await related.waitFor({ timeout: 15000 });

  // A reference that names no item is refused before the form is sent, and the input is marked.
  await related.fill(`[{"target":"${COLLECTION}"}]`);
  await save().click();
  await page.waitForTimeout(600);
  const relatedProblem = await page.locator('.field-cell.problem').count();
  const relatedError = ((await page.locator('.error').first().textContent()) ?? '').trim();
  check(
    '参照が不正なら保存前に止まる',
    page.url().includes('/create') && relatedProblem === 1 && relatedError.includes('related[0]'),
    `${page.url()} / ${relatedProblem} / ${relatedError}`,
  );

  // The item the relation points at exists: the lists in the schema editor are the site's.
  await related.fill(`[{"target":"${COLLECTION}","item":1}]`);

  await saveAndWait();
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
  check(
    '生成した slug が正規形で保存される',
    builtItem?.address === 'first-item',
    JSON.stringify(builtItem?.address),
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
  check(
    'リレーションが参照として保存される',
    JSON.stringify(builtItem?.related) === JSON.stringify([{ target: COLLECTION, item: 1 }]),
    JSON.stringify(builtItem?.related),
  );
  // The reference inside the block was saved where it was picked, not flattened or dropped: an
  // element carries the value that sits in it, and one relation is the bare reference.
  const savedBlockRelationship = builtItem?.blocks?.[0]?.values?.author;
  check(
    '複合の中の参照が保存される',
    JSON.stringify(savedBlockRelationship) === JSON.stringify({ target: COLLECTION, item: 1 }),
    JSON.stringify(savedBlockRelationship),
  );
  // And the index sees it: the item that holds it is a referrer of the item it names, which is
  // what stops the target from being deleted out from under a reference nobody counted.
  const referrers = await api(
    'GET',
    `/models/collections/${COLLECTION}/items/1/references`,
    undefined,
    token,
  );
  check(
    '複合の中の参照が逆引きに出る',
    Array.isArray(referrers) &&
      referrers.some((entry) => entry.name === SCHEMA_COLLECTION && entry.item === 1),
    JSON.stringify(referrers),
  );

  // The list says what the reference points at, by the name the target's schema gives it: an id
  // says nothing to an editor reading the row. `referencedTitle`, read before the reference was
  // picked, is that name.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}`, { waitUntil: 'networkidle' });
  await page.locator('table.items tbody tr').first().waitFor({ timeout: 15000 });
  // The name is looked up after the row is drawn, so the fallback (`e2e_blog #1`) is on screen for
  // a moment: wait for the name rather than reading whichever of the two came first.
  await page
    .waitForFunction(
      (expected) =>
        (
          document.querySelectorAll('table.items tbody tr')[0]?.querySelectorAll('td.value')[1]
            ?.textContent ?? ''
        ).trim() === expected,
      referencedTitle,
      { timeout: 10000 },
    )
    .catch(() => {});
  const referenceCell = (
    await page.locator('table.items tbody tr').first().locator('td.value').nth(1).textContent()
  )?.trim();
  check(
    '一覧の参照列は参照先のタイトルで出る',
    referenceCell === referencedTitle,
    `${referenceCell} (expected ${referencedTitle})`,
  );

  // The inverse direction is on the screen that holds what is pointed at: an item says who points
  // at it, grouped under the name the other schema gives the relation (`inverse_name`).
  await page.goto(`${BASE}/collections/${COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('button.references-toggle').click();
  await page.locator('.reference-group h4').first().waitFor({ timeout: 15000 });
  const pointingHeading = (
    (await page.locator('.reference-group h4').first().textContent()) ?? ''
  ).trim();
  const pointingLink = await page.locator('.reference-group a').first().getAttribute('href');
  check(
    '参照元パネルが逆側の呼び名で出る',
    pointingHeading === 'posts' && pointingLink === `/collections/${SCHEMA_COLLECTION}/edit/1`,
    `${pointingHeading} / ${pointingLink}`,
  );

  // The picker names what it offers by the title the target has *now*, which the checks above have
  // been editing, and the chip the form holds reads the same way.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  const heldReference = (
    await relationCell.locator('mat-chip-row .reference-label').first().textContent()
  )?.trim();
  check(
    '編集画面の参照は参照先のタイトルで出る',
    heldReference === referencedTitle,
    `${heldReference} (expected ${referencedTitle})`,
  );

  await relationCell.locator('button.relation-add').click();
  const offeredLabels = relationCell.locator('app-relation-picker .candidate .label');
  await offeredLabels.first().waitFor({ timeout: 15000 });
  const offeredNames = (await offeredLabels.allTextContents()).map((name) => name.trim());
  check(
    'ピッカーは参照先の現在のタイトルで並べる',
    offeredNames.includes(referencedTitle),
    offeredNames.slice(0, 3).join(' / '),
  );
  await relationCell.locator('button.relation-add').click();

  // Opening it again shows what was stored: the round trip through the form, not just the API.
  await page.goto(`${BASE}/collections/${SCHEMA_COLLECTION}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('.composite-element').first().waitFor({ timeout: 15000 });
  // The reference comes back as the item it points at, named the way the target names it, so a
  // reader sees which item it is without knowing an id.
  const savedReference = (
    await relationCell.locator('mat-chip-row .reference-label').first().textContent()
  )?.trim();
  check(
    '保存した参照が名前でフォームに出る',
    savedReference === referencedTitle,
    `${savedReference} (expected ${referencedTitle})`,
  );
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

  await saveAndWait();
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

  // The slug is unique by being one, so a *differently spelled* slug of the same item is refused
  // while the title (a different value) is not the problem.
  await duplicateTitle.fill('a different title');
  await page.locator('app-value-field input[name=address]').fill('First  Item');
  await page.locator('app-value-field input[name=summary]').fill('summary1');
  // This refusal is the point of the step.
  expectConsoleError(/409 \(Conflict\)/);
  await save().click();
  const slugRefusal = await page
    .waitForFunction(
      () => document.querySelector('.error')?.textContent?.includes('address') ?? false,
      null,
      { timeout: 10000 },
    )
    .then(() => true)
    .catch(() => false);
  check('綴り違いの slug は同じ住所として拒否される', slugRefusal);
  check(
    '拒否された slug の欄が強調される',
    (await page.locator('.field-cell.problem').count()) === 1,
    `${await page.locator('.field-cell.problem').count()} 件`,
  );

  // The schema's limits are the content editor's business too, not just the server's: the input
  // carries them, the hint states them, and a value outside them stops the save before it is
  // sent (the minimum cannot be enforced by the browser, which is the case this covers).
  const summary = page.locator('app-value-field input[name=summary]');
  await summary.waitFor({ timeout: 15000 });
  // The unique field says so on the form: only the server can check it, but the reader is told
  // before they find out from a refusal.
  check(
    '一意なフィールドだと画面に出る',
    // The title, which asked to be unique, and the slug, which is unique by being one.
    (await page.locator('app-value-field .unique-mark').count()) === 2,
    `${await page.locator('app-value-field .unique-mark').count()} 件`,
  );
  check(
    'スキーマの文字数が入力欄に効く',
    (await summary.getAttribute('maxlength')) === '8',
    'maxlength',
  );
  check(
    'スキーマの文字数がヒントに出る',
    ((await summary.locator('xpath=ancestor::mat-form-field').textContent()) ?? '').includes(
      '5–8 characters',
    ),
  );

  await summary.fill('ab');
  await save().click();
  await page.waitForTimeout(500);
  const stillCreating = page.url().includes('/create');
  const summaryProblem = await page.locator('.field-cell.problem').count();
  check(
    '短すぎる値は保存前に止まる',
    stillCreating && summaryProblem === 1,
    `${stillCreating} / ${summaryProblem}`,
  );

  // A value inside the limits is accepted again, so only the duplicate is left to refuse.
  await summary.fill('summary1');
  // ...and the slug collision above is cleared, so the refusal is about the title.
  await page.locator('app-value-field input[name=address]').fill('');
  await duplicateTitle.fill('first item');
  // The refusal is the point of this step, so its 409 is expected.
  expectConsoleError(/409 \(Conflict\)/);
  await save().click();
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

  // Duplicating opens the copy, with the unique field emptied. The schema editor's collection is
  // the one whose `title` is unique, so it is the one where clearing it shows.
  // The refused form still holds edits, so it is left through its own Cancel (the guard asks, and
  // the page's dialog handler accepts).
  await page.click('button:has-text("Back to the list without saving")');
  await page
    .waitForURL(`${BASE}/collections/${SCHEMA_COLLECTION}`, { timeout: 15000 })
    .catch(() => {});
  const rowsReady = await dataRows()
    .first()
    .waitFor({ timeout: 15000 })
    .then(() => true)
    .catch(() => false);
  check('一覧に項目が並ぶ', rowsReady, page.url());
  const beforeCopy = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/items`,
    undefined,
    token,
  );
  await page.locator('button[aria-label^="more actions"]').first().click();
  await page.getByRole('menuitem', { name: 'Copy item' }).click();
  await page
    .waitForURL(`**/collections/${SCHEMA_COLLECTION}/edit/**`, { timeout: 15000 })
    .catch(() => {});
  check('複製すると編集画面が開く', page.url().includes('/edit/'), page.url());
  const copyId = Number(page.url().split('/').pop());
  const copy = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/items/${copyId}`,
    undefined,
    token,
  );
  check('複製では一意なフィールドが空になる', copy.title === '', JSON.stringify(copy.title));
  check(
    '複製では他の値が引き継がれる',
    typeof copy.summary === 'string',
    JSON.stringify(copy.summary),
  );

  // Saving it with a title of its own is the point of clearing the field.
  await page.locator('app-value-field input').first().fill('a copied item');
  await saveAndWait();
  // Saving stays on the item now; the list below is the toolbar's own button, which is also what
  // the batch checks that follow are looking at.
  check('保存しても編集画面にとどまる', page.url().includes('/edit/'), page.url());
  await page.getByRole('button', { name: 'Back to the list without saving', exact: true }).click();
  await page
    .waitForURL(`${BASE}/collections/${SCHEMA_COLLECTION}`, { timeout: 15000 })
    .catch(() => {});
  const afterCopy = await api(
    'GET',
    `/models/collections/${SCHEMA_COLLECTION}/items`,
    undefined,
    token,
  );
  check(
    '複製が 1 件増える',
    afterCopy.length === beforeCopy.length + 1,
    `${beforeCopy.length} → ${afterCopy.length}`,
  );

  // A batch publishes what is selected, and says how many it changed.
  const publishedCount = async () =>
    Object.values(
      await api('GET', `/models/collections/${SCHEMA_COLLECTION}/items/metadata`, undefined, token),
    ).filter((metadata) => metadata.status === 'published').length;
  const publishedBefore = await publishedCount();
  await page.locator('thead input[type=checkbox]').first().check();
  await page.waitForTimeout(200);
  check(
    '行をまとめて選べる',
    (await page.locator('.toolbar .selection').count()) === 1,
    `${await page.locator('tbody input[type=checkbox]:checked').count()} 件`,
  );
  await page.getByRole('button', { name: 'Publish selected', exact: true }).click();
  await page.waitForTimeout(800);
  check(
    '選択した記事をまとめて公開できる',
    (await publishedCount()) === afterCopy.length,
    `${publishedBefore} → ${await publishedCount()}`,
  );
  check(
    '一括の結果が画面に出る',
    (await page.locator('.notice').filter({ hasText: 'changed' }).count()) === 1,
  );

  // And takes them off the site again.
  await page.locator('thead input[type=checkbox]').first().check();
  await page.waitForTimeout(200);
  await page.getByRole('button', { name: 'Unpublish selected', exact: true }).click();
  await page.waitForTimeout(800);
  check(
    '選択した記事をまとめて非公開にできる',
    (await publishedCount()) === 0,
    `${await publishedCount()} 件`,
  );

  // Through the API rather than the screen: this collection exists only for this scenario.
  await deleteIfPresent(`/models/collections/${SCHEMA_COLLECTION}`, token);
}

/** Single pages: switching follows the URL. */
async function singlePagesSwitchingFollowsTheURL() {
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
      (expected) => document.querySelector('app-value-field input[name=title]')?.value === expected,
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
  await save().click();
  await page.waitForTimeout(800);
  const stayedOnPage = page.url().includes(`/single-pages/${pageB}`);
  const savedNotice = (
    (await page.locator('.notice-toast .text').first().textContent()) ?? ''
  ).trim();
  const badgeAppeared = await page.locator('app-item-status .badge').count();
  check(
    '単一ページの保存後も画面に留まる',
    stayedOnPage && savedNotice.length > 0 && badgeAppeared === 1,
    `${stayedOnPage} / ${savedNotice} / ${badgeAppeared}`,
  );

  await page.click('button:has-text("Save and publish")');
  const published = await page
    .waitForFunction(
      () =>
        document.querySelector('app-item-status .badge')?.textContent?.includes('Published') ??
        false,
      null,
      { timeout: 10000 },
    )
    .then(() => true)
    .catch(() => false);
  check(
    '保存して公開が 1 クリックでできる',
    published,
    await page.locator('app-item-status .badge').first().textContent(),
  );

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
  await api('POST', `/models/single_pages/${pageA}/publish`, undefined, token).catch(() => {});
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
}

/** An administrator hands out a password reset link. */
async function anAdministratorHandsOutAPasswordResetLink() {
  const resetUsername = `e2e-reset-${Date.now()}`;
  for (const account of await api('GET', '/auth/users', undefined, token)) {
    if (account.username.startsWith('e2e-reset-')) {
      await api('DELETE', `/auth/users/${account.id}`, undefined, token);
    }
  }
  await createAccount(resetUsername, 'reset-password', {}, token);

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
  check(
    'リセット後はサインイン状態になる',
    resetPage.url().startsWith(`${BASE}/`),
    resetPage.url(),
  );

  // The new password works, the session from before is gone, and the link is spent.
  const afterReset = await request.fetch(`${API}/auth/login`, {
    method: 'POST',
    data: { username: resetUsername, password: 'chosen-by-the-owner' },
  });
  check(
    '新しいパスワードでサインインできる',
    afterReset.status() === 200,
    `status=${afterReset.status()}`,
  );
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
}

/** Guessing a password is not free. */
async function guessingAPasswordIsNotFree() {
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
  await createAccount(throttleEmail, 'throttle-password', {}, token);

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
  check(
    '他のアカウントは影響を受けない',
    otherAccount.status() === 200,
    `status=${otherAccount.status()}`,
  );
}

/** Writing Markdown without knowing Markdown, and box heights. */
async function writingMarkdownWithoutKnowingMarkdownAndBoxHeights() {
  // A collection of its own, so the checks are about the widgets and not about the data the rest of
  // the suite drives.
  const WRITING = 'e2e_writing';
  await deleteIfPresent(`/models/collections/${WRITING}`, token);
  await api(
    'POST',
    `/models/collections/${WRITING}/schema`,
    [
      { name: 'title', field_type: { Text: {} }, required: false, width: 12, height: 1 },
      {
        name: 'lede',
        field_type: { Text: { multiline: true } },
        required: false,
        width: 12,
        height: 3,
      },
      { name: 'body', field_type: { Markdown: {} }, required: false, width: 12, height: 5 },
    ],
    token,
  );
  await api(
    'POST',
    `/models/collections/${WRITING}/item`,
    { title: 'Writing', lede: 'first', body: 'plain text' },
    token,
  );

  await page.goto(`${BASE}/collections/${WRITING}/edit/1`, { waitUntil: 'networkidle' });
  await page.locator('app-value-field textarea').first().waitFor({ timeout: 15000 });

  const sizes = await page.evaluate(() =>
    Array.from(document.querySelectorAll('.field-grid .field-cell')).map((cell) => {
      const widget = cell.querySelector('input, textarea');
      return {
        label: cell.querySelector('.field-label')?.textContent?.trim().split('\n')[0] ?? '?',
        kind: widget?.tagName.toLowerCase() ?? 'none',
        rows: widget?.getAttribute('rows') ?? null,
      };
    }),
  );
  check(
    '高さが入力欄の行数になる',
    sizes[1]?.kind === 'textarea' &&
      sizes[1]?.rows === '9' &&
      sizes[2]?.kind === 'textarea' &&
      sizes[2]?.rows === '15' &&
      sizes[0]?.kind === 'input',
    JSON.stringify(sizes.map((size) => `${size.label}:${size.kind}:${size.rows}`)),
  );

  // The text field with "several lines" is a box; the one without is still a line.
  check(
    '複数行のテキストだけが箱になる',
    sizes[0]?.kind === 'input' && sizes[1]?.kind === 'textarea',
    `${sizes[0]?.kind} / ${sizes[1]?.kind}`,
  );

  // The Markdown buttons write the syntax for an editor who does not know it.
  const body = page.locator('app-value-field textarea').nth(1);
  await body.click();
  await body.press('Control+A');
  await body.pressSequentially('the docs');
  await body.evaluate((element) => element.setSelectionRange(0, 8));
  await page.locator('.markdown-toolbar button[aria-label="Bold"]').click();
  // The value travels through the screen's own state, so the box is read after it has been written.
  await page.waitForTimeout(300);
  const bolded = await body.inputValue();
  check('太字ボタンが選択範囲を囲む', bolded === '**the docs**', JSON.stringify(bolded));

  dialogText = 'https://example.test/';
  await body.evaluate((element) => element.setSelectionRange(0, element.value.length));
  await page.locator('.markdown-toolbar button[aria-label="Link"]').click();
  await page.waitForTimeout(300);
  dialogText = undefined;
  const linked = await body.inputValue();
  check(
    'リンクボタンが URL を聞いて貼る',
    linked === '[**the docs**](https://example.test/)',
    JSON.stringify(linked),
  );

  // And an image goes in from the same library the image fields use. One is put there first: by
  // this point in the suite the library is whatever the earlier checks left behind.
  const bytes = Buffer.from(PNG_BASE64, 'base64');
  const upload = await api(
    'POST',
    '/models/images/get_upload_url',
    { original_filename: 'writing.png', ext: 'png', size: bytes.length },
    token,
  );
  await request.fetch(`${BASE}${upload.upload_url}`, {
    method: 'PUT',
    headers: { Authorization: `Bearer ${token}` },
    data: bytes,
  });

  await body.evaluate((element) =>
    element.setSelectionRange(element.value.length, element.value.length),
  );
  await page.locator('.markdown-toolbar button[aria-label="Image"]').click();
  const firstThumb = page.locator('.library .thumb').first();
  await firstThumb.waitFor({ timeout: 15000 });
  await firstThumb.click();
  await page.waitForTimeout(300);
  const withImage = await body.inputValue();
  check(
    '画像ボタンがライブラリから差し込む',
    /!\[[^\]]+\]\(http[^)]*\/api\/images\/by-id\/\d+\)$/.test(withImage),
    withImage.slice(-60),
  );

  await save().click();
  await page.waitForTimeout(600);
  const written = await api('GET', `/models/collections/${WRITING}/items/1`, undefined, token);
  check(
    '書式つきの Markdown がそのまま保存される',
    String(written.body).includes('https://example.test/') &&
      String(written.body).includes('/api/images/by-id/'),
    String(written.body).slice(0, 80),
  );

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
}

/** The scenarios, in the order they run. */
const SCENARIOS = [
  ['the tab icon, on either tab strip', theTabIconReadsOnAnyTabStrip],
  ['the language switch', theLanguageSwitch],
  ['the deployment names itself', theDeploymentNamesItself],
  ['sign in through the form', signInThroughTheForm],
  ['the deployment names itself in the shell', theDeploymentNamesItselfInTheShell],
  ['a plain array, edited as JSON', aPlainArrayEditedAsJSON],
  ['first page', firstPage],
  ['status and updated columns', statusAndUpdatedColumns],
  ['next page', nextPage],
  ['page size', pageSize],
  ['ordering from the headers', orderingFromTheHeaders],
  ['publish a draft from the list', publishADraftFromTheList],
  ['opening an item from the list', openingAnItemFromTheList],
  ["delete the last page's only row", deleteTheLastPagesOnlyRow],
  ['the image library', theImageLibrary],
  ['pick images while editing', pickImagesWhileEditing],
  ['saving is not publishing', savingIsNotPublishing],
  ['an image array inside a composite', anImageArrayInsideAComposite],
  ['delete them again', deleteThemAgain],
  ["preview links are a schema's choice", previewLinksAreASchemasChoice],
  ['a link shows unpublished work to a guest', aLinkShowsUnpublishedWorkToAGuest],
  ['what is live, and taking changes back', whatIsLiveAndTakingChangesBack],
  ['roles decide what is offered', rolesDecideWhatIsOffered],
  ['a password change ends the old sessions', aPasswordChangeEndsTheOldSessions],
  ['per-resource permissions, granted from the screen', perResourcePermissionsGrantedFromTheScreen],
  ['the schema editor, driven from the screen', theSchemaEditorDrivenFromTheScreen],
  ['single pages: switching follows the URL', singlePagesSwitchingFollowsTheURL],
  ['an administrator hands out a password reset link', anAdministratorHandsOutAPasswordResetLink],
  ['guessing a password is not free', guessingAPasswordIsNotFree],
  [
    'writing Markdown without knowing Markdown, and box heights',
    writingMarkdownWithoutKnowingMarkdownAndBoxHeights,
  ],
];

try {
  for (const [name, run] of SCENARIOS) {
    scenario = name;
    // A heading per scenario, so a long run in a log is navigable.
    console.log(`\n== ${name} ==`);
    await run();
  }
} catch (error) {
  check('検証スクリプトが最後まで走る', false, String(error).split('\n')[0]);
  // Where it stopped, since a locator timeout says what was not found and not what was on screen.
  // Diagnosing a wrong navigation this way took one run instead of three.
  console.log('failure context:');
  console.log(`  scenario: ${scenario}`);
  console.log(`  url: ${page.url()}`);
  const visible = await page
    .locator('body')
    .textContent()
    .catch(() => null);
  console.log(`  screen: ${(visible ?? '(unreadable)').replace(/\s+/g, ' ').slice(0, 400)}`);
} finally {
  await browser.close();
}

const failed = results.filter((result) => !result.ok);
if (failed.length > 0) {
  // Named once more at the end: a run in a log is read from the bottom.
  console.log('\n失敗したチェック:');
  for (const result of failed) {
    console.log(`  [${result.scenario}] ${result.label}`);
  }
}
console.log(`\n${results.length - failed.length}/${results.length} 件のチェックに成功`);
process.exit(failed.length === 0 ? 0 : 1);
