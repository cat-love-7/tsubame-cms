#!/usr/bin/env bash
#
# The browser end-to-end suite: starts a CMS with a throwaway data directory and a dev server,
# drives a real browser against them, then cleans both up.
#
# Prerequisites: Chromium for Playwright (see frontend/sl_cms/e2e/README.md), and ports 8080
# and 4200 free. In this constrained container the browser install also needs
# `npx playwright install-deps chromium`, and both installs want
# PLAYWRIGHT_BROWSERS_PATH=/tmp/pw-browsers.
#
# Usage: scripts/test-e2e.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
frontend="$root/frontend/sl_cms"
data_root="$(mktemp -d /tmp/sl-cms-e2e-XXXXXX)"
# A test fixture, not a secret: this server only ever listens on loopback and is thrown away.
jwt_secret="0123456789012345678901234567890123456789"

cleanup() {
  # Each server was started in its own process group, so stopping the group takes its
  # children (cargo's binary, esbuild, vite) with it.
  for pid in "${backend_pid:-}" "${frontend_pid:-}"; do
    [ -n "$pid" ] && kill -- "-$pid" 2>/dev/null || true
  done
  rm -rf "$data_root"
}
trap cleanup EXIT

echo "== starting the backend on 8080 (data in $data_root) =="
setsid env DATA_ROOT="$data_root" JWT_SECRET="$jwt_secret" \
  ADMIN_USERNAME=admin@example.com ADMIN_PASSWORD=admin-password \
  cargo run --manifest-path "$root/sl_cms/Cargo.toml" >/tmp/sl-cms-e2e-backend.log 2>&1 &
backend_pid=$!

echo "== starting the dev server on 4200 =="
setsid npx --prefix "$frontend" ng serve --port 4200 >/tmp/sl-cms-e2e-frontend.log 2>&1 &
frontend_pid=$!

wait_for() {
  local url="$1" name="$2"
  for _ in $(seq 1 60); do
    if curl -fsS -o /dev/null "$url" 2>/dev/null; then
      echo "$name is up"
      return 0
    fi
    sleep 2
  done
  echo "$name did not come up; see /tmp/sl-cms-e2e-*.log" >&2
  return 1
}
wait_for http://127.0.0.1:8080/ backend
wait_for http://localhost:4200/ frontend

echo "== running the harness =="
cd "$frontend"
BASE_URL=http://localhost:4200 npm run e2e
