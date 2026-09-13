#!/usr/bin/env bash
#
# The Angular suite: unit tests and a production build.
#
# Usage: scripts/test-frontend.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/frontend/sl_cms"

echo "== unit tests =="
npx ng test --watch=false

echo "== production build =="
npx ng build
