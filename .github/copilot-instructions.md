# Working in this repository

A CMS in two halves: a Rust API (`sl_cms/`) and an Angular admin interface (`frontend/sl_cms/`),
with the AWS deployment in `infra/` and the design decisions in `doc/` (Japanese).

## Layout

| Path | What it is |
|---|---|
| `sl_cms/crates/core` | domain, HTTP layer, services - storage-agnostic |
| `sl_cms/crates/on-premises` | the self-hosted adapter (rkv/LMDB; bin `sl-cms`) |
| `sl_cms/crates/aws` | DynamoDB + S3 + Lambda adapter |
| `sl_cms/crates/tests` | the **contract suite**: one set of tests, run against both adapters |
| `frontend/sl_cms/src/app` | `repositories/` (HTTP) and `services/` (state) below the screens |
| `infra/` | Terraform |
| `doc/` | design documents, in Japanese: `content-api.md` is the API contract |

Both adapters lay their repositories out the same way, and it is worth keeping that way: the
traits live in `core/src/repositories/<trait>.rs` (one per trait), an adapter implements them in
`<adapter>/src/repository/<resource>.rs` (one per *resource*: `images`, `collections`,
`single_pages`, `composite_fields`, `users`), and the module file (`repository.rs`) holds what is
shared - the key layout, the read/write helpers, and the tests that span resources. A file name
that says "repository" tells a reader nothing they did not already know from the directory.

A directory module anywhere in the workspace is `<dir>.rs` beside `<dir>/` (`core/src/auth.rs`,
`core/src/http.rs`) - never `<dir>/mod.rs`. The one exception is a module pulled in with `#[path]`,
where the file's stem does not become the module directory, so its submodules would be looked for
in the wrong place (`crates/tests/suite/mod.rs` says so itself).

## Running things

```bash
./scripts/test-rust.sh        # both adapters; the AWS half needs the emulators
./scripts/test-frontend.sh    # ng test (Vitest) + ng build
cd frontend/sl_cms && npm run lint && npm run format:check
./scripts/test-e2e.sh         # real browser against a real backend, own servers on 8080/4200
docker compose -f sl_cms/docker-compose.yml up -d   # DynamoDB Local + MinIO
(cd infra && terraform fmt -check -recursive && terraform init -backend=false && terraform validate)
(cd infra/bootstrap && terraform init -backend=false && terraform validate)
```

## Rules that are easy to get wrong

- **Comments explain why, not what.** A comment that restates the line below it is noise; one that
  says what the alternative would break is the point.
- **Docs and UI text**: `doc/` is Japanese, code comments and commit messages are English.
- **Contract tests are written once** in `crates/tests/suite/` (one file per topic; `mod.rs` is the
  harness) and must pass against both adapters. Adapter-specific tests (emulators, presigning,
  permissions) live with the adapter.
- **Refusal codes are a contract across three places**: the Rust lists in
  `core/src/models/error.rs`, `frontend/sl_cms/src/assets/error-codes.json`, and the
  `errors.<code>` entries in `assets/i18n/en.json` / `ja.json`. A Rust test fails if the first two
  drift, and a frontend test fails if a catalogue is missing a key.
- **Every UI string goes through Transloco**; `keys.spec.ts` fails on a key that is in one
  catalogue and not the other.
- **The frontend lint reads the types** (`eslint.config.mjs`, `npm run lint`): a floating promise,
  a leaked `any` or an unused import is an error. A spec types its fixture with `TypedFixture`
  (`app/core/testing/fixture.ts`), never Angular's `ComponentFixture`, whose `nativeElement` is
  `any` and makes every query on it unchecked.
- **A 500 says nothing.** Internal detail goes to the log (`map_internal_error`), never to the body.
- **The size limits are the deployment's, not the CMS's**: `config::Limits` reads
  `MAX_REQUEST_BYTES` (1MB), `MAX_IMAGE_BYTES` (10MB) and `MAX_RESPONSE_BYTES` (4MB), and
  `http::body_limit` plus the delivery API's page cut apply them. The refusal is
  `payload_too_large`, and `/auth/capabilities` reports the image one so a browser can refuse a
  file before sending it. `infra/` passes each only when an operator sets it, so an unset
  deployment follows the code when a default changes.
- **The frontend never hardcodes `/api`**: use `apiUrl()`. Screens read route parameters from
  `route.paramMap` / `route.queryParamMap` (never `route.snapshot`): the router reuses a component
  when only a parameter changes. A screen that holds edits implements `HasUnsavedChanges` so
  `unsavedChangesGuard` can ask.
- **TypeScript 6 needs an explicit `rootDir`** in a project that emits: `tsconfig.app.json` sets it
  beside `outDir`, because without it `tsc -p tsconfig.app.json` is TS5011 - while `ng build`, which
  type-checks with `noEmit`, stays green and hides it.
- **API types keep the server's `snake_case`** (`published_at`, `has_draft`): there is no mapping
  layer, and adding one to "fix" the casing is not wanted. Component state and inputs are
  camelCase.
- **Rust naming**: types UpperCamelCase with `Id` (not `ID`), functions and fields snake_case.
  `cargo build` and `cargo test` must be warning-free; `cargo clippy` has older lints that are
  not yet worth a rewrite - do not add new ones.
- **Terraform mirrors the deployment's S3 policy** in `deployment_s3_policy`
  (`crates/aws/src/policy.rs`); changing one means changing the other (`doc/aws-plan.md`).
- **Terraform state is remote** (`infra/` uses the S3 backend with `use_lockfile`, configured
  through `infra/backend.hcl`). `infra/bootstrap/` is the one root applied by hand, once: it makes
  the state bucket, and it is the only root whose state is local.
