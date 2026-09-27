# Deploying the AWS backend

`terraform apply` here creates everything the CMS needs on AWS: one DynamoDB table, two S3
buckets (the images, and the built app), a Cognito pool and app client with its hosted sign-in
page, a CloudFront distribution in front of the app *and* the
API, and the Lambda function the API runs as.

## What is verified, and what is not

| Check | Where it runs |
|---|---|
| `terraform fmt -check -recursive` | here |
| `terraform init -backend=false` + `terraform validate` | here, against the real providers, for `infra/` and `infra/bootstrap/` |
| `terraform plan` / `apply` | **staging, 2026-09-19** — account `<account-id>`, `ap-northeast-1`, applied from this tree with the three deployer policies; `plan` after it says *No changes* |
| The built app served by CloudFront | **staging** — `scripts/deploy-frontend.sh`, on `https://cms.example.com` |
| The function actually answering an invocation | **staging** — `scripts/smoke-test.sh https://cms.example.com <function-url>` passes, and `/api/content/collections` answers `200 []` from the DynamoDB table, which is the execution role working rather than just the function being reachable |
| The Lambda artifact building for **arm64** | CI (`.github/workflows/ci.yml`, job `lambda-artifact`), and `scripts/build-lambda.sh --zig` here |
| Someone actually signing in | **staging** — `frontend/e2e/hosted-signin.mjs` (a real browser: PKCE handoff, the registered callback, the code exchange, the admin screen, no console errors) |
| Managing an account at the pool | **staging** — `frontend/e2e/hosted-accounts.mjs`: creates an account, resets it to a temporary password, signs in *as it* and changes the password the provider demands, removes it, and finds the pool refusing it afterwards |

The first deployment is done, and it is the first deployment that found things a `validate` cannot.
In the order they surfaced:

* **IAM gaps, five rounds of them.** `s3:GetBucketTagging` and `s3:GetBucketCORS` (the wildcard
  `s3:GetBucket*` does not reach every bucket read, and the ones it misses are the
  `Get*Configuration` family); `s3:GetAccelerateConfiguration`, whose IAM name does not contain
  "Bucket" at all; `logs:DescribeLogGroups` and `cognito-idp:DescribeUserPoolDomain`, which cannot
  be scoped to a log group or a pool; `logs:ListTagsForResource`, which arrives with the log
  group's ARN *without* the `:*` suffix the statement was scoped to. The three applying policies
  carry all of them now, which is also why they are three: the document outgrew IAM's 6144
  characters for one managed policy.
* **`OPTIONS` cannot be in a function URL's CORS methods.** Lambda validates each member of that
  list against a six-character limit, and the preflight method is seven characters long. A
  preflight still works: the URL forwards it and the router answers it.
* **The advertised `login_url` is a base, not a link.** Asked for on its own, Cognito answers
  "Required parameters missing" - the browser adds `redirect_uri`, the PKCE challenge and the
  state. `scripts/smoke-test.sh` adds the registered callback, so the check covers the callback
  URL as well.
* **The icon font was never applied**, which the first person to look at the deployed screen saw
  at once: `mat-icon` sets the box and the ligature `::before` and leaves the *family* to the
  application, and `@fontsource/material-icons` ships the face rather than the class that names
  it - so every icon drew its own name as text, in every environment, and no suite noticed because
  they assert on text and aria labels. `styles.scss` names the family now, and `check-ui.mjs`
  checks that an icon's computed family is the icon font and that the face actually loaded.

## Before two people apply it

Three things stand between a mistake and a lost deployment:

* **`deletion_protection = true`** (the variable's default). `terraform destroy` then fails on the
  table instead of taking every account, collection and page with it. A scratch deployment sets
  the variable to false *deliberately*.
* **State lives in S3, in a bucket that has to exist first.** `infra/bootstrap/` is the one root
  applied by hand: it creates the bucket, turns versioning on (that is what makes a bad apply
  recoverable) and refuses to be destroyed. Locking is S3's own (`use_lockfile`: a `<key>.tflock`
  object beside the state) — the DynamoDB table older configurations locked with is deprecated and
  goes away in a future Terraform.
