#!/usr/bin/env bash
#
# The Rust test suite.
#
# The core, the local adapter and the contract suite against it need nothing running. The AWS
# adapter's own tests and the contract suite against it need DynamoDB Local and an S3 gateway
# (`docker compose -f backend/docker-compose.yml up -d`); when they are not there this script
# says so and skips them rather than failing, and CI starts them as service containers instead.
#
# Usage: scripts/test-rust.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/backend"

# Four tests at a time, not one per core.
#
# Every contract test builds a whole CMS and the password ones also hash a password with
# Argon2id, which is memory-hard on purpose (19 MiB a hash, and the allocator holds on to what
# it used): about 35 MiB of resident memory per test that is running, so the default of one
# thread per core reached 1.2 GiB on a 32-core machine for a suite that takes 4 seconds. Four
# threads hold it at about 230 MiB, with the suite going from 4 seconds to 15 - a fair trade
# for a suite that is not what anyone waits on. `RUST_TEST_THREADS=16 scripts/test-rust.sh`
# (or any other number) overrides it, and the build keeps its own parallelism either way.
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-4}"

echo "== core, the local adapter, and the contract suite against it =="
cargo test -p tsubame-core -p tsubame-on-premises
cargo test -p tsubame-tests --test on_premises

# Both binaries have to link: there are two now, and the one nothing runs is the one that rots.
echo "== both binaries build =="
cargo build -p tsubame-on-premises -p tsubame-aws

# A TCP connect is enough: the question is only whether anything is listening.
listening() {
  (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
}

if listening 8000 && listening 9000; then
  echo "== the AWS adapter and the contract suite against it =="
  cargo test -p tsubame-aws
  cargo test -p tsubame-tests --test aws
else
  echo "skipped: nothing listening on 8000 (DynamoDB Local) / 9000 (S3 emulator)"
  echo "         start them with: docker compose -f backend/docker-compose.yml up -d"
fi
