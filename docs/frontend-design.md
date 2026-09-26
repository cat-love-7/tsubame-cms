# Screen appearance (the foundation of the design)

**Status: implemented** (2026-09). The theme is still Angular Material 3's `mat.theme()`
(`mat.$azure-palette` + `mat.$blue-palette`); a **common vocabulary** was placed on top of it,
and every screen was rebuilt with that vocabulary. No new library and no new design token were added.

## 1. Decisions

| Item | Decision |
|---|---|
| Theme | **Keep the Material 3 azure palette**. Use only `--mat-sys-*` for colour |
| Hard-coded colours | **Prohibited** (all hard-coded values such as `#666`, `#b3261e`, `#fff4e5` were removed) |
| Dark mode | **Follow the OS setting** (`html { color-scheme: light dark }`). `mat.theme()` emits variables for both schemes, so unless a colour is hard-coded it darkens as is |
| Common shapes | Collected in `src/styles.scss` (toolbar, panel, message, chip, table, action row) |
| Spacing | Per screen, `:host { display: flex; flex-direction: column; gap: 16px }`. Do not balance things out with individual `margin` |
| Shell | Fixed header + sidebar + scrolling body (`100dvh`). The body is centred with a maximum width of 1400px |
| Shell colours | The app bar is `surface-container`. Material's buttons and icons are drawn on the premise that they sit on surface, so the palette is shown in the brand and the navigation |

## 2. Vocabulary (`src/styles.scss`)

On the premise that "a screen is a combination of a few shapes", only the shapes are put here.
Because of Angular's view encapsulation, writing similar rules in each component gives birth to
slightly different greys and reds per screen - and that is what was actually happening.

| Shape | Class | Where to use |
|---|---|---|
| Screen header | `.toolbar` (+ `.spacer`) | The `h2` heading and what can be done on that screen |
| Panel | `.panel` | A unit of content such as a form, a table or settings |
| Message | `.error` / `.status` / `.notice` / `.note` / `.hint` | Failure, success, caution, supplement. All the same shape, only the colour separates the meanings |
| Chip | `.draft-note` | A small state next to the heading, such as unpublished changes |
| Supporting text | `.updated` / `.publisher` / `.user-email` / `.count` | Date and time, publisher, count. Do not let them compete with the body |
| Action row | `.actions` / `.array-actions` / `.picker-actions` / `.resource-actions` / `.schema-actions` / `.element-actions` | A row of buttons. Do not decide the spacing per screen. A row inside a dialog is Material's `mat-dialog-actions` |
| Table | `table.items` / `table.accounts`, `.mat-mdc-table`, `.empty` | A plain table and a `mat-table` look the same |
| Link | `.preview-link` | The URL to hand out (long, so it wraps) |
| Field | `.field-grid` / `.field-cell` / `.field-cell.problem` | The same grid in schema editing and content editing. `problem` is a field the server rejected |
| Read-only value | `.value-text` | A value shown rather than edited (the comparison), sized to the text instead of to the control |

The grid is also nested inside a Composite field (a Composite definition also has `width` / `height`).
The nested 12 columns count the width occupied by that Composite field as 12.

The CDK drag preview (drawn on the overlay, so it cannot be written on the component side) is in
the same file.

## 3. When building a screen

1. Put vertical rhythm on the screen component's `:host` (above).
2. Heading and actions in `.toolbar`, content in `.panel`.
3. A message is one of the five above. **Do not decide the colour yourself**.
4. The action column of a table is `td.actions` (stay a table cell with `nowrap`. Making it flex takes it out of the table layout).
5. What remains in the stylesheet is **only the layout specific to that screen** (the number of grid columns, the image height, and so on).

## 4. Things fixed along the way

- **List columns can now be chosen with the Schema**: the Collection list used to make **every Field** a column
  (with a dozen or so Fields the table became unreadable and Items were hard to tell apart). In schema editing,
  "show in list" is attached per Field, and only those Fields become columns in Schema order. **A Schema with
  nothing attached shows no Field columns** (only id, status and updated at). The setting is a property of the
  Field, so the API round-trips it as `show_in_list` (`docs/content-api.md`). Single pages and Composite definitions have no list,
  so it is not shown on those screens.
