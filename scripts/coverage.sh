#!/usr/bin/env bash
#
# Where the tests actually reach, for both halves.
#
#   Rust:      cargo llvm-cov (needs `cargo install cargo-llvm-cov` and
#              `rustup component add llvm-tools-preview`). Prints regions / functions / lines.
#   Frontend:  `ng test --coverage` (needs the `@vitest/coverage-v8` dev dependency).
#
# The frontend run passes `--no-isolate`. Under coverage, every spec file would otherwise boot
# its own jsdom + Angular environment and re-instrument every module, and with the whole suite
# competing for the CPU a test occasionally sits past Vitest's 5s timeout and is reported as a
# failure even though its body is synchronous. One shared environment removes the contention
# (and is faster); `ng test` keeps isolation for ordinary runs.
#
# Neither number includes the browser check (`scripts/test-e2e.sh`): coverage of the Angular
# templates during a real browser session is not measured, and the server that the check drives
# is a separate process. The Rust number does include the contract suite, which drives the
# router in-process.
#
# Usage: scripts/coverage.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

echo "== Rust (unit + contract suite) =="
(cd "$root/backend" && cargo llvm-cov --workspace --summary-only)

echo
echo "== Frontend (component suite) =="
(cd "$root/frontend" && npx ng test --coverage --no-isolate --watch=false)
