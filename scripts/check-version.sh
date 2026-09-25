#!/usr/bin/env bash
#
# One version for the whole repository, checked instead of remembered.
#
# The CMS, the two npm packages, the admin interface and the git tag all carry the same number
# (see `CHANGELOG.md`), so the number is written in four manifests, repeated in `frontend`'s
# lockfile, and used a sixth time by the tag. The Rust crates do not repeat it: each declares
# `version.workspace = true`, and this insists on that too, because a crate that pins its own number
# is one more place to forget.
#
# CI runs this on every change, and a release runs it against the tag it is about to get:
#
#   scripts/check-version.sh              # do the manifests agree with each other?
#   scripts/check-version.sh v0.2.0       # ... and with the tag being released?
#
# Usage: scripts/check-version.sh [expected-tag]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
expected="${1:-}"

# A release workflow can hand the tag over by doing nothing: GitHub sets both of these on a tag.
if [ -z "$expected" ] && [ "${GITHUB_REF_TYPE:-}" = "tag" ]; then
  expected="${GITHUB_REF_NAME:-}"
fi

echo "== checking the version numbers =="
python3 - "$root" "$expected" <<'PY'
import json
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
expected = sys.argv[2]

problems = []

# The workspace manifest is the source: the crates inherit from it, so it is the number the
# others have to match.
lines = (root / "backend/Cargo.toml").read_text().splitlines()
try:
    start = lines.index("[workspace.package]")
except ValueError:
    sys.exit("backend/Cargo.toml: no [workspace.package] table")

version = None
for line in lines[start + 1 :]:
    if line.startswith("["):
        break
    match = re.match(r'version\s*=\s*"([^"]+)"', line)
    if match:
        version = match.group(1)
        break
if version is None:
    sys.exit("backend/Cargo.toml: [workspace.package] has no version")

places = {"backend/Cargo.toml [workspace.package]": version}
manifests = sorted(root.glob("packages/*/package.json")) + [root / "frontend/package.json"]
for path in manifests:
    places[str(path.relative_to(root))] = json.loads(path.read_text())["version"]

for name, value in places.items():
    if value != version:
        problems.append(f"{name}: {value}, but the workspace says {version}")

# A lockfile repeats the version of the package it describes, and `npm ci` refuses to run when the
# two disagree - so a bump that forgets it fails the build with a message about the lockfile rather
# than about the version. `frontend/package-lock.json` is the only one today; the loop is here so
# that a package growing one is covered the day it does.
for manifest in manifests:
    lock = manifest.with_name("package-lock.json")
    if not lock.exists():
        continue
    wanted = places[str(manifest.relative_to(root))]
    locked = json.loads(lock.read_text())
    for where, value in (
        ("version", locked.get("version")),
        ('packages[""]', locked.get("packages", {}).get("", {}).get("version")),
    ):
        if value != wanted:
            problems.append(
                f"{lock.relative_to(root)}: {where} is {value}, "
                f"but {manifest.relative_to(root)} says {wanted}"
            )

for path in sorted(root.glob("backend/crates/*/Cargo.toml")):
    text = path.read_text()
    name = path.relative_to(root)
    if "version.workspace = true" not in text:
        problems.append(f"{name}: does not inherit `version.workspace = true`")

if problems:
    print("the version numbers disagree:")
    print("\n".join(f"  {problem}" for problem in problems))
    sys.exit(1)

if expected and expected != f"v{version}":
    sys.exit(f"the tag is {expected}, but the version is {version} (expected v{version})")

where = f"{len(places)} places"
tag = f", tag {expected}" if expected else ""
print(f"ok: {version} in {where}{tag}")
PY
