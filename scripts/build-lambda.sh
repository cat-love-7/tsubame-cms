#!/usr/bin/env bash
#
# Build the AWS backend as a Lambda artifact: a `bootstrap` binary in a zip, which is what
# `provided.al2023` runs.
#
# The target is the Lambda execution environment (Linux, x86_64). A musl target makes the binary
# self-contained; without it the build needs the same glibc as the runtime, which is what the
# `provided.al2023` image has.
#
# Usage: scripts/build-lambda.sh [output-zip]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
output="${1:-$root/infra/build/sl-cms-aws.zip}"

echo "== building the aws backend for Lambda =="
cargo build --manifest-path "$root/sl_cms/Cargo.toml" \
  --package sl-cms-aws --bin sl-cms-aws --release

staging="$(mktemp -d /tmp/sl-cms-lambda-XXXXXX)"
trap 'rm -rf "$staging"' EXIT

# Lambda hands the process to `bootstrap`; nothing else about the name matters.
cp "$root/sl_cms/target/release/sl-cms-aws" "$staging/bootstrap"

mkdir -p "$(dirname "$output")"
rm -f "$output"
(cd "$staging" && zip -q -X "$output" bootstrap)

echo "wrote $output ($(du -h "$output" | cut -f1))"
echo "not verified here: it has never been invoked by Lambda. That is what staging is for."
