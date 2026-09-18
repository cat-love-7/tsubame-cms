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

Both adapters lay their repositories out the same way, and it is worth keeping that way: the
traits live in `core/src/repositories/<trait>.rs` (one per trait), an adapter implements them in
`<adapter>/src/repository/<resource>.rs` (one per *resource*: `images`, `collections`,
`single_pages`, `composite_fields`, `users`), and the module file (`repository.rs`) holds what is
shared - the key layout, the read/write helpers, and the tests that span resources. A file name
that says "repository" tells a reader nothing they did not already know from the directory.
| `infra/` | Terraform |
| `doc/` | design documents, in Japanese: `content-api.md` is the API contract |

## Running things

```bash
./scripts/test-rust.sh        # both adapters; the AWS half needs the emulators
./scripts/test-frontend.sh    # ng test (Vitest) + ng build
./scripts/test-e2e.sh         # real browser against a real backend, own servers on 8080/4200
docker compose -f sl_cms/docker-compose.yml up -d   # DynamoDB Local + MinIO
cd infra && terraform fmt -check && terraform validate
```

## Rules that are easy to get wrong

- **Comments explain why, not what.** A comment that restates the line below it is noise; one that
  says what the alternative would break is the point.
- **Docs and UI text**: `doc/` is Japanese, code comments and commit messages are English.
- **Contract tests are written once** in `crates/tests/suite.rs` and must pass against both
  adapters. Adapter-specific tests (emulators, presigning, permissions) live with the adapter.
- **Refusal codes are a contract across three places**: the Rust lists in
  `core/src/models/error.rs`, `frontend/sl_cms/src/assets/error-codes.json`, and the
  `errors.<code>` entries in `assets/i18n/en.json` / `ja.json`. A Rust test fails if the first two
  drift, and a frontend test fails if a catalogue is missing a key.
- **Every UI string goes through Transloco**; `keys.spec.ts` fails on a key that is in one
  catalogue and not the other.
- **A 500 says nothing.** Internal detail goes to the log (`map_internal_error`), never to the body.
- **The frontend never hardcodes `/api`**: use `apiUrl()`. Screens read route parameters from
  `route.paramMap` (never `route.snapshot`), and a screen that holds edits implements
  `HasUnsavedChanges` so `unsavedChangesGuard` can ask.
- **API types keep the server's `snake_case`** (`published_at`, `has_draft`): there is no mapping
  layer, and adding one to "fix" the casing is not wanted. Component state and inputs are
  camelCase.
- **Rust naming**: types UpperCamelCase with `Id` (not `ID`), functions and fields snake_case.
  `cargo build` and `cargo test` must be warning-free; `cargo clippy` has older lints that are
  not yet worth a rewrite - do not add new ones.
- **Terraform mirrors the deployment's S3 policy** in `deployment_s3_policy`
  (`crates/aws/src/lib.rs`); changing one means changing the other (`doc/aws-plan.md`).
