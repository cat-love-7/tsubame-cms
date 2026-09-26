#!/usr/bin/env bash
#
# The one test that runs a real Gatsby build.
#
# `scripts/test-gatsby-source.sh` drives the plugin's own functions with a fake Gatsby API: fast, no
# dependencies, and it cannot check the two things a relation union is - that Gatsby accepts the
# union and the inline fragments a site has to write, and that its `resolveType` and field resolver
# answer what the schema promises. So this builds a three-file site (in a temporary directory, with
# a stub delivery API) and reads the page data back.
#
# It needs `gatsby` installed in the package, which is a few hundred megabytes and about a minute of
# build, so it is not part of the unit suite. Without it this script says so and skips, the way the
# AWS half of `scripts/test-rust.sh` skips when the emulators are down.
#
# Usage: scripts/test-gatsby-build.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
package="$root/packages/gatsby-source-tsubame"

if [ ! -f "$package/node_modules/gatsby/cli.js" ]; then
  echo "skipped: gatsby is not installed in packages/gatsby-source-tsubame"
  echo "         install it with: (cd packages/gatsby-source-tsubame && npm install)"
  exit 0
fi

echo "== a real gatsby build against the stub delivery API =="
cd "$package"
node --test test/build/build.test.mjs
