// The declared GraphQL schema: the type each collection, page and composite definition gets, the
// name each field gets, and the links that make a reference a query rather than a lookup.

import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import typesModule from '../src/types.js';
import { createModel } from './fixtures.mjs';

const { buildTypeDefinitions } = typesModule;
const sdl = buildTypeDefinitions(createModel());

/** The body of one declared type, for assertions that are about one type only. */
function typeBody(text, name) {
  const match = text.match(new RegExp(`type ${name}[^{]*\\{([\\s\\S]*?)\\n\\}`));
  return match === null ? '' : match[1];
}

describe('content types', () => {
  it('declares a node type per collection and per page', () => {
    assert.match(sdl, /type SlCmsBlogItem implements Node \{/);
    assert.match(sdl, /type SlCmsAuthorsItem implements Node \{/);
    // A collection a relation names, which has no published items and is not in the index.
    assert.match(sdl, /type SlCmsEditorsItem implements Node \{/);
    assert.match(sdl, /type SlCmsHomePage implements Node \{/);
  });

  it('asks for no inference on the collection metadata type only', () => {
    assert.match(sdl, /type SlCmsCollection implements Node @dontInfer \{/);
    assert.match(sdl, /type SlCmsMarkdown implements Node \{/);
  });

  it('declares a page a relation names but the index does not list, with the plugin fields only', () => {
    assert.match(sdl, /type SlCmsContactPage implements Node \{/);
    assert.match(sdl, /type SlCmsContactPage implements Node \{\n  name: String!\n  publishedAt: Date/);
  });
});

describe('markdown fields', () => {
  it('gives them their own type and a link to the node', () => {
    assert.match(sdl, /^\s+body: SlCmsMarkdown @link$/m);
    assert.match(sdl, /^\s+body_parts: \[SlCmsMarkdown\] @link$/m);
  });
});

describe('relation fields', () => {
  it('links a single reference to the target type', () => {
    assert.match(sdl, /^\s+author: SlCmsAuthorsItem @link$/m);
  });

  it('links several references to a list of the target type', () => {
    assert.match(sdl, /^\s+editors: \[SlCmsEditorsItem\] @link$/m);
  });

  it('links a page reference to the page type', () => {
    // `home.cta.target` points at `home`; the type is the page's, not a reference struct.
    assert.match(sdl, /^\s+target: SlCmsHomePage @link$/m);
  });
});

describe('reverse references', () => {
  it('gives a target the name the referring schema declared, typed as the referrers', () => {
    assert.match(sdl, /^\s+articles: \[SlCmsBlogItem\] @link$/m);
  });

  it('lists a page declaring an inverse on a collection target', () => {
    assert.match(sdl, /^\s+features: \[SlCmsHomePage\] @link$/m);
  });

  it('lists a collection declaring an inverse on a page target', () => {
    assert.match(sdl, /^\s+editors: \[SlCmsEditorsItem\] @link$/m);
  });

  it('does not turn a name declared inside a composite into a field', () => {
    // `block.link` declares `blocks`; a composite may be embedded by several collections, so it is
    // not a declaration and the author gets no such field. (`blocks` on the blog item is that
    // item's own field and stays.)
    assert.equal(/^\s+blocks:/m.test(typeBody(sdl, 'SlCmsAuthorsItem')), false);
    assert.match(sdl, /^\s+blocks: \[SlCmsCompositeBlock\]$/m);
  });
});

describe('composite types', () => {
  it('declares a type per definition, with its fields typed', () => {
    assert.match(sdl, /type SlCmsCompositeSeo \{/);
    assert.match(sdl, /type SlCmsCompositeBlock \{/);
    assert.match(sdl, /type SlCmsCompositeCta \{/);
    assert.match(sdl, /^\s+og_image: SlCmsImage$/m);
  });

  it('links a markdown field and a relation inside a definition', () => {
    assert.match(sdl, /^\s+text: SlCmsMarkdown @link$/m);
    assert.match(sdl, /^\s+link: SlCmsAuthorsItem @link$/m);
  });

  it('names a definition that reaches itself through an array', () => {
    assert.match(sdl, /^\s+children: \[SlCmsCompositeBlock\]$/m);
  });

  it('keeps a definition\'s own fields out of the plugin\'s', () => {
    assert.match(sdl, /type SlCmsCompositeBlock \{\n  id: String!\n  values: JSON!/);
  });
});

describe('the fields the plugin owns', () => {
  it('keeps them out of the CMS fields', () => {
    assert.match(sdl, /^\s+values: JSON!$/m);
    assert.match(sdl, /^\s+values_2: String$/m);
    assert.match(sdl, /^\s+fieldNames: JSON!$/m);
    assert.match(sdl, /^\s+remoteId: Int!$/m);
    assert.match(sdl, /^\s+collection: String!$/m);
  });

  it('still declares the shared types', () => {
    assert.match(sdl, /type SlCmsCollection implements Node @dontInfer \{/);
    assert.match(sdl, /type SlCmsMarkdown implements Node \{/);
    assert.match(sdl, /type SlCmsImage \{/);
    assert.match(sdl, /type SlCmsComposite \{/);
    assert.match(sdl, /type SlCmsRelationRef \{/);
  });
});

describe('image files', () => {
  it('does not name the File type when images are not downloaded', () => {
    // `File` belongs to gatsby-source-filesystem; a site that does not download images does not
    // have it, and naming it would be an "Unknown type File" schema error.
    assert.equal(/^\s+localFile:/m.test(sdl), false);
  });

  it('links the downloaded file when images.download is on', () => {
    const withFiles = buildTypeDefinitions(
      createModel({ images: { download: true, concurrency: 4, requestHeaders: {} } }),
    );
    assert.match(withFiles, /^\s+localFile: File @link$/m);
  });
});
