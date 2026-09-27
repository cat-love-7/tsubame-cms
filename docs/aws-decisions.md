# AWS decision record

The decisions made when we chose to run this CMS on AWS (Lambda + DynamoDB + S3), and what we learned
by implementing it. The stack itself is in [`infra/README.md`](../infra/README.md), the table design is in
[`aws-dynamodb-design.md`](aws-dynamodb-design.md), and how to run it is in the [README](../README.md).

**The implementation is complete and running in staging** (2026-09). The `[x]` in §2 is kept not as
"we decided that" but as a record of "we built it this way and verified it like this" - for someone
redoing the same decision, having the verification method written down too is more useful. Only the
things not yet finished are in "Remaining work" at the end of §2.

## 0. The current shape

- The backend is **split into crates** (not compile-time feature selection):
  `crates/core` (models, services, HTTP layer) + `crates/on-premises` (rkv and local images) +
  `crates/aws` (DynamoDB, S3, Lambda). The seams are `AppModule<R: Storage>` and
  `http::router`, and `crates/tests` runs **the same contract suite against both**.
- `crates/aws` contains 5 repository implementations, DynamoDB helpers, the S3 upload target, the
  **Lambda entry point** (`lambda.rs`), and Cognito account management (`provisioner.rs`).
- The repository traits are async. The old bridge that "wrapped synchronous traits in a runtime thread"
  is gone (`aws-dynamodb-design.md` §7.1).
- The emulators (DynamoDB Local + versitygw) are in `docker-compose.yml`, deployment is in `infra/`
  (Terraform), and `scripts/test-rust.sh` runs the tests in one command.
- The three things that depended on in-process memory (the login attempt counter, the single-use image
  upload token, and background Webhook delivery) were replaced on the AWS side by a DynamoDB record, an
  S3 presigned PUT, and budgeted synchronous delivery respectively. The decision is in §1, and the
  remaining holes are in "Remaining work" in §2.

## 1. What we decided

| Point | Decision |
|---|---|
| Image delivery | **The S3 URL**. What is stored is a **stable URL** (CloudFront + OAC, or a public bucket), and **presigned URLs are not stored** (once they are in content they rot within minutes). presigned is used only for the upload PUT |
| Authentication | **Local stays as it is, AWS uses Cognito**. Token verification is abstracted (HS256 / RS256 + JWKS), and `sub` is mapped to a local permission record |
| Account management | Creation, deletion, reset, and enable/disable are **the administrator's job in either deployment**, so the routes are shared (`http::auth`). What differs is **what is handed over**: a deployment where the CMS holds the password uses a link, and a deployment where Cognito holds it uses the `AdminSetUserPassword` **temporary password**. The `password_reset` of `GET /api/auth/capabilities` says so in advance, and the `kind` of the response says so at the moment. Only the CMS's own password routes (`/auth/login`, `/auth/password-reset`, `/auth/me/password`) are **501** in the AWS build |
| Webhook | **Budgeted synchronous call** (for example, 5 seconds total). `Notifier` is made async; on-prem keeps spawning, AWS awaits. Overruns and failures are logged and given up on |
| IaC | **Terraform** (with other clouds in view) |
| Publish atomicity | **Use `TransactWriteItems`** (copy to the published copy + delete the draft + update metadata) |

### First startup (how to create the first administrator on AWS)

Even after moving authentication to Cognito, **the authorization records (role and per-resource
permissions) are needed locally**, so a means of creating the first person locally is required.

- Pass `BOOTSTRAP_ADMIN_USERNAMES` (comma-separated) as an environment variable.
- When an unprovisioned Cognito user signs in and their `username` (or email) is in this list,
  **create a local administrator record on the spot** and let them through. If they are not in the list,
  **403 "not provisioned"** (no implicit granting of permissions).
