/**
 * Sign in to a deployed CMS through its identity provider, with a real browser.
 *
 * The browser end-to-end suite (`check-ui.mjs`) drives the *local* password form and
 * `/api/auth/login`, neither of which exists on a deployment where Cognito signs people in - the
 * deployment answers 501 for the endpoint and shows a button to the hosted page instead. So this
 * is the one thing that suite cannot cover, and the one thing a deployment can get wrong while
 * every `curl` in `scripts/smoke-test.sh` still passes: the PKCE handoff, the registered callback
 * URL, the code exchange at `/api/auth/callback`, and the token the app keeps.
 *
 * `hosted-accounts.mjs` goes one step further and manages an account, which needs this first.
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

import { collectProblems, signIn } from './hosted-login.mjs';

const APP = process.env.APP_URL;
const USERNAME = process.env.ADMIN_USERNAME;
const PASSWORD = process.env.ADMIN_PASSWORD;

if (!APP || !USERNAME || !PASSWORD) {
  console.error('APP_URL, ADMIN_USERNAME and ADMIN_PASSWORD are all required');
  process.exit(2);
}

const browser = await chromium.launch();
const page = await browser.newPage();
const problems = collectProblems(page);

try {
  console.log(`1. signing in as ${USERNAME} at ${APP}`);
  const hosted = await signIn(page, { app: APP, username: USERNAME, password: PASSWORD });
  console.log(`2. the provider's page was at ${hosted.host}${hosted.pathname}`);
  console.log(`   it asked to return to ${hosted.searchParams.get('redirect_uri')}`);

  // The token has to be the CMS's own: the browser is only signed in if the CMS accepted the code
  // the provider handed back and wrote a session of its own.
  const stored = await page.evaluate(() => ({
    token: window.localStorage.getItem('sl_cms.token'),
    keys: Object.keys(window.localStorage),
  }));
  console.log(
    `3. the app is at ${page.url()}, storage holds ${stored.keys.join(', ') || '(nothing)'}`,
  );
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
