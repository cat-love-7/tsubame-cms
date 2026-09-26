# Public content API and Draft / Publish

**Every API lives under `/api`** (`tsubame_core::API_PREFIX`). The admin screen, site builds,
preview and images are all the same, and nothing anywhere strips the path prefix (the dev proxy,
nginx and CloudFront all pass it through as is). The only exception is the liveness `GET /`, which
touches neither storage nor tokens. The paths the API returns (preview links, image URLs) **include
this prefix**, so they can be opened as is.

The contract by which static site builds such as Gatsby read the CMS content.
Separate from `/api/models/*` used by the admin screen, there is `/api/content/*`, which returns
**no auth required, published only**.

## 1. Why it is separated from `/api/models/*`

| | `/api/models/*` | `/api/content/*` |
|---|---|---|
| Authentication | Bearer token required | Not required |
| What it returns | All items, including drafts | Published only |
| Main users | Admin screen | Site builds and delivery |
| Schema | Fetched in a separate request | Included in the response |

`/api/models/*` cannot be made public because it includes Schemas and drafts. So a read-only
entrance was split off. Values carry no type tag (the Schema is the only type information), so
`/api/content/*` returns the Schema at the same time. This means there is no need to hit an
authenticated API just to interpret values.

## 2. The draft / publish model

**There are two values.** What an editor saves is the **working copy** (draft), and what the
delivery API reads is only the **published copy** (published). `publish` is the operation that
copies the working copy into the published copy, and `unpublish` only lowers the status, deleting
neither copy.

| Destination | Contents | Who sees it |
|---|---|---|
| Item store | Published copy | `/api/content/*` (site) |
| Working-copy store | Working copy (unpublished changes) | Reads and writes of `/api/models/*` (admin screen, preview) |

- **Saving does not change the live site.** The published copy is replaced only on `publish`.
  This is what makes the "can edit but cannot publish" role safe.
- No working copy = no pending changes. `publish` replaces the published copy with the working
  copy and deletes the working copy (if there is nothing, it only updates `published_at`).
- Status and timestamps live in a **separate store** (`item_metadata`) from the values. A Schema
  field named `status` or `published_at` does not collide.
- Deleting a Collection / Single page / Item also deletes the working copy and the metadata.
- **The working copy can be discarded** (added 2026-09). `DELETE .../draft` only deletes the
  working copy and does not touch the published copy. The admin screen puts it next to "publish
  changes". There was a real deployment where **9 unpublished changes had piled up**, and until
  then there was no way to clear them other than "publish" (the storage-layer operation to delete
  the working copy had existed from the start, but had no caller).
- **The currently published content is read separately.** Reads in the admin screen prefer the
  working copy, so for an Item with pending changes "what the site is serving" is not visible from
  the CMS. `GET .../published` returns it, and the edit screen's "view differences from published"
  lists **only the differing fields** (so the change can be checked before discarding). An
  unpublished Item is 404: the site is not serving anything.
- `/api/content/*` on unpublished content is **404** (not 403). So as not to leak the existence
  itself.
- `published_at` is the **time it was first published**. It does not move on re-publish and is kept
  through unpublish ("the day this article was published" is a fact about the Item, not the current
  state). The **time the update was published** is held by `updated_at`.
- `created_at` is the time it was first saved, `updated_at` is **the time the content last changed**
  (on save, or when changes were published). A publish with nothing pending moves neither (only
  `published_by` is updated).
- The admin API metadata returns `has_draft`. Published + `has_draft: true` is the state "published,
  but with unpublished changes", and the admin screen shows this as "has changes".
- **You may `publish` an already published Item again.** That is the operation that applies pending
  changes, and it never temporarily disappears from the site (unpublishing first and then
  publishing gives a 404 in between). The admin screen's "publish changes" calls this. The responses
  of `publish` / `unpublish` have the same shape as fetching metadata (including `has_draft`), and
  the screen uses them as the next state.

## 3. Endpoints

### Public (no auth required)

| Method | Path | What it returns |
|---|---|---|
| GET | `/api/content/collections` | Array of Collection names that have at least one published Item |
| GET | `/api/content/collections/{name}` | `{ "schema": [...], "items": [...], "total": 12, "limit": 50, "offset": 0, "next_offset": 50 }`. `?sort=` gives the order (default is ascending id) |
| GET | `/api/content/collections/{name}/items/{id}` | `{ "id": 1, "published_at": "...", "last_published_at": "...", "values": {...} }` |
| GET | `/api/content/collections/{name}/items/by/{field}/{value}` | The same shape. Returns one Item from the value of a **unique Field** (404 if unpublished or not found) |
| GET | `/api/content/single-pages` | Array of published Single page names |
| GET | `/api/content/single-pages/{name}` | `{ "schema": [...], "published_at": "...", "last_published_at": "...", "values": {...} }` |
| GET | `/api/content/composite-fields` | The whole set of Composite field definitions (`{ "<id>": [field definitions] }`). The public Schema references them by id, so they can be read without auth |

### Admin (token required. `publish` / `unpublish` need edit permission)

| Method | Path | What it returns |
|---|---|---|
| GET | `/api/models/collections/{name}/items` | `[[id, values], ...]` + the `X-Total-Count` header. **Newest first (descending id)** (paging with `?limit=` / `?offset=`. The picker candidates in the content edit screen use the same order). The order can be given with `?sort=`, with the same spellings as the delivery API (`id` / `published_at` / `created_at` / `updated_at` / field names. Ties are broken by id) |
| GET | `/api/models/collections/{name}/items/metadata` | `{ "1": { "status": "draft", "published_at": null, "created_at": "...", "updated_at": "...", "has_draft": false }, ... }` |
| GET | `/api/models/collections/{name}/items/{id}/metadata` | That Item's metadata |
| PUT | `/api/models/collections/{name}/items/{id}/metadata` | Set the timestamps (see "Timestamps for migration" below). Returns the updated metadata |
| POST | `/api/models/collections/{name}/items/{id}/publish` | The updated metadata (404 for a nonexistent id) |
| POST | `/api/models/collections/{name}/items/{id}/unpublish` | The updated metadata |
| GET | `/api/models/single_pages/{name}/item/metadata` | That page's metadata |
| PUT | `/api/models/single_pages/{name}/item/metadata` | Set the timestamps (see "Timestamps for migration" below) |
| GET | `/api/models/single_pages/items/metadata` | `{ "home": { "status": "published", "updated_at": "...", "has_draft": true, ... }, ... }` (**only pages that can be read**. So the list screen can show the status in one request) |
| POST | `/api/models/single_pages/{name}/publish` | The updated metadata |
| POST | `/api/models/single_pages/{name}/unpublish` | The updated metadata |
| GET | `/api/models/collections/{name}/items/{id}/published` | **What the site is currently serving** (the published copy). `GET .../items/{id}` returns the working copy, so this is its counterpart. 404 for an unpublished Item |
| GET | `/api/models/single_pages/{name}/published` | Same as above (Single page) |
| DELETE | `/api/models/collections/{name}/items/{id}/draft` | **Discard the working copy** and return to the published content (`204`. Requires `can_edit`). Idempotent. The site does not change |
| DELETE | `/api/models/single_pages/{name}/draft` | Same as above (Single page) |
| GET | `/api/models/collections/{name}/items/{id}/api/preview` | `{ "schema": [...], "id": 1, "values": {...} }` (the working copy. Token required) |
| GET | `/api/models/single_pages/{name}/api/preview` | `{ "schema": [...], "values": {...} }` (the working copy. Token required) |
| GET | `/api/models/images` | `[{ "id": 1, "url": "/api/images/...", "thumbnail_url": "/api/images/thumb-...", "original_filename": "logo.png", "uploaded_at": "..." }, ...]` (newest first. `thumbnail_url` is the small tile copy; when it is absent, `url` is shown. `?limit=`/`?offset=` and `X-Total-Count` are in §5.9) |
| DELETE | `/api/models/images/{id}` | Delete the image and the file (404 for a nonexistent id) |

`items/metadata` returns **the entries for all Items**. Items that have never been saved appear as
`draft` too, so the admin screen list can draw the status column with this alone.

**Mutating requests (`POST` / `PUT` / `DELETE`) return `200 OK` with an empty body.** They used to
return the text "… successfully", but Angular's `HttpClient` interprets the body as JSON, so even
though the request had succeeded the client decided that "the save failed" (an empty body is not
parsed). Only responses whose body carries meaning (Item creation gives the id, publish / unpublish
give the metadata, image upload gives the URL) return JSON.

### Images

