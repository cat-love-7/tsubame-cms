// The plugin as Gatsby runs it: declare the schema, then source the nodes.
//
// Gatsby itself is not installed here (the plugin only needs `fetch` and the node API it is handed),
// so these tests drive the two hooks with the same arguments Gatsby gives them and a fake delivery
// API. What is checked is what a site would query: the node types, the links a query follows (to
// markdown nodes and to related content), and the `text/markdown` media type that makes
// gatsby-transformer-remark pick the markdown up.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import plugin from '../gatsby-node.js';
import nodesModule from '../src/nodes.js';
import clientModule from '../src/client.js';
import optionsModule from '../src/options.js';
import { createApiFetch, createGatsbyApi, BLOG_ITEMS } from './fixtures.mjs';

const { sourceAll } = nodesModule;
const { TsubameClient } = clientModule;
const { normalizeOptions } = optionsModule;

const PLUGIN_OPTIONS = { apiUrl: 'https://cms.example.com' };

async function run(pluginOptions = PLUGIN_OPTIONS) {
  const fetchImpl = createApiFetch();
  const originalFetch = globalThis.fetch;
  globalThis.fetch = fetchImpl;
  try {
    const harness = createGatsbyApi();
    await plugin.createSchemaCustomization(harness.api, pluginOptions);
    await plugin.sourceNodes(harness.api, pluginOptions);
    return { ...harness, fetchImpl };
  } finally {
    globalThis.fetch = originalFetch;
  }
}

describe('createSchemaCustomization', () => {
  it('declares the schema it read from the CMS', async () => {
    const { types } = await run();
    assert.equal(types.length, 1);
    assert.match(types[0], /type TsubameBlogItem implements Node \{/);
    assert.match(types[0], /^\s+body: TsubameMarkdown @link$/m);
    assert.match(types[0], /type TsubameHomePage implements Node \{/);
  });

  it('declares a type per composite definition, so a composite field can be followed', async () => {
    const { types } = await run();
    assert.match(types[0], /type TsubameCompositeBlock \{/);
    assert.match(types[0], /^\s+text: TsubameMarkdown @link$/m);
    assert.match(types[0], /^\s+link: TsubameAuthorsItem @link$/m);
  });

  it('declares a type for a relation target the index does not list', async () => {
    const { types } = await run();
    assert.match(types[0], /type TsubameEditorsItem implements Node \{/);
    assert.match(types[0], /type TsubameContactPage implements Node \{/);
  });

  it('refuses to run without an apiUrl', async () => {
    await assert.rejects(() => plugin.createSchemaCustomization(createGatsbyApi().api, {}), /`apiUrl` is required/);
  });
});

describe('sourceNodes', () => {
  it('creates a node per published item, typed after its collection', async () => {
    const { nodes } = await run();

    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.internal.type, 'TsubameBlogItem');
    assert.equal(item.collection, 'blog');
    assert.equal(item.remoteId, 1);
    assert.equal(item.publishedAt, '2026-01-01T00:00:00Z');
    assert.equal(item.lastPublishedAt, '2026-01-03T00:00:00Z');
    assert.equal(item.title, 'Hello');
    assert.equal(item.rating, 4.5);
    assert.equal(item.featured, true);
    assert.equal(item.published_on, '2026-01-02');
    assert.deepEqual(item.tags, ['news']);
  });

  it('keeps the raw values, including the ones it also flattened', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.values.body, '# Hello\n\nworld');
    assert.equal(item.values.values, 'a text field that happens to be called values');
  });

  it('renames a field that would collide with one of its own', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.values_2, 'a text field that happens to be called values');
    assert.equal(item.fieldNames.values, 'values_2');
    assert.equal(item.fieldNames['body-parts'], 'body_parts');
  });

  it('resolves image paths the browser can open', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.deepEqual(item.cover, {
      id: 3,
      url: '/api/images/logo.png',
      absoluteUrl: 'https://cms.example.com/api/images/logo.png',
      stableUrl: 'https://cms.example.com/api/images/by-id/3',
    });
    assert.equal(item.gallery.length, 1);
    assert.equal(item.gallery[0].absoluteUrl, 'https://cms.example.com/api/images/a.png');
  });
});

