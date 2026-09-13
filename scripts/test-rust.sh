#!/usr/bin/env bash
#
# The Rust test suite.
#
# The core, the local adapter and the contract suite against it need nothing running. The AWS
# adapter's own tests and the contract suite against it need DynamoDB Local and MinIO
# (`docker compose -f sl_cms/docker-compose.yml up -d`); when they are not there this script
# says so and skips them rather than failing, and CI starts them as service containers instead.
#
# Usage: scripts/test-rust.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/sl_cms"

echo "== core, the local adapter, and the contract suite against it =="
cargo test -p sl-cms-core -p sl-cms-on-premises
cargo test -p sl-cms-tests --test on_premises

# Both binaries have to link: there are two now, and the one nothing runs is the one that rots.
echo "== both binaries build =="
cargo build -p sl-cms-on-premises -p sl-cms-aws

# A TCP connect is enough: the question is only whether anything is listening.
listening() {
  (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

if listening 8000 && listening 9000; then
  echo "== the AWS adapter and the contract suite against it =="
  cargo test -p sl-cms-aws
  cargo test -p sl-cms-tests --test aws
else
  echo "skipped: nothing listening on 8000 (DynamoDB Local) / 9000 (MinIO)"
  echo "         start them with: docker compose -f sl_cms/docker-compose.yml up -d"
fi