| Method | Path | What it returns |
|---|---|---|
| POST | `/api/models/images/get_upload_url` | `{ "id": 1, "upload_url": "/api/images/<file>?key=..." }` (token required) |
| PUT | `/api/images/{file_name}?key=...` | Store the file (token required. `key` is single-use and is tied to the file name at issue time) |
| GET | `/api/images/{file_name}` | Serve the file. **No auth required** (because `<img>` cannot attach headers) |
| PUT | `/api/models/images/{id}` | Change the display name (`{ "original_filename": "..." }`), confirm a replacement (`{ "file_name": "..." }`), set the upload timestamp (`{ "uploaded_at": "..." }`. See "Timestamps for migration" below) |
| DELETE | `/api/models/images/{id}` | Delete the image and the file |
| POST | `/api/models/images/{id}/replace` | Upload destination for a replacement (`{ "ext": "png" }` → `{ "file_name", "upload_url" }`) |
| PUT | `/api/models/images/{id}/thumbnail?ext=webp` | Store the small tile copy (the body is the bytes. Requires `can_edit`). §5.9 |
| GET | `/api/images/by-id/{id}` | **The file looked up by id** (no auth required). A link that does not break when the image is replaced |

Upload is two-stage (get a location → send). On AWS the S3 presigned URL carries the same contract.
After sending, the browser creates **the small tile copy** and sends it to `PUT .../thumbnail`
(§5.9). The admin screen's "Images" (`/api/images`; it lives with the documents) lists
`GET /api/models/images` and handles upload, renaming, replacement and deletion. From an image field
in the content editor, the same list opens and **an existing image can be selected again**.

**`original_filename` is the display name**, not the storage path. The storage file name is
generated by the server, so the name can be changed freely (empty, over 255 characters, path
separators and control characters are rejected). Changing the name does **not change the id, the URL
or the file**, so content referencing that image is unaffected, and the name alone can be changed
back.

#### Replacing the file (the id does not change)

The operation for "I want to replace the logo but not touch the content that references it". **The
id, display name and upload timestamp do not change; only the file is swapped**.

```
POST /api/models/images/3/replace  { "ext": "png" }   → { "file_name": "...", "upload_url": "..." }
PUT  <upload_url>                                  ← the new file
PUT  /api/models/images/3          { "file_name": "..." }  ← confirm the replacement
```

- **Upload under a new file name** (do not overwrite the same key). Browsers and CDNs cache per
  URL, so if the contents change while the URL stays the same, the old image keeps being served.
  With a different file name it is always fetched again. After the replacement, **the old file is
  deleted**.
- **Upload first, then confirm.** The confirmation checks that the file exists and that it does not
  belong to another image, so even if the upload fails **the current image keeps being served** (no
  broken state is left behind). Nothing changes before the confirmation.
- Two images never point at the same file (rejected, because replacing one would delete the other's
  file).
- `uploaded_at` stays the **time it was first uploaded** (the same idea as the publish date).

#### A link that survives replacement (id link)

`GET /api/images/by-id/{id}` resolves to **where that image is currently being served**. **No auth
required**.

- On-premises it returns the file itself (`Cache-Control: no-cache`, because the target changes).
- On AWS it sends a 302 to the current object URL (also `no-cache`. The object itself may be
  cached).

**Links written by hand** in Markdown bodies and the like **use this**. A URL containing a file name
disappears on replacement, breaking the link. The "copy link" action in the library screen returns
this URL. Note that **an Image field stores an id**, so the `GET` response returns the URL at that
point in time, and after a replacement (as long as the delivery API response is used) it
automatically becomes the new image.

**An image array** (`{ "Array": ["Image"] }`) accepts both `[3]` (ids only) and
`[{ "id": 3, "url": "..." }]` on write, and reads back as an array of `[{ "id", "url" }]`. The admin
screen edits it as a row of thumbnails, and images can be **added several at once** from the
library, reordered and deleted (the view can also be switched to editing the JSON directly).
Combining it with Number, as in `Array: ["Image", "Number"]`, is rejected by both the server and the
admin screen, because a bare number could not be told apart from an id. Nested arrays
(`Array: [{ "Array": [...] }]`) are rejected too (values carry no type tag and the recursion would
have no end).

### Arrays of Composite fields

The public API Schema references Composite definitions **by id** (`{"CompositeField": {"id":
"block"}}`). The definitions themselves are returned by `GET /api/content/composite-fields`
**without auth** (an id → field definition mapping. Same shape and same content as the admin API's
`GET /api/models/composite_fields`). The admin screen reads the admin API side with a token, so
adding this route does not loosen authorization (the admin API still returns 401).

As in `{ "Array": [{ "CompositeField": { "id": "block" } }] }`, **a Composite field can be the
element type of an array**. Elements are objects, so they cannot be mistaken for scalars, and the
referenced definition is looked up and its contents validated. Reads return `{ "id", "values": { ...
} }` per element, and **writes accept either that shape or a bare object** (if `id` is written, that
definition is used even when several definitions could take the same shape; if it is not, the first
matching definition in declaration order).

In the admin screen, the Schema editor's "Composite item types" lets you **select several
definitions the array may hold as elements**, and the content editor shows that definition's form
**per element** (add, delete, reorder). Because there are no type tags, **an array mixing scalars and
composites** is the one case that cannot be turned into a per-element editor (which type each
element is cannot be decided). In that case it is edited as JSON as before.

A Composite referencing a Composite is fine, and **returning to itself through an array is allowed**.
For example you can write a definition where "a block holds an array of blocks" (a tree). This is
cyclic as a reference graph, but there is no path that never ends - array elements are built from
**stored values**, so an empty array expands nothing, and going one level deeper requires a value
one level deeper. Parsing and validation stop at the depth of the value for the same reason.

Only **cycles that do not pass through an array** (`a → b → a`) are rejected. The admin screen draws
a Composite's subfields **from the Schema**, so this kind would expand forever even with an empty
value.

**Deletion does not inspect references.** Items referencing an image by id simply remain, and the
reference stops resolving (the decision is not to do in-use checks or reference rewriting).

### 3.1 References to another Collection or Single page (relations)

An Item can reference **an Item of another Collection** (or a **Single page**). The full design is in
`docs/relations-design.md`. Only the shape as seen from the API is written here.

**A relation holds exactly one reference, and several references are an `Array` of relations** -
the same composition every other field type uses. One array may name **several different targets**
("related content", of either kind), and it may name each of them once.

```json
{ "name": "author", "field_type": { "Relation": {
  "target": { "kind": "collection", "name": "authors" },
  "inverse_name": "articles"
}}, "required": false, "width": 12, "height": 1 }

{ "name": "related", "field_type": { "Array": [
  { "Relation": { "target": { "kind": "collection", "name": "authors" },
                  "inverse_name": "articles" } },
  { "Relation": { "target": { "kind": "collection", "name": "categories" } } }
]}, "required": false, "width": 12, "height": 1 }
```

- The target is `{ "kind": "collection" | "single_page", "name": "..." }`. **The same name with a
  different kind is a different target**, and existence is checked on save (400 if missing).
- `inverse_name` is **the name used on the other side** (for screen headings and `?populate=`). It is
  only a name; no data is held on the other side. **One name per target**: if another Schema tries to
  give the same name to the same target, the save is rejected with **409 `conflict`** (because the
  answer to `?populate=<inverse_name>` would change depending on who asked). The same name is fine
  for a different target (names are looked up from the target), and **the same name twice for one
  target is refused within one Schema too** - two Fields, or two item types of one array, cannot both
  call it the same thing. Two Fields of one Schema may point at the same target under *different*
  names: the reverse direction answers by target, so both names return the same set (a known limit
  of the index, which is a set of (owner, target)).
- The value's shape follows the field's:

| Field | Value |
|---|---|
| `Relation` to a Collection | `{ "target": "authors", "item": 7 }` |
| `Relation` to a Single page | `{ "target": "home" }` (no `item`, since a page has no id) |
| `Array([Relation(…)])` | `[{ "target": "authors", "item": 7 }, { "target": "categories", "item": 3 }]` |

- **"No references" is `null` for one relation and `[]` for an array of them**, and a field nobody
  sent at all is simply absent. On save only **duplicate identical references** are removed (the
  first occurrence keeps its position), so **the written order is stored as is and the delivery API
  returns it in that order**. Reordering is done with the left/right arrows on the chips in the admin
  screen (the index is a set, so reordering does not move the index).
- What is rejected (all 400): a reference whose target name is not the one the field (or the array
  item type) declares; a Collection reference without `item` or with a non-integer `item`; a Single
  page reference carrying `item`; a list where one reference is expected (and the other way round);
  and, at Schema save, **the same target declared twice in one array** (an element carries its
  target, not which declaration wrote it).
- `Slug` **cannot be an array element type either**. A slug is unique by construction and the unique
  index is built from a *field's own* value, so a slug inside an array could never be kept unique -
  which is the whole of what a slug is (Schema save rejects it with 400 `bad_request`). The element
  types the screen offers do not include it.
- `Relation` **can also be written inside a Composite field definition**. The target is a site
  Collection / Single page, and because definitions are stored independently, **the target's
  existence is checked when the definition is saved** and checked again **when the Schema that embeds
  it is saved** (walking into the definitions). The value goes inside the Composite's value, and **it
  is the Item that holds the reference**, so the index, reverse lookup, deletion refusal and
  `?detach=true` reach inside composites and arrays:
  `{ "cta": { "author": { "target": "authors", "item": 7 } } }`. It cannot be a list column or the
  title (a single entry inside a Composite cannot be designated).
- **A `required` relation is asked at publish time** (the same rule as other required fields). The
  working copy saves even when empty, and publish rejects it with `field_required`.
