// The one test that runs the real thing: `gatsby build`, with the plugin loaded by Gatsby.
//
// Everything else in `test/` drives the plugin's own functions with a fake API, which is fast and
// checks the shapes - but it cannot check the two things a relation union is: that Gatsby accepts the
// union and the inline fragments a site has to write, and that `@link` resolves a union of node types
// element by element. Only a build can. So this copies a three-file site to a temporary directory,
// points it at a stub delivery API, builds it, and then reads three things back: the warning the
// plugin printed, the schema Gatsby generated (`.cache/schema.gql`), and the data the page's query
// answered (`public/page-data/index/page-data.json`).
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
import { fileURLToPath } from 'node:url';
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
  let schema;
  let pageData;

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
      output = `${result.stdout}\n${result.stderr}`;
    } catch (error) {
      output = `${error.stdout ?? ''}\n${error.stderr ?? ''}`;
      throw new Error(`the build failed:\n${output}`);
    }

    schema = await readFile(path.join(buildDir, '.cache', 'schema.gql'), 'utf8');
    pageData = JSON.parse(
      await readFile(path.join(buildDir, 'public', 'page-data', 'index', 'page-data.json'), 'utf8'),
    );
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

  const item = () => pageData.result.data.allTsubameBlogItem.nodes[0];

  it('declares the union of the targets the field has, and links it', () => {
    // Gatsby's generated schema is the one a site is written against.
    assert.match(schema, /union TsubameBlogItemRelated = TsubameAuthorsItem \| TsubameEditorsItem/);
    assert.match(schema, /related: \[TsubameBlogItemRelated\]/);
    // The same inside a composite definition, which is not a node type.
    assert.match(schema, /union TsubameCompositeCardRelated = TsubameAuthorsItem \| TsubameEditorsItem/);
    assert.match(schema, /related: \[TsubameCompositeCardRelated\]/);
  });

  it('resolves every element of the union to the node it names', () => {
    assert.deepEqual(item().related, [
      { __typename: 'TsubameAuthorsItem', name: 'Ada' },
      { __typename: 'TsubameEditorsItem', name: 'Grace' },
    ]);
    assert.deepEqual(item().card.related, [
      { __typename: 'TsubameAuthorsItem', name: 'Ada' },
      { __typename: 'TsubameEditorsItem', name: 'Grace' },
    ]);
  });

  it('types a field that lost a target as a list of the target it kept', () => {
    // `mentions` names authors and `nowhere`; only authors is part of the build, so the field is not
    // a union at all, and the element the delivery API served is the node it names.
    assert.match(schema, /mentions: \[TsubameAuthorsItem\]/);
    assert.deepEqual(item().mentions, [{ __typename: 'TsubameAuthorsItem', name: 'Ada' }]);
  });

  it('leaves out a field whose target the CMS does not answer, and says so', () => {
    // `ghost` names `nowhere` and nothing else: it is in no type (`TsubameBlogItem` included), and
    // the reference type the plugin used to fall back to is gone with it.
    assert.equal(/ghost:/.test(schema), false);
    assert.equal(/RelationRef/.test(schema), false);

    // The reporter wraps a long warning, so the lines are joined before they are read.
    const flat = output.replace(/\s+/g, ' ');
    assert.match(flat, /TsubameBlogItem\.ghost names collection 'nowhere'/);
    assert.match(flat, /the field is left out of the schema/);
    // The same target inside a field that kept another one is reported as the target, not the field.
    assert.match(flat, /TsubameBlogItem\.mentions names collection 'nowhere'/);
    assert.match(flat, /that target is left out of the field/);
  });

  it('keeps the raw values readable under values', async () => {
    // What the delivery API sent is on the node as is, for a site that wants the reference rather
    // than the node it points at.
    assert.deepEqual(item().values.mentions, [{ target: 'authors', item: 7 }]);
    assert.deepEqual(item().values.related, [
      { target: 'authors', item: 7 },
      { target: 'editors', item: 2 },
    ]);
  });
});