- **Added guidance to a list that has no columns yet**: a Collection list with nothing selected ends up with only
  id, status and updated at, so "no Fields are selected for display yet" and a **link to the schema screen** are
  shown (only to administrators. Editors have no schema screen, so a link that would be refused when pressed is not shown).
- **Relations are now chosen, not written**: a relation on the content editing screen is held as a **chip** rather
  than typing an id as JSON, and "add relation" opens a **picker** (`shared/relation-picker/`) that lists the target Items **by title**.
  **The order is the value**, so the chip's left and right arrows reorder them (the same operation as an image array). Removing is ×.
  A `Relation` field holds one reference and has no order, so no arrows are shown and re-selecting is a
  replacement; **several references are an `Array` of relations** (`Array([Relation(…)])`), which is what the
  chips with arrows are. The JSON field was kept
  behind "edit as JSON" (for migration, or to write values the picker cannot express).
  Candidates are fetched only when it is opened and up to 100 (filtering on the screen side); beyond that is written as JSON.
- **Reference-source panel**: below the content editing screen (Item and Single page) is "content referring to this"
  (`shared/relation-references/`). It asks the index **only when opened** (the same judgement as the picker), the heading is
  **the name given by that side** (`inverse_name`; if absent, the name of the reference source), and each row is the reference source's title and
  a link to its editing screen. Nothing referring to it yet and a failed load are stated separately
  (do not conflate an empty list with "could not read").
- **Name (title) of the relation target**: a relation holds only "which Item", so `categories #3` says nothing to the reader.
  Schema editing lets you choose **"use as title" per Field** (one per Schema. Checking one unchecks the others), and the
  reference column in the list and the reference field on the content editing screen show that value as the name.
  The name is looked up by `RelationLabelsService`, which asks only about relations shown on the screen and one
  request per target Collection (not cached: an old name showing right after a name is fixed is worse. However **identical
  queries issued at the same time are merged into one** - a relation can also be placed inside a Composite definition, so an array of
  composites issues the same query per element). A target with no title decided shows the relation itself as before.
- **Lists are newest first**: the Collection list and the picker's candidates are in **descending id order** (id is creation order, so the last created is at the top). What an editor looks for when opening is usually what they touched recently, and this was matched to the Image library, which already had the same order. Only the admin side does the reverse (the admin list reads all records and then cuts a window, so no change is needed on the storage side). The delivery API stays in ascending id order as before, and callers who want a different order use `?sort=`.
- **Sorting the list**: the column headings (ID and the Fields chosen with "show in list") are buttons, and pressing one sorts by that column. Pressing again reverses it. The direction is shown by an arrow drawn in CSS (▲/▼), and `th`'s `aria-sort` holds the answer for screen readers - **`mat-icon` is not used** because the ligature name gets mixed into the table text (into the reading, and into tests that read the headings). The order is decided by the server (the admin API's `?sort=`), so paging works correctly with the filtered order.
- **Images and relations in the list**: the image column shows a **thumbnail** rather than the url (an array shows the first 3 + "+N";
  a raw id whose url we do not know shows `id 5`). The relation column shows **`authors #1`** (a Single page shows the page name)
  rather than JSON like `{"target":"authors","item":1}` (or, for several, `[{…},{…}]`) or a url. Expanding the target's contents is the job of the
  delivery API's `?populate=`, not the job of a list cell.
- Cell values are assembled only once per row (`rows` in `list.ts`). The image column reads one value in two ways,
  "how many" and "how many there were", so calling it repeatedly from template bindings is wasteful.
- **Accounts were inside the Schema**: the fourth item under "Settings → Schema" in the sidebar was
  "Accounts". What a Schema describes is **content**, and an Account is not content.
  It was made a sibling of "Schema" directly under Settings, and the name also stays `accounts.title`, the same as on the screen
  (there is a structure test in `sidebar.spec.ts`; the E2E checks that "Accounts is visible just by opening Settings").