- **Publish counts "published targets"** (`docs/relations-design.md` §4). If every target of a
  `required` relation is **unpublished**, publish is rejected with **409 `relation_unpublished`**
  even when the value is filled in (if part of the array is published, it passes). Conversely,
  `unpublish` of a target pointed at by a **published referrer** through a `required` relation is
  rejected with **409 `relation_required_by`** (if other published references remain in that field,
  it passes). An **optional** relation is free in both directions, and the delivery API drops
  unpublished targets. A refusal returns the path in the Schema in `field` (`author`, `cta.author`
  inside a Composite, `blocks[0].author` in an array), so the screen knows which input to open.
#### Reverse lookup (who references it)

References are written as an **index** together with the body, so reverse lookup answers with a read
alone (no full scan).

| Method | Path | What it returns |
|---|---|---|
| GET | `/api/models/collections/{name}/items/{id}/references` | Content referencing that Item (token required) |
| GET | `/api/models/single_pages/{name}/references` | Content referencing that page (token required) |

```json
[{ "kind": "collection_item", "name": "posts", "item": 3 }, { "kind": "single_page", "name": "about" }]
```

- Counts **the union of the draft and the published copy**. Neither an in-progress reference nor a
  published one is a reason that deletion is allowed.
- A missing Item or page is 404 (not "no references").

#### Deleting content that is referenced

`DELETE …/items/{id}` / `DELETE …/single_pages/{name}` are rejected with **409 `still_referenced`**
when there is even one reference (the message names up to 3 referrers). To detach and then delete:

```
DELETE /api/models/collections/authors/items/1?detach=true
```

- `?detach=true` removes the reference from **both copies of the referrer** and then deletes. The
  detaching writes go through the normal save path, so the index moves together with the value.
  Everything is detached before deletion, so a mid-way failure never deletes while references remain.
- Even if it references something itself, it can be deleted as long as it is not referenced (being
  referenced is the only reason for refusal).
- **Deleting a whole Collection also inspects references** (2026-09). If there is even one reference
  pointing at **any item** of that Collection, it is rejected with **409 `still_referenced`** (naming
  up to 3 referrers). There is no `?detach=true`: rewriting the references of an entire Collection is
  too large a decision for an operator who has not looked at its contents, so detaching is done one
  at a time (Item deletion with `?detach=true`).

- **The delivery API returns only the published copy, and drops references whose target is
  unpublished**. The drop is because "the site cannot follow the reference"; the admin API returns
  them as is for editing. Adding `?populate=<field name>` fills that reference with **the published
  copy's value** (the same shape as the screen. Images are `{id,url}`) one level deep. It works on
  lists, single Items and Single pages alike, and **an unknown name is 400** (not silently ignored).
- **A field that references several targets is expanded by naming one**: `?populate=related.authors`
  (`?populate=related` on such a field is a 400 that names its targets, because the name alone does
  not say which one was meant). Only the elements whose target is that name are expanded; the others
  are returned as they are. `?populate=<field>` without a target is kept for a field that declares
  one, and for several different fields it can be given more than once, comma separated.
- **The caller decides the order** (`?sort=`, 2026-09). One of `sort=<key>` / `sort=-<key>`, where
  key is `id` (default) / `published_at` / `created_at` / `updated_at` / **a Field name of that
  Collection**. Ties are **broken by id**, so walking pages with `next_offset` yields no duplicates
  and no gaps. An unknown key or a type whose order cannot be built (Image, array, Composite,
  relation) is 400. **When an order is given, that list reads everything and then sorts and slices
  pages** (same as filtering, because the order cannot be decided one page at a time). The default
  order is still ascending id, and that one keeps the storage-side paging, so a large Collection does
  not read everything. **Reverse-lookup expansion (`?populate=<inverse_name>`) stays in index
  order**, and `?sort=` does not apply (it does apply to a `?where=` list).
- **Reverse lookup can also be read from delivery** (2026-09). A list can be narrowed with
  `?where=<field>:<value>` - or, when that field references several targets,
  `?where=<field>.<target>:<value>` - to **only the published Items holding that reference**
  (`articles?where=category:3`. The value is the Item id for a Collection target and the page name
  for a Single page target. Paging and `total` apply to **the narrowed set**).
  `?populate=<inverse_name>` returns **the published content referencing this Item** under the key of
  the name the other side gave it (`inverse_name`) (for a single Item / Single page, `?limit=` caps
  the count. Default 25). In both cases the index answers the candidates, and **whether the published
  copy actually holds the reference** decides.
- The admin screen's **referrer panel**, the uniqueness check for `inverse_name` (at Schema save) and
  the reference check for **whole-Collection** deletion are all in place (2026-09). As with per-Item
  deletion, a remaining reference is rejected with 409.

### Looking up an Item by value

A unique Field **has a value that points at one entry**, so both APIs have a lookup route (a point
read of the index, no full scan).

| Method | Path | What it returns |
|---|---|---|
| GET | `/api/models/collections/{name}/items/by/{field}/{value}` | `{ "id": 1, "values": {...} }` (token required; a non-unique Field is 400) |
| GET | `/api/content/collections/{name}/items/by/{field}/{value}` | The published Item (same shape as above) |

- **On the admin side the index answers.** An Item whose draft is changing the value can be looked up
  by **either** the value the published copy uses or the value it is about to use (the same answer as
  for values that collide on save).
- **On the public side the published copy decides.** A value the draft is changing gives 404 until
  published. The value currently being served resolves as is.

### Item names (titles)

A reference stores only "which Item", so **an id says nothing to the reader**. A Collection (and a
Single page) can say **which Field is the Item's name**:

```json
{ "name": "name", "field_type": { "Text": {} }, "required": true,
  "width": 12, "height": 1, "is_title": true }
```

- **Only one per Schema.** A second is rejected on save ("'name' is already the title").
- **Only types that read as one line**: Text / Slug / Markdown / Number / Boolean / Date / DateTime /
  TextEnum. Image, arrays, Composites and relations are rejected (they have no such single line).
- Screens that display references (the Collection list, the chips and picker in the content edit
  screen) show this field's value as **the target's name**. A Collection with no title defined still
  shows `categories #3` (the reference itself) as before.
- The value is answered **in the form it is stored** (the screen renders it). It reads the working
  copy, so if the name was corrected before publishing, the corrected name is shown.

**Fetching names in bulk** (admin API. Token required):

| Method | Path | What it returns |
|---|---|---|
| GET | `/api/models/collections/{name}/items/titles?ids=1,2,3` | `{ "1": "Technology", "2": "News" }` |
| GET | `/api/models/single_pages/titles` | `{ "home": "Home" }` |

- A list asks only about "the targets of the rows on the screen" (not the whole Collection). One
  request takes up to 200 ids. Missing ids and ids of a Collection with no title are **not included
  in the answer** (the screen shows the reference itself). Pages are few, so all of them are answered
  together.

### Fields shown in the list (Collection)

The Collection list is **the screen an editor scans to find an Item**, and a Schema with a dozen-plus
fields makes an unreadable table. Which Fields let you tell Items apart is the Schema author's call,
so it is held on the Field side:

```json
{ "name": "title", "field_type": { "Text": {} }, "required": true,
  "width": 12, "height": 1, "show_in_list": true }
```

- Fields with `"show_in_list": true` become columns in that Collection's list **in Schema order**.
- **With no Field marked, no field columns appear at all**: only id, status and updated time are
  shown. Which Field tells Items apart is the author's decision, and an unselected Field is a column
  nobody asked for.
- **An image column shows thumbnails** (not urls). For an array, the first 3 and "+N".
- **A relation column shows the target's name.** If the target has a title, that value (e.g.
  `Technology`); otherwise the reference itself (`authors #1`, or `home` for a Single page). Expanding
  the target's contents is the job of the delivery API's `?populate=`.
- Single pages and Composite field definitions have no list, so this setting is not used (it does not
  appear on screen either).
- `width` / `height` concern the **content edit screen**, whereas this concerns the **list screen**.
  The values themselves still round-trip completely as before (fields not shown as columns are still
  saved, published and delivered).
- When `false` it **does not appear on the wire** (same as `unique`). A key that is absent is read as
  `false`.

### Unique Fields

Marking a Schema Field with `"unique": true` makes its value **unique within the Collection**
(currently **Text only**. It is meaningless for Single pages and Composite field definitions, so it
is rejected on save).

- **The index is "value → Item id"**. Every save does one point read and one conditional write, with
  no full scan (the same shape as the `username` reservation). Saving the same value concurrently
  lets only one succeed.
- **An empty value is not reserved** as "unset". Two entries leaving a non-required unique Field
  empty do not collide.
- **The value held by either copy is unique**: the published copy's value keeps its reservation even
  while the draft is changing to another value (so a live URL is not taken by another Item). A draft
  Item counts only the working copy (the published copy created at creation is a leftover that is not
  delivered, so it does not keep holding that value).
- Changing the value **releases the old value**, and deletion releases it too. Publish makes "the
  working copy's value become the published copy's value", so it **releases only the old published
  value**.
