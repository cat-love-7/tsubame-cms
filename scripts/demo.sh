#!/usr/bin/env bash
#
# The local demo: a CMS with a sample site in it, left running so you can click around.
#
# It starts the on-premises backend on a throwaway data directory and the admin dev server, writes
# the sample site (`frontend/e2e/sample-site.mjs` - the four categories, five articles, home page
# and covers the README's pictures show) into it, and prints where to open it and how to sign in.
# It stays up until you press Ctrl-C, which stops both servers and deletes the data with them.
#
# Nothing outside that temporary directory is written: a demo that left a database behind - or
# overwrote one - would be a thing to clean up rather than a thing to try.
#
# Prerequisites: Rust and Node (`cd frontend && npm ci` once, see README), and ports 8080 and 4200
# free. The first run builds the backend, which takes a few minutes.
#
# Usage: scripts/demo.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
frontend="$root/frontend"
data_root="$(mktemp -d /tmp/tsubame-demo-XXXXXX)"
backend_log=/tmp/tsubame-demo-backend.log
frontend_log=/tmp/tsubame-demo-frontend.log
# A demo fixture, not a secret: this server only ever listens on loopback and is thrown away.
jwt_secret="0123456789012345678901234567890123456789"
admin_username="${ADMIN_USERNAME:-editor@tsubame.dev}"
admin_password="${ADMIN_PASSWORD:-tsubame-demo-password}"
# What the header calls the deployment, so the sample site does not have to wear the product name.
site_name="${SITE_NAME:-Tsubame demo}"

if [ ! -d "$frontend/node_modules" ]; then
  echo "frontend/node_modules is missing: run 'cd frontend && npm ci' first" >&2
  exit 1
fi

# Refuse to start onto a port somebody else is already serving. The seeding replaces the sample
# collections by name, so running it against a server the reader was using would delete their work.
for port in 8080 4200; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
    echo "port $port is already in use; stop what is listening there and run this again" >&2
    exit 1
  fi
done

cleanup() {
  # Each server was started in its own process group, so stopping the group takes its
  # children (cargo's binary, esbuild, vite) with it.
  for pid in "${backend_pid:-}" "${frontend_pid:-}"; do
    [ -n "$pid" ] && kill -- "-$pid" 2>/dev/null || true
  done
  rm -rf "$data_root"
  echo "== the demo stopped, and its content with it =="
}
trap cleanup EXIT

echo "== starting the backend on 127.0.0.1:8080 (data in $data_root) =="
setsid env DATA_ROOT="$data_root" JWT_SECRET="$jwt_secret" PREVIEW_SITE_URL=http://localhost:4200 \
  SITE_NAME="$site_name" ADMIN_USERNAME="$admin_username" ADMIN_PASSWORD="$admin_password" \
  cargo run --manifest-path "$root/backend/Cargo.toml" >"$backend_log" 2>&1 &
backend_pid=$!

echo "== starting the dev server on localhost:4200 =="
setsid npx --prefix "$frontend" ng serve --port 4200 >"$frontend_log" 2>&1 &
frontend_pid=$!

wait_for() {
  local url="$1" name="$2" tries="$3" log="$4"
  for _ in $(seq 1 "$tries"); do
    if curl -fsS -o /dev/null "$url" 2>/dev/null; then
      echo "$name is up"
      return 0
    fi
    sleep 2
  done
  echo "$name did not come up; the last lines of its log:" >&2
  tail -n 20 "$log" >&2
  return 1
}

echo "== the backend is building if this is the first run; a few minutes is normal =="
wait_for http://127.0.0.1:8080/ backend 300 "$backend_log"
wait_for http://localhost:4200/ frontend 120 "$frontend_log"

echo "== writing the sample site in =="
cd "$frontend"
BASE_URL=http://localhost:4200 ADMIN_USERNAME="$admin_username" ADMIN_PASSWORD="$admin_password" \
  node e2e/sample-site.mjs

cat <<EOF

== the demo is running ==

  Admin screen   http://localhost:4200
  Sign in        $admin_username / $admin_password

  Content API    curl -s http://127.0.0.1:8080/api/content/collections/articles
  Logs           $backend_log
                 $frontend_log

Press Ctrl-C to stop it. The content lives in $data_root, and goes with the demo.
EOF

# Hold the terminal here. Ctrl-C ends the script and the trap stops the servers; so does either
# server dying, which is worth a line rather than a bare prompt coming back.
wait || true
echo "== a server stopped; see $backend_log and $frontend_log =="
