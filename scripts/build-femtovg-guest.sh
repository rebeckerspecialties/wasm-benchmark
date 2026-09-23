#!/usr/bin/env bash
# Build the femtovg E2E guest (workloads-rs-cargo/femtovg-guest) into
#   workloads/femtovg-relaxed.wasm   -C target-feature=+simd128,+relaxed-simd
#   workloads/femtovg-simd128.wasm   -C target-feature=+simd128
#   workloads/femtovg-scalar.wasm    -C target-feature=-simd128,-relaxed-simd
# and the native reference bin (target/release/fvg_reference).
#
# Checks each module: the only imports are the five `fvg` functions (no
# wasm-bindgen / web-sys glue survived from femtovg's wasm32 deps), and
# reports how many SIMD-128 and relaxed-SIMD instructions it contains. The
# guest uses no SIMD intrinsics; only the auto-vectorizer emits v128 ops,
# and rustc never contracts a*b+c into relaxed_madd without fast-math, so
# the relaxed build is expected to be the simd128 build.
#
# Toolchain: the same 1.93.1 as workloads-rs/ (wasm32-unknown-unknown).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOLCHAIN="${TOOLCHAIN:-1.93.1}"
export PATH="${HOME}/.rustup/toolchains/${TOOLCHAIN}-aarch64-apple-darwin/bin:${PATH}"
CRATE="${ROOT}/workloads-rs-cargo/femtovg-guest"
cd "${CRATE}"
features() {
  case "$1" in
    relaxed) echo "+simd128,+relaxed-simd" ;;
    simd128) echo "+simd128,-relaxed-simd" ;;
    scalar) echo "-simd128,-relaxed-simd" ;;
  esac
}
for v in relaxed simd128 scalar; do
  RUSTFLAGS="-C target-feature=$(features "${v}")" \
    cargo build --release --lib --target wasm32-unknown-unknown --target-dir "${CRATE}/target/${v}"
  out="${ROOT}/workloads/femtovg-${v}.wasm"
  cp "${CRATE}/target/${v}/wasm32-unknown-unknown/release/femtovg_guest.wasm" "${out}"
  printed="$(wasm-tools print "${out}")"
  imports="$(grep -oE '\(import "[^"]*" "[^"]*"' <<< "${printed}" | sed 's/(import //' | sort -u)"
  if grep -qv '^"fvg" ' <<< "${imports}"; then
    echo "FAIL ${out}: unexpected imports:" >&2
    grep -v '^"fvg" ' <<< "${imports}" >&2
    exit 1
  fi
  v128=$(grep -cE '\b(v128|[if](8x16|16x8|32x4|64x2))\.' <<< "${printed}" || true)
  relaxed=$(grep -cE '\.relaxed_' <<< "${printed}" || true)
  echo "==> ${out}: $(wc -c < "${out}" | tr -d ' ') bytes, imports: $(tr '\n' ' ' <<< "${imports}")"
  echo "    SIMD-128 instructions: ${v128}; relaxed-SIMD instructions: ${relaxed}"
done
cargo build --release --bin fvg_reference --target-dir "${CRATE}/target/native"
cp "${CRATE}/target/native/release/fvg_reference" "${ROOT}/target/release/fvg_reference" 2>/dev/null \
  || { mkdir -p "${ROOT}/target/release"; cp "${CRATE}/target/native/release/fvg_reference" "${ROOT}/target/release/"; }
