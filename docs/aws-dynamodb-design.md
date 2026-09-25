# AWS: DynamoDB design

A design for fitting the `aws` backend's structured data into **one table**. The goal is a state
where the implementation can be written by looking at this document alone. The premise is
`docs/aws-decisions.md` - the decisions.

## 1. Principles

- **One table**, `PK`(S) + `SK`(S). On-demand. No GSI - so that every list is settled by a partition
  query.
- **Values are stored as JSON strings**. This CMS's domain types are "`serde_json::Value` where only
  the schema holds type information", so there is no need to map them onto DynamoDB types. They go
  into a single attribute called `data`.
  - Benefit: zero type-conversion code, and a 1:1 correspondence with on-premises records.
  - Cost: no server-side filtering on fields inside an item - not needed.
- **Reads use `ConsistentRead = true`**. This CMS "looks at a list right after saving", so a miss
  from eventual consistency becomes a UI bug as it is. Reads cost twice as much, but at this scale
  that is noise.
- **Item IDs are zero-padded** into `SK` (`{:020}`). DynamoDB `SK` is byte-ordered, so without zero
  padding 10 comes before 2.

## 2. Key design

| Entity | PK | SK | Notes |
|---|---|---|---|
| Collection schema | `collection#<name>` | `schema` | also serves as the existence check |
| Item - published copy | `collection#<name>` | `item#<id:020>` | the only copy the delivery API sees |
| Item - working copy | `collection#<name>` | `draft#<id:020>` | |
| Item metadata | `collection#<name>` | `meta#<id:020>` | status / published_at / last_published_at / created_at / updated_at / published_by |
| Unique value reservation | `unique#<collection>#<field>` | `<value>` | the item id holding that value (secured by a conditional put, `attribute_not_exists(pk) OR data = :id`) |
| Collection ID counter | `collection#<name>` | `counter` | numbered atomically with `UpdateItem ADD` |
| Collection name list | `collections` | `<name>` | a small item with only the name |
| Single page schema | `page#<name>` | `schema` | |
| Single page content - published | `page#<name>` | `item` | |
| Single page working copy | `page#<name>` | `draft` | |
| Single page metadata | `page#<name>` | `meta` | |
| Single page name list | `pages` | `<name>` | |
| Composite field definition | `composite#<id>` | `schema` | |
| Composite field list | `composites` | `<id>` | |
| User | `users` | `user#<uuid>` | |
| **Username reservation** | `users` | `username#<name>` | the value is the user id. **Uniqueness is guaranteed here** |
| Image record | `images` | `image#<id:020>` | also holds file_name |
| Image ID counter | `images` | `counter` | |
| Single-use upload token | `upload_keys` | `<key>` | the value is file_name, with `expires_at` (TTL) |

- Every list kind becomes "a `Query` with the partition specified":
  - Collection names / page names / composite fields / images / users
  - Items: `PK = collection#<name>` and `SK begins_with "item#"` (published), `draft#`, `meta#`
- Writing the username reservation item in the **same transaction** as the user body makes "two
  people with the same name" impossible to create. The condition is `attribute_not_exists(SK)`.

## 3. Access patterns → operations

| Trait method - representative | Operation |
|---|---|
| `get_collection_schema` / `get_single_page_schema` / `get_composite_field_schema` | `GetItem` |
| `list_collection_names` / `list_all_page_names` / `list_composite_field_schemas` / `get_all_images` / `get_all_users` | `Query` (list partition) |
| `add_*_schema` / `update_*` | `PutItem` |
| `add_collection_item` | `UpdateItem ADD counter 1` → `PutItem item#` |
| `get_collection_item` / `get_collection_item_draft` / `get_item_metadata` | `GetItem` |
| `list_collection_items` / `list_collection_item_drafts` / `list_item_metadata` | `Query` + `begins_with` |
| `delete_collection_item` | `TransactWriteItems` (delete `item#` + `draft#` + `meta#`. Deleting a nonexistent key is ignored) |
| `delete_collection` | `TransactWriteItems` (delete the schema + the list item + the items/drafts/metadata under it) |
| Publish (from `set_item_status`) | `GetItem` the draft → if present `TransactWriteItems` (Put to the published copy + Delete the draft + Put metadata), if absent Put metadata only |
| `add_user` | `TransactWriteItems` (Put user + Put username reservation, with `attribute_not_exists` on the reservation) |
| `delete_user` | `TransactWriteItems` (Delete user + Delete reservation) |
| `generate_image_upload_url` | `UpdateItem ADD` (image ID) → Put record → S3 presigned PUT URL (no token needed, since S3 carries it) |

