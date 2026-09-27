/**
 * Regenerate `public/favicon.ico` from `public/favicon.svg`.
 *
 * The SVG is the tab icon for the browsers that read one, and this is the fallback for the ones that
 * predate it: the two are the same picture, so the icon is rasterised from the file a browser loads
 * rather than drawn a second time here. What is in that file - the brand mark on a plate of the
 * brand's ink, because a tab strip is the browser's and cannot be asked for its colour - is the SVG's
 * business and is documented there. The mark's path in it is a copy of `brand/tsubame-16.svg`, since
 * an icon cannot inherit `currentColor`: a redrawn bird is a change to that copy, and then this
 * runs again.
 *
 * The same Chromium the browser suite uses rasterises the three sizes a tab and a bookmark use, and
 * they are packed into one `.ico`.
 *
 * Usage: cd frontend && node e2e/favicon.mjs
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from 'playwright';

const HERE = dirname(fileURLToPath(import.meta.url));
const SVG = resolve(HERE, '../public/favicon.svg');
const OUT = resolve(HERE, '../public/favicon.ico');
const SIZES = [16, 32, 48];

/**
 * One `.ico` holding PNGs.
 *
 * The format is a header, one 16-byte directory entry per image, then the images: a PNG inside an
 * `.ico` is what every browser since Vista reads, and it is why the sizes can be packed without
 * inventing a bitmap-with-mask encoder.
 */
function ico(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // 1 = icon
  header.writeUInt16LE(images.length, 4);

  let offset = 6 + images.length * 16;
  const entries = images.map(({ size, png }) => {
    const entry = Buffer.alloc(16);
    entry[0] = size >= 256 ? 0 : size;
    entry[1] = size >= 256 ? 0 : size;
    entry[2] = 0; // palette
    entry[3] = 0; // reserved
    entry.writeUInt16LE(1, 4); // colour planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(png.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += png.length;
    return entry;
  });

  return Buffer.concat([header, ...entries, ...images.map((image) => image.png)]);
}

/**
 * The tab icon at one size, as the element to rasterise.
 *
 * The file's own comment is dropped and the size stated, because an icon is rasterised at the sizes
 * a tab and a bookmark ask for rather than at the 16px the SVG says.
 */
function icon(size) {
  return readFileSync(SVG, 'utf8')
    .replace(/^<!--[\s\S]*?-->\s*/, '')
    .replace('width="16" height="16"', `width="${size}" height="${size}"`);
}

async function main() {
  const browser = await chromium.launch({ args: ['--no-sandbox'] });
  const images = [];
  try {
    const page = await browser.newPage({ viewport: { width: 48, height: 48 } });
    for (const size of SIZES) {
      await page.setContent(
        `<html><body style="margin:0;background:transparent">${icon(size)}</body></html>`,
      );
      await page.locator('svg').waitFor();
      // `omitBackground` keeps the corners transparent: an `.ico` with a white box around the
      // swallow is what one of these looked like the first time. Around the plate's radius is the
      // page, which is what this makes see-through.
      const png = await page.locator('svg').screenshot({ omitBackground: true });
      images.push({ size, png });
    }
  } finally {
    await browser.close();
  }

  const bytes = ico(images);
  writeFileSync(OUT, bytes);
  console.log(`wrote ${OUT} (${SIZES.join('/')}px, ${bytes.length} bytes)`);
}

await main();
