# Tsubame

A CMS with a schema an editor draws and an API a site builds from: collections and single pages
whose fields (text, Markdown, numbers, dates, images, relations, arrays, composite blocks) are
defined in the admin screen, edited as drafts, published one item at a time, and read back by a
site through a content API.

Two halves, and two storage backends behind one set of contracts:

* **`backend/`** - the API, in Rust (axum). One workspace, three crates that matter: `core` (models,
  services, HTTP layer, no storage of its own), and the two adapters - `on-premises` (rkv/LMDB and
  image files on disk) and `aws` (DynamoDB, S3, Lambda). The same contract suite runs against
  both, so "works here" and "works there" are the same claim.
* **`frontend/`** - the admin interface, an Angular application (standalone components, signals,
  zoneless). It is served by the same origin as the API in a deployment.

Around them:

```
backend/                        the Rust workspace (API and Lambda)
frontend/                       the Angular admin interface
packages/
  gatsby-source-tsubame/        a Gatsby source plugin for the content API
  tsubame-preview/              renders a signed preview link without a build
infra/                          Terraform: the AWS deployment
scripts/                        build, test and deploy
doc/                            the design documents (Japanese)
brand/                          the mark and the lockup
```

## What it does

* **A schema you draw.** Fields with widths and heights on a 12-column grid; composite field
  definitions reused across collections and pages; arrays of composites that nest to any depth.
* **Drafts and publishing.** Edits stay unpublished until they are published, an item can be
  unpublished, and the working copy can be compared against what is live and discarded.
* **Relations**, with inverses: an item can name others, and the item it names can list its
  referrers. Labels come from the target's own schema, so a reference reads as the item.
* **Images**, uploaded or picked from a library, with thumbnails. Content stores the durable
  address, never a signature that expires.
* **Preview links**: signed, time-limited addresses that render unpublished work for a reviewer,
  on an origin of their own.
* **Sign-in** with passwords (self-hosted) or Cognito (AWS), accounts with roles, and per-resource
  permissions.
* **Webhooks** on publish, signed with a shared secret.

| The items, and what state each one is in | One item |
|---|---|
| ![A collection's items, with covers, a relation column and publish state](doc/images/articles.png) | ![Editing an article: a title, a slug, an image, a date, a relation and a Markdown body](doc/images/editor.png) |

| The schema, drawn field by field | The image library |
|---|---|
| ![The schema editor: field cards with type, width, height and the title and list flags](doc/images/schema.png) | ![The image library, with thumbnails and per-image actions](doc/images/library.png) |

| A single page | Signing in |
|---|---|
| ![A single page: a heading, a hero image and Markdown intro](doc/images/page.png) | ![The sign-in screen](doc/images/sign-in.png) |

A site reads the published copy over HTTP: untyped values beside the schema that gives them
meaning, paginated, and only what has been published (`doc/content-api.md` is the whole contract).

```bash
curl 'https://cms.example.com/api/content/collections/articles?limit=2'
```

```json
{
  "schema": [
    { "name": "title", "field_type": { "Text": {} }, "required": true, "is_title": true, "width": 8 },
    { "name": "cover", "field_type": "Image", "required": false, "width": 4 },
    { "name": "category", "field_type": { "Relation": { "target": { "kind": "collection", "name": "categories" } } } },
    { "name": "body", "field_type": { "Markdown": {} }, "required": false, "width": 12 }
  ],
  "items": [
    {
      "id": 5,
      "published_at": "2026-09-25T05:53:12+00:00",
      "last_published_at": "2026-09-25T05:53:12+00:00",
      "values": {
        "title": "Notes from the first deployment",
        "cover": { "id": 1, "url": "https://cms.example.com/api/images/by-id/1" },
        "category": [{ "target": "categories", "item": 4 }],
        "body": "## What an apply does not make"
      }
    }
  ],
  "total": 4,
  "limit": 2,
  "offset": 0,
  "next_offset": 2
}
```

The pictures above come from `scripts/screenshots.sh`, which seeds a small site and takes them;
run it again after an interface change and they follow.

## Run it locally

Needs Rust 1.98+ (edition 2024) and Node 24+. Nothing else: the on-premises backend keeps its data
in a directory and needs no database and no cloud account.

```bash
cd backend
JWT_SECRET="$(openssl rand -base64 48)" \
ADMIN_USERNAME=admin@example.com ADMIN_PASSWORD=change-me \
cargo run                      # http://127.0.0.1:8080, data in backend/data
```

```bash
cd frontend
npm ci
npm start                      # http://localhost:4200, /api proxied to 127.0.0.1:8080
```

The first account is created from `ADMIN_USERNAME` / `ADMIN_PASSWORD` at startup, and the server
refuses to start with an empty user store and no credentials - it does not come up unauthenticated.
Every setting the two binaries read is in [`.env.example`](.env.example); the server reads the
environment, not a file, so load it however you like (`set -a && . ./.env && set +a`, or
`docker run --env-file`).

## Test it

```bash
scripts/test-rust.sh            # core, both adapters, and the contract suite against each
scripts/test-frontend.sh        # the Angular specs (vitest)
scripts/test-e2e.sh             # a real browser against a real server, end to end
scripts/test-gatsby-source.sh   # the Gatsby plugin
scripts/test-preview.sh         # the preview renderer
```

The AWS halves want the emulators (`docker compose -f backend/docker-compose.yml up -d`: DynamoDB
Local and MinIO); `test-rust.sh` skips them when nothing is listening. The end-to-end suite needs
Chromium for Playwright - see `frontend/e2e/README.md`.

## Deploy it

**Self-hosted**: build one binary and run it behind whatever serves the built app
(`cd frontend && npm run build` writes `dist/tsubame/browser`).

```bash
cd backend && cargo build --release -p tsubame-on-premises
```

**AWS**: Terraform creates the DynamoDB table, the two S3 buckets, the Cognito pool, the
CloudFront distribution and the Lambda function; `scripts/build-lambda.sh` builds the artifact and
`scripts/deploy-frontend.sh` delivers the app. What the deployment needs, what the operator
creates by hand, and how to verify it are in [`infra/README.md`](infra/README.md). The app itself
holds no environment: it asks `/api/auth/capabilities` what its deployment can do, so one build
serves every deployment.

## Design

`doc/` holds the reasoning rather than the summary - the content API (`doc/content-api.md`), the
schema and relations (`doc/relations-design.md`), the frontend's boundaries
(`doc/frontend-design.md`), the AWS plan (`doc/aws-plan.md`), and the preview site
(`doc/preview-site.md`). They are written in Japanese; the code and its comments are in English.
`.github/copilot-instructions.md` is the same conventions in short form.

## Contributing

Issues and pull requests are welcome - [CONTRIBUTING.md](CONTRIBUTING.md) has the setup, the
suites to run, and the conventions a change is reviewed against. The licence needs no CLA.

## License

[MIT](LICENSE).
