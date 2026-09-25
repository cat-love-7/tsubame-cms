# The contract with the preview site

This is the arrangement that turns a signed preview link (`docs/content-api.md` §5.6) into a
**page that someone without an Account can read**. It defines the interface between the CMS API and
the `/preview/*` the site implements. Gatsby-specific matters are only in §8; everything else applies
to any framework (Next.js, a plain SPA, server-side rendering).

The implementation is in `packages/tsubame-preview/` (a zero-dependency npm package).

## 1. Overall flow

```
Admin UI           POST /api/models/.../preview-link   →  { path: "/api/preview/...?token=...", expires_at }
                   (only when the Schema allows preview. Disabled by default - §7)
                   copy link.path under preview_site_url
Reviewer           GET  {preview_site_url}/preview/...?token=...   ← preview site (static)
Preview site       GET  {cms}/api/preview/...?token=...            ← no auth required. {schema, values}
                   GET  {cms}/api/content/...                       ← published reference targets and composite definitions only
                   render with the site's components
```

**It does not go through a build.** A preview needs no rebuild on every Publish, and no source plugin
such as `gatsby-source-tsubame`. It never fetches published content; the browser reads the API the
moment the reviewer opens it.

## 2. URL shape

| | Shape |
|---|---|
| Path the API issues | `/api/preview/collections/{c}/items/{id}?token=…` |
| | `/api/preview/single_pages/{p}?token=…` |
| URL the reviewer opens | `{preview_site_url}` plus the API path with `/api` removed |

That is, `https://preview.example.com/preview/collections/blog/items/7?token=…`.

- **`preview_site_url` is the origin only** (scheme, host, port). A base path is not allowed. This is
  so that no decision is needed about which side's slash joins the root and the base path
  (`tsubame::config::parse_preview_site_url`).
- **The `token` is carried as is.** The token is the credential itself, and the site only hands it
  back to the API.
- The spelling stays the API's. The preview API uses `single_pages` (underscore), the **delivery** API
  uses `single-pages` (hyphen). To avoid mixing them up, both are written only in `routes.js` of
  `tsubame-preview`.

`parsePreviewRoute(pathname)` in `tsubame-preview` reads the destination from its own URL, and
`previewSiteUrl(apiPath, origin)` does the reverse (the function the admin side uses).

## 3. What the API returns

It is `{ schema, values }`. **Values carry no type tag.** Which value is Markdown, which is an image,
which is a reference - only the `field_type` of the neighbouring `schema` says (`docs/content-api.md`
§3).

| `field_type` spelling | Meaning |
|---|---|
| `"Number"` `"Boolean"` `"Date"` `"DateTime"` `"Image"` | no options, so a bare string |
| `{"Text":{}}` `{"Slug":{}}` `{"Markdown":{}}` `{"CompositeField":{"id":"seo"}}` `{"Relation":{…}}` `{"Array":[…]}` `{"TextEnum":[…]} ` | they have options, so a one-key object |

A reference value is `{"target":"authors","item":7}` (without `item`, a Single page).

## 4. The resolved shape (resolved values)

This is the shape `resolvePreview()` returns. Intended to be **framework-independent**, it is built
only from plain objects, strings and arrays, and field names keep **the CMS spelling (snake_case)**.

| Field type | After resolution | Notes |
|---|---|---|
| `Text` / `Slug` | `string \| null` | |
| `Number` | `number \| null` | |
| `Boolean` | `boolean \| null` | |
| `Date` / `DateTime` | `string \| null` | **kept as an ISO string**. Formatting is the site's job |
| `TextEnum` | `string[]` | |
| `Markdown` | `{ raw, html }` | `html` is the result of the injected `renderMarkdown`. If there is none, `null` |
| `Image` | `{ id, url, absoluteUrl, stableUrl }` | no download is performed. There is no `localFile` |
| `Relation` (single) | resolved object \| `null` | unpublished or not found is `null` |
| `Relation` (multiple) | array \| `[]` | **an unpublished element is kept in place as `null`** |
| `CompositeField` | `{ id, values, …each field of the definition }` | if the definition cannot be read, it stays `{id, values}` |
| `Array` (one element type) | an array resolved per element | |
| `Array` (mixed or unknown element types) | the raw JSON the API returned | no type tags, so it cannot be decided per element |
| Unknown type | the raw value the API returned | |

There are three conventions.

- **References are followed one hop by default** (`relationDepth`, default `1`). The target is fetched
  once and its contents filled in, but references that target holds in turn stay `null`. This is a
  budget so that mutual references do not continue forever; `relationDepth: 2` gives two hops. Only
  **published targets** are resolved; unpublished ones are `null`.