* **MFA is available, not required** (`mfa_configuration = "OPTIONAL"` with TOTP). Turning it into
  a requirement is a policy for whoever runs this deployment, and it is a one-line change to
  `REQUIRED` once every account has enrolled.

## Using it

Once, to give the main configuration somewhere to keep its state:

```bash
cd infra/bootstrap
terraform init
terraform apply -var state_bucket=<a globally unique name>   # prints the bucket
cd ..
cp backend.hcl.example backend.hcl    # fill in the bucket it printed; keep the region
terraform init -backend-config=backend.hcl
cd ..                                # back to the repository root
```

Then, per deployment:

```bash
scripts/build-lambda.sh                 # writes infra/build/tsubame-aws-arm64.zip (the default)
cd infra
#   aws secretsmanager create-secret --name tsubame/jwt-secret \
#     --secret-string "$(openssl rand -base64 48)"
cp terraform.tfvars.example terraform.tfvars   # fill in jwt_secret_arn, who may administer, the domain
terraform plan
terraform apply
```

Three of the things in that file are the operator's to make, because Terraform cannot: the
**secret** (above), the **domain**, and its **certificate** (an ACM certificate in `us-east-1`,
whatever region the rest of the deployment is in). The domain has to be known in advance rather
than read from the distribution: the pool's client registers `<app_url>/auth/callback`, the
distribution answers on that name, and the distribution needs the function URL - which needs the
function, which needs the client's id. That circle is why `app_url` is an input, and why the
order is: create the certificate, apply, then point the name at `terraform output frontend_url`.

The app itself is deployed last, because it is a build artifact rather than infrastructure:

```bash
scripts/deploy-frontend.sh              # ng build, aws s3 sync --delete, invalidate index.html
scripts/smoke-test.sh                   # the app and its /api, over HTTP
```

The smoke test is the first thing that asks the deployment a question rather than describing it:
the shell and its fallback, the two cache lifetimes, `/api/*` reaching the function, the refusal
shapes, and the sign-in page the deployment advertises. It takes the app URL as an argument, so
anyone can run it against a deployment without credentials of their own.

The preview site is a **separate artifact** with its own deployment, and it is deliberately not the
app: a preview renders *unpublished* HTML, while the app's origin keeps the editor's token in
`localStorage` and renders no CMS HTML at all, so the two must not share an origin
(`docs/preview-site.md` §7). It lives in the same bucket under `preview/` and gets its own
distribution on its own name (`infra/preview.tf`):

```bash
# The site's preview build is the deployment's own (no source plugin, client-only routes:
# `packages/gatsby-source-tsubame/README.md` §6); this script only delivers the result.
scripts/deploy-preview.sh --dist ../site/public
```

Because the two deployments share one bucket, neither may delete the other's objects:
`scripts/deploy-frontend.sh` excludes `preview/*` from its `--delete`, and `deploy-preview.sh` syncs
(and deletes) only inside `preview/`. The function's `PREVIEW_SITE_URL` and the preview origin in
`CORS_ALLOWED_ORIGINS` are both set from `preview_url` (`infra/locals.tf`), so a deployment does not
have to remember either.