describe('references', () => {
  it('points a relation at the target node, so a query follows it', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.author, 'node:tsubame-item:authors:7');
  });

  it('points a list relation at the ids of the nodes', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.deepEqual(item.editors, ['node:tsubame-item:editors:2']);
  });

  it('points a page relation at the page node', async () => {
    const { nodes } = await run();
    const page = nodes.get('node:tsubame-page:home');
    assert.equal(page.cta.target, 'node:tsubame-page:home');
  });

  it('still keeps the reference itself under values', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.deepEqual(item.values.author, [{ target: 'authors', item: 7 }]);
  });

  it('links a relation declared inside a composite definition', async () => {
    const { nodes } = await run();
    const block = nodes.get('node:tsubame-item:blog:1').blocks[0];
    assert.equal(block.link, 'node:tsubame-item:authors:7');
    assert.equal(block.values.link[0].item, 7);
  });
});

describe('reverse references', () => {
  it('gives a target the referrers, by the name the referring schema declared', async () => {
    const { nodes } = await run();
    const author = nodes.get('node:tsubame-item:authors:7');
    // `blog.author` declares `inverse_name: "articles"`, so the author holds the article.
    assert.deepEqual(author.articles, ['node:tsubame-item:blog:1']);
  });

  it('lets a page be the referrer', async () => {
    const { nodes } = await run();
    const author = nodes.get('node:tsubame-item:authors:7');
    // `home.featured_author` declares `features`, so the author holds the page.
    assert.deepEqual(author.features, ['node:tsubame-page:home']);
  });

  it('counts a reference held inside a composite, as the delivery API does', async () => {
    const { nodes } = await run();
    const author = nodes.get('node:tsubame-item:authors:7');
    // Blog item 1 names the author both directly and from a block. One article, not two.
    assert.equal(author.articles.length, 1);
  });

  it('leaves out content that does not hold the reference', async () => {
    const { nodes } = await run();
    const author = nodes.get('node:tsubame-item:authors:7');
    assert.equal(author.articles.includes('node:tsubame-item:blog:2'), false);
    assert.equal(author.articles.includes('node:tsubame-item:blog:3'), false);
  });

  it('does not answer a name declared only inside a composite definition', async () => {
    const { nodes } = await run();
    const author = nodes.get('node:tsubame-item:authors:7');
    // `block.link` carries `inverse_name: "blocks"`; the target has no such field.
    assert.equal('blocks' in author, false);
  });

  it('reports the inverse names in fieldNames', async () => {
    const { nodes } = await run();
    const author = nodes.get('node:tsubame-item:authors:7');
    assert.equal(author.fieldNames.articles, 'articles');
    assert.equal(author.fieldNames.features, 'features');
  });

  it('declares the name on a target that has no published items', async () => {
    const { types } = await run();
    // `editors.homepage` declares `editors` on the page; there is no editors node to be a referrer,
    // so the field exists but is empty.
    assert.match(types[0], /^\s+editors: \[TsubameEditorsItem\] @link$/m);
  });
});

describe('markdown fields', () => {
  it('creates one text/markdown node per field and links the item to it', async () => {
    const { nodes, links } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    const markdownId = item.body;

    assert.equal(markdownId, 'node:tsubame-markdown:node:tsubame-item:blog:1:body');
    const markdown = nodes.get(markdownId);
    assert.equal(markdown.internal.type, 'TsubameMarkdown');
    assert.equal(markdown.internal.mediaType, 'text/markdown');
    assert.equal(markdown.internal.content, '# Hello\n\nworld');
    assert.equal(markdown.raw, '# Hello\n\nworld');
    assert.equal(markdown.field, 'body');
    assert.equal(markdown.path, 'body');
    assert.equal(markdown.parent, item.id);
    assert.equal(markdown.itemId, 1);
    assert.equal(markdown.collection, 'blog');
    assert.equal(markdown.pageName, null);

    assert.ok(links.some((link) => link.parent === item.id && link.child === markdownId));
    assert.ok(item.children.includes(markdownId));
  });

  it('links an array of markdown element by element', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');

    assert.deepEqual(item.body_parts, [
      'node:tsubame-markdown:node:tsubame-item:blog:1:body-parts.0',
      'node:tsubame-markdown:node:tsubame-item:blog:1:body-parts.1',
    ]);
    assert.equal(nodes.get(item.body_parts[0]).raw, 'part one');
    assert.equal(nodes.get(item.body_parts[0]).path, 'body-parts.0');
    assert.equal(nodes.get(item.body_parts[1]).raw, 'part two');
  });

  it('reaches a markdown field inside a composite, one level down and two', async () => {
    const { nodes } = await run();
    const block = nodes.get('node:tsubame-item:blog:1').blocks[0];

    const outer = nodes.get(block.text);
    assert.equal(outer.internal.content, 'First **block**');
    assert.equal(outer.path, 'blocks.0.text');
    assert.equal(outer.field, 'blocks');
    assert.equal(outer.collection, 'blog');

    const child = nodes.get(block.children[0].text);
    assert.equal(child.internal.content, 'child text');
    assert.equal(child.path, 'blocks.0.children.0.text');
    assert.equal(child.parent, 'node:tsubame-item:blog:1');
  });

  it('reaches a markdown field inside a composite of a single page', async () => {
    const { nodes } = await run();
    const page = nodes.get('node:tsubame-page:home');
    const body = nodes.get(page.cta.body);
    assert.equal(body.internal.content, 'The **cta** body');
    assert.equal(body.path, 'cta.body');
    assert.equal(body.pageName, 'home');
    assert.equal(body.itemId, null);
  });

  it('still carries an empty markdown field, so the link is not null', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:2');
    // An empty string is a value, not a missing one: the node is created with empty content, which
    // is what keeps `body { childMarkdownRemark { html } }` from failing on a half-written item.
    assert.equal(nodes.get(item.body).internal.content, '');
  });

  it('has a node for every markdown value the content holds', async () => {
    const { nodes } = await run();
    const markdown = [...nodes.values()].filter((node) => node.internal.type === 'TsubameMarkdown');
    assert.equal(markdown.length, 10);
  });
});

