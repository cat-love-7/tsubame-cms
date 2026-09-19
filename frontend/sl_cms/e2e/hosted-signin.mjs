/**
 * Sign in to a deployed CMS through its identity provider, with a real browser.
 *
 * The browser end-to-end suite (`check-ui.mjs`) drives the *local* password form and
 * `/api/auth/login`, neither of which exists on a deployment where Cognito signs people in -
 * the deployment answers 501 for the endpoint and shows a button to the hosted page instead. So
 * this is the one thing that suite cannot cover, and the one thing a deployment can get wrong
 * while every `curl` in `scripts/smoke-test.sh` still passes: the PKCE handoff, the registered
 * callback URL, the code exchange at `/api/auth/callback`, and the token the app keeps.
 *
 * Usage (from `frontend/sl_cms`, where playwright is installed):
 *
 *   APP_URL=https://cms.example.com \
 *   ADMIN_USERNAME=cat ADMIN_PASSWORD=... node e2e/hosted-signin.mjs
 *
 * Prerequisites are the same as the other suite's: a browser for Playwright, and
 * `PLAYWRIGHT_BROWSERS_PATH` if it is installed outside the default cache.
 */
import { chromium } from 'playwright';

const APP = process.env.APP_URL;
const USERNAME = process.env.ADMIN_USERNAME;
const PASSWORD = process.env.ADMIN_PASSWORD;

if (!APP || !USERNAME || !PASSWORD) {
  console.error('APP_URL, ADMIN_USERNAME and ADMIN_PASSWORD are all required');
  process.exit(2);
}

const problems = [];
const browser = await chromium.launch();
const page = await browser.newPage();

page.on('console', (message) => {
  if (message.type() === 'error') problems.push(`console: ${message.text()}`);
});
page.on('pageerror', (error) => problems.push(`pageerror: ${error.message}`));
page.on('response', (response) => {
  if (response.status() >= 500) problems.push(`http ${response.status()} ${response.url()}`);
});

try {
  // The app's own sign-in screen: no password box where the provider has the password, only the
  // button that starts the PKCE flow.
  await page.goto(`${APP}/login`, { waitUntil: 'networkidle' });
  console.log(`1. the sign-in screen is at ${page.url()}`);
  const button = page.locator('button.full-width');
  if ((await button.count()) === 0) {
    throw new Error('no "sign in at the provider" button on the login screen');
  }

  await button.first().click();
  await page.waitForURL(/amazoncognito\.com/, { timeout: 30_000 });
  const hosted = new URL(page.url());
  console.log(`2. the provider's page is at ${hosted.host}${hosted.pathname}`);
  if (!hosted.searchParams.get('code_challenge')) {
    throw new Error('the sign-in URL carries no PKCE challenge');
  }
  if (hosted.searchParams.get('redirect_uri') !== `${APP}/auth/callback`) {
    throw new Error(`the sign-in URL asks to return to ${hosted.searchParams.get('redirect_uri')}`);
  }
  console.log(`   it asks to return to ${hosted.searchParams.get('redirect_uri')}`);

  // The hosted page carries two forms - one for wide screens and one for narrow, only one of them
  // visible - so every locator here is the visible one.
  await page.locator('input[name=username]:visible').first().fill(USERNAME);
  await page.locator('input[name=password]:visible').first().fill(PASSWORD);
  for (const submit of [
    'button[type=submit]:visible',
    'input[name=signInSubmitButton]:visible',
    'input[type=submit]:visible',
  ]) {
    if ((await page.locator(submit).count()) > 0) {
      await Promise.all([
        page.waitForURL((url) => url.href.startsWith(APP), { timeout: 60_000 }),
        page.locator(submit).first().click(),
      ]);
      break;
    }
  }
  console.log(`3. back from the provider at ${page.url().split('?')[0]}`);

  await page.waitForFunction(() => !location.pathname.startsWith('/login'), null, {
    timeout: 30_000,
  });
  await page.waitForLoadState('networkidle');
  console.log(`4. signed in, the app is at ${page.url()}`);

  // The token has to be the CMS's own: the browser is only signed in if the CMS accepted the code
  // the provider handed back and wrote a session of its own.
  const stored = await page.evaluate(() => ({
    token: window.localStorage.getItem('sl_cms.token'),
    keys: Object.keys(window.localStorage),
  }));
  console.log(`   storage holds: ${stored.keys.join(', ') || '(nothing)'}`);
  if (!stored.token) {
    throw new Error('the CMS stored no token: the code exchange did not complete');
  }

  const body = (await page.textContent('body')) ?? '';
  const text = body.replace(/\s+/g, ' ').trim();
  console.log(`   the screen says: ${text.slice(0, 160)}`);
  if (/not provisioned|has not been given access/i.test(text)) {
    throw new Error('the account signed in but the CMS did not accept it as an administrator');
  }

  if (problems.length) {
    throw new Error(`the page reported problems: ${problems.join(' | ')}`);
  }
  console.log('signed in, with no console errors');
} finally {
  await browser.close();
}