`backend.hcl` is what makes the state shared: `terraform init` writes it to the bucket under the
key the file names, so two operators who run `plan` at once share one state and one lock instead of
two guesses. It is not committed (it names one account's bucket); `backend.hcl.example` is the
shape. `bootstrap/` keeps its own state locally — it is the thing that makes the remote state
possible, so it cannot use it — and that file is worth keeping: without it the bucket still exists,
but Terraform no longer knows it made it.

## A second environment

`local.name` is `"<project>-<environment>"`, and everything named from it follows: the DynamoDB
table, the images and the app bucket, the function with its role and log group, the three
CloudFront functions. A `tsubame-prod` beside a `tsubame-staging` is therefore a `terraform.tfvars`
with a different `environment` - plus the things only that file can decide, because they are
global names or the world outside the account:

| What | Why it is not derived |
|---|---|
| `jwt_secret_arn` | one secret per environment (`<project>/<environment>/jwt-secret`): two deployments sharing one can mint a token the other accepts |
| `bootstrap_admin_usernames` | who administers *this* deployment |
| `app_url`, `frontend_certificate_arn` | the name the provider sends the browser back to, and its certificate |
| `preview_url`, `preview_certificate_arn` | a name of its own, never a path under `app_url` (`docs/preview-site.md` §7) |
| `cognito_domain_prefix` | unique across *all* accounts in the region, so the default usually has to change |
| `cors_allowed_origins` | the preview origin, and anything else that calls the API |
| `site_name` | what the screens of *this* deployment call the site they administer; empty shows the product's name, as before |
| the state key in `backend.hcl` | one state per environment: `<project>/<environment>/terraform.tfstate` |
| `frontend_bucket`, `images_bucket` | only when the default (`<name>-app`, `<name>-images`) is taken - S3 names are unique across every account |

The two buckets are the only names with that global uniqueness; the table, the function and the
rest are unique per account and region, which `environment` already separates. The Lambda artifact
is one file per build (`infra/build/tsubame-aws-<arch>.zip`, or `function_zip`), so two
environments deployed from one checkout want it built - or copied aside - per environment.

## The identity that deploys

Four files, for the two things one does with this stack:

* **`infra/deployer-policy-storage.json`**, **`-compute.json`**, **`-edge.json`** (133 actions
  together) - the identity that applies it. Three files rather than one because **IAM caps a
  managed policy at 6144 characters**, and this policy was 6330 compact (8759 as written here);
  splitting it is what keeps it attachable at all. Storage is S3, Secrets Manager and DynamoDB;
  compute is Lambda, its log group and its execution role; edge is CloudFront, Cognito and the
  service-linked role.
* **`infra/deployer-policy-plan.json`** (61) - the identity that only *reads*: a plan, a review, a
  CI check. It is a strict subset of the three above, and the only things it writes are the state
  lock (`s3:PutObject` and `s3:DeleteObject` on `<state-key>.tflock`, because a plan takes the
  lock too) - plus the log reads, which are there for looking at a deployment rather than for
  planning.

All four are checked against AWS's own action list by `scripts/check-iam-actions.sh`, which also
insists the plan policy stays a subset. Run it after changing any of them: **a misspelt action is
invisible when the policy is attached** - IAM accepts the name and it grants nothing, so the
deployment fails later with `AccessDenied` on the real action, which reads like a missing
permission rather than a misspelt one (`s3:PutBucketLifecycleConfiguration` was one; the action is
`s3:PutLifecycleConfiguration`).

The `<name>` placeholder is one project, and a rename needs two: attach a second copy of the four
documents instantiated for the new name, and detach it once the old deployment is gone. They are
policies rather than one policy over both names on purpose - the next rename should not have to
widen them again.

Two S3 details worth knowing in the same area, both found by the first real `apply`:

* the wildcard `s3:GetBucket*` covers most of what a refresh reads and *not* the ones whose names
  do not start with it - `GetLifecycleConfiguration`, `GetEncryptionConfiguration`,
  `GetReplicationConfiguration`, `GetAccelerateConfiguration` and the other
  `Get*Configuration` reads - so those are listed beside it. The state bucket and the deployment
  buckets both rely on that wildcard rather than naming a dozen reads one at a time, which is what
  a refresh asks for in an order nobody can predict.
* `DescribeLogGroups` and `DescribeUserPoolDomain` cannot be scoped to a resource: they take an
  account-level permission (`"Resource": "*"`), and putting them in a statement scoped to a log
  group or a user pool denies them. Both are in statements of their own now.

All four take the same five placeholders.

| Placeholder | What it is |
|---|---|
| `<account>` | the account id the stack is applied in |
| `<region>` | where the resources live, `var.region` (the state bucket may be somewhere else) |
| `<name>` | `${var.project}-${var.environment}`, the prefix of almost everything the stack creates |
| `<state-bucket>` | the bucket `backend.hcl` names |
| `<state-key>` | the key `backend.hcl` names, e.g. `tsubame/staging/terraform.tfstate` |

### What a run needs, from the same file

| Run | What it calls |
|---|---|
| `plan` (and the refresh inside every apply) | the 59 reads: `Get*`, `Describe*`, `List*`, the log queries - plus the two writes the state lock takes |
| `apply`, nothing changed | the same |
| `apply`, something changed | those, plus the `Put*`/`Update*`/`Set*`/`Tag*` of the resources that changed (38 of them in total) |
| the first `apply`, or one that adds a resource | those, plus the 17 `Create*`/`Add*`/`Associate*`, and `iam:PassRole` for the function |
| an apply that *replaces* a resource | the same, plus the deletes of what it takes away - `terraform plan` says `# forces replacement`, and that is a delete and a create |
| `terraform destroy` | the 18 `Delete*`/`Remove*`/`Disassociate*` |
| `scripts/deploy-frontend.sh`, every time | `<name>-app` objects (`ListBucket`, `GetObject`, `PutObject`, `DeleteObject`) and `cloudfront:CreateInvalidation`/`GetInvalidation` |
| `scripts/deploy-preview.sh`, every time | the same bucket (`preview/*` only) and the same two invalidation calls, on the preview distribution - so no new action is needed for it |

The difference between the first deployment and later ones is therefore only the create half - and
it comes back whenever a resource is replaced, which is why the apply identity keeps it. What a
`plan` cannot do is written into the plan policy: no `Create*`, no `Put*` outside the lock.

The statements are scoped by that name prefix wherever AWS allows it: the table, the function, the
role, the log group and the buckets all carry it. CloudFront, Cognito and the
`Create*` calls that have no resource to name yet are `*`, which is how those services work.

Two things are deliberately **not** in it:

* **The signing secret's value.** The statement only covers creating the secret (the one-time step
  in *Using it*); reading it is the function's execution role, which the stack creates for it.
  Whoever deploys never sees the secret.
