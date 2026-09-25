# gatsby-source-tsubame

A Gatsby source plugin that reads the **published content API** (`/api/content/*`) of this CMS (`Tsubame`).
It is the thin plugin described under "Using it from Gatsby" in `docs/content-api.md`, made usable in practice.

- **No authentication.** The delivery API returns published content only, so builds need no token.
- **Builds GraphQL types from the CMS schema.** The collection `blog` becomes `TsubameBlogItem`,
  the single page `home` becomes `TsubameHomePage`, and the composite field definition `block` becomes `TsubameCompositeBlock`.
- **References are traversable in GraphQL** (forward and inverse). A `Relation` field is
  `@link`ed to the node it points at, so both `author { name }` and `author { articles { title } }`
  (inverse lookup, `inverse_name`) can be traversed in one query. **The same holds for references inside composite fields.**
- **A Markdown field becomes a `text/markdown` node**, so adding `gatsby-transformer-remark` alone
  makes it usable as HTML. **The same holds for Markdown inside composite fields.**
- **Images can become `File` nodes with `images.download`**, so `gatsby-transformer-sharp` +
  `gatsby-plugin-image` can produce `gatsbyImageData` (§5).

## 1. Usage

```console
$ npm install gatsby-transformer-remark
$ npm install ../path/to/packages/gatsby-source-tsubame   # the plugin in this repository
```

```javascript
// gatsby-config.js
module.exports = {
  plugins: [
    {
      resolve: 'gatsby-source-tsubame',
      options: {
        // Root of the CMS. Do not append `/api` (that is the plugin's job).
        apiUrl: process.env.TSUBAME_URL || 'http://127.0.0.1:8000',
      },
    },
    // Turn Markdown into HTML. Tsubame only attaches a mediaType, so the conversion is
    // left to this plugin (as split in `docs/content-api.md` §8).
    'gatsby-transformer-remark',
  ],
}
```

`apiUrl` is the root of the CMS. The API always lives under `/api`, and the image URLs the
delivery API returns still contain `/api/images/...`, so the plugin turns them into absolute
URLs with `apiUrl`. Specify `apiPrefix` only when you deploy under a different prefix.

### Options

| Option | Default | Meaning |
|---|---|---|
| `apiUrl` | (required) | Root of the CMS. Example `http://127.0.0.1:8000` |
| `apiPrefix` | `/api` | API prefix. Only when it is served elsewhere |
| `pageSize` | `50` | Items per request when walking a collection (the API limit is 200) |
| `typePrefix` | `Tsubame` | Prefix of the generated GraphQL type names |
| `requestTimeout` | `30000` | Time limit for one request (ms) |
| `retries` | `2` | Number of retries on network failure or 5xx/429 |
| `concurrency` | `4` | Number of concurrent requests when reading the schema |
| `fetchOptions` | `{}` | Extra options passed to `fetch` (e.g. `headers` when something upstream authenticates) |
| `images.download` | `false` | Download images and create `File` nodes (sharp integration. §5) |
| `images.concurrency` | `4` | Number of images downloaded at the same time |
| `images.requestHeaders` | `{}` | Headers when fetching images (when the delivery side requires authentication) |

Pages are walked with **`next_offset`**, not by item count. The delivery API cuts a page by
bytes as well as by count, so advancing with `offset + limit` can miss items (`docs/content-api.md` §3.1).

## 2. Generated nodes

| Node | Type | Main fields |
|---|---|---|
| Collection | `TsubameCollection` | `name` / `itemTypeName` / `itemCount` / `schema` / `fieldNames` |
| Item | `Tsubame<Collection>Item` | `remoteId` / `collection` / `publishedAt` / `lastPublishedAt` / `values` / `fieldNames` + each field |
| Single page | `Tsubame<Page>Page` | `name` / `publishedAt` / `lastPublishedAt` / `values` / `fieldNames` + each field |
| Markdown | `TsubameMarkdown` | `raw` / `field` / `path` / `collection` / `pageName` / `itemId` |
| Composite field | `TsubameComposite<Id>` | `id` / `values` + each field of the definition |

`values` is exactly the raw values the API returned (without type tags). Typed fields sit on top of it.
`fieldNames` is the mapping from "CMS name → GraphQL name" (described below).

