#!/usr/bin/env bash
#
# The Rust test suite, plus the build-selection guards.
#
# Usage: scripts/test-rust.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/sl_cms"

echo "== tests (default backend: on-premises) =="
cargo test --all-targets

# Selecting no backend used to build a binary that did nothing at all. That is refused on
# purpose, so a change that quietly removes the guard is a regression worth catching here.
# The output is captured first: the command *fails*, and under `set -o pipefail` a pipeline
# ending in a successful `grep` would still report that failure as the pipeline's status.
expect_refused() {
  local expected="$1"
  shift
  local output
  output="$("$@" 2>&1 || true)"
  if grep -q "$expected" <<<"$output"; then
    echo "ok"
  else
    echo "expected the build to be refused with '$expected', got:" >&2
    echo "$output" >&2
    exit 1
  fi
}

echo "== a build with no backend selected is refused =="
expect_refused "select a storage backend" cargo check --no-default-features

# The aws adapter is checked (and tested) with its own feature set; with the default backend on
# as well, the "exactly one backend" guard would refuse the build. The server itself is still
# unimplemented, so `run` reports that rather than serving.
echo "== the aws backend builds, and its tests pass against DynamoDB Local if it is running =="
cargo test --no-default-features --features aws --all-targets
