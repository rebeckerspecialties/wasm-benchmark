#!/usr/bin/env bash
# Build the component-model async / WASI 0.3 benchmark component
# (workloads-rs-cargo/cm-async-bench) into workloads/cm_async_bench.wasm.
#
# The crate is `#![no_std]` so Rust's std adds no WASI 0.2.6 imports, and
# the core module is componentized with `wasm-tools component new` instead
# of rustc's wasm-component-ld step: wasm-component-ld unifies the 0.2.4
# imports of the `wasip2` crate up to its own 0.2.6 WIT, and zwasm's
# component linker resolves 0.2.4 but not 0.2.6 (UnknownImport).
#
# Toolchain: the same 1.93.1 that builds workloads-rs/ (plus the
# wasm32-wasip2 std: `rustup target add wasm32-wasip2 --toolchain 1.93.1`).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOLCHAIN="${TOOLCHAIN:-1.93.1}"
export PATH="${HOME}/.rustup/toolchains/${TOOLCHAIN}-aarch64-apple-darwin/bin:${PATH}"
CRATE="${ROOT}/workloads-rs-cargo/cm-async-bench"
cd "${CRATE}"
RUSTFLAGS="-C link-arg=--skip-wit-component" \
  cargo build --release --target wasm32-wasip2 --target-dir "${CRATE}/target"
CORE="${CRATE}/target/wasm32-wasip2/release/cm_async_bench.wasm"
OUT="${ROOT}/workloads/cm_async_bench.wasm"
wasm-tools component new "${CORE}" -o "${OUT}"
wasm-tools validate --features all "${OUT}"
echo "==> ${OUT} ($(wc -c < "${OUT}" | tr -d ' ') bytes)"
wasm-tools component wit "${OUT}" | sed -n '/^world/,/^}/p'
# The async-lifted run export and the async-lowered host call must be there.
wasm-tools print "${OUT}" | grep -q '\[callback\]\[async-lift\]wasi:cli/run@0.3.0#run'
wasm-tools print "${OUT}" | grep -q '\[async-lower\]wait-for'
