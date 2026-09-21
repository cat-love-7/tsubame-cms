#!/usr/bin/env bash
#
# Build the AWS backend as a Lambda artifact: a `bootstrap` binary in a zip, which is what
# `provided.al2023` runs.
#
# The default is **arm64** (Graviton), which is what `infra` deploys: cheaper per GB-second than
# x86_64, and a little faster for this kind of work. The artifact is named after the architecture
# so the two cannot be mixed up: a function whose `architectures` and whose bytes disagree fails
# when it is invoked, not when it is deployed.
#
# Cross-compiling from x86_64 needs a toolchain for the target. `ring` (the TLS provider this
# workspace chose over `aws-lc-rs`, see `sl_cms/Cargo.toml`) compiles C, so a cross *C compiler* is
# required, not only a linker. Either of these does it:
#
#   # zig, which brings its own libc and linker (nothing to install system-wide)
#   cargo install cargo-zigbuild && scripts/build-lambda.sh --zig
#
#   # the distribution's cross toolchain
#   apt-get install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross
#   rustup target add aarch64-unknown-linux-gnu
#
# `cargo lambda build --arm64` is a third way, with its own toolchain.
#
# Usage: scripts/build-lambda.sh [--arch arm64|x86_64] [--zig] [output-zip]
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
arch="arm64"
cross_cc=""
use_zig=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      arch="${2:-}"
      shift 2
      ;;
    --arch=*)
      arch="${1#--arch=}"
      shift
      ;;
    --zig)
      use_zig=1
      shift
      ;;
    *)
      break
      ;;
  esac
done

case "$arch" in
  arm64)
    target="aarch64-unknown-linux-gnu"
    cross_cc="${CC_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-gcc}"
    ;;
  x86_64)
    target="x86_64-unknown-linux-gnu"
    ;;
  *)
    echo "unknown architecture '$arch' (expected arm64 or x86_64)" >&2
    exit 2
    ;;
esac

output="${1:-$root/infra/build/sl-cms-aws-$arch.zip}"

# What the C build has to be told to use the cross tools rather than the host ones. Without this,
# cc-rs calls the host `as`, and the build dies with "as: unrecognized option '-EL'". Zig brings its
# own compiler and linker, so it must be left to set these itself.
if [[ -z "$use_zig" && "$arch" == "arm64" && "$(uname -m)" != "aarch64" ]]; then
  export CC_aarch64_unknown_linux_gnu="$cross_cc"
  export AR_aarch64_unknown_linux_gnu="${AR_aarch64_unknown_linux_gnu:-aarch64-linux-gnu-ar}"
  export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:-$cross_cc}"
fi

# Fail with the commands that fix it, rather than with a compiler error halfway through.
host_arch="$(uname -m)"
case "$host_arch" in
  x86_64|amd64) host_arch="x86_64" ;;
  aarch64|arm64) host_arch="arm64" ;;
esac
if [[ "$arch" != "$host_arch" ]] && [[ -z "$use_zig" ]]; then
  if [[ -n "$cross_cc" ]] && ! command -v "$cross_cc" >/dev/null 2>&1; then
    echo "cross-compiling for $arch needs '$cross_cc', which is not installed." >&2
    echo "  apt-get install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross" >&2
    exit 1
  fi
  if command -v rustup >/dev/null 2>&1 && ! rustup target list --installed | grep -qx "$target"; then
    echo "the Rust standard library for $target is not installed." >&2
    echo "  rustup target add $target" >&2
    exit 1
  fi
  # A cross compiler can be present and still be unusable - it calls the host assembler when the
  # target's binutils are missing, and the failure then surfaces deep inside a C dependency. One
  # trivial compile says whether the toolchain works.
  if [[ -n "$cross_cc" ]]; then
    probe="$(mktemp -d /tmp/sl-cms-cross-XXXXXX)"
    printf 'int main(void){return 0;}\n' > "$probe/probe.c"
    if ! "$cross_cc" "$probe/probe.c" -o "$probe/probe" >/dev/null 2>&1; then
      echo "'$cross_cc' cannot build for $arch (the target's assembler or linker is missing)." >&2
      echo "  apt-get install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu libc6-dev-arm64-cross" >&2
      rm -rf "$probe"
      exit 1
    fi
    rm -rf "$probe"
  fi
fi

if [[ -n "$use_zig" ]]; then
  for tool in cargo-zigbuild zig; do
    if ! command -v "$tool" >/dev/null 2>&1; then
      echo "--zig needs '$tool' on PATH (cargo install cargo-zigbuild, and a zig release)." >&2
      exit 1
    fi
  done
fi

echo "== building the aws backend for Lambda ($arch) =="
if [[ -n "$use_zig" ]]; then
  # zig is both the C compiler and the linker, and carries its own libc: nothing has to be
  # installed for the target, and the glibc it links against is old enough for the runtime.
  cargo zigbuild --manifest-path "$root/sl_cms/Cargo.toml" \
    --package sl-cms-aws --bin sl-cms-aws --release --target "$target"
else
  cargo build --manifest-path "$root/sl_cms/Cargo.toml" \
    --package sl-cms-aws --bin sl-cms-aws --release --target "$target"
fi

# Where cargo actually wrote it. `CARGO_TARGET_DIR` moves the artifact, and a caller who set it (a
# CI job sharing a cache, a container with a small workspace) would otherwise get the copy left in
# the default place - an older binary, which deploys code that looks deployed and is not.
target_root="${CARGO_TARGET_DIR:-$root/sl_cms/target}"
binary="$target_root/$target/release/sl-cms-aws"

# The artifact and the function's `architectures` must agree, and a mismatch is invisible until the
# function is invoked. Checking here costs nothing when `file` is available.
if command -v file >/dev/null 2>&1; then
  description="$(file -b "$binary")"
  case "$arch:$description" in
    arm64:*aarch64*|x86_64:*x86-64*) echo "-- $description" ;;
    *)
      echo "the binary is not $arch: $description" >&2
      exit 1
      ;;
  esac
fi

staging="$(mktemp -d /tmp/sl-cms-lambda-XXXXXX)"
trap 'rm -rf "$staging"' EXIT

# Lambda hands the process to `bootstrap`; nothing else about the name matters.
cp "$binary" "$staging/bootstrap"

mkdir -p "$(dirname "$output")"
rm -f "$output"
(cd "$staging" && zip -q -X "$output" bootstrap)

echo "wrote $output ($(du -h "$output" | cut -f1)) for $arch"
echo "not verified here: it has never been invoked by Lambda. That is what staging is for."
