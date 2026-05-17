#!/usr/bin/env bash
# Build each `workloads-rs/*.rs` file into a standalone `workloads/*.wasm`
# via rustc. No cargo, no [[lib]] config — each .rs file is a self-
# contained `cdylib` with its own panic handler.
#
# These .wasm files are checked into the repo (per the project brief) so
# the watchOS / iOS / macOS apps don't need a wasm toolchain at app-build
# time; benchmark-core's build.rs simply `include_bytes!`'s them.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${ROOT}/workloads-rs"
OUT="${ROOT}/workloads"

# Use the same stable toolchain as the host benchmark-core crate; wasm32
# is Tier-1, no build-std needed.
TOOLCHAIN="${TOOLCHAIN:-1.93.1}"
TOOLCHAIN_BIN="${HOME}/.rustup/toolchains/${TOOLCHAIN}-aarch64-apple-darwin/bin"
export PATH="${TOOLCHAIN_BIN}:${PATH}"

mkdir -p "${OUT}"

for src in "${SRC}"/*.rs; do
  name=$(basename "${src}" .rs)
  out="${OUT}/${name}.wasm"
  echo "==> ${name}.wasm"

  # SIMD policy:
  #  - matmul_simd / matmul_fma INTENTIONALLY use v128 intrinsics
  #    (`v128_load`, `f32x4_*`, `f32x4_relaxed_madd`, ...) — keep
  #    `+simd128,+relaxed-simd` for them.
  #  - Every other workload uses only scalar math, BUT with `+simd128`
  #    the Rust compiler's auto-vectorizer freely emits v128 LOCAL
  #    slots even when no v128 value is ever read. wasm3 0.5.1 rejects
  #    these modules at parse time ("unknown value_type"), and wasmz
  #    silently fails the call without surfacing a trap (leaves the
  #    runtime in a state that SIGBUSes the next call on iOS — verified
  #    on iPhone 12 A14, 2026-05-16). Compile non-SIMD workloads with
  #    `-simd128 -relaxed-simd` so the auto-vectorizer can't emit
  #    v128 slots and both interpreters can run them cleanly.
  case "${name}" in
    matmul_simd|matmul_fma)
      SIMD_FEATS="+simd128,+relaxed-simd"
      ;;
    *)
      SIMD_FEATS="-simd128,-relaxed-simd"
      ;;
  esac

  # `-C panic=abort` avoids the unwinding personality function.
  rustc \
    --target wasm32-unknown-unknown \
    --edition 2024 \
    --crate-type cdylib \
    --crate-name "${name}" \
    -C opt-level=3 \
    -C lto=fat \
    -C panic=abort \
    -C target-feature="${SIMD_FEATS},+tail-call,+bulk-memory,+multivalue,+reference-types" \
    "${src}" \
    -o "${out}"
  size=$(wc -c < "${out}" | tr -d ' ')
  echo "    -> ${out} (${size} bytes)"
done

echo
echo "All workloads built. ${OUT}:"
ls -la "${OUT}"/*.wasm