- **Settings are for administrators only**: the settings branch (3 schema screens + accounts) is **shown only to
  administrators**. Previously only a comment said so, and in reality the schema links were shown to every Account
  (the server rejects changes with 403, so it was a path that always failed when pressed).
  Since the whole branch is not shown, no empty "Settings" is left for a non-administrator Account.
  The address is also protected by `adminGuard` (the absence of a link is not the rule: there are bookmarks, history and the
  address bar, and landing on a screen whose save is rejected is a refusal with no explanation).
  **Reading a Schema is separate**: the content editing screen fetches the Schema from the API, so that stays
  available to all Accounts.
- **Current location in the sidebar**: `routerLinkActive` was not working (`RouterLinkActive` was not in
  `imports`, so the attribute was ignored). Now the link for the current screen is
  highlighted with `secondary-container`.
- **Missing administrator link**: the tree read `isAdmin()` only once, so if it was rendered before the profile
  arrived, "Accounts" disappeared (this happens when the settings screen is reloaded).
  Fixed by including the Account in the stream (there is a test in `sidebar.spec.ts`).
- **Last sign-in in the account list**: an ISO string was output as is, so it was changed to the same
  `Intl` format as the other screens (`DateTimeFormat`).
- **Empty lists**: the Single page and Composite field lists were "tables with only a heading", so
  `matNoDataRow` now shows "nothing yet".
- **Keyboard focus**: added `:focus-visible` outlines to plain links and buttons.
- **Width and height inside a Composite field**: a Composite definition's `width` / `height` was
  ignored on the content editing screen, and the contents were all stacked vertically at the same width (which also disagreed with
  the preview on the schema editing screen). Inside a composite is now drawn with the same 12-column grid. The column count counts **the composite
  field's own width** as 12 (`.field-grid` is nested inside `.composite`).
- **The item toolbar reads state first, then the actions** (added 2026-09). The only "save" was at the end of
  a long form and a screen away from "save and publish", so one was added to the toolbar - but ahead of the
  status, which put it before the thing it acts on. It sits after the status and the publisher now, and
  before the publish controls; the template looks at `metadata()` twice for exactly that split.
- **The publisher's name is labelled** (added 2026-09). The toolbar showed `published_by.username` bare
  after the status badge, which reads as the author of the content. It is not: `published_by` is the
  **Account that last published** (`docs/content-api.md`), so it reads "Published by: <name>", keeping
  the tooltip that spells the same thing out. The lists say it once instead of on every row: the status
  column they carry the name in is headed **"Status (publisher)"**.
- **The image picker is a dialog, and it uploads** (added 2026-09). The wall of tiles used to be a panel
  *under* the control that opened it: taller than the form around it, so it pushed everything below it down
  and then scrolled inside a 200px box - and the Markdown box has no upload of its own, so an empty library
  was a dead end there ("upload one first" with nothing to upload with). It is `MatDialog` now - the only
  dialog in the app - carrying its own upload, closing as soon as an image is chosen. The same component
  serves the image field, the image array and the Markdown image button; the file controls those fields
  already had stay, because a file in hand is one gesture instead of two.

## 5. Discrepancies between schema editing and content editing (found and fixed in the 2026-09 audit)

We listed the things that "can be decided in schema editing but are not reflected in content editing". Value validation and the shape of rejection are
written in `docs/content-api.md` §3.0.

- **Width and height inside a Composite field** (see §4).
- **Character count limits**: the content editing input did not know about `max_length` / `min_length`. Now it
  shows `maxlength` / `minlength` and a count hint, and stops **before saving** when out of range (some lower bounds the browser
  cannot enforce, so the widget itself reports the problem and the parent rejects the save).
- **A hint for `unique`**: only the server can decide uniqueness, but the form had no such display. "Unique" is shown
  on the label (a duplicate is rejected with 409 + `field` + highlighting of the relevant field).
- **The character count input was saved as a string**: Max/Min Length in schema editing was a plain
  `input`, and Angular's ngModel returns a string. The server reads `Option<usize>` strictly, so
  saving a Schema with limits resulted in 422 (body deserialisation failure). The input was made
  `type="number"` and is normalised to a number on save (`schemaForSaving`).
- **The default Field type was shared by all Fields**: `FieldDefaults` is one
  object per type, and field editing writes directly into that type. A newly added Text Field pointed at the same
  object, so putting a max length in one affected **all the other Text Fields** (and
  all added afterwards). The same applied to Enum choices and the Array element type. It is now
  copied per Field (`newFieldType`).
