# Changelog

Notable changes, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the numbering follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

One version covers the whole repository: the CMS, the two npm packages, the admin interface and the
git tag carry the same number, checked by `scripts/check-version.sh`. The Rust crates do not repeat
it - each inherits `version.workspace = true` - and the npm packages are published from the same
tag, so "which version of `tsubame-preview` goes with this CMS" has one answer. The Rust crates
themselves are not published at all: `cargo add tsubame-core` is not a way to use this CMS.

## [Unreleased]

### Changed

- No crate is publishable. Every manifest under `backend/crates/` says `publish = false`, so
  `cargo publish` refuses all four by name, and `scripts/check-version.sh` fails the build if a new
  crate forgets to say it.

### Fixed

- A slug can no longer be an array item. `Array([Slug])` used to save and then refuse every value
  written to it, and a slug inside an array could not have been unique anyway - a slug's uniqueness
  is the whole of what it is, and the index is built from a field's own value.
- A blank array element is no longer reported as a missing required field. `null`, an empty options
  list and a null composite are empty *values*, so a working copy and publishing agree about them
  instead of failing only at publish time; `required` still asks the array to hold something.
- Adding a field type to the model is now a compile error in validation, formatting and
  `test_required`, rather than a new type that silently validated as a mismatch and formatted to the
  field's default.

## [0.1.0] - 2026-09-25

The first release. The same CMS runs either on-premises (rkv and local files, one binary) or on AWS
(Lambda, DynamoDB, S3 and CloudFront, nothing idle to pay for); both answer one contract suite.
Content localisation is not in this release - the interface ships in `en` and `ja`, the content does
not. The npm packages are published from short of a build: `tsubame-preview` renders a preview link,
`gatsby-source-tsubame` turns the delivery API into Gatsby GraphQL types.

### Added

- **The schema editor**: fields with a width and a height on a 12-column grid, composite field
  definitions reused across collections and single pages, and arrays of composites that nest.
- **Drafts and publishing**: an edit stays off the site until it is published; an item can be
  unpublished without losing either copy; the working copy can be compared with what is live and
  discarded (`DELETE .../draft`).
- **Relations**, in both directions: an item names others, and the item it names lists its
  referrers. Labels come from the target's own schema, `inverse_name` gives the reverse side a name,
  and publishing and deletion both check the references they would break.
- **Images**: upload or pick from a library, with generated thumbnails, replacement that keeps the
  id, a trash that keeps serving what already referenced it, and a reference index that answers "what
  is using this image" before deleting it.
- **Preview links**: signed, time-limited addresses that render unpublished work on an origin of
  their own. `packages/tsubame-preview` implements the site half with no dependencies, and
  `docs/preview-site.md` is the contract for other frameworks.
- **Sign-in and accounts**: local passwords or Cognito, roles (Viewer / Editor / Publisher),
  per-resource permissions, and account management that works the same way over either provider.
- **Webhooks** on publish, signed with a shared secret.
- **Two interfaces of the API**: `/api/models/*` for the admin screen, `/api/content/*` for site
  builds - published only, no authentication, schema included in the response.
- **`packages/gatsby-source-tsubame`**: builds Gatsby GraphQL types from the CMS schema, with a live
  preview adapter that renders a preview link through the site's own components.
- **The AWS deployment in Terraform**: a Lambda function URL (no API Gateway), DynamoDB on demand,
  two S3 buckets, two CloudFront distributions and Cognito, with the deploy, smoke-test and
  screenshot scripts around it.
- **The design documents** in `docs/`: the content API, relations, the frontend's vocabulary,
  localisation, the preview-site contract, and the AWS decision record.

[Unreleased]: https://github.com/cat-love-7/tsubame-cms/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cat-love-7/tsubame-cms/releases/tag/v0.1.0
