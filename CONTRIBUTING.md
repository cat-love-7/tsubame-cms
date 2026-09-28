# Contributing

Thanks for looking. This file is how to get a change from an idea to a merged commit without
guessing at the conventions.

## What you need

* **Rust 1.98+** (the workspace is edition 2024) - `rustup` is enough.
* **Node 24+** for the admin app, and for the two packages under `packages/`.
* **Docker** - only for the AWS half: `docker compose -f backend/docker-compose.yml up -d` starts
  DynamoDB Local and an S3 gateway, which the `tsubame-aws` tests run against.
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
| `scripts/test-gatsby-build.sh` | the plugin under a real `gatsby build` | `npm install` in `packages/gatsby-source-tsubame` (it is skipped without it) |
| `scripts/test-preview.sh` | the preview renderer | `node` |
| `scripts/check-package-types.sh` | the packages' JSDoc types (`tsc --noEmit`, `checkJs`) | `npm ci` in `frontend` (it borrows that toolchain) |
| `scripts/coverage.sh` | coverage for the Rust and Angular suites | `cargo-llvm-cov` |
| `scripts/smoke-test.sh <url>` | a deployment, over HTTP | nothing (no credentials) |

**A change is not ready until the suites it can affect are green.** The browser suite is the one
that catches what the unit tests cannot: it drives the interface, the API and the storage
together, and more than one real bug in this project was only visible there.

## The pictures in the README, and the favicon

`docs/images/*.png` are generated rather than drawn. `scripts/screenshots.sh` starts a CMS with
throwaway data, seeds a small site (four categories, five articles, one page, four images - see
`frontend/e2e/screenshots.mjs`), and photographs the screens with a real browser. It wants the same
Chromium as the browser suite and the same two free ports.

Run it when a change moves something a reader would see, and commit what it writes: a picture that
no longer matches the screen is worse than no picture. If a screen looks wrong while you are at it,
that is usually the cheapest way to find a layout bug - two of them were found this way.

The tab icon is `frontend/public/favicon.svg`: the mark from `brand/tsubame-16.svg`, painted on a
plate of the brand's ink, because a tab strip belongs to the browser - an icon inherits no colour from
the page, and asking the browser whether it is in dark mode is not answered everywhere (WebKit ignores
`prefers-color-scheme` inside an SVG icon, which is what left near-black on near-black there).
`frontend/public/favicon.ico` is that same file rasterised at 16/32/48px by
`cd frontend && node e2e/favicon.mjs`, so the browsers that predate SVG icons load the same picture
rather than a second drawing. A redrawn bird therefore means updating the copy in the SVG and running
that. `scripts/test-e2e.sh` paints both files over a light tab strip and a dark one and reads the
pixels back, so an icon that only reads on one of them is a failing check rather than a reader's
report.

## The conventions that matter

* **Code, comments, commit messages, the design documents in `docs/` and this file are English.**
  The documents are part of the repository's prose, not a translation laid beside it.
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

## Releasing

One version covers the whole repository: the CMS, both npm packages, the admin interface and the git
tag carry the same number, and `scripts/check-version.sh` fails the build if they drift. The Rust
crates inherit `version.workspace = true` rather than repeating it.

Before `1.0.0` there is no major number to move, so the convention here is that **a breaking change
is a minor bump** and a fix is a patch: `0.1.x` is compatible with `0.1.0`, and `0.2.0` is where the
relation model changed. A release that only adds something is a minor bump too; there is no
"compatible feature" rung below it while the number starts with zero.

1. Bump the number in all four places: `backend/Cargo.toml` (`[workspace.package]`), both
   `packages/*/package.json`, and `frontend/package.json` - private, but it carries the number so
   that `grep 0.2.0` finds every place at once. `frontend/package-lock.json` repeats it, because
   `npm ci` compares the two: run `npm install --package-lock-only` in `frontend/` (or let the check
   below name the file for you).
2. Move `CHANGELOG.md`'s `Unreleased` section under the new number with the date.
3. Run the release check with the tag the commit is about to get: `scripts/check-version.sh v0.2.0`.
4. Tag it and push the tag: `git tag -a v0.2.0 -m v0.2.0`, then `git push origin v0.2.0`.
5. Create the GitHub release from the tag, with the changelog section as its notes.
6. Publish the packages when the release carries something for them:

```bash
cd packages/tsubame-preview
npm pack --dry-run          # what would ship: the `files` list, plus README and LICENSE
npm publish
```

Both are already `"license": "MIT"` with the licence text beside them, and neither has a dependency;
`gatsby-source-tsubame` declares its `peerDependencies.gatsby` range, which needs widening when
Gatsby's major moves. If a package ever has to move on its own - a Gatsby major that the CMS should
not wait for - that is the moment to give it its own number and drop it from the check.

## Reporting a security issue

Please do not open a public issue for a vulnerability. Use
[https://github.com/cat-love-7/tsubame-cms/security/advisories/new](https://github.com/cat-love-7/tsubame-cms/security/advisories/new) - GitHub's private advisory form -
with steps to reproduce, and give us a chance to fix it before it is published. [`SECURITY.md`](SECURITY.md)
says what is in scope and what to expect.
