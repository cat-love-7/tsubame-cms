# Changelog

Notable changes, newest first. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the numbering follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

One version covers the whole repository: the CMS, the two npm packages, the admin interface and the
git tag carry the same number, checked by `scripts/check-version.sh`. The Rust crates do not repeat
it - each inherits `version.workspace = true` - and the npm packages are published from the same
tag, so "which version of `tsubame-preview` goes with this CMS" has one answer. The Rust crates
themselves are not published at all: `cargo add tsubame-core` is not a way to use this CMS.

## [Unreleased]

## [0.2.0] - 2026-09-27

### Added

- **A deployment can say what it is the admin screen for.** `SITE_NAME` (a Terraform variable, or an
  environment variable for a self-hosted deployment) is reported by `/auth/capabilities` as
  `site_name` and drawn in the app bar - with `Tsubame` under it, so neither name is lost - on the
  sign-in card, and in the browser tab title. Until now every deployment showed the product's name
  alone, which says nothing to somebody who administers two sites with it. The name is the
  operator's wording and is not translated: it is one line, at most 80 characters, and a value that
  would not fit the places it is drawn in (a newline, or a paragraph) stops the process at startup
  rather than being cut. A deployment that sets nothing shows the product's name, exactly as before.

### Changed

- **A relation holds one reference, and several references are an `Array` of relations.** The
  `has_many` flag is gone: `Relation` is `{ target, inverse_name }` and its value is
  `{ "target": "authors", "item": 7 }` (or `null`), while several are `Array([Relation(…)])` with an
  array value (or `[]`) - and one array may name **several different targets**, one item type each.
  This is a breaking change to the management and delivery APIs: a single relation that used to read
  `[{ … }]` now reads `{ … }`, and a field that said `has_many: true` is now an `Array` of relations.
  Stored content and schemas have to be migrated (the Strapi import is run again for that).
- **Delivery expansion names a target when a field references several**:
  `?populate=related.authors`, and `?where=related.authors:3` for the reverse filter.
  `?populate=<field>` keeps working while the field declares one target;
  `?populate=<inverse_name>` is unchanged.
- **`gatsby-source-tsubame` types a multi-target relation array as a GraphQL union** of the targets'
  node types, named after the type and field that own it (`TsubameBlogItemRelated`), so every element
  is still the node it names and a query names the member it wants with an inline fragment. That
  shape used to be `[TsubameRelationRef]`, so a site that queried it as a reference must change the
  query, and the `TsubameRelationRef` type is gone.
- **A relation target the CMS does not answer is warned about and left out of the schema**, instead
  of being served as a reference that could never hold anything. The delivery API drops references it
  cannot resolve, and `/api/content/collections/{name}` only answers 404 when the collection does not
  exist, so this is a schema naming something somebody deleted: the whole field is left out, or just
  that target when the field has others (an array that loses one target of two becomes a plain list).
  A site that queries such a field fails with "Cannot query field", with the warning above saying
  why; the raw value stays readable under `values`.
- **A refusal that is about a value now says which value, and which Item.** `code`s that mean "that
  value is taken" carried the detail in the English `message` only, so a screen showing its own
  wording said "a value is already used by another item" and left the reader to find out which.
  Responses now carry a `details` object (`{"value":"intro","owner":"1"}`), and a Schema save that
  makes a field unique or a slug **answers codes of its own** - `duplicate_values` (two stored Items
  hold one value; both are named) and `slug_not_canonical` (a stored value has to become a slug
  first; the Item is named) - instead of `value_taken` / `invalid_slug`. A client keying on those
  two has to accept the new ones for a Schema save; a save of an Item is unchanged apart from
  `details`. The screen fills its wording with them.
