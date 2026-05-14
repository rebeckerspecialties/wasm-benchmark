#!/usr/bin/env bash
# Experiment B: arm64_32-apple-watchos build with cross-language LTO.
#
# Two-stage:
#   stage 1 = minimal:  --features pulley           (no runtime, no std → no cc-rs invocation)
#   stage 2 = realistic: --features pulley,runtime,std (will exercise wasmtime/build.rs's cc::Build path)
#
# Toolchain pin is critical:
#   - rustup `nightly` resolves to a newer nightly that breaks Xcode 26 bitcode compat.
#   - We must pin to nightly-2026-01-25 (rustc 1.95.0-nightly f134bbc78).
#
# Outputs (under out/exp-b/<stage>/):
#   build.log      — full stdout/stderr
#   exit-code.txt
#   artifact.txt   — path/size of the produced staticlib if any

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASMTIME_DIR="${ROOT}/wasmtime"
TOOLCHAIN="${TOOLCHAIN:-nightly-2026-01-25}"
TARGET="${TARGET:-arm64_32-apple-watchos}"
STAGE="${1:-minimal}"

# `rustup run TOOLCHAIN` sets RUSTUP_TOOLCHAIN but doesn't prepend the
# toolchain's bin to PATH. With the user's default toolchain (1.93) earlier
# in PATH, `cargo`/`rustc` lookups inside cargo-spawned subprocesses resolve
# to stable instead of the nightly we activated, and -Z build-std fails with
# "the option Z is only accepted on the nightly compiler". Prepend the
# nightly toolchain's bin directly to make the resolution unambiguous.
TOOLCHAIN_BIN="${HOME}/.rustup/toolchains/${TOOLCHAIN}-$(uname -m | sed 's/x86_64/x86_64/;s/arm64/aarch64/')-apple-darwin/bin"
if [[ ! -d "${TOOLCHAIN_BIN}" ]]; then
  echo "ERROR: toolchain bin not found at ${TOOLCHAIN_BIN}" >&2
  echo "Install with: rustup toolchain install ${TOOLCHAIN}" >&2
  exit 2
fi
export PATH="${TOOLCHAIN_BIN}:${PATH}"

case "${STAGE}" in
  minimal)   FEATURES="pulley" ;;
  realistic) FEATURES="pulley,runtime,std" ;;
  fat-lto)   FEATURES="pulley,runtime,std" ;;
  *) echo "unknown stage: ${STAGE} (use 'minimal', 'realistic', or 'fat-lto')" >&2; exit 2 ;;
esac

OUT="${ROOT}/out/exp-b/${STAGE}"
mkdir -p "${OUT}"

cd "${WASMTIME_DIR}"

if [[ "${STAGE}" == "fat-lto" ]]; then
  export RUSTFLAGS="-C lto=fat -C linker-plugin-lto -C embed-bitcode=yes"
else
  export RUSTFLAGS="-C linker-plugin-lto -C embed-bitcode=yes"
fi

echo "==> Stage: ${STAGE}"
echo "==> Toolchain: ${TOOLCHAIN}"
echo "==> Target: ${TARGET}"
echo "==> Features: ${FEATURES}"
echo "==> RUSTFLAGS: ${RUSTFLAGS}"
echo "==> wasmtime HEAD: $(git rev-parse HEAD)"
echo

set -x
cargo rustc \
  --lib \
  -Z build-std=std,panic_abort \
  --target "${TARGET}" \
  --release \
  --crate-type staticlib \
  -p wasmtime \
  --no-default-features \
  --features "${FEATURES}" \
  2>&1 | tee "${OUT}/build.log"
EC=${PIPESTATUS[0]}
set +x

echo "${EC}" > "${OUT}/exit-code.txt"

ARTIFACT="target/${TARGET}/release/libwasmtime.a"
if [[ -f "${ARTIFACT}" ]]; then
  ls -la "${ARTIFACT}" | tee "${OUT}/artifact.txt"
else
  echo "(no artifact at ${ARTIFACT})" | tee "${OUT}/artifact.txt"
fi

echo
echo "==> Exit code: ${EC}"
exit "${EC}"