**Publish "when there is no draft"** does not put a delete into the transaction at all (it reads
first and branches). The question `docs/aws-decisions.md` left as needing confirmation - "does
deleting a nonexistent key fail the transaction" - **is not an issue** in this design (no delete is
issued in that case).

## 4. Limits and verification items

- **400KB per item**: the item value, schema and composite field definition must fit. If they exceed
  it, return 400 (DynamoDB returns ValidationException). Add a test that inserts a fairly large value
  using many composite fields.
- **`Query` 1MB**: lists are cut off at 1MB. For an item list, **repeat pages with
  `LastEvaluatedKey` to take all** (the trait contract is "return all", so it loops internally).
- **`TransactWriteItems` 100 items / 4MB**: publish is 3 items, user creation is 2 items, collection
  deletion depends on the number under it → **collection deletion runs in batches of 25** (no
  transaction; delete drafts/metadata/items in order. If it fails partway, it can be re-run).
- **TTL**: used for the upload token's `expires_at` (deletion is asynchronous, so also check the
  expiry at consumption time).

## 5. How to write tests

- **Create the table per test**: create `test_<uuid>` and delete it in `Drop`. Put the AWS version of
  `open_test_repository` in `aws.rs`, and widen the gate in `http/tests.rs` to `any(on-premises, aws)`.
- **Endpoint substitution**: `AWS_ENDPOINT_URL` (default `http://localhost:8000`),
  `AWS_REGION=us-east-1`, `AWS_ACCESS_KEY_ID=test` / `AWS_SECRET_ACCESS_KEY=test`.
  DynamoDB Local requires credentials (same as the real service).
- **S3 is versitygw**: `S3_ENDPOINT_URL` (default `http://localhost:9000`), `force_path_style(true)`,
  `ROOT_ACCESS_KEY=test` / `ROOT_SECRET_KEY=test-secret`. Actually round-trip presign → PUT → GET
  (it verifies SigV4, so the test fails if the presigner is broken). MinIO's community edition
  stopped distribution in 2025, so we switched to a maintained gateway (`aws-decisions.md` §2).
- Startup: `docker compose up -d` (`backend/docker-compose.yml`).

## 6. Dependencies and build

Planned additions under the `aws` feature: `aws-config`, `aws-sdk-dynamodb`, `aws-sdk-s3` (the
features needed for presigned URLs will be **confirmed at implementation time**). `aws-sdk-*` has
many dependencies and is heavy to build, so the `aws` feature's build time is clearly longer than
on-premises. It does not affect the default build (wrapped in `dep:`).

## 7. Deciding first: synchronous traits and the async SDK

**The repository traits are synchronous** (`fn get_collection_schema(&self, ...) -> Result<...>`),
because on-premises' rkv is synchronous. **The AWS SDK is asynchronous**, so they cannot be
implemented as they are.

| Option | Content | Cost |
|---|---|---|
| **A. Make the traits async** | Make 41 trait methods + 47 service-layer methods async. HTTP handlers are already async, so just add `.await`. The on-prem implementation becomes an "async fn that does not await". Tests become `#[tokio::test]` + `.await` | Mechanical, but the diff is large (53 service tests). Object safety is not needed (everything is generic), so it can be written with `async fn` in trait |
| **B. Block inside the adapter** | Stand up one thread with a dedicated runtime, throw from the synchronous methods into it and wait | The diff is small, but **every DB call blocks across a thread**. It works on Lambda, but on a long-running server it limits concurrency by the number of threads. Replacing it with A later means doing it twice |

**A is recommended.** This CMS's layer structure (trait → service → HTTP) was originally built so
that "storage can be swapped", and making the traits async is the extension of that when plugging in
asynchronous storage. The 230 tests are the safety net.

### 7.1 The order actually taken: through B′ to A - complete

Applying A mechanically all at once was tried 3 times and failed 3 times (same-named sync/async
methods exist on several types, and calls to `to_response` and `notify` turn into methods of a
different type. Detecting the "enclosing function" is also unstable because of multi-line chains and
the `{}` inside `format!`). So the order was changed:

