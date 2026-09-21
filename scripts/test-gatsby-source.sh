#!/usr/bin/env bash
#
# The Gatsby source plugin's unit tests.
#
# Nothing here needs a network, a Gatsby or a CMS: the plugin's only runtime dependency is `fetch`,
# its tests are `node:test`, and the delivery API is faked from the shapes `doc/content-api.md`
# promises. An end-to-end build against a real CMS is described in
# `frontend/gatsby-source-sl-cms/README.md`.
#
# Usage: scripts/test-gatsby-source.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/frontend/gatsby-source-sl-cms"
node --test test/*.test.mjs
