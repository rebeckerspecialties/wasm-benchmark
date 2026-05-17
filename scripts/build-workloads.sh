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

  # Canonical feature set: every workload is compiled with the full
  # wasm-3.0-ish proposal stack. Runtimes that can't handle a given
  # feature surface ERROR rows in the harness — that's the
  # cross-runtime signal we want, not a per-workload neutering.
  # (Earlier this script gated `+simd128` to matmul-only so that
  # wasm3 wouldn't choke on auto-vectorizer-emitted v128 locals;
  # patches/wasm3/0001-wasm3-accept-v128-as-opaque-slot.patch makes
  # wasm3 accept those slots without executing SIMD ops, so we can
  # restore the canonical `+simd128 +relaxed-simd` for every
  # workload without re-introducing the iPhone-side wasmz SIGBUS.)
  #
  # `-C panic=abort` avoids the unwinding personality function.
  rustc \
    --target wasm32-unknown-unknown \
    --edition 2024 \
    --crate-type cdylib \
    --crate-name "${name}" \
    -C opt-level=3 \
    -C lto=fat \
    -C panic=abort \
    -C target-feature=+simd128,+relaxed-simd,+tail-call,+bulk-memory,+multivalue,+reference-types \
    "${src}" \
    -o "${out}"
  size=$(wc -c < "${out}" | tr -d ' ')
  echo "    -> ${out} (${size} bytes)"
done

echo
echo "All workloads built. ${OUT}:"
ls -la "${OUT}"/*.wasm