* **The certificate.** It is created outside Terraform like the secret, so nothing here touches ACM.

Verification needs almost none of this: the end-to-end suite talks HTTP to the deployment, so the
same person can run it without AWS credentials at all. The log reads are in the policy because a
deployment that answers 500 is otherwise a mystery.

## What the deployment is told

`terraform output function_environment` prints exactly the variables the function runs with —
`sensitive`, so it also prints the secret. The code reads those names in
`backend/crates/aws/src/settings.rs`, and `AWS_IMAGE_BASE_URL` is what makes the URL stored in
content match the bucket.

## How the app and the API are reached

One CloudFront distribution serves both, which is what makes the frontend simple: `apiUrl()` asks
for `/api/...`, and the browser sees one origin, so nothing needs CORS and nothing strips a prefix.

| Request | Origin | What is cached |
|---|---|---|
| `/api/*` | the function URL | nothing: every answer is a token's answer, and the viewer's headers (`Authorization`) are forwarded |
| a path with a file extension | the app bucket | as the object says (the bundles ask for a year, `index.html` for nothing) |
| any other path | the app bucket, as `/index.html` | a route, not a file: the app is one document with a router |

Two details are worth knowing before changing either:

* **The SPA fallback is a CloudFront Function on the default behavior**, not a distribution-wide
  custom error response. A custom error response cannot tell one origin from another, and would
  rewrite the API's own 403s and 404s (a viewer who may not publish, an item that is not there) into
  `index.html` with a 200.
* **The app's cache policy is the one that lets objects decide their own age** (`min_ttl = 0`), and
  `scripts/deploy-frontend.sh` is what tells them: a year for the content-hashed bundles, and
  revalidation for everything whose name survives a deployment.

The bucket is private and read through an origin access control; the policy grants `s3:GetObject`
only, because nothing here lists the bucket.

## Architecture

The function runs on **arm64** (Graviton): cheaper per GB-second than x86_64, and no slower for
this kind of work. `var.function_architecture` (default `arm64`) is what the function is declared
with, and `local.function_zip` derives the artifact name from it, so the bytes and the declaration
cannot disagree — Lambda only reports a mismatch when the function is invoked.

