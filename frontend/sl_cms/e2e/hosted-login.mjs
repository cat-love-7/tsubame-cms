/**
 * Signing in to a deployment through its identity provider, for the staging checks.
 *
 * The hosted page is not ours to shape, and it is where these checks meet the real thing: two
 * forms (one for wide screens and one for narrow), the PKCE handoff the app builds, and - the
 * first time an account is used - a step that makes the person choose a new password, which is
 * exactly what an administrator-issued reset produces.
 *
 * `hosted-signin.mjs` and `hosted-accounts.mjs` both start here.
 */

/** The submit buttons the hosted page has had, newest first; the first one present wins. */
const SUBMITS = [
  'button[type=submit]:visible',
  'input[name=signInSubmitButton]:visible',
  'input[type=submit]:visible',
];

/** Everything the browser reported while the checks ran. */
export function collectProblems(page, problems = []) {
  page.on('console', (message) => {
    if (message.type() === 'error') problems.push(`console: ${message.text()}`);
  });
  page.on('pageerror', (error) => problems.push(`pageerror: ${error.message}`));
  page.on('response', (response) => {
    if (response.status() >= 500) problems.push(`http ${response.status()} ${response.url()}`);
  });
  return problems;
}

async function submit(page, app) {
  for (const selector of SUBMITS) {
    if ((await page.locator(selector).count()) > 0) {
      // Back to the app, or still at the provider (a step left to do): either way the click has
      // happened, so the caller decides what the page is now.
      await Promise.all([
        page.waitForURL((url) => url.href.startsWith(app), { timeout: 60_000 }).catch(() => {}),
        page.locator(selector).first().click(),
      ]);
      return;
    }
  }
  throw new Error(`no submit button on the provider's page (${page.url()})`);
}

/**
 * Sign in and land inside the app.
 *
 * `newPassword` is what to choose if the provider asks for one - which it does when the password
 * handed to this account was temporary, as a reset produces. Returns the address the app sent the
 * browser to, so a caller can report the PKCE handoff it came with.
 */
export async function signIn(page, { app, username, password, newPassword }) {
  await page.goto(`${app}/login`, { waitUntil: 'networkidle' });
  const start = page.locator('button.full-width');
  if ((await start.count()) === 0) {
    throw new Error('no "sign in at the provider" button on the login screen');
  }
  await start.first().click();
  await page.waitForURL(/amazoncognito\.com/, { timeout: 30_000 });

  const hosted = new URL(page.url());
  if (!hosted.searchParams.get('code_challenge')) {
    throw new Error('the sign-in URL carries no PKCE challenge');
  }
  if (hosted.searchParams.get('redirect_uri') !== `${app}/auth/callback`) {
    throw new Error(`the sign-in URL asks to return to ${hosted.searchParams.get('redirect_uri')}`);
  }

  // The hosted page carries two forms, and only one of them is visible.
  await page.locator('input[name=username]:visible').first().fill(username);
  await page.locator('input[name=password]:visible').first().fill(password);
  await submit(page, app);

  if (page.url().includes('amazoncognito.com')) {
    if (!newPassword) {
      throw new Error(`the provider is asking for a new password: ${page.url()}`);
    }
    // Whatever the fields are called (`password`, `confirm_password`, …), they are the visible
    // password boxes, and the provider wants both of them filled the same way.
    const boxes = await page.locator('input[type=password]:visible').all();
    if (boxes.length === 0) {
      throw new Error(`the provider wants a new password but shows no field: ${page.url()}`);
    }
    for (const box of boxes) {
      await box.fill(newPassword);
    }
    await submit(page, app);
  }

  await page.waitForURL((url) => url.href.startsWith(app), { timeout: 60_000 });
  // Arriving at `/auth/callback` is not being signed in: the app exchanges the code there, which
  // is a request of its own. Waiting only for "not /login" reads the session before that request
  // has answered - a race this check lost once - so it waits for the app to move *past* both.
  await page.waitForURL(
    (url) => url.href.startsWith(app) && !/\/(login|auth\/callback)/.test(new URL(url).pathname),
    { timeout: 60_000 },
  );
  await page.waitForLoadState('networkidle');
  return hosted;
}
