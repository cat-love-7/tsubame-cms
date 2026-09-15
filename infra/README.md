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
  limit is the one `crates/aws/src/lambda.rs` already enforces a body size against.
- **The bucket is publicly readable.** Content stores an object's own address, which has to keep
  working long after a signature would have expired. CloudFront in front of it is the next step
  if that is wanted; `AWS_IMAGE_BASE_URL` is already the variable for it.
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
