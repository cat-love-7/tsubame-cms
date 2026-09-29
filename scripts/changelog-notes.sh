#!/usr/bin/env bash
#
# One release's notes: the `CHANGELOG.md` section for a tag, printed the way `gh release create
# --notes-file` wants them.
#
# The changelog is where a release is explained, and a GitHub release that repeats it is what a
# reader sees when they follow a tag. Writing those notes by hand is one more place to forget a fix,
# so the release workflow asks the changelog for them instead.
#
# The link definitions at the end of the file (`[0.1.0]: https://...`) are not part of any section
# and are not printed; a version with no section is an error rather than an empty release body.
#
# Usage: scripts/changelog-notes.sh v0.2.0      (a version without the `v` works too)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
tag="${1:-}"
if [ -z "$tag" ]; then
  echo "usage: scripts/changelog-notes.sh <tag|version>" >&2
  exit 2
fi
version="${tag#v}"

notes="$(
  awk -v heading="## [$version]" '
    index($0, heading) == 1 { inside = 1; next }
    inside && (/^## \[/ || /^\[[^]]+\]:/) { exit }
    inside { print }
  ' "$root/CHANGELOG.md"
)"

if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "CHANGELOG.md has no section for $version" >&2
  exit 1
fi
printf '%s\n' "$notes"
