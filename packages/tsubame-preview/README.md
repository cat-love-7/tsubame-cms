# tsubame-preview

A small package for rendering `Tsubame` **signed preview links** as pages without going
through a build. It **depends on neither Gatsby nor React** (zero dependencies, just
`fetch` and plain objects).

The full contract lives in `docs/preview-site.md`. This README covers usage and the API only.

## What it does

```
Reviewer opens                       https://preview.example.com/preview/collections/blog/items/7?token=…
Preview site reads                   GET https://cms.example.com/api/preview/collections/blog/items/7?token=…  → {schema, values}
Reads related items (published only) GET https://cms.example.com/api/content/authors/items/9
Reads composite field definitions    GET https://cms.example.com/api/content/composite-fields
```

1. Read the destination with `parsePreviewRoute(location.pathname)`
2. Fetch the working copy with `fetchPreview({ target, token })`
3. Resolve it into a page-readable shape with `resolvePreview({ schema, values, renderMarkdown, ... })`
4. Render it with the site's own components

```console
$ npm install ../path/to/packages/tsubame-preview
```

## Usage (framework-agnostic)

```js
import {
  parsePreviewRoute,
  fetchPreview,
  createContentClient,
  resolvePreview,
} from 'tsubame-preview';

const target = parsePreviewRoute(window.location.pathname); // {kind:'collection', collection:'blog', id:7}
const token = new URLSearchParams(window.location.search).get('token');

const apiUrl = 'https://cms.example.com';
const client = createContentClient({ apiUrl });

const { schema, values } = await fetchPreview({ apiUrl, target, token });

const resolved = await resolvePreview({
  apiUrl,
  schema,
  values,
  loadPublished: client.loadPublished,
  loadCompositeSchema: client.loadCompositeSchema,
  // Pass the same pipeline as production (recommended). If omitted, Markdown stays raw.
  renderMarkdown: (raw) => myMarkdownPipeline(raw),
});
```

If the API is on the same origin, `apiUrl` can be `''` (paths stay `/api/...`).

## API

### Where the preview is

| Function | Returns |
|---|---|
| `previewApiPath(target, apiPrefix?)` | The API path (`/api/preview/collections/blog/items/7`) |
| `previewRoutePath(target)` | The preview site route (`/preview/collections/blog/items/7`) |
| `parsePreviewRoute(pathname)` | The `target` the site URL points to, or `null`. The base path does not matter |
| `previewSiteUrl(apiPath, siteOrigin, apiPrefix?)` | The URL the admin UI copies (carries the token with it) |
| `checkTarget(target)` | An explanation if invalid, `null` if valid |

`target` is either `{ kind: 'collection', collection, id }` or `{ kind: 'single_page', page }`.

### Reading the CMS

| Function | Returns |
|---|---|
| `fetchPreview({ apiUrl, target, token, apiPrefix?, fetchImpl? })` | `{schema, values}`. Rejections are an `Error` with `status` |
| `createContentClient({ apiUrl, apiPrefix?, fetchImpl? })` | `{ loadPublished, loadCompositeSchema }`. A 404 is `null` |
| `joinUrl(base, path)` | Naive URL joining |

### Resolving values

`resolvePreview(options) → Promise<resolved>`. The options are as in the table in `docs/preview-site.md` §4.

| Option | Default | Meaning |
|---|---|---|
| `schema` / `values` | (required) | What `fetchPreview` returned |
| `loadPublished` | `async () => null` | Reads published related items. `null` means "unpublished" |
| `loadCompositeSchema` | `async () => null` | Reads composite field definitions |
| `renderMarkdown` | none | `(raw) => string \| Promise<string>`. **Pass the same one as production** |
| `apiUrl` / `apiPrefix` | `''` / `'/api'` | Building the image `absoluteUrl` / `stableUrl` |
| `relationDepth` | `1` | How many hops to follow references for. With `0`, references are `null` |
| `onProblem` | none | Receives `{path, message}`. Reports without stopping rendering |

## Matching production

- **Markdown**: pass the same pipeline as production to `renderMarkdown`.
  The `html` from `gatsby-transformer-remark` is `remark.parse` → `mdast-util-to-hast` →
  `hast-util-to-html` (both with `allowDangerousHtml: true`, gfm and footnotes by default).
  `remark-parse` → `remark-rehype` → `rehype-stringify` does not match.
- **Images**: they are not downloaded. Use `absoluteUrl` (§4).
- **What is missing**: `localFile` / `gatsbyImageData`, `excerpt` / `timeToRead` / `headings` /
  `tableOfContents`, inverse references. Absorb them in the site's own view-model (`docs/preview-site.md` §6).

## For Gatsby sites

Using `previewFieldNames()` from `packages/gatsby-source-tsubame` gives the same GraphQL names
as the build (`published-at` → `published_at`). That adapter is all there is; the rest is the
view-model's job. See the "Live preview" section of `gatsby-source-tsubame/README.md` for details.

## Tests

```console
$ npm test
# or
$ scripts/test-preview.sh
```

No network, CMS, or Gatsby is needed. `fetch` is injectable, and the delivery API is
replaced with fake responses shaped like `docs/content-api.md`.