- **Markdown is turned into HTML at read time.** There is no build, so `gatsby-transformer-remark`
  nodes do not exist. The renderer is **injected** (`renderMarkdown`). Pass the same pipeline as
  production and the appearance matches; pass nothing and only `raw` remains. Even if the renderer
  throws, the preview does not go blank; it becomes `html: null` and an `onProblem` notification.
- **A failure does not silently go missing.** If loading a composite definition or a reference target
  fails, it goes to `onProblem`, and only that part stays unresolved.

## 5. Differences from the production build (important)

To "render with the same templates as production", both sides must **be normalized to the same
shape**. First, the things a preview cannot fill in principle are enumerated.

| Production (GraphQL) | Preview | Site-side handling |
|---|---|---|
| `cover { gatsbyImageData }` | `cover { absoluteUrl, stableUrl }` | accept the image as a union (§6) |
| `cover { localFile }` | **none** | `gatsby-transformer-sharp` does not work in the preview |
| `body { childMarkdownRemark { html } }` | `body { html, raw }` | the site injects the same remark pipeline |
| `excerpt` / `timeToRead` / `headings` / `tableOfContents` | **none** | parts of the production template that require these need a branch |
| inverse lookup `articles { … }` | **none** | the preview schema does not carry the referring side's `inverse_name` |
| `Date` scalar | ISO string | align on the view-model side |
| GraphQL names via `fieldNames` | the CMS spelling as is | `previewFieldNames()` (§8) |
| `@link`ed reference nodes | the resolved object | map into the same view-model |

**On Markdown fidelity.** `gatsby-transformer-remark`'s `html` comes from `remark.parse` →
`mdast-util-to-hast` → `hast-util-to-html` (all with `allowDangerousHtml: true`), with `remark-gfm`
and `remark-footnotes` enabled by default. To get the same HTML, you must assemble the same sequence
with **the same version of the same libraries**. `remark-parse` → `remark-rehype` →
`rehype-stringify` does not match (the way raw HTML passes through and the spelling of footnotes
differ). Also, `gatsby-transformer-remark`'s `plugins` are not unified plugins but functions that
receive the Gatsby context (`getNode`, `files`), so **`gatsby-remark-*` that need files or nodes do
not work in the browser**. Ones that touch only the AST can be ported. Using the reference
implementation in the README as is is the safe choice.

## 6. canonical presentation shape (recommended pattern)

To share components between production and preview, decide on one **third shape that is neither**.
This is called the site's canonical shape. Components know only this shape.

```ts
// src/view/article.ts
export type ImageView =
  | { kind: 'sharp'; alt: string | null; source: IGatsbyImageData }  // production only
  | { kind: 'url'; alt: string | null; src: string };                // preview

export interface ArticleView {
  title: string;
  slug: string;
  publishedAt: string | null;      // align to an ISO string
  bodyHtml: string | null;
  cover: ImageView | null;
  author: { name: string; slug: string } | null;
  blocks: BlockView[];
}

// production: from the GraphQL result
export function articleFromGraphQL(node: TsubameBlogItem): ArticleView { … }

// preview: from tsubame-preview's resolved values
export function articleFromPreview(resolved: Record<string, unknown>): ArticleView { … }

// components receive only the view-model
export function Article({ doc }: { doc: ArticleView }) { … }
```

- **Images are absorbed by a union.** This expresses in the type the constraint that "in a preview
  `localFile` is null". A template that calls `getImage()` unconditionally breaks in the preview, so
  branch on `kind`.
- **Fields that cannot be filled in are made optional in the type.** If `excerpt` stays required, you
  end up writing a separate template for the preview, and sharing loses its meaning.
- **The remark configuration in `gatsby-config.js` that the preview uses is passed from one module,
  shared with the build.** Written in two places, it will inevitably drift.
- **One test is enough for verification.** Run a fixture with the same content through both
  `articleFromGraphQL` and `articleFromPreview` and assert that they produce the same `ArticleView`.
  This is the only guarantee of "the same templates as production".

## 7. Security

The preview **does not go through a build and is placed on a different origin from the production
site**. The reason is the admin token.

- The admin JWT sits in `localStorage`, and the code itself states that it is readable from scripts of
  this origin (`frontend/src/app/core/auth/auth.service.ts`). The admin never renders CMS HTML, so for
  now there is no XSS entry point on the same origin. **The preview renders draft Markdown as HTML**,
  so putting it on the same origin makes that the entry point (the production site is a different
  origin, so a published XSS does not reach the admin).
- **A separate subdomain is an origin boundary.** Splitting only the path (`/preview`) shares
  `localStorage`, so it is not a boundary.
