# Designing content relations

**Status: implemented** (2026-09; the ordering of references is in too). The picker, `inverse_name`,
the referenced-by panel, the expansion in delivery, the consistency of publishing, and the
reference check on collection deletion all work. The first user is the migration from Strapi
(`scripts/migrate-from-strapi/`), and that tool currently drops relations by default (§7).

The equivalent of Strapi's relation - an item of a Collection referencing an item of another
Collection - goes into this CMS's design. **Image references are already its prototype**, so rather
than bringing in a new concept, this generalises the mechanism that works for images.

## 1. What to build

- A **reference field** can be defined on an item of a Collection (the target is another Collection,
  or a Single page)
- The value is a **set** of references (it has no order). One (single) or several (`has_many`)
- **Reverse lookup** (the content referencing this item) is visible in the API and on screen.
  **It can also be read in the delivery API** - "the articles of category 3" comes back with paging
  (a filter), and "this category and its articles" comes back in one level (the expansion of the
  reverse lookup)
- In the public API, **only one level is expanded**, and only when it is explicit (published targets
  only)
- **Deleting a referenced item is rejected** (showing the reason and the referencing items; use
  `?detach=true` to detach and delete)
- The migration tool can import Strapi's relation (`--relations=relation`)

**What is not built**: references to users (permissions are a separate matter), deep recursive
expansion of references, and manual management of both directions of a reference (the reverse side
comes automatically from the index).

## 2. What we decided

| Point | Decision | Reason |
|---|---|---|
| The defining side | **One side only**. The side with the field is the "owning" side; the reverse side is read from the index | Removes the double management of Strapi's `mappedBy`/`inversedBy`. The same as image references |
| Cardinality | **Only two kinds, via `has_many: bool`** | Strapi's oneToOne / oneToMany / manyToOne fold into these two (putting a single reference on the many side gives oneToMany) |
| The shape of the value | An **ordered list** of `{ "target": "<collection or page>", "id"?: n }` | Changed from a set in 2026-09. The order carries meaning, as in "featured articles in this order", and the site serves it as it is. On save, **only duplicates of the same reference** are removed, and the order stays as written (the index, deletion and detach treat it as a set, so they are unaffected) |
| The target | **Both Collections and Single pages**. A Single page only allows `has_many: false` | A page has no id, so only "is it referenced" becomes the value (holding several is meaningless) |
| Reverse lookup | **An index** (the schema only holds the name). Management reads it with `GET …/references`; delivery sees it through filters and expansion | The same as the image's `GET /models/images/{id}/references`. **No recomputation is needed to read it** (the index is updated when writing) |
| The name of the reverse side | `inverse_name: Option<String>` can be written in the schema. **It holds no data** | Used for the heading on screen ("the articles of this category") and for `?populate=<inverse_name>` in delivery. Unlike Strapi's `mappedBy`, it is not doubly managed |
| Writing the index | **Together with the body** (`TransactWriteItems` on DynamoDB; the same lock on-prem) | Because it is used to decide deletion, it is stricter than image references (an image takes two steps, body then index, and a drift only makes deletion conservative) |
| Publishing while referencing unpublished targets | **Rejected if a `required` relation would be empty at Publish time**. An optional relation is dropped in delivery | Validation on save already says "required does not allow empty", so publishing uses the same rule. If part of an array is unpublished, the rest is served, so it passes |
| Unpublish of an item that is referenced by published content | **The same rule**: rejected if, as a result of unpublishing, a `required` referrer would be empty | It is explained by the same single rule as publishing |
| The side being deleted | **Rejection is the default**. `?detach=true` detaches the references first, then deletes (all references are detached) | The same idea as the image's "trash → check references → delete permanently" |
| Expansion in the public API | **By default only the id**; `?populate=<field>` expands one level | Images are always expanded, but many-to-many makes responses heavy, and cycles and unpublished targets need explaining. Being explicit is safer |
| Cycles and self-references | **Allowed** | They are values, not definitions, so there is no creation-order problem. Expansion is one level, so it does not recurse |
| The element type of an Array | **Cannot be one**. Only `has_many` holds several | A relation is already a set, so making it an Array would be a second way of saying "a set of sets". It could be neither edited nor delivered (rejected by validation) |
| Where it can go | **In Fields and in Composite field definitions**. It cannot be the element type of an Array | A definition is stored and reused independently, so the target is decided by the site's Collections / Single pages (it does not depend on the embedding side). The value goes inside the Composite, but **the item is what holds the references**, so the owner of the index does not change |

