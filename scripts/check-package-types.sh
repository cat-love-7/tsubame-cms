#!/usr/bin/env bash
#
# The packages' JSDoc types, checked with the admin app's TypeScript.
#
# `packages/*` ship JavaScript: what npm publishes is the file a reader opens, and neither package
# has a runtime dependency (`tsubame-preview` is zero-dependency by design, see
# `docs/preview-site.md`). Their types live in JSDoc, so there is nothing to compile - this runs
# `tsc` over them with `checkJs` (`packages/*/tsconfig.json`), which is what makes those
# annotations worth writing: without a check, a JSDoc type is a comment that quietly stops being
# true, and a wrong one is worse than none at all.
#
# It borrows the TypeScript and `@types/node` the admin app installs - the tsconfigs point their
# `typeRoots` there - so the packages need no toolchain of their own. The admin app's dependencies
# have to be installed first; there is no skip here, because a type check that can pass by not
# running is not one.
#
# Usage: scripts/check-package-types.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tsc="$root/frontend/node_modules/.bin/tsc"

if [ ! -x "$tsc" ]; then
  echo "the admin app's TypeScript is not installed: run (cd frontend && npm ci)" >&2
  exit 1
fi

for package in tsubame-preview gatsby-source-tsubame; do
  echo "== $package =="
  (cd "$root/packages/$package" && "$tsc" -p tsconfig.json)
done