- A collision is **409 + `code: "value_taken"` + `field`**. `field` is an optional field present only
  in the body, and the screen embeds it in the message to show "which field". The code is part of the
  existing `error-codes.json` contract, so a mismatch among the three parties - Rust, frontend and
  catalogue - fails a test.
- **Enabling it with existing data**: at Schema save the Collection is scanned once, and if there are
  duplicates the save is rejected and the offending Items are returned. If there are no duplicates
  the index is built before saving (so as not to create a state where the index missed existing
  Items).
- **Removing `unique` removes it from the index too**: at Schema save, a Field that the previous
  Schema made unique but the new Schema does not has **all its reservations returned**. Otherwise a
  value reused in the meantime would be **rejected as "in use" when `unique` is put back later** (even
  though nobody actually holds it).

### Slug (the value used in URLs)

`{"Slug": {}}` is a type for **a string usable in a URL**. The difference from Text with `unique` is
**normalization**, which is what prevents "two Items with different spellings of the same slug".

- **The canonical form is lowercase ASCII alphanumerics joined by a single hyphen** (runs of
  `[a-z0-9]` joined with `-`). Any other character is treated as a separator, and **non-ASCII
  characters are dropped rather than romanized** (`café` → `caf`). So that arbitrary spellings do not
  end up in URLs.
- **Normalization happens on write.** The stored value, the value in the unique index and the value a
  URL looks up are all the same. **Lookups normalize the queried value too**, so both `Hello%20World`
  and `hello-world` hit the same Item (both in the admin API's `/items/by/{field}/{value}` and in the
  delivery API).
- **Slug is unique as a type**. `unique` is redundant and rejected. Empty is not reserved, as "no
  URL".
- **The length limit is 200 characters** (a property of the type, not a setting). Over it gives
  `field_too_long`.
- A value that leaves nothing after normalization (such as a Japanese-only value) is **`invalid_slug`**.
  It is not silently saved as empty.
- A Schema can point `generate_from` at **a Text field in the same Schema**. The content edit screen
  only shows a "generate from …" button; it **does not run automatically** (so published URLs do not
  move on their own).
- **When changing an existing Text into Slug**, if a stored value is not in canonical form the Schema
  save is **rejected** (the index would contain a spelling that matches no query). The rejection names
  the offending Items, so the value is fixed to canonical form and then saved.
- It cannot be used on Single pages or Composite field definitions (a page's address is its page name,
  and a Composite's contents are embedded in the Item).

### 3.0 Value validation (required, length) and the shape of a refusal

What the Schema decides about values is **validated by the server, and the screen says the same thing
first**. A mismatch would mean "you only find out after saving", so refusals are returned in a
machine-readable shape.

| Rule | Schema | Refusal `code` |
|---|---|---|
| A required value is empty | `"required": true` | `field_required` |
| Over the maximum length | `{"Text":{"max_length":N}}` (`Markdown` too) | `field_too_long` |
| Under the minimum length (empty excluded) | `{"Text":{"min_length":N}}` | `field_too_short` |
| The value type does not match (including array elements) | the `Array` element type | `field_type_mismatch` |
| A value not among the options | `{"TextEnum":[...]}` | `invalid_enum_value` |
| (not a refusal) a multi-line input | `{"Text":{"multiline":true}}` | — |
| The referenced Composite field does not exist | `{"CompositeField":{"id":"..."}}` | `unknown_composite_field` |
| The Composite's id differs from the Schema | same as above | `composite_id_mismatch` |
| An array inside an array | `Array` | `nested_arrays` |
| A Slug with no URL-usable character | `{"Slug":{}}` | `invalid_slug` |
| Two Items already hold the value a Schema save would make unique | `"unique": true` / `Slug` | `duplicate_values` |
| A stored value is not a Slug yet, on the save that makes the field one | `{"Slug":{}}` | `slug_not_canonical` |

- **Length counts characters (code points)**. It is not bytes, so `max_length: 20` passes both 20
  Japanese characters and 20 Latin characters. The screen counts the same way (`[...value].length`).
- **`multiline` is how Text looks** (added 2026-09). When `true` it is a multi-line box rather than a
  one-line input. Markdown is already a box, so it is not specified (and ignored if present).
  **Absent means `false`**, and old Schemas keep their one line. The box's **height** comes from the
  Field's `height` (the layout minimum height, in 72px units), with 1 unit = 3 lines of text for
  `rows` (the minimum is 3 lines for Text and 6 for Markdown = the same look as before the change).
  **The server does not restrict newlines in the value**: `multiline` is about the input, not about
  validation.
- **`field` is the path to the input**: `title`, `tags[2]`, `seo.description`. Even for a refusal
  inside nesting, the screen knows which input to highlight (`.field-cell.problem`).
- **"Required" is asked at publish time** (changed 2026-09). What a save writes is the **working
  copy**, which the editor has written only part of, so an empty required field still saves. The
  moment it goes on the site = publish is when completeness is first required, returning **which
  field** with `field_required`. There are two reasons: (a) to avoid **making existing Items
  impossible to save at all** when a required field is added to a Schema later, and (b) so duplication
  can create a draft with unique fields emptied (as in `§5.12`). Rules other than required (type,
  length, options, Slug) are checked on save too.
- **A blank array element is not a required-field refusal.** `required` asks the *array* to hold
  something at all, so `[]` is refused while an element that holds nothing - `null`, or `""` in a
  text array - is stored as it is. It used to come back as `field_required`, which a working copy
  accepts and publishing does not, so a draft could be saved and never published on a field that was
  never marked required.
- Situational codes other than value validation (auth, conflicts, publish races, etc.) are listed
  under `situational` in `assets/error-codes.json`. For example, **`draft_changed`** is the 409 when a
  publish raced with a save (below).
- A refusal is **400 + `code` + `field` + an English `message`**, and - when the code is about
  *something* - **`details`**: a flat object of strings naming it, e.g.
  `{"value":"intro","item":"3","owner":"1"}`. `code` is part of the `assets/error-codes.json`
  contract, and the screen shows wording in its own language, filling it with `field` and `details`
  (`The value 'intro' of 'slug' is already used by item 1.`). `message` is for clients that do not
  know the code and for logs; it names the same things in one English sentence.
- **Making a field unique or a slug checks the stored values, and says which ones stop it**:
  `duplicate_values` (two items hold the same value - `details.value`, `details.item`,
  `details.owner`) and `slug_not_canonical` (a stored value is not a slug yet - `details.value`,
  `details.item`). Both are 409s on the Schema save, and the screen tells the editor which items to
  fix rather than "a value is taken".
- On the screen, length limits are applied to the input **before saving** (`maxlength` / `minlength`
  and a count hint). Some lower bounds the browser cannot enforce, so the widget itself reports the
  problem and stops the save.
- Arrays are edited as JSON, so the screen **shows the declared element type as a hint** (`Element
  type: Number`). It also stops before saving only "elements that none of the declared types can
  read". This check is conservative and does not replace the server's decision (try in declaration
  order and take the first type that reads).

### 3.1 Pagination

Item lists accept `?limit=&offset=`. The order is fixed to **ascending Item id**, so advancing
`offset` walks everything with no duplicates and no gaps.

| | Public API (`/api/content/collections/{name}`) | Admin API (`/api/models/collections/{name}/items`) |
|---|---|---|
| `limit` omitted | `50` (`total` and `next_offset` show the continuation) | **Everything** (the API default. The admin screen always requests `25` at a time) |
| `limit` maximum | `200` | `200` |
| Total count | `total` in the response | The `X-Total-Count` header |
| Body shape | Still `{schema, items, ...}` | Still `[[id, values], ...]` |

- `limit=0`, `limit=201` and a non-numeric `limit` are **400**. Nothing is silently rounded to the
  default.
- An `offset` past the end is not an error but **an empty final page** (`next_offset: null`).
- **A page of the public API is cut by bytes as well as by count** (`MAX_RESPONSE_BYTES`, default
  4MB). Lambda can answer at most 6MB per invocation, and base64 encoding adds 30%, so trying to
  return 50 large Items **makes the function fail and nothing arrives**. What does not fit is pushed
  to the next page, and `next_offset` points at **the first Item that did not fit**. So as long as
  `next_offset` is followed there are no gaps and no duplicates (if a single Item exceeds the budget,
  that one Item is returned so an empty page does not loop forever). An implementation that walks by
  count misses Items in this case, so **use `next_offset` as the next `offset`, not `offset`**.
- Admin API lists are cut by count (the admin screen computes and shows a page count, and a silently
  short page skips rows). If fetching everything without paging is too large, the Lambda-side response
  guard answers with the CMS error (413).
- `total` is "the number of published Items in this Collection" (public API) / "the number of all
  Items" (admin API).
- The admin screen list lets you pick 10 / 25 / 50 / 100 from `mat-paginator`. Deleting the last Item
  of the last page moves back one page (so you are not left on an empty page).
- Currently the adapter reads everything and then slices. Paging itself is part of the API contract,
  so the DynamoDB adapter has room to push it down to `Limit` / `ExclusiveStartKey`.