1. **Write the adapter as async** (the shape the AWS SDK wants. This is the implementation that
   ultimately remains).
2. The trait's synchronous methods delegate, one round trip per method, to a `BlockingRuntime` that
   holds the runtime on a dedicated thread (the diff is small).
3. Run the contract suite against DynamoDB Local and harden the adapter's correctness first.
4. Then do **A** (make the traits async), leave the implementation from 1 in place and delete 2.

"Calling a synchronous method from inside the runtime" is the most fragile part, so
`aws::repository::tests::a_collection_round_trips_against_dynamodb_local` pins it down by calling
the synchronous trait from inside `#[tokio::test]`.

**→ A is complete (2026-09).** The traits became
`fn f(...) -> impl Future<Output = ...> + Send`, and the `BlockingRuntime` bridge, `bridge.rs`,
`s3_blocking` / `delete_table_blocking` are gone. What we learned:

- **Write `impl Future + Send`, not `async fn`.** `async fn` in trait does not promise Send, and an
  axum handler generic over `R: Storage` fails with "future cannot be sent between threads". The
  implementation side can stay `async fn`.
- **Let the compiler drive the conversion.** The moment the traits become async, every call site
  becomes a type error. rustc emits "put `await` here" as a span and a replacement string, which is
  more reliable than replacing by name (that is how we previously broke methods of a different type
  for `to_response` / `notify`). The same suggestion appears twice, for lib and test, so
  **deduplication is mandatory**.
- **A half-converted adapter deadlocks.** When a trait through the bridge and a trait with direct
  async use the same SDK client, the connection pool is per-runtime, so it hangs. Convert the
  adapter as one unit.
- The remaining mountain was **borrowing**: tests like `let result = service.call(&"x".into()); ...
  result.await` are both fixed by moving the await to the definition side
  (`let result = service.call(&"x".into()).await;`). The formatting pass (`models/field.rs`) was
  pulling images inside an iterator closure, so it was made a pure function taking a **resolved image
  map** (one query per request, faster than the previous "once per value").

The throwaway tools used for the conversion (a rewriter that handles brace matching correctly, and a
script that inserts `.await` from rustc's diagnostic spans) have been **deleted**. The migration is
complete and the `BlockingRuntime` bridge they targeted is gone, so keeping them would leave
something that cannot run. The code is in the git history. The lessons that remain are the bullets
above (**do not rewrite Rust with regular expressions. Feed the compiler's diagnostics in**), and
that is also this section's value.

**Confirmed**: table creation, the schema and list indexes, atomic numbering (1 → 2), the 3 records
for publish/draft/metadata, 3 records on item deletion, and that collection deletion removes
everything under it including the indexes.

**The bridge's rule - traps hit during implementation**: the SDK's HTTP client pools connections,
and those connections **belong to the runtime that opened them**. Pushing them from another runtime
waits forever, because the owning side is blocked in `block_on`. In other words, "a client's first
request must go through the bridge" is mandatory, and in tests too, hitting S3 bucket creation
directly with the SDK freezes the later `delete_image` (hit for real). The test-only
`AwsRepository::s3_blocking` exists for this.

## 8. What implementation settled, and remaining issues

The 3 points marked "confirm at implementation time" have been settled by the shape of the
implementation.

- **Presigned URL**: issue the PUT with `aws-sdk-s3` presigning (15-minute expiry). The single-use
  token became a mechanism for the on-prem local PUT path only.
- **Numbering and transactions**: numbering is an independent `UpdateItem` using `ADD` (`next_id` in
  `repository.rs`). It is a separate call from the body write, and there was no need to mix `ADD`
  into `TransactWriteItems` - on failure only one id is skipped, and there is no collision.
- **`ConsistentRead`**: used for "read what was just saved" reads (single fetch, in-partition lists,
  published-list status records). The list order breaks ties by id, so the order never wobbles.

Remaining design issues (these work in the current implementation, listed in the order they will
matter when scale arrives):

- **Pushing down the admin screen's list**. The delivery API now scans status records in id order and
  reads only the bodies that fall in the window, but the admin screen's list is the union of
  published copies and working copies, so the two key spaces must be merged in id order.
- **The delivery API's cursor scheme**. Changing offset/limit to `LastEvaluatedKey` removes the scan,
  but it means giving up `total` and changes the API's shape (`content-api.md` §3.1).
