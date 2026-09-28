/**
 * The screenshots the README shows.
 *
 * It puts the sample site (`e2e/sample-site.mjs`) into an empty CMS - driving the interface to
 * create content is the browser suite's job, not this one's - then takes one picture per screen
 * with a real browser over the result. `scripts/screenshots.sh` boots a server with throwaway data
 * first, so this can be run again whenever the interface changes.
 *
 * Usage: scripts/screenshots.sh
 *        BASE_URL=http://localhost:4200 node e2e/screenshots.mjs   (servers you started yourself)
 */
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from 'playwright';

import { seedSampleSite } from './sample-site.mjs';

const BASE = process.env.BASE_URL ?? 'http://localhost:4200';
const USERNAME = process.env.ADMIN_USERNAME ?? 'admin@example.com';
const PASSWORD = process.env.ADMIN_PASSWORD ?? 'admin-password';
const OUT = resolve(dirname(fileURLToPath(import.meta.url)), '../../docs/images');

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
    const { featured } = await seedSampleSite({
      base: BASE,
      username: USERNAME,
      password: PASSWORD,
      log: (line) => console.log(`  ${line}`),
    });

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