- The on-prem adapter **serializes storage operations one by one**. LMDB cannot open a named DB inside
  a transaction, and this adapter opens the store per operation, so allowing concurrent access gives
  500s (the admin screen fetches lists, statuses and navigation in parallel and was in fact hitting
  this). Throughput is equivalent to a single writer, but concurrent requests do not corrupt anything.
  Parallelizing reads and writes is a task for the AWS adapter, which handles scale-out.

```bash
# First page
curl 'http://127.0.0.1:8000/content/collections/blog?limit=2'
# => {"schema":[...],"items":[{...},{...}],"total":5,"limit":2,"offset":0,"next_offset":2}

# Follow next_offset to the last page
curl 'http://127.0.0.1:8000/content/collections/blog?limit=2&offset=2'

# Admin side: the total count is in the header
curl -D - 'http://127.0.0.1:8000/models/collections/blog/items?limit=2' -H "Authorization: Bearer $TOKEN"
# => x-total-count: 5
```

### 3.2 `updated_at` and `created_at`

An Item (or the single entry of a Single page) carries two timestamps besides the publish status.

| Field | Where it appears | Meaning |
|---|---|---|
| `created_at` | Admin API | When the value was first saved |
| `updated_at` | Admin API | When **the content last changed** (on save, or when changes were published) |
| `published_at` | Both | When it was **first published** (kept through unpublish) |
| `last_published_at` | Both | When it was **last published** (returns to `null` on unpublish) |
| `published_by` | Admin API | The **Account** that last published (`{ id, username }`). Returns to `null` on unpublish |

The public API does **not expose** `updated_at`. With two copies, "the time it was edited" and "the
time the published thing changed" differ, and it would show unpublished edits as `lastmod`. Use
`published_at` for the published thing's last update (the published copy changes only on publish).

- **`updated_at` is "when the content last changed"**. It advances on save and also **when changes are
  published** (it does not move for a publish with nothing pending). It advances for unpublished edits
  too, so it is not exposed in the public API.