- Once created, they are tied together by `external_id = sub`, so **the list only has effect the first
  time**. Later additions are the administrator's job from the account screen: creating a record there
  asks Cognito first (`AdminCreateUser`, and the person's `sub` is stored on the spot), so a token from
  the pool resolves to the record without a second binding step.
- **An account the pool already had is adopted, not reset.** An operator who made the person in the
  Cognito console first (or the same person coming back after the CMS's own records were replaced) keeps
  the credential they have: the create answer says `needs_credential: false` and the screen adds the
  record without handing over a temporary password, which a reset would have replaced. Only an account
  this CMS had to create answers `true` and gets the temporary password described above.
- If the user table is empty and the list is also unset, **warn at startup** (the same experience as
  on-prem's "set `ADMIN_USERNAME`").
- Assumption: the Cognito pool uses **username as the identifier**. The character set of `username` has
  already been aligned with Cognito.
- Alternatives and their drawbacks: writing the record directly into DynamoDB with Terraform (couples to
  the internal JSON shape and breaks easily), passing `sub` in advance via an environment variable (the
  bother of looking it up in the console), automatic viewer creation on first sign-in (implicit granting
  of permissions).

### Expressing per-deployment differences through the API

Introducing Cognito creates a state where "what can be done differs by deployment". So that UI
branching, E2E branching, and documentation statements do not scatter, `GET /api/auth/capabilities`
(or a field of `/api/auth/me`) returns **what this deployment can do**. Example:
`{"password_login": true/false, "password_reset": "link"|"temporary", "image_upload":
"proxied"|"presigned", "login_url": …}`.

## 2. Implementation record

Each item has a "completion condition (= what it is verified by)". `[x]` is implemented and verified,
`[ ]` is remaining.

### Foundation (can proceed without an AWS adapter)

- [x] Took image byte reads and writes out of the shared trait (`LocalImageBytes`). The shared layer now
      speaks only of "what every backend is obliged to implement", and the routes `GET/PUT
      /api/images/{file_name}` are registered only in the local feature
      → Verification: 229 tests + browser E2E 59/59 (image upload/delivery is exercised by E2E over the real path).
- [x] Consolidated test-repository construction into a per-backend helper
      (`on_premises::open_test_repository`), and gated the integration tests on the backend feature
      → Remaining: once the AWS-side helper exists, widen the gate to `any(on-premises, aws)`.
- [x] `docker-compose.yml` (DynamoDB Local + S3 emulator)
      → **Verified**. On DynamoDB Local, CreateTable → `UpdateItem ADD` (atomic counter. Confirmed 1 → 2)
      → ListTables → DeleteTable. For S3, actually round-tripped a presigned PUT / GET / DELETE against the
      emulator (a tampered signature is refused = the emulator really verifies SigV4).
      **LocalStack was not adopted**: the current image does not start without a license token
      (`License activation failed!`, exit 55). That would assume developers and CI have an account.
      DynamoDB Local requires some kind of credentials just like the real service, so the SDK is passed
      `AWS_ACCESS_KEY_ID=test` / `AWS_SECRET_ACCESS_KEY=test`.

      **2025-10 addendum: from MinIO to versitygw.** MinIO archived the open-source server, client, and
      images and ended distribution (Docker Hub is 404, quay.io is 401, and `dl.min.io` is 410 down to the
      archive listing. As the official announcement said, "we receive neither security updates nor
      vulnerability reports"). Pinning a community mirror in CI would mean running software with no updates
      and no way to verify it. Instead we switched to **versitygw** (Apache-2.0, an S3 gateway that is still
      released): it verifies SigV4, and with `--iam-dir` users + a **bucket policy** it can create "a user
      with the same permissions as the deployment", so tests that depend on permissions keep their meaning
      as they are (`deployment_bucket_policy`).
- [x] Made the tests into one command (`scripts/test-rust.sh` / `test-frontend.sh` / `test-e2e.sh`) and
      CI (`.github/workflows/ci.yml`: the 3 jobs rust / frontend / e2e)
      → Verification: the scripts have been run and confirmed locally. **The workflow itself is unverified
      because there is no runner in this environment**.
- [x] Created a place for the AWS adapter. **The shape later changed with the workspace split** (P7 below):
      the backend became a **package** (`crates/aws`) rather than a feature, and the "choose exactly one"
      guard and the `compile_error!` disappeared. What chooses is the **selection of the build target**
      itself: `-p tsubame-aws` / `-p tsubame-on-premises`.
- [x] Organized the AWS configuration. `Config` reads `AWS_REGION` / `DYNAMODB_TABLE` / `S3_BUCKET` /
      `COGNITO_USER_POOL_ID` / `BOOTSTRAP_ADMIN_USERNAMES`, and `aws_settings()` verifies "what is needed
      for startup" and names the first missing one. The JWKS URL is **derived** from the region and the pool
      ID (so they cannot disagree). Comments make explicit that `DATA_ROOT` and `ADMIN_*` are on-premises
      only (if `ADMIN_*` is set on AWS, warn at startup)
      → Verification: tests for `parse_usernames` + tests for `aws_settings()` (the error for missing items and the JWKS URL).
- [x] Made `Notifier::notify` **awaitable** (a boxed future, to preserve object safety). The on-prem
      implementation still spawns in the background and completes immediately, and the AWS side can await
      → Verification: all the existing Webhook tests (delivery, retries, behavior on disconnect) pass.
- [x] Wrote the entry-point shape (`aws::run`) in `aws.rs`, and made the AWS arm of `main.rs` call it.
      Up to configuration verification and the warnings "`ADMIN_*` is ignored" and
      "`BOOTSTRAP_ADMIN_USERNAMES` is empty"
      → **Done** (the configuration moved to `crates/aws/src/settings.rs`, and `AwsSettings::from_env()`
      performs the same verification. 4 tests including
      `the_jwks_url_is_derived_so_pool_and_region_cannot_disagree`).

### DynamoDB adapter

- [x] Decided the **single-table key design** and wrote it in
      [`docs/aws-dynamodb-design.md`](aws-dynamodb-design.md). 1 table + `PK`/`SK`, **no GSI** (all
      listings are partition queries), values are JSON strings, reads use `ConsistentRead`, item IDs are
      zero-padded, username uniqueness uses a reserved item + conditional write, and publishing uses
      `TransactWriteItems`
      → Verification: the access patterns the implementation needs are covered exhaustively in a table (for 44 methods).
- [x] **Implemented the 44 methods** (collections 16 / single pages 11 / users 6 / composite fields 4 /
      images 4 + S3). The implementation is written async and delegates to the synchronous trait via
      `BlockingRuntime` (the history and order are in
      [`docs/aws-dynamodb-design.md`](aws-dynamodb-design.md) §7.1).
      **The HTTP contract suite passes entirely against DynamoDB Local + MinIO**
      (`cargo test --workspace`, 284 tests including the HTTP contract suite's 43 × 2 backends).
      Run: `docker compose -f backend/docker-compose.yml up -d` and then the command above.
      When there is no emulator, only the tests that need an emulator skip themselves.
      Points to hold down:
      - Item IDs are **allocated atomically with `UpdateItem`'s `ADD`** (the on-prem `id_counter`
        equivalent. Must not collide under concurrent creation).
      - Draft listings and metadata listings are **`begins_with` queries on SK** (DynamoDB has no prefix scan).
      - Publishing groups 3 records together with **`TransactWriteItems`**.
      - Listing paging is pushed down to `Limit` / `ExclusiveStartKey`
        → achieved for the delivery API ("Pushing down paging for published listings" below). The admin
        screen listing is still remaining because it needs a merge of two key spaces.
      - Conditional writes make things like "delete a nonexistent item" idempotent (produce the same
        result as the behavior the rkv implementation holds).
      → **Completion condition**: **the shared contract suite passes entirely against DynamoDB Local**
      → **Achieved** (the 231 tests above. Run against DynamoDB Local + MinIO).
- [x] Size verification: a 300KB value round-trips, and a value over 400KB is **refused**
      (`a_large_value_round_trips_and_an_oversized_one_is_refused`). Moving image bytes out to S3 is the
      basis for fitting within 400KB per record.
- [x] Concurrent creation verification: 8 threads × 5 items gives **IDs 1..=40 with no duplicates**
      (`concurrent_creates_do_not_share_an_id`). The atomicity of `UpdateItem ADD` itself.
- [x] The 1MB query page boundary: listing 10KB × 120 items (over 1MB) returns **every item exactly once**.
      The test itself also counts the number of round trips and confirms that the boundary was actually
      crossed (`a_list_longer_than_one_query_page_is_read_to_the_end`).
- [x] Publish atomicity: added `apply_item_status` / `apply_page_status`; AWS uses a single
      `TransactWriteItems`, and on-prem uses a single rkv write transaction to write "published copy,
      working copy, status" together. A working copy over 400KB was used to make the transaction fail, and
      it was tested that **all 3 records remain unchanged**.
- [x] **Pushing down paging for published listings** (`list_published_items_page`): the delivery API's
      listing **scans the status records (`meta#`) in ID order** and reads only the bodies (`item#`) that
      fall in the window. Since it no longer reads every body before narrowing down, the first page of a
      10,000-item collection costs "10,000 small status records + one page of bodies". `total` is counted
      by scanning the statuses, so `X-Total-Count` and `next_offset` are as before.
- [ ] Pushing down the admin screen listing (`get_collection_items_page`): this one is **the union of
      published copies and working copies** (the edit screen looks at the working copy), so the two key
      spaces must be merged in ID order. Unlike the delivery API, it is an authenticated admin screen and
      the counts fit within the scale of a collection, so it was deferred (the design is the same "hand the
      window to storage").
- [ ] Whether to make the delivery API **cursor-based** (use `LastEvaluatedKey` as-is as the key for the
      next page). Right now it remains offset/limit, so it scans the status records to the end in order to
      produce `total`. With a cursor, giving up `total` also removes the scan, but **the API shape changes**
      (the room left in `docs/content-api.md` §3.1).

### S3 (images)

- [x] Made `generate_image_upload_url` a **presigned PUT** (expires in 15 minutes). The single-use token
      was a mechanism for on-prem's local PUT path and became unnecessary on S3: the signature carries
      "only once, only for a specific key" (a PUT after it ends is 403). `take_upload_key` /
      `read_image_bytes` / `write_image_bytes` (`LocalImageBytes`) are on-prem-only contracts, so the AWS
      adapter does not need them (the `Storage` cfg branch stays as is).
      → **Completion condition**: a round-trip test of presign → PUT → GET via the public URL → delete
      → **Achieved** (the round-trip test in `aws::repository::images`. Run against MinIO).
- [x] The stored URL is a **stable URL** (`AWS_IMAGE_BASE_URL` > the S3 endpoint's path-style >
      `https://<bucket>.s3.<region>.amazonaws.com/<key>`). **Presigned URLs are not put into content**
      (they expire the moment they are placed on a page). Tests also confirm the signature does not leak.
- [x] **Made signed delivery a deployment option** (2026-09, `AWS_IMAGE_DELIVERY`).
      `public` (default) = hand out the object's address + the bucket is public-read.
      `presigned` = **the bucket is private**, and the URL the API returns is a **signed GET** every time
      (`AWS_IMAGE_URL_TTL_SECONDS`, default 1 hour, 60 seconds to 7 days).
      **What it suits is "a site that fetches at build time and serves it itself"** (where the SSG
      transforms images and hands out its own copies): the signature only needs to live during the build,
      and nobody can read the bucket. Conversely, for a site that puts the CMS's URL on the page as-is,
      **the cached URL dies when it expires**, so it is not the default. The forwarding of
      `/api/images/by-id/{id}` also goes through the same path, so it works regardless of the mode
      (hand-written Markdown links can be used as-is).
      → **Completion condition**: in signed mode, (1) the API's URL is signed, (2) the entity can be
      fetched via that URL, (3) **it cannot be fetched without the signature** → achieved with
      `a_presigned_deployment_serves_a_url_that_expires`.
- [x] **Put a stable URL in the upload information** (`NewImageInfo.url`). An S3 presigned URL is a
      "signature" and not the object's public address, so deriving it from `upload_url` on the client was
      impossible on AWS (on-prem only had to drop `?key=`). Since the API now returns it, the frontend goes
      through the same path in both deployments. The interceptor also **does not attach the bearer to
      external hosts** (adding `Authorization` to a PUT to S3 can cause a signature mismatch, and signing
      out on an S3 401 would be wrong).
- [x] Whether delivery should be CloudFront or the bucket should be public-read (decided on the Terraform
      side) → **Decision: public-read** (the bucket policy in `storage.tf` allows `s3:GetObject` to `*`).
      The reason is the same as "the stored URL is a stable URL" below: content **stores the address**, and
      the site's build fetches it **later**. A signed URL expires in at most 7 days, so it becomes a dead
      link the moment it is placed in a static site's HTML. Even when putting CloudFront in front, the
      premise is to read it as an origin, **not via signatures** (just point `AWS_IMAGE_BASE_URL` at the
      CDN; neither the CMS nor the content needs changing). **Remaining**: visual confirmation in staging
      (E2E passes over the on-prem delivery path).

### Workspace split (2026-09)

Switching with features means "1 build = 1 feature set", so `#[cfg]` leaks into the shared layer (the
double definition of `Storage`, branching of image routes, and type aliases and `#![cfg]` on the test
side), and on top of that **you cannot test both backends with one command**. We moved the boundary to
packages:

| Package | Contents | Binary |
|---|---|---|
| `crates/core` | models / repositories / services / http / auth / config / webhook | — |
| `crates/on-premises` | rkv + local images | `tsubame` |
| `crates/aws` | DynamoDB + S3 (configuration is here too) | `tsubame-aws` |
| `crates/tests` | contract suite (depends on both adapters) | — |

- **core has not a single `cfg(feature = ...)`**. Capabilities are expressed not with features but with
  **traits** and **composition**: `LocalImageBytes` is an ordinary trait, the routes that handle bytes are
  in `http::local_images`, and each backend's `build_router` decides whether to mount them.
- Building only one side is `cargo build -p tsubame-aws` (it does not compile rkv. Dependency crates go
  from 519 ↔ 946). With `default-members`, a plain `cargo build` / `cargo test` stays on on-premises.
- The contract suite has `suite/` (a file per topic + the harness in `mod.rs`) pulled in as modules by two
  runners via `#[path]`, and runs against **both with one command** (71 × 2). Tests that handle bytes skip
  themselves with `Backend::SERVES_IMAGE_BYTES`.
- The AWS contract suite **fails if there is no emulator** (it does not silently pass).
  `scripts/test-rust.sh` looks at the ports and skips whole files, and the CI `rust-aws` job starts the
  emulators from `docker-compose.yml` and runs it for real.

### Making the storage layer uniformly async (2026-09)

**A** of `docs/aws-dynamodb-design.md` §7. Made the 6 traits and 48 methods `impl Future + Send`, made the
services, handlers, and tests async, and **deleted the AWS bridge**.

- The core services are `async fn`, and the on-prem implementations are "async fns that do not await".
- `crates/aws/src/bridge.rs` is gone, and the AWS implementations are plain `async fn`.
- The conversion was applied mechanically from rustc's suggested spans (the throwaway tooling was deleted
  after the migration. The lessons are in `docs/aws-dynamodb-design.md` §7).
- **The contract suite's 43 × 2 backends is green as before** (281 tests with `cargo test --workspace`).
  The AWS-side runtime went from 15 seconds → 3.5 seconds (because the per-call thread round trip disappeared).

### Lambda entry point

- [x] Mounted the existing router with `lambda_http` (`crates/aws/src/lambda.rs`). The form passes the
      router to `lambda_http::run(service_fn(...))` as a `tower` service. The event shape is
      **deserialized through the same path as the real thing** into
      `lambda_http::request::LambdaRequest` (public but doc-hidden) and then converted to `http::Request`,
      so both API Gateway **REST (1.0) / HTTP API (2.0)** are tested with synthetic events.
      - **base64**: tested that a body with `isBase64Encoded: true` is decoded and reaches the router
        (`a_base64_body_arrives_as_the_bytes_it_stands_for`). The response is returned as `Body::Binary`,
        so API Gateway treats it as base64 and it comes back as-is along with the Content-Type.
      - **6MB/base64**: Lambda's invocation limit is 6MB, and when API Gateway turns a binary body into
        base64 it grows by 1/3. The CMS's requests are JSON (images go directly to presigned S3), so
        **4MB is the limit**, and `MAX_BODY_BYTES` returns 413 first (with the reason in the body).
        Test: `a_body_over_the_lambda_limit_is_refused_with_a_reason`.
      - **The response side uses the same arithmetic**: the buffered response limit is 6MB (1/3 larger via
        base64 if treated as binary), so to keep responses from exceeding `MAX_RESPONSE_BYTES` (4MB), the
        delivery API **also cuts pages by byte count** (`http::content`). For other paths (such as the
        fetch-all admin listing), the guard in `lambda.rs` answers with the CMS's 413. If it cut only by
        item count, 50 items × 400KB would fail the whole function, and the client would only receive a 502.
      - **Response streaming is not adopted for now (2026-09 decision)**: with
        `InvokeMode: RESPONSE_STREAM`, the response limit goes from 6MB → **200MB** (the first 6MB is
        unlimited, then 2MB/s, execution time is billed to the end even if the client disconnects, and the
        console always shows it buffered). Rust can handle it too with
        `lambda_http::run_with_streaming_response`, but **our responses are assembled to the end with
        axum's `Json` and then returned**, so TTFB does not improve and only the ceiling goes up. What
        needs large responses is "the fetch-all admin listing" and exports, and we will switch
        `invoke_mode` along with it **when we build an API that streams while producing** (such as NDJSON).
        The request side of `MAX_BODY_BYTES` does not change with streaming either (it stays 6MB).
      - Local startup (when there is no `AWS_LAMBDA_RUNTIME_API`) is served by the same router, and
        `run_local` creates the table if it does not exist. Verified on the real thing: against the
        emulator, `cargo run -p tsubame-aws` → `/` 200, unauthenticated 401, and a log of automatic table
        creation.
      → **Completion condition**: tests that pass synthetic events + a staging smoke test
      → Tests achieved (4). **The staging smoke test waits on P5 (Terraform)**.
- [ ] Countermeasure for post-response freezing: put the Webhook on SQS (as decided in 1-3). `Notifier`
      already returns a boxed future, so add an `SqsNotifier` for AWS and have `build_notifier` choose per
      backend. Testing requires an SQS emulator (a decision is needed on whether to add ElasticMQ to
      `docker-compose.yml` or make do with just unit tests of `Notifier`). The Terraform side needs a
      queue + Lambda publish permission + a delivery Lambda (P5).
      → **Completion condition**: confirm in staging that publish responds without waiting for delivery to
      complete and that delivery is not lost.
- [~] Inventory of in-process state (as of 2026-09):

      | State | Where it lives now | With 2 Lambda instances | Destination |
      |---|---|---|---|
      | Login attempt counter (`auth/throttle.rs`) | The process's `HashMap` | **Not held on AWS** (sign-in is Cognito. The CMS does not see attempts) | **Decision: on-prem only** (P4 "Handling of login attempt counts" below) |
      | Image single-use token | **Does not exist** on AWS (presigned S3) | No problem | — |
      | Webhook | Delivered during request processing | **Lost** (the execution environment can be frozen after the response) | SQS in the next item |
      | Issuer (token / preview / reset) | Derived from a secret key, stateless | No problem | — |
      | rkv's `Manager` singleton | on-prem only | Out of scope | — |

      → **Completion condition**: zero items where "running Lambda with 2 instances breaks it".
      → What remains is the two above (Webhook and login attempts).

### Cognito (as decided in §1)

#### Where password authentication lives (completed 2026-09)

We drew the line as "credentials belong to the deployment that holds the credentials". `User` holds only
identity and permissions, and the on-prem adapter stores the password as a separate record. AWS does not
hold passwords, so that capability disappears from both the crate graph and the route table (only the 501
stub remains).

Remaining: **the UI reading capabilities and branching on them** (the Angular side), and P4's "call
Cognito user creation, deletion, and password reset from the CMS's account management".

#### Handling of login attempt counts (2026-09 decision: **adopt A**)

**AWS holds no attempt limit.** Sign-in is completed between the browser and Cognito, and the CMS only
verifies the JWT, so the CMS does not see the attempts themselves. Cognito also **has no API that returns
the failure count** (an `InitiateAuth` failure is `NotAuthorizedException`, and there is no means to query
the count). threat protection is Plus-plan risk scoring, and the documentation states explicitly that "it
does not watch traffic; attach AWS WAF for DDoS". There is also no trigger that fires on failure
(`PreAuthentication` is called at attempt time but is not given success or failure, and by default is not
called for a nonexistent user).

**However, we will not attach WAF (2026-09 decision).** Following the quote above, the countermeasure
against large-volume access is WAF's rate-based rule, but what it can protect is **only sign-in attempts**,
and it cannot protect CloudFront, `/api`, or image delivery. Meanwhile, the pool's per-category quotas
(for example, `UserAuthentication` is 120 RPS for the account and the whole region) already set an upper
bound on the damage. A web ACL adds resources and a monthly cost (about $5 + $1 per rule + $0.60 per
million requests), and we judged that at the current scale there is nothing to gain from it. Account-level
protection (risk scoring and lockout) is Plus-plan threat protection, and WAF cannot replace it. If it
becomes necessary, adding it on the CloudFront side (CLOUDFRONT scope, us-east-1) - which can protect both
the API and the app - becomes a separate decision.

| | on-premises | AWS |
|---|---|---|
| Failure count counter | **`auth/throttle.rs`** (5 attempts/15 minutes, 429 + `Retry-After`, hides the existence of the identifier) | **Not held** (Cognito does not return the count. Only Plus threat protection) |
| Hiding nonexistent identifiers | The same message + count all identifiers | `PreventUserExistenceErrors: ENABLED` (app client setting) |
| Large-volume access | In-process counter | **Not held** (left to Cognito's per-category quotas) |
| Password-related APIs | Usable | `/api/auth/login` etc. are **501** + expressed via capabilities |

Therefore we will build **neither a DynamoDB counter nor retrieval of the count from Cognito.** Holding it
in two places would make it impossible to explain "which one rejected it". We judged that the option of
doing `AdminInitiateAuth` on the server and counting ourselves (B) is not worth the cost of implementing
the MFA and `NEW_PASSWORD_REQUIRED` challenge flows.

- [x] Abstracted the token verifier (2026-09). `TokenVerifier` in `auth/identity.rs` returns an `Identity`
      (Local = our own HS256 / External = the provider's `sub`), and `auth/cognito.rs` verifies RS256 +
      JWKS. Key retrieval is behind `JwksSource` (HTTPS in production, a fixed key set in tests), and the
      cache (10 minutes + re-fetch only on an unknown `kid`) is on the verifier's side.
      → **Completion condition**: valid, expired, iss mismatch, aud mismatch, a different key, `alg`
      confusion (signing the public key as a secret key with HS256), unknown `kid`, `token_use` being
      access, no `exp`, key set unreachable → **achieved with 11 tests**. The RSA keys for tests are
      `auth/test_rsa_key*.pem` / `test_jwks.json` (test-only. Production only verifies, so it holds no
      secret key). The expiry leeway (`leeway`) was fixed at **10 seconds** (the default is 60 seconds, and
      a token that should have expired survives for 1 minute).
      **Not wired up yet**: `AuthService::user_from_token` still looks only at our own tokens (it is
      connected in the next `external_id` item).
- [x] Added `external_id` (Cognito's `sub`) to `User` (`#[serde(default)]`). Added
      `UserRepository::get_user_from_external_id`; on-prem looks it up in a dedicated `identity` DB (the
      same shape as `credential`), and AWS looks it up with the same reserved record as `username#`
      (`external_id#<sub>`). Addition, change, and deletion on both sides maintain **the index in the same
      transaction as the record**, and release identifiers that have come loose (an old `sub` must not
      resolve to someone forever). `AuthService::user_from_token` now branches on the `Identity` returned
      by `TokenVerifier` (our own token = generation check, provider = resolve via `external_id`).
      → **Completion condition**: a test that resolves a permission record from `sub`
      → **Achieved** (`a_provider_identity_is_resolved_through_external_id`).
- [x] Implemented handling of unprovisioned users and first-time bootstrap. When someone in
      `BOOTSTRAP_ADMIN_USERNAMES` signs in for the first time, create an administrator record tied to their
      `sub`. Anyone outside the list gets **403** ("we do not make it so that anyone in the organization can
      sign in to the pool = anyone can edit the site"). From the second time on, resolve via `external_id`.
      A disabled account is 403 even if it resolves. On the AWS side, `COGNITO_CLIENT_ID` was added to the
      configuration, and the **Lambda startup path** (`build_deployed_module`) installs `CognitoVerifier`
      (+ `HttpJwks`) and passes `BOOTSTRAP_ADMIN_USERNAMES` to the auth service. **Local startup
      deliberately goes through a different composition** (`build_app_module` = our own HS256): tests sign
      in with a token signed by `JWT_SECRET`, and **in a deployment `JWT_SECRET` is also used to sign
      reset/preview links**, so if the deployment accepted our own tokens, anyone who knew that secret
      could forge an administrator token.
      → **Completion condition**: the deployment's composition rejects our own tokens (401), and the
      verifier points at the pool correctly → achieved with `deployed_verifier_tests`.
      → Cognito user creation, deletion, and reset were wired up later (P4 items below, 2026-09).
- [x] **Made Hosted UI sign-in work end to end** (2026-09). Until then there was only a link to
      `login_url`, there were **no `callback_urls` and no OAuth permission on the Cognito side**, and there
      was no path to exchange the `code` that came back.
      → Set `callback_urls` (`<app_url>/auth/callback`), `allowed_oauth_flows = ["code"]`, and
      `allowed_oauth_scopes` on the Terraform client; the screen adds a PKCE challenge + `state` +
      `redirect_uri` and sends it; and `/auth/callback` (a screen route) checks the state and asks
      `POST /api/auth/cognito/exchange` to do the exchange (the exchange is on the server: the token
      endpoint does not return CORS). Verification is done by `CognitoVerifier`.
      → **Completion condition**: the challenge matches the RFC 7636 example, and the 3 paths of state
      mismatch, cancellation, and exchange refusal are shown on the screen → achieved with 8 frontend tests.
      → **Completion condition**: one person in the list becomes an administrator on first sign-in, someone
      outside the list is 403, and the second time resolves via `external_id`
      → **Achieved** (the same test confirms the 4 points: creation, re-resolution, disabling, and an outsider).
- [x] Made the local password-related capabilities **per-deployment** (2026-09).
      - Credentials come out of the `User` record and are **held by the on-prem adapter**
        (`crates/on-premises` implements `repositories::local_credentials::LocalCredentials`. Argon2id and
        rkv's `credential` DB). **core does not depend on argon2** - since `User` has no password field, it
        is structurally impossible for it to leak via debug output or logs.
      - The password-related routes are in `http::password_auth` (the same shape as `local_images`), and
        **only on-prem's `build_router` composes them**. Account creation and reset are in `http::auth`'s
        **shared routes** (because the administrator's job exists in either deployment).
      - AWS registers the remaining password paths (`POST /auth/login`, `POST /auth/password-reset`,
        `POST /auth/me/password`) as **501** (because a 404 would be read as "the URL is wrong"). The
        messages name Cognito.
      - Both expose `GET /api/auth/capabilities`: `password_login` / `password_reset`
        (`link` | `temporary` | null) / `image_upload` (`proxied` | `presigned`) / `login_url` /
        `site_name` (what the deployment is the admin screen for; unset unless an operator named it).
      - The contract suite skips the 6 password-related tests with `Backend::PASSWORD_LOGIN`, and asserts
        the shape of "what is handed over" with `Backend::PASSWORD_RESET`. AWS account creation goes
        through the **real routes** (only the provider is fake, because there is no Cognito emulator).
      → **Completion condition**: the UI branches on capabilities. On AWS too, **the account screen
      appears** (creation, deletion, and reset are the administrator's job), but it hands over a temporary
      password instead of a link, and the change-your-own-password field does not appear (that stays 501
      and is replaced with explanatory text)
      → Tests achieved (contract suite 46 × 2, including the capabilities and 501 tests).
      - [x] **The UI side was handled too** (`CapabilitiesService`). The first screen reads
        `GET /api/auth/capabilities` exactly once, and **until the answer comes it assumes "password
        method"** (so the UI does not break on an old server that does not know capabilities, or if the
        response is lost). The branching is:
        * Login screen: if `password_login` is false, instead of the form it says "this deployment signs in
          via an identity provider" (it does not show a password field that returns 501).
        * User list: it likewise hides the password field and "Reset password" / "Issue reset link".
        Tests: 4 for `CapabilitiesService` (default, applied, only once, on failure) and 1 for the login
        screen branching (121 frontend tests green).
        **Remaining**: if a deployment returns `login_url` (such as Cognito's hosted UI) in capabilities,
        we can direct users there instead of showing explanatory text. Right now we do not know the
        destination, so it is text only.
- [x] Permissions (role and per-resource) are **the CMS's**, and are not placed in Cognito. The boundary
      is "Cognito = identity (password, MFA, session), CMS = authorization (role and per-resource
      permission)". `User.permission` / `is_admin` / `is_active` / `collection_permissions` /
      `single_page_permissions` are in the account record, and `PATCH /api/auth/users/{id}` is a **shared
      route** so it can be used on AWS as is. Because it does not decide from the token's contents (the
      middleware looks up the record every time), **a permission change takes effect from the next
      request** - it does not wait for a token reissue. The reason we did not move to Cognito groups is
      the 3 points: (a) per-resource overrides (the 3 bits per collection) do not fit in a group,
      (b) reflecting changes is bound to token refresh, (c) auditing and the UI need the local side either
      way.
      → **Completion condition**: a test that local permissions work with a Cognito token
      → **Achieved**: `a_role_change_takes_effect_on_the_next_request` (confirmed with the same token:
      viewer → edit allowed → per-collection denial. Run on both backends).

- [x] **Cognito user management** (2026-09). Added `aws-sdk-cognitoidentityprovider`, and
      `CognitoAccountProvisioner` in `crates/aws/src/provisioner.rs` implements
      `auth::provisioner::AccountProvisioner`. Creation, deletion, reset, and enable/disable reach the
      pool side too, and `AuthService` calls **the provider first** (if it is refused, nothing changes on
      the CMS side). The seam is `CognitoAdmin`'s 4 calls, and **since there is no Cognito emulator**, all
      that can be seen locally is the mapping - that is why the unit tests use a fake pool to confirm
      "what is called, in what order, with what values" and "a refusal is not made to look like success".

      Two things we learned during implementation, both of which are "hard to notice just by reading the code":

      - **`create` returns the pool-side identifier (`sub`)**. The CMS's token resolution only looks up by
        `external_id`, so if this is not recorded on the account the administrator created, the person is
        rejected with `not_provisioned` even when they sign in (it exists in both, yet cannot be found from
        one). If the name is already in the pool, it is picked up with `AdminGetUser`.
      - **Reset uses `AdminSetUserPassword` (`Permanent=false`) for a temporary password**. The CMS does
        not hold passwords, so it cannot create a value; instead it "makes them change it on next sign-in".
        `AdminCreateUser` uses `MessageAction::Suppress` (the invitation does not arrive, since this is a
        deployment that does not send email). The value to hand over is created by the reset - the screen
        has the same shape of "have them copy what is handed over" whether it is a link or a temporary
        password, and `password_reset` says so in advance while the response's `kind` says so at the moment.

      `POST /api/auth/users` and reset became **shared routes** (`http::auth`), and the 501 stub shrank to
      only changing your own password. The contract suite asserts the shape with `Backend::PASSWORD_RESET`,
      and the AWS side goes through the **real routes** with an in-memory fake pool.

      → **Verified (staging, 2026-09)**: `frontend/e2e/hosted-accounts.mjs` goes through, in a real
      browser, creation → temporary password → **signing in as the person and being forced to change it**
      → deletion → after deletion the pool refuses (against the administrator `cat` and a real Cognito pool).

### Deployment and operations

#### Serving the UI (S3 + CloudFront, 2026-09)

The admin screen is served from **a single CloudFront origin**. The app is S3 (private + OAC), the API is
a Function URL, and there are only 2 behaviors:

| Request | Origin | Cache |
|---|---|---|
| `/api/*` | Function URL | None (every answer depends on the token). `Authorization` is forwarded |
| Paths with an extension | S3 | As the object specifies (bundles for 1 year, `index.html` not) |
| Other paths | S3 (as `/index.html`) | Screen routes. The app is a single document |

- Because it is **the same origin**, CORS from the app to the API is unnecessary, and there is no work to
  strip `/api` from `apiUrl()` (the server holds `API_PREFIX`). Only the presigned PUT is cross-origin
  from the browser to S3, so the `app_url` origin is always added to the image bucket's CORS.
- **SPA deep links** have no object in the bucket. What rewrites them is a **CloudFront Function (default
  behavior only)**, and it is not attached to `/api/*`. If it were a custom error response for the whole
  distribution, even **the API's own 403/404** (a user without publish permission, a nonexistent item)
  would turn into a 200 of `index.html` - this was somewhere we actually stepped on, so we put it on the
  Function side.
- **Let the object decide the cache**: we create a cache policy for the app with `min_ttl = 0`, and
  `scripts/deploy-frontend.sh` specifies "bundles with a content hash are immutable for 1 year, and things
  whose name does not change (`index.html` / `favicon.ico` / notification assets) are revalidated every
  time". The managed CachingOptimized has a `min_ttl` of 1 hour, so `index.html`'s `no-cache` gets rounded
  up to 1 hour.
- **`app_url` is an input**. Deriving it from the distribution domain would create the cycle
  `distribution → function URL → function → Cognito client → callback_urls → distribution`. The operator
  also prepares the certificate (ACM in us-east-1) and DNS records - in the same style as the JWT secret,
  Terraform only outputs "what it points at" (`terraform output frontend_url`).
- Deployment is `scripts/deploy-frontend.sh` (`ng build` → `aws s3 sync --delete` → invalidate `/` and
  `/index.html`). What Terraform holds is up to the bucket, distribution, and policies, and **the contents
  are build artifacts**, so they are not put in IaC.

- [x] Stack definition in IaC (Lambda + Function URL, DynamoDB, S3, CORS, environment variables, secrets in
      Secrets Manager) - **Done**. `infra/` is that, and it can be created with one `terraform apply`. No
      API Gateway is inserted: a function URL is enough.
- [x] Run the existing E2E against staging - **Done**. Admin screen sign-in is
      `frontend/e2e/hosted-signin.mjs` / `hosted-accounts.mjs` over a real browser.
- [x] IAM least privilege, logs/metrics, cost confirmation - **Done**. The deployment policy is
      `infra/deployer-policy-*.json` (133 actions), logs are log groups with a retention period, and costs
      are written in the README's "What it costs to run" with measured values.

### Remaining work

- **Pushing down the admin screen listing**: the delivery API's listing now scans status records in ID
  order and reads only the bodies that fall in the window, but the admin screen listing is the **union**
  of published copies and working copies, so the two key spaces must be merged in ID order. It is
  authenticated and the counts fit within the scale, so it is deferred (the design is the same "hand the
  window to storage").
- **Whether to make the delivery API cursor-based**: right now it is offset/limit, and it scans status
  records to the end to produce `total`. Using `LastEvaluatedKey` as the key for the next page removes the
  scan, but it means giving up `total`, and the API shape changes (the room left in `content-api.md` §3.1).
- **Putting the Webhook on SQS**: since the execution environment can be frozen after the response,
  delivery is currently covered by "budgeted synchronous delivery" (§1). Once the scale is such that lost
  delivery is a problem, add a queue + publish permission + delivery Lambda to Terraform, and choose
  `Notifier` per backend. Testing requires a queue emulator.

#### Execution architecture (2026-09: **arm64 / Graviton by default**)

Lambda runs on **arm64**. It is cheaper per GB-second than x86_64 and at least as fast for this kind of
processing, so there is no reason not to choose it. The switch requires only the following 2 places.

| Where | What |
|---|---|
| `scripts/build-lambda.sh` | Builds for `aarch64-unknown-linux-gnu` with `--arch arm64` (default) and writes `infra/build/tsubame-aws-arm64.zip` |
| `infra` | `function_architecture` (default `arm64`) goes into `architectures`, and `local.function_zip` points at **the zip with the same name** |

**An architecture mismatch is not discovered until invocation** (the deploy succeeds). So the script
inspects the artifact with `file`, puts the architecture in the zip's name too, and the Terraform side
derives that name from a variable.

Cross-building from x86_64 requires **a C compiler for the target** (because `ring` compiles C. A linker
alone is not enough). **The zig path is the default guidance** (it installs nothing into the system, and
zig carries libc too. Verified).

```
cargo install cargo-zigbuild
# zig is from https://ziglang.org/download/ (this repository's CI uses 0.13.0)
scripts/build-lambda.sh --arch arm64 --zig
```

The same can be done with the distribution's cross toolchain (when `--zig` is not passed).

```
apt-get install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross
rustup target add aarch64-unknown-linux-gnu
scripts/build-lambda.sh --arch arm64
```

- The script first checks **whether the cross toolchain actually works** with a one-line C file. This is
  because even if the compiler exists, without binutils it calls the host's `as` and fails in a confusing
  way with `as: unrecognized option '-EL'` (this check is unnecessary with `--zig`, so it is skipped).
- Whether the artifact really is that architecture is checked with `file`. If it is not noticed here,
  Lambda reports it at invocation time.
- **CI has an arm64 build job** (`lambda-artifact`). It installs zig, builds with `--zig`, and checks all
  the way down to whether the zip's contents are aarch64. Even if the local development machine is x86_64,
  this path does not rot.
- The glibc required by a binary built with zig is up to **2.28** (verified), and it runs on
  `provided.al2023`'s 2.34.
- Whether it can actually **start** is confirmed in staging (the same treatment as the P5 E2E below).
- The same target can be used for the on-prem binary too (`cargo build --target aarch64-unknown-linux-gnu`,
  though rkv/LMDB is also C so the same cross toolchain is required).

### rustls provider at startup (2026-09 fix)

`reqwest` does not choose a rustls crypto provider by itself (we avoid aws-lc-rs's CMake with
`rustls-no-provider`), so **the process must install one before creating an HTTPS client**. The only place
installing it was the Webhook notifier, and in a deployment that does not use Webhooks it was panicking
when `HttpJwks` created its client. We call `install_crypto_provider()` in `HttpJwks::new` and at AWS
binary startup.

### Separate topics (out of scope this time)

- **UI internationalization**: the policy and the term decision list are in [`docs/i18n.md`](i18n.md).
  Content internationalization is separated out as a TODO in its §4 (a different scale from UI wording).

- Migration scripts from other CMSes (agreed on the policy of hitting HTTP from a separate project). Only
  if the volume is large enough to be unbearable, **add a bulk-create endpoint on the CMS side**.
- ~~Updating `docs/swagger.yaml`~~ → **Removed** (it covered only part of the implementation, and the
  Petstore sample text remained. The contract is `docs/content-api.md` and the contract tests).

### Credentials are decided by "endpoint override" (2026-09 fix)

Because it created emulator credentials from the mere presence of `AWS_ACCESS_KEY_ID` /
`AWS_SECRET_ACCESS_KEY`, **on Lambda it was receiving the execution role's temporary credentials under the
same variable names while signing with fixed credentials that dropped the session token** (every request
failed, and role rotation was ignored too). We changed the criterion to **whether there is an endpoint
override**, and if there is an `AWS_SESSION_TOKEN` we include it too. The emulator credentials are confined
to `emulator_credentials`, and `credential_tests` in `cargo test -p tsubame-aws` pins down "do not use them
if there is no override" and "if there is an override, use them together with the token".

### S3's "missing" becomes 403 depending on permissions (2026-09 fix)

`ImageService::replace_image` checks the existence of the entity with **HeadObject** before applying a
replacement, and refuses a 404 as "not yet uploaded". However, **without `s3:ListBucket`, S3 returns 403
even for a nonexistent key** (so as not to reveal existence), so it did not reach that refusal and became a
500. Furthermore, the execution role had no `s3:GetObject`, and **it happened to pass via the public bucket
policy's `s3:GetObject` (principal `*`)** - the moment the bucket is placed behind CloudFront, HeadObject
itself fails. We made `infra/lambda.tf` explicit about `s3:GetObject` (for HeadObject) and `s3:ListBucket`
(to get the 404 back).

**We made it reproducible on the emulator too**: create **a user with the same policy as the deployment**
(`user_with_the_deployment_policy`. Currently a versitygw user + a bucket policy with that user as
principal), and call `image_bytes_exist` with those credentials. If code uses a permission that is not in
the deployment policy it becomes a 403, so the state that happened to pass with root credentials (the
`s3:GetObject` case above) is no longer hidden. **This test had not run even once for a while** (the path
to the compose file was off by one level, so `run_in_emulator` returned `None` and the test passed through
as a "skip"). The `run_in_emulator` at the time **collapsed failure and "there is no emulator" into the
same `None`**, so even if the emulator refused the policy or the user could not be created, it stayed
green. Now:

- **A skip is only "there is no emulator"** (`emulator_reachable` checks and returns `None`). A compose
  path that does not resolve, a `docker` failure, or a gateway refusal are **all test failures**.
- Therefore, be careful when **reading `cargo test`'s "24 passed" as coverage**: in a run that has not
  started the emulators, this family of tests stays skipped and green (that is intended behavior, and
  `scripts/test-rust.sh` does not run the AWS side at all unless 8000/9000 are open).

**The part that cannot be reproduced on the emulator**: real S3 answers **403** even for "a nonexistent
key" when there is no `s3:ListBucket` (it does not let you distinguish absence from insufficient
permission), but **the emulator returns 404 regardless of whether ListBucket is present**. The test
explicitly pins down that there is no difference with a user that lacks `ListBucket`, and the side of the
reason "403 happens, so 404 is needed" is carried by comments in `infra/lambda.tf` and
`deployment_s3_policy`.

Another emulator quirk we stepped on along this path: **DynamoDB Local does not allow non-alphanumeric
characters in access key IDs** (`cms-...` gives `UnrecognizedClientException`). The restricted user's name
is alphanumeric only because there is a test that passes the same credentials to the DynamoDB client too.

The remaining hole is that **the Terraform and test policies are managed in two places**: we bridge it
with a comment saying to also look at `infra/lambda.tf` when changing `deployment_s3_policy` (changing
only one side will not be noticed by the tests).

### Do not write image records back as they were read (2026-09 fix)

An image record is a single JSON string (`data`) in DynamoDB, so "update only this one field" is not
possible. Because a replacement request **unconditionally wrote back the entire record it had read** in
order to record `pending_replacement`, if the replacement application completed in the meantime it could
**overwrite the new `file_name` saved by the application with the old value** - and since the application
has already deleted the old object, the image can no longer be displayed.

We added `write_if_unchanged` (a conditional `put_item` with `#data = :expected`) and made it **write only
when the record matches the one read**. If it is rejected by the condition, it re-reads and retries
(`RECORD_ATTEMPTS = 5`). `rename_image` and `set_image_deleted_at` are also the same read-modify-write, so
they were routed through the same path (`change_image`). The application side also does **the wait check
and consumption in a single conditional write**, and "reapplying the file currently being served" returns
200 without writing anything and **does not clear the wait**. On-prem has read and written inside the
single lock of `Repository::begin()` from the start.

## 3. Testing policy (summary)

| Layer | Local | Real AWS |
|---|---|---|
| Router / domain | `oneshot` + **the shared contract suite** (both backends) | — |
| DynamoDB | DynamoDB Local (`endpoint_url` swap) | GSI propagation delay, throttling, limits |
| S3 | **versitygw** (presign → PUT → GET. Verified. Does not reproduce the 403-ization of absence) | IAM, CloudFront, transfer |
| Lambda entry point | `serve` on localhost for E2E / `cargo lambda watch`. CI builds the **arm64 artifact** and checks down to its architecture | Event shape, cold start, freezing |
| Cognito | Generate keys and inject JWKS (unit) | Real pool configuration |
| Whole | — | A disposable stack + the existing E2E |

The emulators **imitate the API but do not imitate the performance characteristics**. So we do not treat a
green local suite as "proof that it works on AWS", and we keep the staging smoke test.

## 4. What we will not do

- Rewrite the on-prem rkv implementation to match DynamoDB (we keep both. Choose with features).
- Couple migration scripts to this repository's internal API (HTTP only).
- Make do with only emulators (omitting verification on real AWS).