- **Adding an account that the identity provider already had no longer resets its password.** The
  create answer carries `needs_credential`, which is `true` only for an account the provider had to
  create: an account that was already there (made in the provider's console, or coming back after the
  CMS's own records were replaced) has its owner's own way in, and the screen adds the record without
  issuing a reset. A deployment that holds the passwords itself has nothing to adopt from, so it always
  answers `true`.
- **`inverse_name` is one name per target**, and a schema can no longer give one target two names -
  neither through two Fields nor through two item types of one array.
- **The Strapi import carries the media library's thumbnails.** This CMS's small tile copy is made
  by the browser that uploads a picture (`docs/content-api.md` §5.9), so a migrated library had none
  and every tile in the library and the pickers downloaded the original. The tool now sends the
  `formats.thumbnail` Strapi generated, and a file Strapi made no copy of falls back to its original
  the way an image uploaded through the API does. An image a re-run reuses is left alone, so
  `--force` is what re-uploads a library imported before this.

- No crate is publishable. Every manifest under `backend/crates/` says `publish = false`, so
  `cargo publish` refuses all four by name, and `scripts/check-version.sh` fails the build if a new
  crate forgets to say it.
- The image library opens as a **dialog** with an **upload** of its own, rather than a wall of tiles
  under the control that opened it. The Markdown box had no upload at all, so an empty library was a
  dead end there. The same picker serves the image field, the image array and the Markdown image
  button, and it closes as soon as an image is chosen.
- An item editor offers **save in the toolbar** as well as at the end of the form, so "save" and
  "save and publish" are no longer a screen apart, and **saving no longer returns to the list** - a
  second edit used to mean finding the item again.
- **Copying an item asks first**, like every other button on that row (delete, publish, unpublish,
  discard): the copy is created and opened only once the question is answered.
- **A list row opens its item.** Clicking anywhere on a row of a Collection list opens that item's
  editor, which is what clicking a page's name already did in the Single page list - the two screens
  now answer the same click the same way. The cells holding a control of their own (the selection
  checkbox, the row's buttons) keep their clicks, and the edit icon stays for the keyboard and for a
  name to read.
- **A row's buttons stay in reach however many columns the Schema shows.** Copy and delete moved
  behind the row's **⋮** menu, and the actions column is pinned to the right edge of the list: a Schema
  marking many fields for the list made the table wider than the window, and the buttons travelled off
  it with the last columns. What the account may do is still the only thing the menu holds.
- **The comparison of a draft against what is published is sized to its content.** It used to draw
  each side with the box the Schema asks for, so a paragraph sat under the height of a twelve-row
  editor; a multi-line Text or Markdown value is now rendered as text, and the cells inside a
  Composite no longer keep their layout minimum there.
- The way out of an item editor now reads **"Back to the list without saving"** (it was "Cancel",
  which said neither what it does nor where it goes).
- An item editor's toolbar reads **state first, then the actions**: "save" sits after the status and
  the publisher and before the publish controls, rather than ahead of both.
- The publisher's name in the toolbar is **labelled** ("Published by:"). Bare, it read as the author
  of the content; it is the Account that published it. In the content lists the column heading says it
  once for the whole column - **"Status (publisher)"** - instead of a label on every row.

### Fixed

- **Scrolling a list sideways no longer carries the toolbar and the pager with it.** The table, the
  toolbar that holds "new item" and the pager shared one scroll box, so a table wider than the window
  (a Schema with many list columns) moved the button that adds an item out of the way whenever anybody
  looked at the right-hand columns. The table scrolls in a box of its own now: the screen around it
  stays put, and the pinned actions column still comes to rest exactly where the table ends.
- **The pinned actions column no longer covers the end of the last column.** A table wider than the
  screen overflows into the scroll area's own gutter, so the column pinned to its right edge came to
  rest one gutter's width short of where the table ends: scrolled all the way right, the last 24px of
  the last data column - the end of a timestamp - sat under the buttons. The pin is measured from the
  gutter now, which is the same edge the overflow reaches; a table that fits is unaffected, because a
  pin only moves when there is something to scroll.
- **The tab icon no longer disappears on a dark tab strip.** The favicon was a bare mark with a
  `prefers-color-scheme` rule in its SVG, which is not an answer every browser gives inside an icon
  (WebKit ignores it), so a dark strip got the near-black bird drawn on near-black. The mark now
  stands on a rounded plate of the brand's ink with the mark in the paper colour, and the `.ico`
  beside the SVG is that same file rasterised, so the two cannot differ: what the icon is drawn on is
  part of the icon, and no browser has to be asked which scheme it is in. The browser suite paints
  both files over a light strip and a dark one and reads the pixels back, so an icon that reads on
  only one of them fails the run.
- **Changing a field's type to `Slug` in the schema editor no longer leaves the type blank or is
  refused as "already unique".** The dropdown draws its value through the pipe that names the types,
  and that pipe did not know `Slug`: with no option to match, the field it was chosen in was drawn
  empty. A field that had been marked unique carried the flag into its life as a slug, which the
  server refuses because a slug is unique by construction - the flag is now left off the wire for a
  slug (the checkbox is not shown for one, so a save is the only place that can be settled).
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

[Unreleased]: https://github.com/cat-love-7/tsubame-cms/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/cat-love-7/tsubame-cms/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cat-love-7/tsubame-cms/releases/tag/v0.1.0