- What a site's incremental build uses is **`last_published_at`** (when the published thing changed).
  `published_at` is the first publish date, so it does not move on re-publish (it is for showing as
  the article's publish date).
- Saving a value advances only `updated_at`, and does not change `created_at` or the publish status
  (editing a published Item does not put it back to draft).
- Content saved before this feature has `created_at` / `updated_at` as `null` (no migration needed).
  Old metadata records without timestamps also read as is.
- Admin API metadata also returns `has_draft`, so published + unpublished changes can be told apart.
- `published_by` is **for auditing**: it records the identifier (username) and id at the time of
  publishing. The name is the value at that time, so the record stays readable even if the Account is
  later renamed or deleted. Like `published_at`, it is cleared by unpublish (what came down from the
  site does not keep "who published it"). **It is not exposed in the public API.**

#### Timestamps for migration (importing from another CMS)

When moving content from another CMS, **the original dates should be carried over as they are**. If
the creation date became "the day of migration", both the list and the site's ordering would lose
their meaning. So the admin API also accepts **setting** the timestamps:

| Method | Path | Body |
|---|---|---|
| PUT | `/api/models/collections/{name}/items/{id}/metadata` | `{ "created_at"?, "updated_at"?, "published_at"?, "last_published_at"? }` |
| PUT | `/api/models/single_pages/{name}/item/metadata` | Same as above |
| PUT | `/api/models/images/{id}` | `{ "uploaded_at" }` (images have only one; a display-only value) |

- **A patch**: only the written fields change. Timestamps not given, `status` and `published_by` stay
  as they are, so **it is safe to run concurrently with content being edited or published** (as with
  save and publish, the read and write are one step in the adapter).
- **`null` cannot erase.** There is no value in making callers distinguish the two cases "not written
  means untouched" and "null means forget".
- **The publish timestamps (`published_at` / `last_published_at`) need publish permission.** They are
  the values a build diffs against, so they are separate from edit permission. Creation and updated
  dates need only edit permission.
- **Impossible timestamps are rejected**: a future time (more than this CMS's clock + 60 seconds),
  `updated_at < created_at` and `last_published_at < published_at` are 400. Applying a timezone twice
  and mistyping the year are typical, and once saved the mistake is not noticed until things are
  reordered.
- **A missing Item / page / image is 404.** So as not to create orphan metadata records.
- The typical migration order: **create → publish → set the timestamps** (publish sets `published_at`
  to "now", so it is overwritten last with the intended date). For a Single page, `PUT …/item` then
  `PUT …/item/metadata`.


#### Example

```bash
TOKEN=$(curl -s -X POST http://127.0.0.1:8000/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin@example.com","password":"..."}' | jq -r .token)

# Publish
curl -X POST http://127.0.0.1:8000/models/collections/blog/items/1/publish \
  -H "Authorization: Bearer $TOKEN"
# => {"status":"published","published_at":"2026-09-13T07:19:42.672723691Z",
#     "published_by":{"id":"...","username":"admin@example.com"},"created_at":"...","updated_at":"..."}

# Anyone can read
curl http://127.0.0.1:8000/content/collections/blog
# => {"schema":[...],"items":[{"id":1,"published_at":"...","updated_at":"...","values":{"title":"Hello"}}],"total":1,"limit":50,"offset":0,"next_offset":null}
```

## 4. Using it from Gatsby

```text
GET /api/content/collections                → List of published Collections
GET /api/content/collections/{name}         → Schema + published Items (one page)
GET /api/content/collections/{name}?offset= → Repeat until next_offset is null
GET /api/content/single-pages/{name}        → Schema + the Single page's values
```

The straightforward form is to fetch these at build time and write a thin source plugin that
`createPages` them as Gatsby nodes. Because `schema` is bundled in, interpreting values needs no extra
request. Starting a rebuild is left to Webhooks (section 5). The division of labour is that the CMS
only notifies that "something changed", and the site builds by its usual procedure.

When crossing pages, pass `next_offset` straight into `?offset=`. Reading everything happens only
once, at build time, so keeping `updated_at` around allows extending to an incremental build that
processes "only Items newer than last time" on subsequent runs.

Starting from `/api/content/collections` does not list "Collections with no published Items". To make
pages for empty Collections too, use `/api/models/collections` (token required) or fix the list on
the site side.

## 5. Webhook (publish / unpublish notifications)

To start a site rebuild, a JSON body is POSTed to the configured URL on every `publish` /
`unpublish`.

### Configuration (environment variables)

| Variable | Meaning |
|---|---|
| `WEBHOOK_URLS` | Notification targets (comma separated). Unset or empty disables the webhook. Only `http` / `https` are accepted, and a bad URL is an error **at startup** |
| `WEBHOOK_SECRET` | The HMAC-SHA256 signing key for the body (optional). If unset, the body is sent unsigned |

### Request

```http
POST /hook HTTP/1.1
Content-Type: application/json
X-CMS-Event: collection_item.published
X-CMS-Delivery: c8876228-2f03-4b39-801d-f19f60131969
X-CMS-Signature: sha256=...
```

```json
{
  "event": "collection_item.published",
  "collection": "blog",
  "id": 1,
  "status": "published",
  "published_at": "2026-09-13T07:26:21.005941238Z",
  "occurred_at": "2026-09-13T07:26:21.006176750Z"
}
```

| `event` | Meaning |
|---|---|
| `collection_item.published` / `collection_item.unpublished` | An Item's publish status changed (`collection` and `id`) |
| `single_page.published` / `single_page.unpublished` | A Single page's publish status changed (`page`) |

- **No values are included** in the body. The receiver goes and fetches the `/api/content/*` it needs
  (sending everything would bloat the body and duplicate management with the immediate re-fetch).
- `published_at` is the publish time, and `null` for unpublish. `occurred_at` is the time the event
  occurred.
- `X-CMS-Delivery` is a per-delivery UUID and can be used by the receiver for deduplication.

### Signature verification (HMAC-SHA256)

`X-CMS-Signature` is `sha256=` + the HMAC-SHA256 (lowercase hex) over **the raw request body**.
Parsing the JSON and re-serializing changes the bytes, so always verify against the raw body.

```python
expected = "sha256=" + hmac.new(SECRET, raw_body, hashlib.sha256).hexdigest()
if not hmac.compare_digest(request.headers["X-CMS-Signature"], expected):
    return 401
```

### Delivery properties (important)

- Delivery **does not make the request wait**. Publish responds as soon as the status is saved, and
  the send happens in the background.
- On failure it retries up to 3 times at 500ms → 1s intervals. `4xx` is not retried (the receiver
  understood and refused).
- Publish succeeds (200) even if the receiver is down. A failed delivery appears in the server log as
  `webhook delivery failed`.
- Multiple URLs are delivered independently. If one is down the others still receive.
- Certificate verification for `https` uses **the deployment target's trust store** (no root
  certificates are bundled). A minimal container without CA certificates fails verification, so
  confirm that the deployment target has a system CA bundle. The crypto implementation registers
  `ring` at startup (because `reqwest` 0.13 does not bundle a provider).
- **Nothing is sent when the status was not saved**, such as when `publish` is a 404.
- **No outbox is provided** (the current policy): if the process dies before delivery, that event is
  lost. For the "start a rebuild" use case a missed event can be redone by the next publish, so this
  is accepted.
- **On AWS Lambda the execution environment may be frozen after the response**, so a background send
  may not complete. When running on Lambda, delivery must go through SQS / EventBridge or be made
  synchronous.
- Order is not guaranteed. The receiver is assumed to use it for little more than "start a rebuild".

## 5.5 Permissions

| Operation | Required |
|---|---|
| Read (including drafts) | `can_view` |
| Create and edit values (working copy) | `can_edit` |
| Publish / unpublish, delete content | `can_publish` |
| Change Schemas, Collections, Single pages, Composite fields | `is_admin` |
| Account management (`/api/auth/users`) | `is_admin` |
| Image upload (`/api/models/images/get_upload_url`) | **`can_edit` on any one resource** |
| Image change, replacement, deletion | `can_edit` (account-wide) |

Roles are treated as combinations of these flags.

| Role | can_view | can_edit | can_publish | is_admin |
|---|---|---|---|---|
| Viewer (view only) | ✓ | – | – | – |
| Editor (up to drafts) | ✓ | ✓ | – | – |
| Publisher (edit + publish operations) | ✓ | ✓ | ✓ | – |
| Admin | ✓ | ✓ | ✓ | ✓ |

The decision "reads use `can_view`, everything else uses `can_edit`" is made in one place of
middleware, and things that the method alone cannot decide (publish, delete, structural changes) are
additionally decided in the handler. **Images belong to no Collection or page**, so there is no
resource to decide against. Therefore **upload alone uses "`can_edit` on any one resource"** (an
editor trusted with only one Collection also needs a way to upload the images that Collection uses),
while **changing or deleting an existing image stays account-wide `can_edit`** (because other content
may be using it). **Editing does not touch the published copy**, so a role with only `can_edit` can be
operated safely.

#### Publish promotes only "the working copy it read"

Publish is the operation that moves the working copy onto the published copy and **deletes the
working-copy record**. If another save came in between reading the working copy and moving it, that
new save would be deleted, so **the move is conditional on "the stored working copy being exactly
what the publish operation read"** (the content comparison does not depend on key order).

- If the condition fails, **nothing is applied and it is 409 + `code: "draft_changed"`**. The screen
  shows "It was saved again during publishing. Reopen and publish again".
- Publishing a Single page (`apply_page_status`) follows the same rule.
- If publish runs **after** the new save (the opposite order), nothing is lost: the save only writes a
  new working copy.
- The same goes when two publishes race: the loser ends with `draft_changed` (the published copy never
  ends up half swapped).

### Account management API

| Method | Path | Contents |
|---|---|---|
| GET | `/api/auth/users` | List |
| POST | `/api/auth/users` | Create (`username` / optional `email` / `is_admin` / `permission`. **No password is taken**). The answer is the Account plus `needs_credential` |
| PATCH | `/api/auth/users/{id}` | Partial update of `is_admin` / `is_active` / `permission` |
| DELETE | `/api/auth/users/{id}` | Delete |
| POST | `/api/auth/me/password` | Change your own password (the current password is required) |

**Only the person themselves can choose a password.** A created Account has no credentials and cannot
sign in until the person sets one with a reset link (putting `password` in the body is **ignored** -
neither the API nor the screen has a route for deciding someone else's password on creation). All an
admin can hand over is the reset link.

**`needs_credential` in the create answer says whether that is so for this Account.** Where an identity
provider owns the credentials (`password_login: false`), creating an Account also asks the provider for
one, and the provider may answer **"it is already there"** - an operator who made the person in the
provider's console, or an Account coming back after the CMS's own records were replaced. Such an
Account has **its own way in**, so it answers `needs_credential: false` and the screen adds it without
issuing a reset (resetting would take that way in away). An Account the provider had to create has
none, so it answers `true` and the screen offers the reset straight away. Where this CMS holds the
credentials itself there is nothing to adopt from, so a created Account always answers `true`.

The only exception is the **initial admin** the deployment creates itself (`ADMIN_PASSWORD`). When
nobody can sign in there is no one to issue a link, so only here is the password chosen on the
person's behalf. This creation and the credentials save form one pair; if the save fails the Account
is deleted too (leaving it would make the next startup skip bootstrap and produce a deployment nobody
can enter).

`/api/auth/users*` is admin only. `/api/auth/me/*` operates on yourself, so **it is usable even by an
Account with no write permission** (to avoid a situation where a view-only person cannot change their
password).

#### Account identifiers

- Sign-in uses **`username`**, and `email` is **an optional contact address**. Operations without
  email addresses (`ops`, `team-editor`, etc.) work as they are.
- `username` is 1–128 characters from `A-Z a-z 0-9 + = , . @ _ -` only. This character set matches
  what Cognito accepts, so it can be taken into a pool as is (an address like `ops@example.com` is
  valid as a username too).
- Case and surrounding whitespace are treated as the same (`Ops.User` and `ops.user` are the same
  Account).
- When `email` is given only its form is validated (an empty string is treated as "unset"). It **is
  not used for sign-in**, so an unreachable address is fine.
- `username` is an identifier, so it is not changed after creation (if a change is needed, recreate).
  Cognito usernames are immutable too, so this assumption carries over as is.
- The initial admin uses `ADMIN_USERNAME` and `ADMIN_PASSWORD` (only this one person has a
  deployment-chosen password). Without `ADMIN_USERNAME`, `ADMIN_EMAIL` is used as the identifier (it
  still starts with the older setting name). `ADMIN_EMAIL` is also recorded as the contact address at
  the same time.

- **A change that leaves zero enabled admins is rejected** (demoting, deactivating or deleting the
  last admin is 409). You cannot demote yourself into a lockout either.
- A deactivated Account cannot log in and its existing tokens are rejected. Deletion removes the
  history too.
- **Changing a password invalidates every existing token.** An Account has a token generation
  (`token_version`), and a token carries the generation from when it was issued. Authentication reads
  the Account every time, so a token with an old generation is 401 regardless of its remaining
  lifetime (no clock agreement is needed either).
  - `POST /api/auth/me/password` returns **a token of the new generation**. The changer's own session
    is cut too, so the screen saves this and continues (sessions on other devices stay ended).
    Response: `{"token":"...","expires_at":"..."}`
  - Completing a reset link (`POST /api/auth/password-reset`) likewise invalidates only that
    Account's tokens. An admin merely issuing a link invalidates nothing.

### Login attempt limiting

To stop brute force, **failures for the same email address are counted**, and over a certain number a
429 is returned.

| Item | Value |
|---|---|
| Allowed failures | 5 |
| Failure counting window | 15 minutes (failures spaced wider apart are not counted) |
| Lock length | 15 minutes from the last failure |
| On a successful sign-in | The counter is cleared |

- The 429 body gives the reason and the wait time, and the header carries `Retry-After` (seconds).
- During a lock **even the correct password does not pass** (it is refused before the password is
  looked at).
- **Nonexistent addresses are counted the same way.** Counting only Accounts that exist would let the
  presence of an Account be inferred from "whether it became a 429", nullifying the point of unifying
  the 401 message.
- Repeated attempts during a lock **do not extend the lock** (it always clears 15 minutes after the
  last failure). One person's repeated mistakes cannot stop the whole organization either (the
  counter is per address).
- The counter lives **in process memory**. It is lost on restart and not shared across processes (an
  environment where instances are replaced, such as Lambda, needs shared storage).
- The same counter is used for verifying the current password in `POST /api/auth/me/password`.

### Password reset (an admin hands over a new way in)

For an Account whose password is forgotten / unknown, **an admin issues a reset**. The CMS does not
send email: the admin hands what was issued to the person (Slack, verbally, printed, etc.). Therefore
**it works for Accounts without an email address too**.

**What is handed over differs by deployment** (`password_reset` in `GET /api/auth/capabilities`
promises it in advance, and `kind` in the response says the same thing):

| Method | Path | Contents |
|---|---|---|
| POST | `/api/auth/users/{id}/password-reset` | Issue a reset (requires `is_admin`). `{"kind":"link","token":…,"expires_at":…}` or `{"kind":"temporary","password":…}` |
| POST | `/api/auth/password-reset` | **Public**. Sets a new password with `{ token, new_password }` and returns a new token (link deployments only. 501 in temporary-password deployments) |

- **`link`**: a deployment where the CMS holds passwords. The person opens the link and decides their
  own password, so nobody else - the admin included - knows the value.
- **`temporary`**: a deployment where the identity provider owns sign-in (Cognito). The CMS holds no
  passwords and cannot create a value, so the provider sets a **temporary password** and forces a
  change at the next sign-in (`AdminSetUserPassword` with `Permanent=false`). The admin hands the
  value shown on screen to the person.
- The screen URL is assembled by the frontend: `/reset-password?token=<token>` (the API returns only
  the token. The UI's route is not something the API should know).
- The lifetime is `PASSWORD_RESET_TTL_MINUTES` (default 30 minutes). The token is an HMAC-SHA256 over
  `password-reset:v1:<user id>:<token_version>:<expiry>` (`JWT_SECRET`, a dedicated prefix).
- **Single use.** Completing it changes the password and advances `token_version`, so the same link
  is refused afterwards. **Every existing session from before the reset is cut too** (the same effect
  as a first response to a leak).
- How failures are returned: used, expired and deactivated Accounts are **403** (the reason is clear);
  a broken token or a nonexistent Account is **401** (a public endpoint does not leak existence).
- Attempts are limited with the same counter as login.
- **Only the person themselves can choose a password** (link deployments). All an admin can do is
  issue a link, and neither the API nor the screen has a route to set someone else's password directly
  (there used to be `POST /api/auth/users/{id}/password`, which was removed). The same goes for
  Account creation: the screen does not ask for a password and issues this reset right after creating
  (in temporary-password deployments it hands over the value the provider set).

### Per-resource permissions

In addition to the Account-wide role, **permissions can be overridden per Collection / Single page**.
An override **replaces** permissions for that resource only, so it can widen or narrow them.

| Example | `permission` | `collection_permissions` |
|---|---|---|
| View only everywhere | viewer | – |
| Can edit only `blog` | viewer | `{ "blog": editor }` |
| Touch anything but `legal` | editor | `{ "legal": all false }` |

- Send **the whole map** in `collection_permissions` / `single_page_permissions` of
  `PATCH /api/auth/users/{id}` (it is not a partial merge). Sending `{}` clears all overrides.
- The decision uses "that resource's effective permission". Reads use `can_view`, writes use
  `can_edit`, publish, unpublish and delete use `can_publish`. **An admin can always do everything**
  (an override cannot shut them out).
- The lists in `/api/models/collections` and `/api/models/single_pages` return **only what can be
  read**, so a denied resource does not appear in navigation either (opening it directly gives 403).
- Granting to a nonexistent name is 400. A misspelling does not remain as "a permission that appears
  nowhere and does nothing".
- The targets are Collections and Single pages only. The Image library and Composite field definitions
  stay Account-wide (`can_edit` / admin).

## 5.6 Shareable preview URLs

A **signed, time-limited** URL for showing a draft to someone without an Account (a client, a
translator). The other party needs neither a token nor an Account.

```bash
# The admin side issues a link (requires can_edit)
curl -X POST http://127.0.0.1:8000/models/collections/blog/items/1/preview-link \
  -H "Authorization: Bearer $TOKEN"
# => {"path":"/api/preview/collections/blog/items/1?token=1758000000.3f9c...","expires_at":"..."}

# The recipient can open it without a token (the working copy is visible)
curl http://127.0.0.1:8000/preview/collections/blog/items/1?token=1758000000.3f9c...
```

**A per-Schema permission is required.** The default is **disabled**, and only Collections / Single
pages saved with "allow preview links" turned on in the Schema editor can issue links (requires
admin).

| Method | Path | Contents |
|---|---|---|
| GET | `/api/models/collections/{name}/settings` | `{ "preview": false }` (token required. The default when unset) |
| PUT | `/api/models/collections/{name}/settings` | `{ "preview": true }` (requires admin) |
| GET / PUT | `/api/models/single_pages/{name}/settings` | Same as above |
| POST | `/api/models/collections/{name}/items/{id}/preview-link` | Issue a link (requires `can_edit` and `preview` enabled) |
| POST | `/api/models/single_pages/{name}/preview-link` | Same as above |
| GET | `/api/preview/collections/{name}/items/{id}?token=...` | Return the working copy (no auth required) |
| GET | `/api/preview/single_pages/{name}?token=...` | Same as above |

- Issuing to a Schema that is not allowed and opening such a link are both **403 `preview_disabled`**
  (the message names the Collection or page). **Turning the setting off makes already-issued links
  unopenable on the spot** (a link lives for minutes, and there is no ledger to revoke against).
  Existing deployments have no setting, so even Collections that used to work **cannot issue links
  until an admin turns it back on**.
- The setting lives in **a separate record** from the Schema definition (`collection_settings` /
  `single_page_settings`. In DynamoDB, `settings` in the same partition). Saving fields never writes
  the setting back, and deleting the Collection or page deletes the setting too. The display order is
  the same: a saved setting is not carried over to a Schema recreated later with the same name.
- The lifetime is `PREVIEW_LINK_TTL_MINUTES` (default 60 minutes). The token has the form
  `expiry.signature`, and the signature is an HMAC-SHA256 over **the destination and the expiry
  itself** (using `JWT_SECRET`; a dedicated prefix is added to the message so it cannot be reused with
  other signatures).
  - Rewriting the destination makes the signature mismatch, so **one link can open only one working
    copy**.
  - The expiry cannot be extended either. Expired is 403; a wrong signature or a broken token is 401.
- There is **nothing stored on the server** (the expiry is part of the signature, so there is no
  record to delete).
- However, a link **is a credential to whoever holds it**. Until it expires it can read that one
  draft, so be mindful of who it is given to and of the lifetime. To revoke, **turn off that Schema's
  permission** (effective immediately), shorten `PREVIEW_LINK_TTL_MINUTES`, or change `JWT_SECRET`
  (all tokens become invalid).
- The returned body has the same shape as the admin-side preview (schema + values), so the site needs
  only one parser.
- **The link handed over is mapped onto the preview site's URL.** Handing over the API path as is
  shows the reviewer JSON. `preview_site_url` in `GET /api/auth/capabilities` is the preview site's
  origin, and the path is the API path with `/api` removed
  (`/preview/collections/{c}/items/{id}`). In a deployment without `preview_site_url`, the admin
  screen does not copy and shows "preview site not configured" (it does not hand over a raw JSON URL).
  The full contract is in `docs/preview-site.md`.

### Replacement applies only to "the upload given to this image"

`PUT /api/models/images/{id}` (specifying `file_name` to apply a replacement) decides using **the
record looked up by id**. The file name is chosen by the server, and at the time of `POST .../replace`
it is **recorded in that image's pending queue**. Nothing else can be applied:

| Situation | Answer |
|---|---|
| The image does not exist | 404 |
| The file is not in the bucket yet | 404 `the uploaded image is not there yet` |
| `file_name` is **the file this image is currently serving** | **200 (do nothing)** - the screen sending the same file is not an error. **The pending entry is not cleared** |
| `file_name` is **the pending upload given to this image** | **200 (applied)**. The pending entry is cleared at the same time |
| Anything else (**another image's file**, a file nobody requested) | **400** `that is not the upload this replacement was for` |

- **Only the last request is pending.** Calling `POST .../replace` again replaces the previous pending
  entry with the new one (trying to apply the old one afterwards gives 400).
- **Sending the currently served file does not clear the pending entry.** If it did, the upload
  produced by the immediately preceding "replace" button would be invalidated by a mere resend from
  the screen.
- Both `POST .../replace` and the apply **write only while the record is as they read it** (on-premises
  reads and writes inside the same lock). Writing back an old record after an apply intervened would
  keep pointing at the file the apply deleted, making the image undisplayable.

**Why id**: what the decision needs is not "whose file is this" but "**is it the upload given to this
image**". The former would mean scanning the whole library for "a record that happens to use that
name", and would say nothing about **an upload that was just made (no record points at it yet)**. The
latter is a point read by id and does not depend on the shape of the delivery URL (whether it is
signed or not).

The lesson from this design is that **the file name must not be read from the URL**: in a
signed-delivery deployment the URL ends with a signature, so a comparison that looks at the URL
**quietly stops working** (it once was that way).

### Image URLs differ by deployment (stable URL or signed)

The `url` of an `Image` value, and the `url` in an upload response, are returned **in the delivery
style the deployment chose**.

| Deployment setting | Returned URL | Bucket |
|---|---|---|
| `AWS_IMAGE_DELIVERY=public` (default) | The object's address (or the CDN URL if one is in front). **No expiry** | Public read |
| `AWS_IMAGE_DELIVERY=presigned` | **A presigned GET** (`AWS_IMAGE_URL_TTL_SECONDS`, default 1 hour). **Expires** | Private |

- In both cases **what is stored in content is the id**. The URL is resolved every time the API reads,
  so even after a signature expires the next fetch returns a new URL.
- Signed mode suits **sites that fetch images at build time and serve them themselves** (the SSG
  transforms and serves its own copies). For a site that puts CMS URLs straight onto pages, **a cached
  URL dies when it expires**, so the default is `public`.
- For hand-written Markdown use **`/api/images/by-id/{id}`**. The CMS forwards to the current file, so
  it works regardless of mode and after a replacement (the forwarding target becomes the signed URL at
  that point).

## 5.9 Image thumbnails (for tiles) and paging

Admin screen tiles are 180px wide, while a single original photo is on the order of 1MB. **Using
originals for tiles downloads 154MB to page through a 275-image library to the end** (measured.
`loading="lazy"` only delays the fetch; what is fetched is the original). So **the browser creates a
small copy at upload time**, it is stored as a separate file from the original, and the list and
picker display that. The server does not decode images (it treats them as bytes, same as originals),
so adapters do not diverge.

| Method | Path | Contents |
|---|---|---|
| PUT | `/api/models/images/{id}/thumbnail?ext=webp` | Send the small copy's bytes in the body. `204`. Requires `can_edit` |
| GET | `/api/models/images?limit=&offset=` | One page. The total is in `X-Total-Count` |
| GET | `/api/models/images/trash?limit=&offset=` | Same as above (trash) |

- Each list element carries `thumbnail_url`. **When it is absent, `url` is displayed** (images
  uploaded through the API, or images from before this feature). `thumbnail_url` uses the same
  delivery method as the original (on-premises `/api/images/thumb-<uuid>.<ext>`, AWS the bucket/CDN or
  a signed URL).
- `ext` becomes the extension of the stored name and decides the type at delivery. Only safe
  extensions are accepted, **a "small copy" over 512KiB is 413**, and an empty body is 400.
- **A replacement (§5.8) discards the small copy**: the copy describes "that photo", and if the photo
  changes the old copy becomes a lie. The browser sends a copy again after a replacement.
- **Permanent deletion removes the copy too** (along with the original). Moving to the trash leaves
  both the file and the copy.
- `limit`/`offset` follow the same rules as a Collection's Item list (`limit` is 1..=200, default
  everything. `limit=0` is 400). The screen loads 60 at a time and appends the next with "load more".

## 5.10 Image trash

Deletion is **two-stage**. `DELETE` is permanent deletion and cannot be reached without going through
the trash.

| Method | Path | Contents |
|---|---|---|
| GET | `/api/models/images` | The library (does not include trash contents) |
| GET | `/api/models/images/trash` | The trash (newest deletion time first) |
| POST | `/api/models/images/{id}/trash` | Move to the trash (requires `can_edit`) |
| POST | `/api/models/images/{id}/restore` | Return to the library (requires `can_edit`) |
| DELETE | `/api/models/images/{id}` | **Permanent deletion** (the record and the file. Requires `can_edit`). **Only images in the trash** |

- Moving to the trash **keeps both the record and the file**, so content referencing that image
  **keeps being displayed** (both the file and the delivery of `/api/images/by-id/{id}` stay alive). A
  stage to avoid "it is gone when you thought you had only deleted it".
- An image in the trash **does not appear in the library list or the picker**, so new content cannot
  select it.
- Only permanent deletion removes the file. After that, images in content that referenced that id stop
  resolving.
- Moving to the trash is **idempotent** (moving an image already in the trash again is not an error).
  A nonexistent id is 404.

## 5.11 Where an image is used (reference index)

So that "what is using it" can be answered before deleting an image, there is **a reverse reference
index**.

| Method | Path | Contents |
|---|---|---|
| GET | `/api/models/images/{id}/references` | The list of content using that image |

The response is `[{"kind":"collection_item","name":"blog","item":7},
{"kind":"single_page","name":"about"}]`. The screen **includes this list in the confirmation text**
for moving to the trash and for permanent deletion (if in use it names them: "used by N Items (blog
#7, about)").

What the index counts:

- **`Image` fields** (including inside arrays and inside Composite fields). The value is an id, so it
  is exact.
- **`/api/images/by-id/<id>` links in Markdown** (the form the library's "copy link" creates).
  **Hand-written URLs (direct S3 or CDN links) cannot be followed** - they cannot be told apart from
  ordinary links, and a URL that changes on replacement would be used. This does not apply if the
  operation writes direct links in bodies.
- **Both the published copy and the working copy**. However, **the published copy is counted only
  while published** (a draft merely holds the record from creation and nobody is serving it. The same
  rule as the unique index).

Update timing: saving an Item / page, publish, unpublish, deletion. Even if the index update fails
**the save still succeeds**, and only one warning is missing (it is left in the log).

## 5.12 Duplicating articles and bulk publish

| Method | Path | Contents |
|---|---|---|
| POST | `/api/models/collections/{name}/items/{id}/duplicate` | Duplicate (201 + the new id. Requires `can_edit`) |
| POST | `/api/models/collections/{name}/items/status` | `{ "ids": [...], "status": "published"\|"draft" }` (requires `can_publish`) |

**Duplicate**:

- What is copied is **the content the edit screen sees** (the working copy if there is one, otherwise
  the published copy).
- The copy is **always created as a draft**, and does not inherit the publish date or the publishing
  Account.
- **Unique fields are emptied** (Text with `unique`, and Slug). Two Items cannot hold the same value,
  so a plain duplicate would be rejected. The editor fills in the blanks - for a Slug, "generate from
  title" works as is.

**Bulk publish / unpublish**:

- **The result is returned per entry**: `[{"outcome":"changed","id":1,"metadata":{...}},
  {"outcome":"refused","id":999,"code":"not_found","message":"..."}]`. One refusal (nonexistent, or
  saved while publishing) does not stop the others.
- Each entry goes through the same path as a single publish (`set_item_status`), so the guarantees and
  refusals are the same.
- The limit per call is **100 entries** (each entry is a transaction + index + Webhook, so bulk is a
  convenience for the range the screen is showing on one page). An empty array is 400.

## 5.13 Signing in with an external provider (Cognito)

In a deployment where the CMS does not handle passwords, sign-in happens **on the provider's page**,
and the CMS **only verifies the token**. `login_url` in `GET /api/auth/capabilities` is that page's
address, and `password_login: false` is the signal that "passwords are not accepted here".

Flow:

1. The screen adds **the PKCE challenge (S256), `state` and `redirect_uri`** to `login_url` and sends
   the browser there (the verifier and state are kept in that tab's `sessionStorage`).
2. The provider returns `code` and `state` to `redirect_uri` (`<app_url>/api/auth/callback`).
3. The screen checks the state and **asks the server to exchange**:

| Method | Path | Contents |
|---|---|---|
| POST | `/api/auth/cognito/exchange` | **Public**. `{ code, code_verifier, redirect_uri }` → `{ token, expires_at }` |

4. The server form-POSTs to the provider's token endpoint (`/oauth2/token` on the same domain as
   `login_url`) and returns the `id_token`. **The exchange is not done directly from the browser**
   because the token endpoint returns no CORS headers.
5. The screen adopts that token as the session and reads the permission record with `GET /api/auth/me`
   (on first sign-in, anyone in `BOOTSTRAP_ADMIN_USERNAMES` is created as an admin, and anyone outside
   the list gets 403).

- **A public client, so there is no secret**. The only proof that the code came back to the same
  caller is the PKCE verifier.
- Verification of the signature, issuer, audience, expiry and `token_use` is done by
  `CognitoVerifier` (RS256 + JWKS, 10-minute cache).
- The exchange route is composed **only by deployments that use the provider** (on-prem has no such
  code).
- On the Terraform side the client is configured with `callback_urls = ["<app_url>/api/auth/callback"]`,
  `allowed_oauth_flows = ["code"]` and `allowed_oauth_scopes = ["openid","email"]` (the `app_url`
  variable). Without this the provider returns `redirect_uri is not registered`.
- CMS sign-out clears **only the local session**. The provider-side session remains, so the next
  sign-in returns without confirmation (using the provider's logout is separate).

## 6. What does not exist yet

- **Incremental builds using `published_at`**: the value is returned, but the site-side implementation
  is still to come.
- **Audit log (history)**: currently only "the Account that last published" is held. There is no
  **history** of who saved, published or deleted what and when (an append-only log would be needed).
- **Email sending**: neither resets nor notifications are **sent by email** (the approach is to issue
  a link and hand it over). The idea of configuring SMTP and sending automatically is unimplemented,
  and adding it would need `SMTP_URL` / `MAIL_FROM` and an absolute URL for the body.
- **A user-initiated "forgot password" flow**: unimplemented. Implementing it would make `email`
  required and need a design that always returns the same response so as not to leak the existence of
  an Account (currently only admin issuance).
- **A machine-readable specification (OpenAPI)**: there used to be `docs/swagger.yaml`, but it covered
  only part of the implemented routes (4 paths), and `info.description` still contained the Swagger
  Petstore sample text verbatim. **This document is the contract**, and tests pin it down, so it was
  **deleted**. If a specification readable by external tools is needed, decide to "write a new one"
  together with code generation (managing it by hand in two places always drifts).

## 7. AWS support

The reasons for deciding to run on Lambda + DynamoDB + S3, and what was learned by implementing it,
are in [`docs/aws-decisions.md`](aws-decisions.md). The forks that cannot be undone once decided
(image delivery style, adopting Cognito, Webhook delivery style, IaC), the testing policy and the
remaining tasks are gathered there.

## 8. The policy of not giving the CMS side GraphQL

Gatsby's GraphQL is **a build-time data layer**, and the CMS has no need to speak GraphQL.
`gatsby-source-graphql` is effectively deprecated, and Gatsby itself is not active.
Therefore, first put the REST public API in order, and if the need arises proceed in this order.

1. `/api/content/*` (this document. Implemented)
2. Webhooks triggered by publish / unpublish (this document. Implemented)
3. A Gatsby source plugin (REST → GraphQL nodes)
4. Consider a GraphQL layer only when consumers grow and GraphQL becomes genuinely necessary