- **If the same bucket is shared, deny on the origin side.** If the preview's files are put under
  `preview/` in the admin bucket, the objects themselves **can also be read from the admin
  distribution** (paths with an extension are served as is, without rewriting). That is why the
  admin-side routing function (`infra/app-routing.js`) rejects `/preview` and `/preview/*` with 404.
  Without this, the preview scripts would be served on the admin origin, and the separation above
  would depend on the accident that "the preview app does not render paths it does not know". If you
  split into a separate bucket, this denial is unnecessary (at the cost of more resources).
- The preview is also placed under `X-Robots-Tag: noindex` (`infra/no-index.js` already takes effect
  for the whole distribution). It matters that token-bearing URLs do not appear in search results.
- **Set `Referrer-Policy: no-referrer`.** The token is in the query and leaks through the Referer to
  third-party resources.
- **CSP is a breakwater, not a boundary.** `script-src 'self'` (inline forbidden) closes almost all
  paths by which a `<script>` mixed into Markdown reads `localStorage`, but it works only
  per-document, and it breaks if `unsafe-inline` becomes necessary in the future. A separate origin
  removes this dependency.
- **The token is a credential.** Its lifetime is `PREVIEW_LINK_TTL_MINUTES` (default 60 minutes).
  Queries can remain in CloudFront, S3 and nginx access logs, so pay attention to log retention as
  well.

### Per-Schema permission (disabled by default)

That a preview site exists and **which Schemas may be previewed** are different matters. Each Schema
(Collection, Single page) has `preview`, and **the default is disabled**. Only those for which "allow
preview links" was checked and saved on the Schema edit screen can issue links (administrator
required. `GET`/`PUT /api/models/collections/{name}/settings`,
`/api/models/single_pages/{name}/settings`).

- Issuing for a Schema without permission and opening such a link are both **403
  `preview_disabled`**. The opening side also checks this so that **links already issued die the
  moment the setting is turned off** (this is the §7 means of "revoking", and it avoids rotating
  `JWT_SECRET`).
- Only someone with `can_edit` can issue, and only an administrator can change the permission. To
  avoid creating a path that hands previews to someone who cannot edit, the admin **does not show the
  button at all for Schemas without permission**.
- The setting is a record separate from the Schema definition (`collection_settings` /
  `single_page_settings`; in DynamoDB, `settings`). Saving a Field never writes the setting back, and
  deleting a Collection or page deletes its setting. **Existing deployments have no settings, so even
  Collections that were served until now cannot be issued until an administrator enables it once.**

## 8. Deployment

### AWS (subdomain)

Stand up **a separate CloudFront distribution** from the admin on the preview subdomain
(`infra/preview.tf`). Use the same bucket and set `origin_path = "/preview"` on the origin. Because
the bucket is shared, the admin side can also read `preview/` objects, so the admin distribution
makes `/preview/*` a 404 in `infra/app-routing.js` (§7). The certificate can be reused if the admin's
covers the subdomain (a `*.example.com` wildcard, or a SAN that includes `preview.example.com`).
`PREVIEW_SITE_URL` is passed to Lambda, and `/auth/capabilities` serves it as `preview_site_url`.
Since it becomes a different origin, `CORS_ALLOWED_ORIGINS` also needs the preview origin (Terraform
adds it automatically).

### On-premises

Add a preview `server` (a different host name) to the same nginx that serves the admin. The
admin-side `location` stays as is.

```nginx
# Preview site: a different host name, so a different origin from the admin's localStorage.
server {
    listen 443 ssl;
    server_name preview.cms.example.com;

    root /var/www/tsubame-preview;

    # The token is in the query, so do not leak it via Referer.
    add_header Referrer-Policy "no-referrer" always;
    add_header X-Robots-Tag "noindex, nofollow" always;
    add_header Content-Security-Policy "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' https://cms.example.com; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'" always;

    # Paths without an extension fall through to the site's own root (client-side routing).
    location / {
        try_files $uri /index.html;
    }
}
```

Then set `PREVIEW_SITE_URL=https://preview.cms.example.com` on the CMS. If it is not set,
`preview_site_url` does not appear in `/auth/capabilities`, and the admin shows "preview site not
configured" instead of sharing (it does not copy the raw JSON URL).

### Build

Separate the production build and the preview build with `BUILD_MODE` or similar, and **do not include
`/preview/*` in the production artifacts**. The preview build does not load `gatsby-source-tsubame`
and registers only client-only routes. Examples are in `packages/gatsby-source-tsubame/README.md`.

## 9. Related

- `packages/tsubame-preview/README.md` — the package API and usage examples
- `packages/gatsby-source-tsubame/README.md` — wiring in Gatsby and `previewFieldNames()`
- `docs/content-api.md` §5.6 — the API that issues preview links