Building it on an x86_64 machine needs a toolchain for the target, because a dependency compiles C
(`ring`; see `backend/Cargo.toml`). Zig is the one CI uses, and the one that needs nothing installed
system-wide:

```bash
cargo install cargo-zigbuild
# zig from https://ziglang.org/download/ (CI pins 0.13.0)
scripts/build-lambda.sh --arch arm64 --zig
```

The distribution's cross toolchain does the same, without `--zig`:

```bash
apt-get install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross
rustup target add aarch64-unknown-linux-gnu
scripts/build-lambda.sh --arch arm64
```

`scripts/build-lambda.sh --arch x86_64` builds the other one, for a comparison or if a deployment
has to stay on x86_64 for some reason.

## Deliberate choices

- **A function URL, not API Gateway.** No stage or route table to configure, and its 6MB payload
  limit is the one `crates/aws/src/lambda.rs` already enforces a body size against. A public URL
  (`authorization_type = "NONE"`) needs **two** resource-based permissions -
  `lambda:InvokeFunctionUrl` and `lambda:InvokeFunction` (the latter with
  `invoked_via_function_url`, so it is not also a permission to invoke the function directly);
  with only the first, every request is answered with 403 before the function runs.
- **`AWS_REGION` is not among the function's environment variables.** The runtime sets it and the
  key is reserved, so a configuration that names it is rejected outright; the code still reads it
  (`crates/aws/src/settings.rs`), and in Lambda it is simply already there.
- **Buffered responses, not streaming.** A function URL can stream (`invoke_mode =
  "RESPONSE_STREAM"`), which raises the reply ceiling from 6MB to 200MB — the first 6MB uncapped,
  the rest at 2MB/s, billed to the end even if the client leaves. The CMS's answers are JSON built
  in memory, so streaming would move the ceiling without improving the time to first byte; the
  delivery API cuts its pages to fit instead (`docs/content-api.md`). Streaming earns its place
  when an answer is *built* while it is sent (an export), which is a feature, not a setting.
- **The bucket is publicly readable by default, and that is a choice** (`image_delivery`). Content
  stores an object's own address, which has to keep working long after a signature would have
  expired. CloudFront in front of it is the next step if that is wanted; `AWS_IMAGE_BASE_URL` is
  already the variable for it.
- **`image_delivery = "presigned"` is the other choice**, for a site that fetches its images during
  a build and serves transformed copies of its own: the bucket stays private, every URL the API
  returns is a signature over the object (`image_url_ttl_seconds`, an hour by default), and a page
  that cached one of those URLs would carry a dead link once it expires.
- **No Cognito groups.** Roles, per-collection and per-page grants live in the CMS's records
  (`docs/aws-decisions.md`): Cognito answers who someone is, the CMS what they may do.
- **`allow_admin_create_user_only`.** Nobody signs themselves up; an administrator creates the
  account, and `BOOTSTRAP_ADMIN_USERNAMES` is how the first one appears.
- **The API is served under `/api`.** `tsubame_core::API_PREFIX` nests the whole surface there, so
  a CloudFront distribution in front of the admin app can send `/api/*` to this function URL
  without rewriting anything, and nothing has to strip a prefix on the way in.
- **No WAF.** Nothing rate-limits sign-in per address. The pool's own per-category quotas bound
  what a flood can spend (e.g. `UserAuthentication`, 120 requests/second across the account and
  region), and per-account lockout after repeated failures is threat protection, which is the
  Plus plan's rather than a web ACL's (`docs/aws-decisions.md`). So a busy address can still spend
  the pool's share; a regional rate-based rule is what fixes that, and it was left out on purpose
  rather than forgotten - a handful of resources and roughly $5/month plus $1 per rule.
- **No account recovery by email, for anyone.** Cognito's own recovery sends a code to a verified
  address, and this deployment sends no mail, so an account whose password is lost is reset by an
  administrator instead: the account screen sets a **temporary password** (`AdminSetUserPassword`)
  that the person changes the first time they sign in. Nothing is mailed, so an account with no
  address on file is covered too - which is also why creating one does not email an invitation.