### Field type mapping

| CMS `field_type` | GraphQL |
|---|---|
| `Text` / `Slug` | `String` |
| `Markdown` | `TsubameMarkdown` (`@link`) |
| `Number` | `Float` (the CMS uses `f64`) |
| `Boolean` | `Boolean` |
| `Date` / `DateTime` | `Date` (Gatsby scalar. `formatString` works) |
| `Image` | `TsubameImage` |
| `TextEnum` | `[String]` |
| `Relation` (single) | the target type (`@link`) |
| `Relation` (multiple) | `[target type]` (`@link`) |
| `CompositeField` | `TsubameComposite<Id>` |
| `Array` (one element type) | `[element type]` |
| `Array` (several or unknown element types) | `JSON` |
| Unknown type | `JSON` |

`TsubameImage` has `id` / `url` (the API value) / `absoluteUrl` (a URL you can open as is) /
`stableUrl` (an `/api/images/by-id/{id}` that survives replacement).
Enabling `images.download` also adds `localFile` (an `@link` to a `File` node) (§5).

### Name rewriting

A GraphQL name must match `[_A-Za-z][_0-9A-Za-z]*`, so a CMS name cannot always be used as is.

- Type names: `press-releases` → `TsubamePressReleasesItem`, `お知らせ` → `TsubameUnnamedItem`
- Field names: `published-at` → `published_at`

The rewriting is **the same on every build**, and the mapping goes into the node's `fieldNames`
(and `TsubameCollection.itemTypeName`). On a collision, names get a numeric suffix, as in
`values` → `values_2`.

## 3. Traversing references in GraphQL

A `Relation` field holds **the id of the referenced node** and is resolved with `@link`. The site
does not have to write the matching itself.

```graphql
query {
  allTsubameBlogItem {
    nodes {
      title
      author { name }          # single reference → the referenced node
      editors { name }         # multiple references → an array of nodes
      seo { description og_image { absoluteUrl } }
      blocks {                 # composite fields are typed too
        caption
        text { childMarkdownRemark { html } }
        link { name }          # references inside composite fields are traversable too
        children { caption text { childMarkdownRemark { rawMarkdownBody } } }
      }
      values                   # raw values: references stay as {"target": "...", "item": 7}
    }
  }
}
```

- **Single or multiple** is decided by `has_many` in the schema (a reference to a single page is always single by definition).
- **Multiple references keep the order they were written in**. A relation is not a set but an
  **ordered list**, and the CMS stores the order and delivers it as is (only duplicate references
  are removed, and the first position stays). The plugin links the nodes in that order, so
  "featured articles in this order" shows up on the site as is. Reordering is the CMS's business,
  and the site can see the same order through `values.<field>` too.
