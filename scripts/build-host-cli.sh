#!/usr/bin/env bash
# Build the macOS host CLIs (run_matrix, run_dispatch_workloads, ...) with
# the same interpreter dispatch the device libraries use: the pinned nightly
# plus --cfg=pulley_tail_calls (Pulley) and the nightly-dispatch feature
# (tinywasm's tail-call loop). A plain `cargo build` on stable gets the
# slower default `match`-loop dispatch for both, which is what the M4
# numbers used before 2026-09.
#
# Rust-side fat LTO, as in scripts/build-lib.sh (no -C linker-plugin-lto:
# Xcode's libLTO cannot read the LLVM-22 bitcode of these Rust versions).
#
# Usage: scripts/build-host-cli.sh [cargo build args...]
#   default: --bins
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
NIGHTLY_TC="$(grep -m1 '^NIGHTLY_TC=' scripts/build-lib.sh | cut -d'"' -f2)"
BIN="${HOME}/.rustup/toolchains/${NIGHTLY_TC}-aarch64-apple-darwin/bin"
[[ -d "${BIN}" ]] || { echo "missing toolchain ${NIGHTLY_TC}: rustup toolchain install ${NIGHTLY_TC}" >&2; exit 2; }
export PATH="${BIN}:${PATH}"
export RUSTFLAGS="--cfg=pulley_tail_calls -C target-cpu=apple-a12"
# Same optimization as the device libraries (scripts/build-lib.sh).
export CARGO_PROFILE_RELEASE_LTO=fat
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
if [[ $# -eq 0 ]]; then set -- --bins; fi
exec cargo build --release -p benchmark-core --features nightly-dispatch "$@"