describe('single pages', () => {
  it('creates a node typed after the page', async () => {
    const { nodes } = await run();
    const page = nodes.get('node:tsubame-page:home');

    assert.equal(page.internal.type, 'TsubameHomePage');
    assert.equal(page.name, 'home');
    assert.equal(page.values.title, 'Home');
    assert.equal(page.collection, undefined);
    assert.equal(nodes.get(page.intro).internal.mediaType, 'text/markdown');
    assert.equal(nodes.get(page.intro).raw, '## Welcome');
    assert.equal(nodes.get(page.intro).pageName, 'home');
    assert.equal(nodes.get(page.intro).itemId, null);
  });
});

describe('collection metadata', () => {
  it('reports the schema, the count and the type it gave the collection', async () => {
    const { nodes } = await run();
    const collection = nodes.get('node:tsubame-collection:blog');

    assert.equal(collection.internal.type, 'TsubameCollection');
    assert.equal(collection.name, 'blog');
    assert.equal(collection.itemTypeName, 'TsubameBlogItem');
    assert.equal(collection.itemCount, BLOG_ITEMS.length);
    assert.equal(collection.schema[0].name, 'title');
    assert.equal(collection.fieldNames['body-parts'], 'body_parts');
  });

  it('creates no collection node for a collection with no published items', async () => {
    const { nodes } = await run();
    assert.equal(nodes.has('node:tsubame-collection:editors'), false);
  });
});

describe('reporting', () => {
  it('says what it sourced', async () => {
    const { infos } = await run();
    assert.match(infos.join('\n'), /sourced 4 item\(s\) from 2 collection\(s\), 1 single page\(s\), and 10 markdown/);
  });
});

describe('options', () => {
  it('applies the pageSize it was given', async () => {
    const { fetchImpl } = await run({ apiUrl: 'https://cms.example.com', pageSize: 1 });
    assert.ok(fetchImpl.requests.some((url) => url.includes('limit=1&offset=1')));
  });

  it('warns and clamps a pageSize over the API maximum', async () => {
    const harness = createGatsbyApi();
    const fetchImpl = createApiFetch();
    const originalFetch = globalThis.fetch;
    globalThis.fetch = fetchImpl;
    try {
      await plugin.sourceNodes(harness.api, { apiUrl: 'https://cms.example.com', pageSize: 500 });
    } finally {
      globalThis.fetch = originalFetch;
    }
    assert.match(harness.warnings.join('\n'), /`pageSize` 500 is outside 1\.\.200; using 200/);
  });
});

