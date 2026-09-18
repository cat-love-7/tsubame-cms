# Deploying the AWS backend

`terraform apply` here creates everything the CMS needs on AWS: one DynamoDB table, one S3
bucket, a Cognito pool and app client with its hosted sign-in page, a WAF rule in front of that
page, and the Lambda function the API runs as.

## What is verified, and what is not

| Check | Where it runs |
|---|---|
| `terraform fmt -check` | here |
| `terraform init -backend=false` + `terraform validate` | here, against the real providers |
| `terraform plan` / `apply` | **nowhere yet** — they need AWS credentials |
| The function actually answering an invocation | **nothing yet**; that is staging |
| The Lambda artifact building for **arm64** | CI (`.github/workflows/ci.yml`, job `lambda-artifact`) |

So this is a configuration that Terraform agrees is well-formed, not one that has been applied.
The first `apply` is where IAM semantics, service quotas, the Cognito domain's global uniqueness
and the bucket policy meet reality.

## Before two people apply it

The table holds everything, so two things are on by default and one is left to the operator:

* **`deletion_protection = true`** (the variable's default). `terraform destroy` then fails on the
  table instead of taking every account, collection and page with it. A scratch deployment sets
  the variable to false *deliberately*.
* **State has to live somewhere shared** before a second pair of hands runs `plan`. The commented
  `backend "s3"` block in `versions.tf` is the shape to fill in (bucket + lock table); without it,
  each operator has their own state file and the second `apply` is a guess.
* **MFA is available, not required** (`mfa_configuration = "OPTIONAL"` with TOTP). Turning it into
  a requirement is a policy for whoever runs this deployment, and it is a one-line change to
  `REQUIRED` once every account has enrolled.

## Using it

```bash
scripts/build-lambda.sh                 # writes infra/build/sl-cms-aws-arm64.zip (the default)
cd infra
cp terraform.tfvars.example terraform.tfvars   # fill in jwt_secret and who may administer
terraform init
terraform plan
terraform apply
```

The backend the plan talks about is the local one (`backend "local"`, the default). A shared
deployment wants an S3 backend so two operators cannot apply different plans; that is a block to
add, not a default to guess.

## What the deployment is told

`terraform output function_environment` prints exactly the variables the function runs with —
`sensitive`, so it also prints the secret. The code reads those names in
`sl_cms/crates/aws/src/settings.rs`, and `AWS_IMAGE_BASE_URL` is what makes the URL stored in
content match the bucket.

## Architecture

The function runs on **arm64** (Graviton): cheaper per GB-second than x86_64, and no slower for
this kind of work. `var.function_architecture` (default `arm64`) is what the function is declared
with, and `local.function_zip` derives the artifact name from it, so the bytes and the declaration
cannot disagree — Lambda only reports a mismatch when the function is invoked.

Building it on an x86_64 machine needs a toolchain for the target, because a dependency compiles C
(`ring`; see `sl_cms/Cargo.toml`). Zig is the one CI uses, and the one that needs nothing installed
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
- **The bucket is publicly readable by default, and that is a choice** (`image_delivery`). Content
  stores an object's own address, which has to keep working long after a signature would have
  expired. CloudFront in front of it is the next step if that is wanted; `AWS_IMAGE_BASE_URL` is
  already the variable for it.
- **`image_delivery = "presigned"` is the other choice**, for a site that fetches its images during
  a build and serves transformed copies of its own: the bucket stays private, every URL the API
  returns is a signature over the object (`image_url_ttl_seconds`, an hour by default), and a page
  that cached one of those URLs would carry a dead link once it expires.
- **No Cognito groups.** Roles, per-collection and per-page grants live in the CMS's records
  (`doc/aws-plan.md`, P4): Cognito answers who someone is, the CMS what they may do.
- **`allow_admin_create_user_only`.** Nobody signs themselves up; an administrator creates the
  account, and `BOOTSTRAP_ADMIN_USERNAMES` is how the first one appears.
- **WAF, not threat protection.** Cognito's threat protection is risk scoring (Plus plan) and its
  documentation points at WAF for volume. This rule is the volume case; the per-account lockout
  is Cognito's own.
- **No account recovery by email for accounts without one.** Recovery uses a verified address;
  an account an operator created without one is reset by an administrator
  (`AdminSetUserPassword`, still to be wired into the account screen).
