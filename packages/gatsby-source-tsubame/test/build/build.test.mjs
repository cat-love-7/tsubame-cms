// The one test that runs the real thing: `gatsby build`, with the plugin loaded by Gatsby.
//
// Everything else in `test/` drives the plugin's own functions with a fake API, which is fast and
// checks the shapes - but it cannot check the two things a relation union is: that Gatsby accepts
// the union and the inline fragments the site has to write, and that its `resolveType` and field
// resolver answer what the schema promises. Only a build can. So this copies a three-file site to a
// temporary directory, points it at a stub delivery API, builds it, and reads the page data back.
//
// It is not part of `scripts/test-gatsby-source.sh`: it needs `gatsby` installed in this package
// (`npm install`) and it takes about a minute, while the unit tests need neither. Run it with
// `scripts/test-gatsby-build.sh`.

import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { cp, mkdtemp, readFile, rm, symlink } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { after, before, describe, it } from 'node:test';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

import { startDeliveryStub } from './delivery-stub.mjs';

const execFileAsync = promisify(execFile);

const PACKAGE_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const SITE_DIR = path.join(PACKAGE_DIR, 'test', 'build', 'site');
const NODE_MODULES = path.join(PACKAGE_DIR, 'node_modules');

/** Whether Gatsby is installed here; without it there is nothing to build with. */
function gatsbyCliPath() {
  try {
    // `gatsby`'s own entry point, resolved the way the build would resolve it.
    const require = createRequire(import.meta.url);
    return require.resolve('gatsby/cli.js');
  } catch {
    return null;
  }
}

const gatsbyCli = gatsbyCliPath();

describe('a real Gatsby build', { skip: gatsbyCli === null ? 'gatsby is not installed in packages/gatsby-source-tsubame' : false }, () => {
  let stub;
  let buildDir;
  let homeDir;
  let output;

  before(async () => {
    stub = await startDeliveryStub();
    buildDir = await mkdtemp(path.join(tmpdir(), 'tsubame-gatsby-build-'));
    // Gatsby keeps per-site state under the XDG directories. A temporary home keeps the test from
    // writing into the developer's own (`~/.config/gatsby`) and makes runs independent of each
    // other.
    homeDir = await mkdtemp(path.join(tmpdir(), 'tsubame-gatsby-home-'));
    await cp(SITE_DIR, buildDir, { recursive: true });
    // The temporary site has no dependencies of its own: it borrows this package's, which is where
    // `gatsby`, `react` and `react-dom` live once the package is installed.
    await symlink(NODE_MODULES, path.join(buildDir, 'node_modules'), 'dir');

    try {
      const result = await execFileAsync(process.execPath, [gatsbyCli, 'build', '--no-color'], {
        cwd: buildDir,
        maxBuffer: 64 * 1024 * 1024,
        timeout: 10 * 60 * 1000,
        env: {
          ...process.env,
          // Gatsby builds without a browser and without telemetry: this test may run offline.
          GATSBY_TELEMETRY_DISABLED: '1',
          XDG_CONFIG_HOME: path.join(homeDir, 'config'),
          XDG_CACHE_HOME: path.join(homeDir, 'cache'),
          XDG_DATA_HOME: path.join(homeDir, 'data'),
          TSUBAME_API_URL: stub.url,
          TSUBAME_PLUGIN_PATH: PACKAGE_DIR,
        },
      });
      output = result.stdout;
    } catch (error) {
      output = `${error.stdout ?? ''}\n${error.stderr ?? ''}`;
      throw new Error(`the build failed:\n${output}`);
    }
  });

  after(async () => {
    for (const directory of [buildDir, homeDir]) {
      if (directory !== undefined) {
        await rm(directory, { recursive: true, force: true });
      }
    }
    if (stub !== undefined) {
      await stub.close();
    }
  });

  it('declares the relation union, so the query with inline fragments compiles', () => {
    // The build only gets this far if the schema the plugin declared accepted the page's query.
    assert.match(output, /Done building/);
  });

  it('resolves the elements to the nodes they name, and an unknown target to the reference', async () => {
    const pageData = JSON.parse(await readFile(path.join(buildDir, 'public', 'page-data', 'index', 'page-data.json'), 'utf8'));
    const [item] = pageData.result.data.allTsubameBlogItem.nodes;

    // `related` names authors and editors, both part of this build: every element is its node.
    assert.deepEqual(item.related, [
      { name: 'Ada', __typename: 'TsubameAuthorsItem' },
      { name: 'Grace', __typename: 'TsubameEditorsItem' },
    ]);

    // `mentions` names authors (here) and `nowhere` (404, so no node type): the element whose target
    // has no type stays the reference the union declares, in the same list, in the same order.
    assert.deepEqual(item.mentions, [
      { name: 'Ada', __typename: 'TsubameAuthorsItem' },
      { target: 'nowhere', item: 5, kind: 'collection', __typename: 'TsubameRelationRef' },
    ]);
  });

  it('keeps the raw references readable under values', async () => {
    const pageData = JSON.parse(await readFile(path.join(buildDir, 'public', 'page-data', 'index', 'page-data.json'), 'utf8'));
    const [item] = pageData.result.data.allTsubameBlogItem.nodes;

    // Read through `values`, where `docs/content-api.md` promises the value as the API sent it.
    assert.deepEqual(item.values.mentions, [
      { target: 'authors', item: 7 },
      { target: 'nowhere', item: 5 },
    ]);
  });

  it('does the same for an array of relations inside a composite definition', async () => {
    const pageData = JSON.parse(await readFile(path.join(buildDir, 'public', 'page-data', 'index', 'page-data.json'), 'utf8'));
    const [item] = pageData.result.data.allTsubameBlogItem.nodes;

    // A composite is not a node type, so its own union is declared and resolved there instead.
    assert.deepEqual(item.card.related, [
      { __typename: 'TsubameAuthorsItem', name: 'Ada' },
      { __typename: 'TsubameEditorsItem', name: 'Grace' },
    ]);
  });
});
