/**
 * The account screen against a deployment whose identity provider owns the password.
 *
 * The local suite cannot reach any of this: there is no Cognito emulator, so the provisioner's
 * calls are *read* there and only the routes around them are driven. Here they are run for real -
 * create an account, reset it, sign in with the temporary password the reset produced (the provider
 * makes the person change it), and remove it again - which is what makes "an administrator manages
 * accounts" more than a claim about code.
 *
 * Every sign-in gets its own browser context: the provider keeps a session cookie of its own, so
 * signing in as a second account in the same context would only re-use the first one's session.
 *
 * Usage (from `frontend`, where playwright is installed):
 *
 *   APP_URL=https://cms.example.com \
 *   ADMIN_USERNAME=cat ADMIN_PASSWORD=... node e2e/hosted-accounts.mjs
 *
 * It leaves nothing behind: the account it makes is deleted at the end, and also when a check
 * fails, so a broken run does not leave a user in the pool.
 */
import { chromium } from 'playwright';

import { collectProblems, signIn } from './hosted-login.mjs';

const APP = process.env.APP_URL;
const USERNAME = process.env.ADMIN_USERNAME;
const PASSWORD = process.env.ADMIN_PASSWORD;
/** A name no one else will have, so a leftover from an interrupted run cannot collide with it. */
const ACCOUNT = process.env.NEW_ACCOUNT ?? `e2e-account-${Date.now()}`;
/** What the account's owner picks when the provider asks, after the temporary one. */
const CHOSEN = 'Chosen-by-its-owner-1!';

if (!APP || !USERNAME || !PASSWORD) {
  console.error('APP_URL, ADMIN_USERNAME and ADMIN_PASSWORD are all required');
  process.exit(2);
}

const browser = await chromium.launch();
const contexts = [];
const problems = [];

/** A browser of its own: no provider session, no session storage, nothing left over. */
async function freshPage(watch = true) {
  const context = await browser.newContext();
  contexts.push(context);
  const page = await context.newPage();
  if (watch) {
    collectProblems(page, problems);
  }
  return page;
}

/** Call the API the way the account screen does: from the page, with the token it holds. */
async function api(page, method, path, body, token) {
  return page.evaluate(
    async ({ method, path, body, token }) => {
      const response = await fetch(path, {
        method,
        headers: {
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
          ...(body ? { 'Content-Type': 'application/json' } : {}),
        },
        body: body ? JSON.stringify(body) : undefined,
      });
      const text = await response.text();
      return { status: response.status, body: text ? JSON.parse(text) : null };
    },
    { method, path, body, token },
  );
}

const token = (page) => page.evaluate(() => localStorage.getItem('tsubame.token'));

let created = null;
try {
  const admin = await freshPage();
  console.log(`1. signing in as ${USERNAME}`);
  await signIn(admin, { app: APP, username: USERNAME, password: PASSWORD });

  console.log(`2. creating the account ${ACCOUNT} from the account screen's own endpoint`);
  const created_response = await api(
    admin,
    'POST',
    '/api/auth/users',
    {
      username: ACCOUNT,
      email: null,
      is_admin: false,
      permission: { can_view: true, can_edit: false, can_publish: false },
    },
    await token(admin),
  );
  if (created_response.status !== 201) {
    throw new Error(
      `creating an account answered ${created_response.status}: ${JSON.stringify(created_response.body)}`,
    );
  }
  created = created_response.body;

  // The screen itself, not only the endpoint: the row is there, and its reset control is the one a
  // deployment whose provider holds the password shows - a `key` (a value to hand over) rather
  // than a `link`.
  console.log('   the account screen lists it, with a reset control for a temporary password');
  await admin.goto(`${APP}/settings/users`, { waitUntil: 'networkidle' });
  const row = admin.locator('tbody tr', { hasText: ACCOUNT });
  await row.waitFor({ timeout: 15_000 });
  if ((await row.locator('mat-icon', { hasText: 'key' }).count()) === 0) {
    throw new Error(`no "key" reset control in the row for ${ACCOUNT}`);
  }

  console.log('3. resetting it, which is where the provider sets a temporary password');
  const reset = await api(
    admin,
    'POST',
    `/api/auth/users/${created.id}/password-reset`,
    undefined,
    await token(admin),
  );
  if (reset.status !== 200) {
    throw new Error(`the reset answered ${reset.status}: ${JSON.stringify(reset.body)}`);
  }
  if (reset.body.kind !== 'temporary') {
    throw new Error(
      `this deployment should reset with a temporary password, not ${reset.body.kind}`,
    );
  }
  const temporary = reset.body.password;
  console.log(`   the pool set one ${temporary.length} characters long for it to change`);

  console.log(`4. signing in as ${ACCOUNT} with that temporary password`);
  const member = await freshPage();
  await signIn(member, {
    app: APP,
    username: ACCOUNT,
    password: temporary,
    newPassword: CHOSEN,
  });
  // The account was created here, so the CMS knows it: a token from the pool resolves to that
  // record by the `sub` the create stored. Without it this is where `not_provisioned` appears.
  const body = ((await member.textContent('body')) ?? '').replace(/\s+/g, ' ').trim();
  if (/not provisioned|has not been given access/i.test(body)) {
    throw new Error('the account signed in but the CMS did not have a record for it');
  }
  if (!(await token(member))) {
    throw new Error('the CMS stored no token for the new account');
  }
  console.log(`   it is inside the app, at ${member.url()}`);

  console.log('5. removing it again, as the administrator');
  const removed = await api(
    admin,
    'DELETE',
    `/api/auth/users/${created.id}`,
    undefined,
    await token(admin),
  );
  if (removed.status !== 200) {
    throw new Error(`deleting the account answered ${removed.status}`);
  }
  created = null;

  console.log('6. and it can no longer sign in, because the pool no longer has it');
  // Not watched for console errors: this page is *supposed* to be refused, and the app asking who
  // it is without a session is a 401 the browser logs as one.
  const probe = await freshPage(false);
  await probe.goto(`${APP}/login`, { waitUntil: 'networkidle' });
  await probe.locator('button.full-width').first().click();
  await probe.waitForURL(/amazoncognito\.com/, { timeout: 30_000 });
  await probe.locator('input[name=username]:visible').first().fill(ACCOUNT);
  await probe.locator('input[name=password]:visible').first().fill(CHOSEN);
  await probe
    .locator(
      'button[type=submit]:visible, input[name=signInSubmitButton]:visible, input[type=submit]:visible',
    )
    .first()
    .click();
  await probe.waitForTimeout(2000);
  if (!probe.url().includes('amazoncognito.com')) {
    throw new Error(`a removed account was still signed in: ${probe.url()}`);
  }
  console.log('   the provider refused it, as it should');

  if (problems.length) {
    throw new Error(`the page reported problems: ${problems.join(' | ')}`);
  }
  console.log('account management works end to end, with no console errors');
} finally {
  // Best effort: a failed run must not leave an account in the pool.
  if (created) {
    try {
      const page = contexts[0];
      const current = await token(page);
      if (current) {
        await api(page, 'DELETE', `/api/auth/users/${created.id}`, undefined, current);
      }
    } catch {
      // Nothing useful to add: the failure above is the one worth reporting.
    }
  }
  for (const context of contexts) {
    await context.close();
  }
  await browser.close();
}