describe('images.download', () => {
  /**
   * `sourceAll` with a stand-in for `createRemoteFileNode`: the real one needs
   * gatsby-source-filesystem, which the tests do not install (and should not have to).
   */
  async function runWithDownload({ failing = [], files = [] } = {}) {
    const fetchImpl = createApiFetch();
    const originalFetch = globalThis.fetch;
    globalThis.fetch = fetchImpl;
    try {
      const harness = createGatsbyApi({ files });
      const options = normalizeOptions(
        { apiUrl: 'https://cms.example.com', images: { download: true } },
        harness.api.reporter,
      );
      const client = new TsubameClient(options, { reporter: harness.api.reporter, fetchImpl });
      const downloads = [];
      const createRemoteFileNode = async ({ url, ext, createNode }) => {
        if (failing.some((name) => url.endsWith(name))) {
          throw new Error(`404 for ${url}`);
        }
        downloads.push({ url, ext });
        const id = `file:${url}`;
        createNode({ id, parent: null, children: [], internal: { type: 'File', mediaType: 'image/png' } });
        return { id };
      };
      await sourceAll(harness.api, client, options, { createRemoteFileNode });
      return { ...harness, downloads };
    } finally {
      globalThis.fetch = originalFetch;
    }
  }

  it('downloads each image once, at its absolute URL', async () => {
    const { downloads } = await runWithDownload();
    assert.deepEqual(downloads, [
      { url: 'https://cms.example.com/api/images/logo.png', ext: '.png' },
      { url: 'https://cms.example.com/api/images/a.png', ext: '.png' },
      { url: 'https://cms.example.com/api/images/og.png', ext: '.png' },
    ]);
  });

  it('creates a File node and links the image value to it', async () => {
    const { nodes } = await runWithDownload();
    const item = nodes.get('node:tsubame-item:blog:1');
    const fileId = 'file:https://cms.example.com/api/images/logo.png';

    assert.equal(item.cover.localFile, fileId);
    assert.equal(nodes.get(fileId).internal.type, 'File');
    assert.equal(nodes.get(fileId).internal.mediaType, 'image/png');
  });

  it('reaches an image inside an array and inside a composite', async () => {
    const { nodes } = await runWithDownload();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.gallery[0].localFile, 'file:https://cms.example.com/api/images/a.png');
    assert.equal(item.seo.og_image.localFile, 'file:https://cms.example.com/api/images/og.png');
  });

  it('leaves the remote fields alone', async () => {
    const { nodes } = await runWithDownload();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.cover.url, '/api/images/logo.png');
    assert.equal(item.cover.stableUrl, 'https://cms.example.com/api/images/by-id/3');
  });

  it('answers a null localFile for an image the download could not reach', async () => {
    const { nodes, warnings } = await runWithDownload({ failing: ['og.png'] });
    const item = nodes.get('node:tsubame-item:blog:1');
    // One picture missing is a warning and a null link, not a failed build.
    assert.equal(item.seo.og_image.localFile, null);
    assert.equal(item.seo.og_image.absoluteUrl, 'https://cms.example.com/api/images/og.png');
    assert.match(warnings.join('\n'), /could not download image .*og\.png/);
  });

  it('reuses the File a previous build fetched, and touches it so Gatsby keeps it', async () => {
    // A presigned deployment: the same object, a different signature - so a different URL, and the
    // same identity once the query is dropped.
    const cached = {
      id: 'file:cached',
      url: 'https://cms.example.com/api/images/logo.png?X-Amz-Signature=from-an-earlier-build',
      internal: { type: 'File', owner: 'gatsby-source-filesystem' },
    };
    const { nodes, downloads, touched } = await runWithDownload({ files: [cached] });

    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.cover.localFile, 'file:cached');
    assert.ok(touched.includes('file:cached'));
    // logo.png was not fetched again; the other two images were.
    assert.deepEqual(
      downloads.map((entry) => entry.url),
      ['https://cms.example.com/api/images/a.png', 'https://cms.example.com/api/images/og.png'],
    );
  });

  it('downloads again when the object key changed, which is what a replacement does', async () => {
    const replaced = {
      id: 'file:replaced',
      url: 'https://cms.example.com/api/images/logo-replaced.png',
      internal: { type: 'File', owner: 'gatsby-source-filesystem' },
    };
    const { nodes, downloads, touched } = await runWithDownload({ files: [replaced] });

    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal(item.cover.localFile, 'file:https://cms.example.com/api/images/logo.png');
    assert.deepEqual(touched, []);
    assert.ok(downloads.some((entry) => entry.url.endsWith('/logo.png')));
  });

  it('does not download anything when the option is off', async () => {
    const { nodes } = await run();
    const item = nodes.get('node:tsubame-item:blog:1');
    assert.equal('localFile' in item.cover, false);
  });
});