- **The Array element type was not shown in content editing**: a scalar array's only editing means is a JSON text
  area, so the declared element type appeared nowhere. It is shown in the hint in the form "element type: Number", and
  **an element that cannot be read as any of the declared types** is stopped before saving (for example, a string in a numeric array). This check is
  deliberately conservative and does not replace the server's judgement of "try in declared order and take the first type that could be read"
  (wrongly rejecting a readable value is worse than being refused by the server).
- **A problem found on the client side is not highlighted**: stopping a save is the same whether it comes from a
  server rejection or a widget report, so both highlight the relevant field (`isProblem`). The `field` of a
  rejection is a path (`seo.description`, `tags[2]`), so a rejection inside a nested structure highlights the outer cell.

## 6. Icon tooltips

For icon-only buttons, **the same wording as `aria-label`** is also put into `matTooltip`. The reading name and the
hover explanation match, and the wording has a single source.

- The display is **delayed by 300ms** (`MAT_TOOLTIP_DEFAULT_OPTIONS`). Do not make an icon the pointer merely passes over
  flicker.
- **`disableTooltipInteractivity: true` is required**. Material's tooltip is by default operable
  (a button can be placed inside it), so the hint of a pressed icon **steals the click of the neighbouring icon**.
  In fact the E2E sidebar expansion broke because of this, and the cause was "the click landing on `.mat-mdc-tooltip`".
  A hint-only tooltip should let clicks through, so this setting disables it.

## 7. Stale responses and unsaved changes

**Verify who answered before reflecting it.** Moving from the list to the editing screen reuses the same component,
so a response for the previous Item arrives later. Calling `set` as is displays **the previous Item's
content under another Item's address**, and saving overwrites it with that content. The editing screen holds a sequence number
(`loadToken`) per load and does nothing if it is not its turn. Responses to publish and save are likewise
reflected only when the Item at the time of pressing and the current Item match.

**Publish sends the "saved" version.** The server's publish is an operation that replaces the published copy with the working
copy, so pressing it with unsaved changes in the form **publishes a version different from the screen**. Therefore:

- Compare the form with the saved value and hold **whether there are unsaved changes** (`fingerprint` in
  `core/value-changes.ts`, a comparison that does not depend on key order), and show "unsaved changes" on the screen when there are.
- The Collection editing screen makes the primary button **"save and publish"** when there are unsaved changes (publish after the save
  succeeds. If the save is rejected, do not publish).
- The Single page editing screen **disables** the publish button and puts the reason in `title` ("save and publish" is
  in the footer).
- On leaving, `unsavedChangesGuard` (`canDeactivate`) confirms, and `beforeunload` catches reloads and
  tab closes. The confirmation wording is held as a catalogue key.

## 8. Route parameters and screen state

**Read parameters from the stream** (subscribe to `route.paramMap`). Reading `route.snapshot` only
once does not work: switching to another target in the sidebar **changes only the parameter while the route definition stays the same**, so
Angular reuses the component. Reading the snapshot results in **only the URL changing and the content staying as
before** (this happens for Single page to Single page, Collection to Collection, Item to Item and Schema to Schema alike).

- The values representing the target are made **signals** (with OnPush + zoneless, overwriting a mere field does not
  update the view).
- When a parameter changes, **discard all state belonging to that target and then reload it** (Schema, values,
  metadata, errors, highlighting). Keeping the previous target's values causes an accident of writing to another target on save.
- Tests use `stubActivatedRoute` in `app/core/testing/activated-route.ts` and reproduce the switch with
  `route.navigate({...})` (a stub with only `snapshot` cannot verify this).

**Query parameters are the same.** Even if only `?token=...` changes on the same route, the component is reused,
so subscribe to `route.queryParamMap`: the password reset screen's `token` and the Cognito callback's
`code` / `state` are the cases (if either is read once and kept, the second link **sends the previous
token/code**). The stub takes the query as a second argument, and the switch is reproduced with
`route.navigateQuery({...})`. Since the value is not shown on screen, it is not made a signal.

## 9. Naming

