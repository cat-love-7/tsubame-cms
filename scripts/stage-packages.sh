#!/usr/bin/env bash
#
# Hand this version's npm packages to npm's staging area, where a maintainer approves them with 2FA
# before they go live.
#
# The release workflow (`.github/workflows/release.yml`) runs this on a tag. Staging takes the place
# of `npm publish` there, so a release that is wrong reaches npm's staged list - reviewable,
# rejectable, and not yet installable by anybody - instead of the registry, and the trust the
# workflow holds is only "may stage" (each package's trusted publisher is configured that way, and
# its publishing access can forbid tokens outright).
#
# Staging cannot be the *first* publish: npm stages a version of a package that already exists. The
# first release of each package was made by hand, which `CONTRIBUTING.md` describes once.
#
# A version that is already live is skipped rather than failed: re-running a release that
# half-finished should be able to finish, and there is nothing to stage for a version the registry
# already has.
#
# Running this locally stages the packages for real (it is the same command the workflow runs) and
# needs `npm login`.
#
# Usage: scripts/stage-packages.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"

# Staging needs `npm stage`, which arrived in 11.15.0. Saying so beats an "unknown command" half way
# through a release.
if ! npm stage --help >/dev/null 2>&1; then
  echo "this npm has no 'npm stage' (staged publishing needs 11.15.0 or later): $(npm --version)" >&2
  exit 1
fi

staged=0
for dir in "$root"/packages/*/; do
  # Every directory under `packages/` is a published package today; a directory without a manifest
  # is somebody's scratch space and not this script's business.
  [ -f "$dir/package.json" ] || continue
  name="$(node -p "require('$dir/package.json').name")"
  version="$(node -p "require('$dir/package.json').version")"

  # Staging is for a version of a package npm already has. The first release of a package is the
  # one exception (`CONTRIBUTING.md`, "The first publish of a package, once"), and saying so here
  # beats npm's own refusal, which arrives as a staging conflict.
  if ! npm view "$name" version >/dev/null 2>&1; then
    echo "$name is not on npm at all: publish its first version by hand first" >&2
    exit 1
  fi

  if npm view "$name@$version" version >/dev/null 2>&1; then
    echo "== $name@$version is already on the registry; nothing to stage =="
    continue
  fi

  echo "== staging $name@$version =="
  (cd "$dir" && npm stage publish)
  staged=1
done

if [ "$staged" = 1 ]; then
  echo
  echo "A person approves a staged version: 'npm stage list', then 'npm stage approve <stage-id>'"
  echo "(or the Staged Packages tab on npmjs.com), which asks for 2FA."
fi