## 3. The shape of the data

### Schema (`core/src/models/schema.rs`)

```rust
FieldType::Relation(RelationOptions {
    target: RelationTarget,       // Collection(name) | SinglePage(name)
    has_many: bool,               // only false for a Single page target (rejected by validation)
    inverse_name: Option<String>, // the name on the reverse side. Used only for display and ?populate=
})
```

- Validation happens **in the same place as Composite field references**: that the target exists,
  that a Single page has `has_many: false`, and that `inverse_name` is unique on the target
  (`referenced_targets` is added next to `referenced_composite_ids`).
  - **The existence of the target** needs the list of Collections and Single pages, so it is checked
    in the schema save service (`RelationTargetSource` answers both names). **A relation inside a
    Composite definition has the same target**, so `referenced_relation_targets` walks into
    Composite definitions as well (a definition is walked once, because a definition can come back
    to itself through an Array). There are two places to check: **when the definition is saved**
    (`CompositeFieldService`; a definition that nobody embeds would otherwise be saveable) and
    **when the embedding side's schema is saved** (a Collection / Single page).
  - **The uniqueness of `inverse_name`** is checked when the schema is saved
    (`ensure_inverse_names_are_unique`). Giving the same name twice to the same target would make
    it undecidable which one the expansion of the reverse lookup returns.
- `inverse_name` is **only a name** and holds no data. The reverse lookup is always read from the
  index.
- A constraint on the order of creating definitions appears (the target comes first). Strapi's
  mutual references are avoided by adopting only one side (§7).

### Values (`core/src/models/values.rs`)

```rust
// The value is only "the target's name" and "the item's id". The schema knows which kind it is.
FieldValue::Relation(Vec<RelationRef>)    // RelationRef { target: String, item: Option<u64> }
FieldValueResponse::Relation(Vec<RelationResponse>)   // the shape of §5
```

- For a Collection target, `{ "target": "authors", "item": 7 }`; for a Single page,
  `{ "target": "home", "item": null }` (a page has no id). A value whose shape disagrees with the
  schema's kind is rejected by `from_untyped`.
- **The order is the value**: the order written is stored as it is, and the delivery API returns it
  in that order too. The same reference written twice is one entry and stays at **the position where
  it first appeared** (`from_untyped`). The index is a set of (owner, target), so **reordering does
  not move the index** (`RelationIndexChanges::between` is a set difference).
- The chips in the admin screen are **reordered with left and right arrows**, and the picker appends
  to the end in the order chosen (a single reference has no order, so no arrows appear).
- **Store it as a set**: on save, sort and deduplicate by `(target, item)`. Without an order, it is
  necessary to keep comparisons (the diff against the published copy), the index's difference
  calculation, and the assertions in tests stable.

What has to be added is every branch the existing types pass through:

| Place | Content |
|---|---|
| `FieldValue` / `FieldValueResponse` | A new variant |
| `from_untyped` | Accepts `[{"target": "...", "item": n}, ...]` (the same shape as Array element type validation) |
| `test_required` | `has_many: false` requires one or more; `true` allows empty |
| `FieldSchema::get_default_value` (schema.rs L150) | An empty array |
| `FieldValue::to_response` (values.rs L721) | Make it **pass in the information for resolution**, as with images (today it takes `&HashMap<ImageId, Image>`) |
| Next to `referenced_images` | `referenced_items` (walks the value tree; it also looks inside Arrays and Composites; `without_reference` is the same) |
| The frontend `value-field` | The branch per type (§6) |

**The shape the management API returns** (`FieldValueResponse` returns the array of references as it
is; only the delivery API expands):

- `RelationTarget` is internally tagged as `{ "kind": "collection" | "single_page", "name": "..." }`.
  The same name with a different kind is a different target, so validation matches it against the
  list for each kind.
- A Single page's value is `{ "target": "home" }` (`item` is not attached). Sending `item` is
  rejected.
- It cannot be an Array's element type (`validate_field_type` rejects it). On read, `validate_at`
  also returns "an Array element was a relation" as a type mismatch.

### The reverse-lookup index