When the same name exists twice, **ways to avoid it increase** (in fact `dashboard.routes.ts`
imported seven `List`s and three `Edit`s under aliases. Moreover the alias spellings wavered between `CollectionList` and
`schemaCollectionList`). Rules so this does not happen.

- **Component class names are `<target><role>`**: `CollectionItemList`, `SinglePageEdit`,
  `CompositeFieldSchema`. Do not give names that are only `List` or `Edit`. Keep route definitions in a state where they can be imported
  as is, **without aliases**.
- **Selectors are `app-` + kebab and unique**. Do not make them **prefixes of other selectors**: `app-edit` was
  a prefix of `app-edit-schema`, so searching for `app-edit` turned up another component.
  Angular does not complain about duplicates until they happen to enter `imports` at the same time either (in fact
  there were two `app-single-page-list`).
- **File names are kebab**: `composite-fields.service.ts`. If one word, as is (`list.ts`).
  Match the spelling of the feature directory (`dashboard/single-pages/`). Matching the API resource name
  (`single_pages`) is a matter of **the type side**, not the file name.
- **TS identifiers are camelCase** (variables, methods, `@Input()` / `@Output()` names:
  `value`/`valueChange`, `schema`/`schemaChange`).
- **Only fields of types that mirror the API are snake_case** (`published_at`, `has_draft`,
  `original_filename`). This is **the JSON the server returns as is**, a manifestation of the policy of not putting a mapping
  layer in. The types in `app/models/` and `app/repositories/` are like this. "Fixing" this to camelCase
  would require conversion code per repository - do not fix it, keep this boundary.
