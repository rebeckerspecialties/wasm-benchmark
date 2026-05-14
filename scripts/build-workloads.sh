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
  # `+simd128` enables vanilla wasm SIMD (v128). `+relaxed-simd` enables
  # the relaxed-simd proposal — gives us access to `f32x4_relaxed_madd`
  # etc., which Pulley lowers to its `Vfma32x4`/`Vfma64x2` bytecodes.
  # Existing workloads call only explicit simd128 intrinsics, so adding
  # `+relaxed-simd` doesn't auto-rewrite them; new workloads can opt in.
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
