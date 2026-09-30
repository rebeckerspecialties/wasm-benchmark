#!/usr/bin/env bash
# Builds the iOS benchmark app with benchmark-core's `tos-model` feature: the tos-bench variants
# become `tos-bench <variant> (m=<copies>)` cases, run under any engine. The app goes to
# apps/build/DerivedData-tw-tosb, so scripts/tinywasm-ab-iphone.sh can run it as variant `tosb`:
#
#   scripts/tos-bench/build-ios.sh
#   REPS=5 WORKLOADS="tos-bench" scripts/tinywasm-ab-iphone.sh <out> tosb
#
# Same flags as scripts/build-lib.sh plus -Z merge-functions=disabled, which keeps the model's
# identical handler copies apart. The target dir is its own (target/tos-model), and the default
# iOS lib is restored afterwards.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "${ROOT}"
python3 scripts/tos-bench/gen.py > /dev/null
NIGHTLY_TC="$(grep -m1 '^NIGHTLY_TC=' scripts/build-lib.sh | cut -d'"' -f2)"
export PATH="${HOME}/.rustup/toolchains/${NIGHTLY_TC}-aarch64-apple-darwin/bin:${PATH}"
# shellcheck source=../cpu-flags.sh
source scripts/cpu-flags.sh
export RUSTFLAGS="${RUST_CPU_IOS} --cfg=pulley_tail_calls -Z merge-functions=disabled"
export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
TARGET_DIR=target/tos-model
LIB=target/aarch64-apple-ios/release/libbenchmark_core.a
SAVED="${TARGET_DIR}/libbenchmark_core.default.a"
mkdir -p "${TARGET_DIR}"
# Put the default library back afterwards, or remove the model's if there was none, so later
# app builds never link the model by accident.
had_default=0
if [ -f "${LIB}" ]; then cp "${LIB}" "${SAVED}"; had_default=1; fi
restore() {
  if [ "${had_default}" = 1 ]; then mv "${SAVED}" "${LIB}"; else rm -f "${LIB}"; fi
}
trap restore EXIT

start=$(date +%s)
if ! cargo build --release -p benchmark-core --lib --features nightly-dispatch,femtovg-e2e,tos-model \
    --target aarch64-apple-ios --target-dir "${TARGET_DIR}" > "${TARGET_DIR}/cargo-tosb.log" 2>&1; then
  grep -E '^error' -A5 "${TARGET_DIR}/cargo-tosb.log" >&2
  echo "[tosb] cargo build failed, see ${TARGET_DIR}/cargo-tosb.log" >&2
  exit 1
fi
built="${TARGET_DIR}/aarch64-apple-ios/release/libbenchmark_core.a"
mkdir -p "$(dirname "${LIB}")"
cp "${built}" "${LIB}"
if ( cd apps && xcodebuild -project WasmBenchmark.xcodeproj -scheme WasmBenchmarkIOS -configuration Release \
      -destination "generic/platform=iOS" -derivedDataPath build/DerivedData-tw-tosb \
      -allowProvisioningUpdates build ) > "${TARGET_DIR}/xcodebuild-tosb.log" 2>&1; then
  echo "[tosb] app built in $(( $(date +%s) - start ))s: apps/build/DerivedData-tw-tosb"
else
  echo "[tosb] xcodebuild failed, see ${TARGET_DIR}/xcodebuild-tosb.log" >&2
  exit 1
fi
