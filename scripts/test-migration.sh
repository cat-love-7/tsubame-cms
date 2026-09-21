#!/usr/bin/env bash
#
# The migration script's unit tests: the attribute mapping, the value translation, the component
# ordering, and reading definitions out of a v3 checkout. Nothing here needs a network, a Strapi
# or a CMS - the end-to-end run against a real CMS is described in
# `scripts/migrate-from-strapi/README.md`.
#
# Usage: scripts/test-migration.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/scripts/migrate-from-strapi"
node --test test/*.test.mjs
