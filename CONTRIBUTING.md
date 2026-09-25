# Contributing

Thanks for looking. This file is how to get a change from an idea to a merged commit without
guessing at the conventions.

## What you need

* **Rust 1.98+** (the workspace is edition 2024) - `rustup` is enough.
* **Node 24+** for the admin app, and for the two packages under `packages/`.
* **Docker** - only for the AWS half: `docker compose -f backend/docker-compose.yml up -d` starts
  DynamoDB Local and MinIO, which the `tsubame-aws` tests run against.
* **Chromium for Playwright** - only for the browser suite, which has its own instructions in
  `frontend/e2e/README.md`.

Nothing needs a cloud account. The self-hosted backend keeps its data in a directory, and the AWS
tests run against the emulators.

## Getting set up

```bash
cd backend && cargo build          # the API
cd ../frontend && npm ci           # the admin app
```

To run it, see "Run it locally" in the [README](README.md). `.env.example` lists every setting the
two binaries read.

## Running the tests

| Command | What it covers | Needs |
|---|---|---|
| `scripts/test-rust.sh` | the core, both storage adapters, and the contract suite against each | Docker for the AWS half (it is skipped when the emulators are down) |
| `scripts/test-frontend.sh` | the Angular specs | `npm ci` |
| `scripts/test-e2e.sh` | a real browser against a real server, end to end | Chromium; ports 8080 and 4200 free |
| `scripts/test-gatsby-source.sh` | the Gatsby source plugin | `node` |
| `scripts/test-preview.sh` | the preview renderer | `node` |
| `scripts/coverage.sh` | coverage for the Rust and Angular suites | `cargo-llvm-cov` |
| `scripts/smoke-test.sh <url>` | a deployment, over HTTP | nothing (no credentials) |

**A change is not ready until the suites it can affect are green.** The browser suite is the one
that catches what the unit tests cannot: it drives the interface, the API and the storage
together, and more than one real bug in this project was only visible there.

## The conventions that matter

* **Code, comments, commit messages and this file are English.** The design documents in `doc/` are
  Japanese, and stay that way.
* **One change per commit**, with a subject in the imperative mood ("Add the images bucket
  override") and a body that says *why* - what was wrong, what it costs, what was measured. The
  history is the design record; a commit that only restates its diff is a lost paragraph.
* **Behaviour-preserving refactors and behaviour changes do not share a commit.** The first is
  reviewed by reading the diff, the second by reading the tests.
* **Tests live next to what they test**, and a bug fix comes with the test that would have caught
  it.
* **The storage adapters stay interchangeable.** Anything that both backends must do belongs in
  the contract suite (`backend/crates/tests`), not in one adapter's tests, so that "works on
  DynamoDB" and "works on LMDB" remain one claim.
* **Two contracts are enforced by tests, and both will fail loudly if you forget them**: every
  translation key used in a template must exist in both catalogs (`frontend/src/app/core/i18n/keys.spec.ts`),
  and `frontend/src/assets/error-codes.json` must match the codes the Rust side defines.

`.github/copilot-instructions.md` is the same conventions in short form, next to the code.

## Sending a change

1. Fork, branch, make the change with its tests.
2. Run the suites it can affect; say in the pull request which ones you ran.
3. Explain the *why* in the description if the commit body does not already.

There is no CLA and no copyright assignment: contributions are accepted under the
[MIT licence](LICENSE), the same terms the project is released under.

## Releasing the packages

`packages/gatsby-source-tsubame` and `packages/tsubame-preview` are published to npm on their own
schedule, not with the CMS:

```bash
cd packages/tsubame-preview
npm pack --dry-run          # what would ship: the `files` list, plus README and LICENSE
npm publish
```

Both are already `"license": "MIT"` with the licence text beside them, and neither has a
dependency. Two things want doing before the first publish: bump `version` in the manifest (and
the `peerDependencies.gatsby` range when Gatsby's major moves), and add `repository` (with
`directory: "packages/<name>"`), `homepage` and `bugs` pointing at the repository - this file
cannot name the URL until the project is public.

## Reporting a security issue

Please do not open a public issue for a vulnerability. Use the repository's private
**Report a security vulnerability** form (GitHub's security advisories) with steps to reproduce,
and give us a chance to fix it before it is published.
