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

# Selecting no backend used to build a binary that did nothing at all, and selecting `aws`
# used to fail with whatever error came first. Both are refused on purpose, so a change that
# quietly removes the guard is a regression worth catching here.
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

echo "== selecting the unimplemented aws backend is refused =="
expect_refused "not implemented yet" cargo check --no-default-features --features aws