Hold the **bidirectional** key families with the generic `ItemOwner`, as with image references:

**The owner and the target are written in the same shape** (`ItemOwner`:
`collection:<name>:<id>` / `page:<name>`). The target is "the referenced content" itself, so it can
be named the same way as the owner.

| Question | on-prem (rkv) | DynamoDB |
|---|---|---|
| What does this content reference | `<owner>\|rel\|<target>` | `refs#<owner>` / `rel#<target>` |
| Who references this content | `rel\|<target>\|<owner>` | `rel#<target>` / `ref#<owner>` |

- The value is the owner's storage key (the same as the image index; only the key is used when
  reading).
- The forward direction of the image index also uses `refs#<owner>` as its partition (the `image#`
  ordering), so on AWS one owner's "outgoing references" line up in the same partition.

- On every write path - save, publish, delete - the difference is calculated from **the value after
  saving** and the index is updated (the same as `set_image_references`, but in the same transaction
  as the body).
  - The difference is calculated **inside the adapter, inside the write lock / transaction**. It is
    passed which copy is being written (`Written::Published` / `Written::Draft`) and reads the other
    copy on the spot. The service is not involved: as long as the write goes through the value, the
    index follows on every path.
  - **No reindexing is needed on schema save**. `referenced_items` looks at **the stored values**,
    not the schema, so saving again with a schema that removed the field removes the index together
    with the values, and an added field comes in with that save.
- The reverse lookup counts **the union of both copies (draft / published)** (the same conservative
  rule as images).

### Deletion (implemented 2026-09)

- `DELETE …/items/{id}` / `DELETE …/single_pages/{name}` return **409 if it is referenced**
  (`still_referenced`). The message names at most 3 referrers.
- **`?detach=true`** detaches all references first, then deletes. What is detached is **both copies
  of the referencing side** (the published copy that is delivered and the working copy being
  edited), and the write goes through the normal save path, so the index moves with it. Because
  everything is detached before deleting, a failure part-way does not turn into "it disappears while
  references remain".