- When you need **the reference itself (the counterpart's name and id)**, read `values.<field>`.
  It is the delivery API response as is.
- When **the referenced target is unpublished**, that node does not exist, so the value is `null`
  (an empty array when multiple). The build does not fail. It is the plain answer that a
  "counterpart not yet on the site" cannot be traversed.
- **Even when the referenced collection has no published items**, its type is declared. That is
  because the plugin walks `/api/content/collections/{name}` (which returns the schema even when
  empty) and reads the definition. Only an unpublished single page has no published schema (404),
  so the plugin declares a type with only its own fields. The real fields appear on the next build
  after that page is published.

### Inverse lookups (`inverse_name`)

Writing `inverse_name` on a relation field makes **the referenced side** hold the referring items
under that name. The plugin turns it into a GraphQL field, so you can traverse from a category to
its articles in one query too.

```json
{ "name": "author", "field_type": { "Relation": {
  "target": { "kind": "collection", "name": "authors" },
  "has_many": false,
  "inverse_name": "articles"
}}}
```

```graphql
query {
  allTsubameAuthorsItem {
    nodes {
      name
      articles { title }   # blog.author declares inverse_name: "articles"
      features { title }   # home.featured_author declares "features"
    }
  }
}
```

- **Direction**: `inverse_name` is written on the field of **the referring side**. A name is
  **unique per target** (if another schema gives the same name to the same target, the CMS rejects
  it with 409), so an inverse field is **an array of the single type that declared it** (not a union).
- **A relation inside a composite field does not become a declaration.** A composite definition
  can be embedded in several collections, so "who is referring" is not determined to one. This is
  the same rule as the delivery API's `?populate=<inverse_name>`, and the plugin also reads only
  top-level relation fields as declarations.
- **Implementation**: an inverse lookup is answered by **building a local index** from the
  published content that build has already read. The delivery API has
  `?where=<field>:<value>` (filter by reference) and `?populate=<inverse_name>` (expand the
  referring items), but a build holds every published item, so this is faster than one request per
  item and has no cap (25 by default). The decision is the same as the API's: it is decided by
  **whether the published copy actually holds that reference**, and references inside composites
  and arrays count too.
- The name of an inverse field also goes into `fieldNames` (original name → GraphQL name).
- **Inverse lookups have no order.** The referring items are a set looked up from the index, and
  the CMS holds no order either (the plugin sorts by collection name, then item id, to keep builds
  stable). Order only has meaning for the forward list on the side that holds the references.

## 4. Turning Markdown into HTML

A Markdown field is created as a child node that has `internal.mediaType: "text/markdown"` and
`internal.content` (the raw source). `gatsby-transformer-remark` selects what to process by media
type alone and reads `internal.content`, so no configuration goes in between. The item's field is
`@link`ed to that node, so you can traverse `childMarkdownRemark` as is.

```graphql
query {
  allTsubameBlogItem(sort: { publishedAt: DESC }) {
    nodes {
      title
      slug
      publishedAt
      cover { absoluteUrl }
      body {
        childMarkdownRemark {
          html
          excerpt(format: PLAIN, pruneLength: 120)
          timeToRead
        }
      }
    }
  }
}
```

Creating article pages in `createPages`:

```javascript
// gatsby-node.js
const path = require('path')

exports.createPages = async ({ graphql, actions }) => {
  const { data, errors } = await graphql(`
    {
      allTsubameBlogItem {
        nodes {
          slug
        }
      }
    }
  `)
  if (errors) throw errors

  for (const item of data.allTsubameBlogItem.nodes) {
    actions.createPage({
      path: `/blog/${item.slug}/`,
      component: path.resolve('./src/templates/blog-post.js'),
      context: { slug: item.slug },
    })
  }
}
```

```javascript
// src/templates/blog-post.js
import React from 'react'
import { graphql } from 'gatsby'

export default function BlogPost({ data }) {
  const item = data.slCmsBlogItem
  return (
    <article>
      <h1>{item.title}</h1>
      <div dangerouslySetInnerHTML={{ __html: item.body.childMarkdownRemark.html }} />
    </article>
  )
}

export const query = graphql`
  query BlogPost($slug: String!) {
    slCmsBlogItem(slug: { eq: $slug }) {
      title
      body {
        childMarkdownRemark {
          html
        }
      }
    }
  }
`
```

**Markdown inside composite fields** works the same way. Since the definitions
(`/api/content/composite-fields`) are published, the plugin can know which strings are Markdown.
The Markdown node is a child of the same item, and `path` shows where it lives (`blocks.0.text`,
`blocks.0.children.0.text`).

When Markdown is an element of an **array** (`Array: [{"Markdown":{}}]`), the plugin creates a node
per element and `@link`s them as an array.

## 5. Processing images with sharp (`gatsby-transformer-sharp`)

`gatsby-transformer-sharp` only sees **local `File` nodes** (the `internal.mediaType` must be an
image and the body must be on disk). The delivery API returns URLs, so `gatsbyImageData` cannot be
produced as is. Enabling `images.download` makes the plugin fetch each image into Gatsby's cache,
create a `File` node, and link it from `TsubameImage.localFile`.

```console
$ npm install gatsby-source-filesystem gatsby-plugin-sharp gatsby-transformer-sharp gatsby-plugin-image
```

```javascript
// gatsby-config.js
module.exports = {
  plugins: [
    // Needed for createRemoteFileNode and the File type (not a source, so path can be anything).
    { resolve: 'gatsby-source-filesystem', options: { name: 'unused', path: './src/images' } },
    {
      resolve: 'gatsby-source-tsubame',
      options: { apiUrl: 'http://127.0.0.1:8000', images: { download: true } },
    },
    'gatsby-plugin-sharp',
    'gatsby-transformer-sharp',
  ],
}
```

```graphql
query {
  slCmsBlogItem(slug: { eq: "hello" }) {
    cover {
      absoluteUrl
      localFile { childImageSharp { gatsbyImageData(width: 800) } }
    }
    gallery { localFile { childImageSharp { gatsbyImageData(width: 400) } } }
    seo { og_image { localFile { childImageSharp { gatsbyImageData } } } }
  }
}
```

You can pass that `gatsbyImageData` to `GatsbyImage` / `getImage` from `gatsby-plugin-image` as is.
Images inside composite fields work the same way (the plugin reads the definitions and walks the
value tree, so both `seo.og_image` and `blocks[0].image` have `localFile`).

- **Off by default.** Downloading is real work, and sites that do not use sharp do not need it.
- **The same image is fetched only once** (references in any number of places are merged by id).
- **An image that fails to download gets a warning and `localFile: null`**. The build does not
  stop. `url` / `absoluteUrl` stay as they are.
- A `File`'s media type is decided by **the file extension**. The API's `url` is
  `/api/images/<file>.<ext>`, so it can be used for fetching as is (`stableUrl` has no extension,
  so it is not used for downloading).
- **Replacement is followed too.** Replacement in the CMS **changes the file name (= URL) without
  changing the id**, so the plugin fetches the **current `url`** the API returned for that build. On
  the next build `url` / `absoluteUrl` / `localFile` / `gatsbyImageData` become new, and only
  `stableUrl` (`/api/images/by-id/{id}`) does not change — because it is a link meant not to
  change. It is updated even when you rebuild with Gatsby's cache kept (without `--clear-cache`).
- **Unchanged images are not fetched again.** If a `File` node for the same body exists in the
  previous build's store, the plugin reuses it and keeps it in Gatsby with `touchNode`. Identity is
  `origin + pathname` — the URL **with the query removed**, i.e. the object key. This CMS assigns a
  new file name on every upload and never reuses one on replacement, so **the same key means the
  same body, and a changed key means a replacement** — that much can be stated flatly.
- **Reuse also works with signed delivery (AWS's `AWS_IMAGE_DELIVERY=presigned`).** The signature
  is in the query and changes on every read, but the path stays the object key, so the decision is
  not affected. Measured, a rebuild where only the signature changed resulted in **zero image
  fetches**, with `File` node ids and sharp output unchanged, and a replacement fetched once and
  produced a new `File` and transformation result.
  - However, the `url` / `absoluteUrl` the API returns change with each signature (they are values
    themselves, so they are not hidden). Therefore, in presigned mode, the `contentDigest` of a
    page containing images changes on every build, and the page is regenerated (images are not
    re-transformed).
  - The `url` of a reused `File` node is **still the previous build's signature**. Use `localFile`
    through `childImageSharp` / `gatsbyImageData` and do not read the URL directly
    (presigned mode is a mode for "fetch at build time and serve it yourself").

## 6. Live preview (without going through a build)

An entry point for showing a pre-publication working copy to someone who has no account. It is the
job of the shared preview URL (`docs/content-api.md` §5.6), and **neither this plugin nor a build
is involved**. The full contract is `docs/preview-site.md`, and the implementation is
`packages/tsubame-preview/` (a package that does not depend on this plugin and has zero
dependencies).

**This plugin does not depend on `tsubame-preview`.** The site installs both. Moving the preview
to a site other than Gatsby does not add any dependency to this plugin.

### Wiring

On the consuming site, split `gatsby-config.js` / `gatsby-node.js` by environment variable so that
**the production artifacts do not contain `/preview/*`**.

```javascript
// gatsby-config.js
const preview = process.env.BUILD_MODE === 'preview'

module.exports = {
  plugins: [
    // A preview build reads no published content at all: the browser fetches the working copy from the API.
    ...(preview ? [] : [{ resolve: 'gatsby-source-tsubame', options: { apiUrl: process.env.TSUBAME_URL } }]),
    // Use the same Markdown pipeline in both builds (below).
    ...(preview ? [] : ['gatsby-transformer-remark']),
  ],
}
```

```javascript
// gatsby-node.js
const path = require('path')

exports.createPages = async ({ actions }) => {
  if (process.env.BUILD_MODE === 'preview') {
    // Just one client-only route. The destination is read from the URL.
    actions.createPage({
      path: '/preview',
      matchPath: '/preview/*',
      component: path.resolve('./src/preview/preview-page.js'),
    })
    return
  }
  // Production: generate every page as before (README §4).
}
```

`src/preview/preview-page.js` only reads with `tsubame-preview` and passes the result to the site's
view components.

```javascript
import React, { useEffect, useState } from 'react'
import {
  parsePreviewRoute, fetchPreview, createContentClient, resolvePreview,
} from 'tsubame-preview'
import { renderMarkdown } from '../markdown'   // the same sequence as the build (below)
import { articleFromPreview } from '../view/article'
import { Article } from '../view/article-view'

export default function PreviewPage({ location }) {
  const [doc, setDoc] = useState(null)
  useEffect(() => {
    const target = parsePreviewRoute(location.pathname)
    const token = new URLSearchParams(location.search).get('token')
    const apiUrl = process.env.GATSBY_TSUBAME_URL
    const client = createContentClient({ apiUrl })
    fetchPreview({ apiUrl, target, token })
      .then(({ schema, values }) =>
        resolvePreview({
          apiUrl, schema, values,
          loadPublished: client.loadPublished,
          loadCompositeSchema: client.loadCompositeSchema,
          renderMarkdown,
        }),
      )
      .then((resolved) => setDoc(articleFromPreview(resolved)))
      .catch((error) => setDoc({ error: error.message }))
  }, [location.pathname, location.search])

  return doc ? <Article doc={doc} /> : null
}
```

### Names and Markdown

- **Field names**: the preview returns the CMS spelling (`published-at`). If you need the same
  GraphQL names as the build, use `previewFieldNames(kind, schema)` (`src/preview-adapter.js`). It
  uses the same planner, so it does not drift from the build.
- **Markdown**: pass **the same pipeline as production** to `renderMarkdown`. The `html` from
  `gatsby-transformer-remark` is `remark.parse` → `mdast-util-to-hast` → `hast-util-to-html`
  (both with `allowDangerousHtml: true`, gfm and footnotes by default). A `gatsby-remark-*` that
  needs files or nodes does not run in the browser, so only ones that touch the AST alone are usable.
- **Differences from production** (`localFile`, `excerpt`, inverse references, and so on) and how
  to absorb them in the view-model are in `docs/preview-site.md` §5 and §6.

### Where to put it

- **AWS**: put it on a subdomain separate from the admin UI (`infra/preview.tf`). The origin is
  separated from the admin UI so that draft HTML is not rendered on the same origin as the admin
  UI's `localStorage` (`docs/preview-site.md` §7).
- **Cloudflare Pages / on-premises**: serve it under a separate host name and set
  `PREVIEW_SITE_URL` in the CMS. An on-premises nginx example is in `docs/preview-site.md` §8.

## 7. What this version cannot do

These are limits decided by the delivery API contract.

- **Drafts are not visible**. That is intentional. Preview is the job of the shared preview URL
  (`docs/content-api.md` §5.6) and a different entry point from the build.
- **A collection with zero published items has zero nodes**. The type is declared, but there are no
  items (because `/api/content/collections` lists only collections that have published items).
- **`Date` / `DateTime` are Gatsby's `Date`**. The raw string stays in `values`.
- When a composite field's definition id is not in the delivery API, that value becomes
  `TsubameComposite` (only `id` and `values`). A warning is emitted when the CMS is older than
  `/api/content/composite-fields`.
- **The plugin does not call the delivery API's `?where=` / `?populate=`.** A build holds all
  published content, so it answers both filtering and expansion of references locally (for the
  reason, see the inverse lookups in §3). A client that reads only a part at runtime should use
  these parameters directly.

## 8. Tests

Neither Gatsby nor the CMS is needed. There are no dependencies (only `fetch` and Node's `node:test`).

```console
$ scripts/test-gatsby-source.sh
# or
$ cd packages/gatsby-source-tsubame && npm test
```

The tests run the two hooks against a fake delivery API shaped like the contract
(`docs/content-api.md`) and with the same arguments Gatsby passes. They cover walking pages,
assigning types, resolving composite definitions, linking references, and creating and linking
Markdown nodes.
