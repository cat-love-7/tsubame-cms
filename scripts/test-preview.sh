#!/usr/bin/env bash
#
# The preview package's unit tests.
#
# Nothing here needs a network, a Gatsby or a CMS: `tsubame-preview` has no runtime dependency beyond
# `fetch` (which the tests inject), and the delivery API is faked from the shapes
# `docs/content-api.md` promises. The contract the package implements is `docs/preview-site.md`.
#
# The Gatsby plugin keeps its own script (`scripts/test-gatsby-source.sh`) even though it uses this
# package's shape: the two run on different runtimes, and the plugin is deliberately not a dependency
# of the preview package or the other way round.
#
# Usage: scripts/test-preview.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/packages/tsubame-preview"
node --test test/*.test.mjs