- On the Rust side types are UpperCamelCase, acronyms too are `Id` (`ImageId`, not `ImageID`.
  Align with `CollectionItemId` / `UserId`), functions and fields are snake_case, and JSON keys are
  decided by `serde` (`docs/content-api.md` is the API's spelling).

## 10. Image tiles show the small copy the browser made

The list and the picker loaded the **original** 1MB-class photo into a 180px-wide tile. Measured,
**viewing a library of 275 images to the end was 154MB** (the first render alone was 85 images and 47.7MB). `loading="lazy"` was
present, but it only delays, and what is fetched is the original, so it did not help.

So **`ImagesService.uploadImage` makes a small copy in the browser right after upload**
(`core/image-thumbnail.ts`. `createImageBitmap` → canvas → WebP 360px), and sends it with
`PUT /api/models/images/{id}/thumbnail`. The server does not decode the image - it treats it as a byte string
just like the original, so no per-adapter difference appears.

- **Failures are swallowed**: a browser that cannot make a small copy and a server that refuses the save both
  leave the upload itself as a success (`sendThumbnail` turns it into `undefined` with `catchError`).
  The image is usable, the tile is just heavy.
- **Display is `image.thumbnail_url || image.url`**. Images uploaded via the API, or images from before this feature,
  have no copy, so the original is shown as is.
- **The list is paged**. `ImagesService.listImages(offset)` reads 60 at a time, and the screen
  adds more with "load more". After a delete or replace, **as many pages as had been loaded** are re-read
  (do not send an editor who had looked up to page 4 back to the top).

## 11. Unpublished changes are "compare, then discard"

The admin screen's reads are **working-copy-first** (`working_item`), so for an Item carrying unpublished changes
**the content the site is currently serving appears nowhere in the CMS**. There was also no means to discard
it (the three choices were publish, unpublish, or retype the invisible thing by hand, and none of them is "discard").

Two things were added to the editing screen. Both are operations on **saved changes**, separate from unsaved edits in the form:

- **"View differences from published"**: reads `GET .../published` on the spot and `changedFields()` lists
  **only the Fields that differ**. The comparison is `fingerprint` (a rendering that does not depend on key order), and the published side is
  compared after passing through `withDefaults` (so that a difference the form merely filled in with defaults is not called a "change").
  The display uses `ValueField` with `[disabled]="true"` - the same usage as the schema editing preview, so
  the comparison does not drift from the form - plus `[compact]="true"`, which is the one place the two
  differ: a form box holds the height the Schema asked for, which is a minimum for **editing**, and a
  paragraph in a twelve-row box is blank space on both sides of a comparison a reader only reads. Compact
  renders a multi-line Text or a Markdown value as **text, sized to the text** (`.value-text`), and drops
  the layout minimum from the cells inside a Composite. Long values are capped at 24rem and scroll.
- **"Discard changes"**: `DELETE .../draft` behind a confirmation. **It does not touch the site**, so it is
  different from unpublish (that one makes the page disappear and leaves the working copy). It reloads after execution, so the form shows the published
  content. The notification is shown **after the reload** (because `load` clears notifications at the start).

The condition for appearing is `published() && hasDraft()`. An Item that has never been published has no "published" version, so it is not shown
(the value at creation is on the published-copy side, but the site is serving nothing, so it is not a "place to return to").

## 12. Notifications are toasts, Markdown is buttons, height is rows

### 12.1 Show that a save happened at the top of the screen (toast)

The schema editing save message was at the top of the page, but the save button is at the **bottom** of the page. Measured,
with content 2632px and `scrollTop` 1976, `.status` was **y = −1888px** (invisible). The article editing screen has the
same structure, and the same thing happens.

One `shared/notice-toast` was made and shown fixed at the **top of the viewport** (just below the header). 9 screens
use it (schema editing ×3, article/Single page editing, Image library, user list, password). Rules:

- **A success notification** disappears automatically after about 5 seconds, and also with ×. If the same wording arrives again it is
  **shown again** (a second save is not swallowed as "the message to show already exists").
- **Errors are both inline and toast**. The inline one has `role="alert"` and a retry button and does not disappear from
  the screen (it does not disappear even if the toast is closed - the toast's dismiss touches only state inside the
  component). **The toast side is `aria-hidden`**: so as not to read the same failure twice.
- **The result of bulk publish stays inline**: it is read as a pair with the list of rejections, so it is not
  placed somewhere that flows away and disappears.
- The toast is `pointer-events: none` (only the × button can be pressed): an element floating over the
  toolbar swallowing clicks is worse than the inconvenience it was meant to fix.

### 12.2 Markdown can be written with formatting buttons

Markdown was a plain `<textarea>`, and an editor who did not know the notation had no clue. A toolbar was added and
acts **on the selection**: bold / italic / heading / link / image / bullet list / numbered list / quote.

- Wrapping kinds (bold, italic, link) wrap the selection and **leave the inserted thing selected**. Typing after
  pressing the button replaces the placeholder.
- Line-prefix kinds (heading, list, quote) are added to **every line the selection touches** (if you select the middle of
  a paragraph and press list, the paragraph becomes a list). Lines that already have the prefix are not touched (pressing twice does not break it).
- Links ask for the URL with `window.prompt`. Images reuse the **Image library picker** and
  insert `![name](absolute URL)` at the caret position. The URL is the same as the library's "copy link",
  a **link that does not break** and is looked up by id (`/api/images/by-id/{id}`). Rendering Markdown is
  the site's job and may be on a different origin from the CMS, so the URL is absolute.
- **The selection is read from the DOM** (`#markdownInput` is passed as a template reference). The caret is restored
  **after** the value is written back through the binding (`setTimeout`). Other than that, no state is held outside
  `ValueField`.

### 12.3 Height becomes the number of rows of the input

`height` is **the cell's minimum height** (72px × N), and the label was also "minimum height", but **the input ignored**
it (measured: a Text of height 3 was a **24px** input in a 216px cell, a Markdown of height 5 was a **144px** box in a
360px cell, and a Markdown of height 1 was also the same 144px and could not be made shorter).

- **The number of rows is derived from `height`**: `rows = max(lower bound, height × 3)`. 1 unit = 72px and one
  line of text ≒ 24px, so 3 rows. The lower bounds are the appearance before the change (Text 3 rows, Markdown 6 rows), so existing Schemas
  change nothing.
- **Text is a box only when `multiline`**. A title becoming multi-line on its own is surprising, so it becomes a
  `<textarea>` only when the Schema says "multi-line". Markdown is a box from the start.
- The cell's minimum height and the number of rows **come from the same number**, so moving the height handle in the
  schema editing preview stretches the box, and the same happens on the content editing screen.