- **`DELETE …/collections/{name}` (a whole Collection) also looks at references** (implemented
  2026-09). It asks the index about each item of that Collection one by one, and if there is a
  reference it rejects with **409 `still_referenced`** (there is no `?detach=true`: rewriting a whole
  Collection's references in one query is too large). The cost is one index read per item, and this
  is a rare destructive operation, so it is accepted.
- The image index is not used to decide deletion (the two steps of trash → permanent deletion, plus
  showing the references), so only here is the rule different: **relations reject by default, images
  only show**.

## 4. The relationship with publishing (the heart of this design)

There is **only one** rule: **if, as a result of publishing, a `required` relation no longer holds
any "published target", that publish is rejected** (409 + which field points at which target).

- In an Array (`has_many: true`), if part is unpublished → the rest is served, so it **passes** (the
  site just has one fewer).
- In an Array, if all are unpublished → it becomes empty, so if `required` it is rejected, and if
  optional it passes.
- In a single (`has_many: false`), if the target is unpublished → it becomes empty, so if `required`
  it is rejected, and if optional it passes.
- **An optional relation is silently dropped** (the delivery API returns only "published targets").
  On the site it appears as "there is no such reference" for an Array, or as "not set" for a single.

Validation on save already says "required does not allow empty", so **publishing gets a rule with
the same meaning**. The reason not to make it "a single is always rejected": that would reject even
an optional single reference, and merely unpublishing the target would make the referencing item
unpublishable (losing the freedom to publish).

- **`unpublish` is checked by the same rule**: if, as a result of unpublishing, a `required` referrer
  becomes empty, it is rejected. Otherwise it may be Unpublished, and the reference disappears from
  the delivery API.
- The value goes into each of the two copies. **The published copy's value** is the subject of the
  rule above; the working copy is free (it may point at an unpublished target. The screen shows it
  as "unpublished").
- It is also a **question of order**: publish the target first. The migration script publishes in
  dependency order (§7).

## 5. API

### Management API

| Method | Path | Content |
|---|---|---|
| (existing) | `POST/PUT …/items` | Send `values: { "author": [{"target":"authors","item":7}] }` as the value |
| GET | `/api/models/collections/{name}/items/{id}/references` | The content referencing this item (reverse lookup) |
| GET | `/api/models/collections/{name}/items/{id}/related?field=<name>` | The candidates for the picker (search and paging) |
| DELETE | `…/items/{id}?detach=true` | Detach the references and delete (the default is 409) |

- The management API's values **return unpublished targets too**. Editing needs to see them. Whether
  they are unpublished is marked by the reverse lookup (and the candidate list).

### Public API (delivery)

```jsonc
// 1) Forward direction: by default only the target and the id (published targets only)
"category": [{ "target": "categories", "item": 3 }]

// 2) Forward expansion: ?populate=category (published targets only)
"category": [{ "target": "categories", "item": 3, "values": { "name": "Engineering" } }]

// 3) Filter: read from the reverse side ("the articles of category 3"). Paging is the existing limit/offset
//    GET /api/content/collections/articles?where=category:3&limit=25&offset=0

// 4) Expansion of the reverse lookup: ?populate=<inverse_name> ("this category and its articles")
"articles": [{ "target": "articles", "item": 12, "values": { … } }]
```

- The depth of expansion is fixed at one level (`populate` is only an enumeration of field names or
  `inverse_name`. There is no recursive specification).
- **The filter is only equality on a relation**: `?where=<field>:<id>` (several separated by
  commas). No further query language is built (it is the only form "narrow by category" actually
  needs, and the existing paging can be used as it is).
- The expansion of the reverse lookup has **an upper bound on the count** (25 by default, changeable
  with `?limit=`). A large set is read with the filter and paging.
- An unpublished target **is dropped in delivery** (the rule of §4). A publish in which a `required`
  relation thereby becomes empty is rejected in the first place.
- An image is always expanded to `{id, url}` as before (it is a kind of reference, but it carries a
  different meaning, URL resolution).

## 6. Screens

- **Schema editing**: choosing `Relation` in the type dropdown brings up three things: the target
  (Collection / Single page), `has_many`, and the name on the reverse side (`inverse_name`). No
  dedicated component is made; it is completed inside `dashboard/settings/shared/field` (because
  there are only two choices).
- **Content editing**: `shared/relation-field` has chips, a reference picker, and a JSON box. The
  order is rearranged with the chips, and the name is read from **the title field of the target's
  schema** (if it cannot be read, the reference itself is shown, as in `categories #3`). A reference
  with a mismatched target or shape is rejected on screen before saving.
- **Reverse lookup**: "Referenced by" on the item screen and the Single page screen reads
  `…/references`. The same shape as the image's "used in".
- Type names (`Relation` / `Text` …) are not translated on screen either. What is touched when
  adding one is only the types in `models/schema/fields.ts`, the two components `field` /
  `relation-field`, and their specs and catalogue.

## 7. Migration (Strapi)

- Strapi v3's `oneToOne` / `oneToMany` / `manyToMany` / `oneWay` / `manyWay` are **folded into "one
  side + `has_many`"**. When both sides have a definition, **only one side is adopted** and the
  other is put out as a warning (the CMS's reverse lookup is automatic, so no information is lost).
- References to a `single type` (Single page) can also be migrated (they were included in the
  targets by the 2026-09 design change).
- **Two passes are needed**: the first pass creates the items and builds a Strapi id → CMS id table
  (the state already holds it). The second pass sets the references. A reference whose target has
  not been created is warned about and dropped.
- **The order is not migrated** (the CMS's value is a set). If the order carries meaning on the
  Strapi side, it is warned about at migration time.
- **The order of publishing**: publish the referenced side first (dependency order). The migration
  tool can follow the dependencies from the table.
- `relation` is added to the tool's `--relations` (the default `skip` stays; as a safety measure
  before migrating, `text` stays too).
- **`created_at` / `published_at` can already be set from the API** (2026-09, `PUT …/metadata`), so
  the tool README's statement "the date and time are set by the server, so it becomes the date and
  time after migration" needs updating.

## 8. Tests

**What this design brought in**:

- unit (`core/src/models/{schema,values}.rs`): validation of the target, rejection of `has_many` on
  a Single page, rejection of an Array element, that a Composite definition can hold a relation,
  the collection of targets that walks into Composite definitions (it stops even for a definition
  that comes back to itself through an Array), the kind mismatch of `validate_relation_targets`, the
  canonicalisation of values (sorting and deduplication), a mismatched target, a missing item id,
  several for a single, the rejection of a Single page's `item`, and the emptiness of `required`
- service (`core/src/services/{collection,single_page,composite_field}_service.rs`): saving a schema
  that names a nonexistent target is 400 (a kind mismatch is 400 too). **A relation inside a
  Composite definition** is 400 both when the definition is saved and when the schema that embeds it
  is saved
- contract suite (`crates/tests/suite/relations.rs`, both adapters): **the round trip of an ordered
  list** (the written order; a duplicate takes the first position), saving a reorder, the shape of a
  Single page's value, 4 kinds of 400 on schema save (including saving a Composite definition),
  4 kinds of 400 on values, a publish that leaves a `required` relation empty is 400, **a reference
  inside a Composite** (both a Composite field and its Array) lands in the index, deletion is
  rejected with 409, and `?detach=true` detaches all the way into the Composite
- Consistency of publishing (2026-09, the unit tests of `repositories/relation_rules.rs` + the
  contract suite): publishing an item with a `required` relation that holds only unpublished targets
  is 409 `relation_unpublished`; publishing the target first passes; an `unpublish` while a
  published referrer exists is 409 `relation_required_by`; it passes if the referrer is unpublished;
  it passes if published references remain in the same field; an optional relation passes both ways;
  a `required` inside a Composite or an Array is rejected with the path; a vanished target counts as
  unpublished
- spec (`fields.spec` / `field.spec` / `value-field.spec`): the branch per type, normalisation before
  submission, lazy loading of the list, validation of the value's JSON box
- The index (`RelationIndexChanges::between`'s set difference, the walk of `referenced_items`,
  `without_reference`). In the contract suite: that the index catches up on each of save, publish
  and delete; that the union of both copies is counted; that `?detach=true` detaches the references
  from both copies; and the 409 and `still_referenced` of a referenced deletion. The four
  combinations of the referenced side (item / Single page) and the referencing side (item / Single
  page).

**What is still needed**:

- **Contract suite** (both adapters):
  - ✅ The public API: by default only the target and the id / one-level expansion with
    `?populate=` / unpublished targets do not appear / an unknown name is 400
  - ✅ The reverse-lookup filter (`?where=`): only published ones come back, paging works, and
    unpublished referrers do not appear
  - ✅ The expansion of the reverse lookup (`?populate=<inverse_name>`): one level, published only,
    an upper bound on the count
  - ✅ The uniqueness of `inverse_name`: the same name for the same target is 409, a different target
    passes, saving again passes, and a page declaring it follows the same rule
  - The reverse-lookup filter (`?where=category:3`): only published ones come back, paging works, and
    unpublished referrers do not appear
  - The expansion of the reverse lookup (`?populate=<inverse_name>`): one level, published only, an
    upper bound on the count
  - ✅ **Deletion per Collection** looks at references (409 `still_referenced`. It does not disappear
    until the item is deleted or the reference is detached)
- ✅ **E2E**: define a relation in schema editing → choose it with the picker → save → publish →
  expand in the delivery API (`check-ui.mjs`). The referenced-by panel is checked by specs (5 of
  them).
- **The migration tool**: the mapping of relations (adopting one side, two passes, the order
  warning)
- The new rejection codes (a `detach` is needed / `required` becomes empty) are added to the
  **three-point contract** (Rust ↔ `error-codes.json` ↔ the en/ja catalogue).

## 9. Confirmed by this design (2026-09)

| Point | Decision |
|---|---|
| Publishing while referencing an unpublished target | **Rejected if a `required` relation becomes empty; an optional one is dropped in delivery** (§4). The same rule as "required does not allow empty" on save |
| One-sided definition / cardinality / the shape of the value | One side, two kinds via `has_many`, a set of `{target, item}` |
| The target | Collections + Single pages (a Single page only allows `has_many: false`) |
| Reverse lookup | An index (the same transaction as the body). Management uses `references` and the screen; delivery uses filters and expansion |
| The name of the reverse side | `inverse_name` (used only for display and `?populate=`. It holds no data) |
| Expansion in delivery | Only the explicit `?populate=`, one level |
| Deletion | Rejection is the default; `?detach=true` detaches all references and deletes |
| Where a relation can go | In Fields and in Composite field definitions (it cannot be an Array's element type). The index, the rejection of deletion, and detach reach inside Composites and Arrays |
