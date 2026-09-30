#!/usr/bin/env bash
# Build the iOS (or tvOS) benchmark app against a local tinywasm revision, for
# A/Bs of tinywasm changes (scripts/tinywasm-ab-iphone.sh runs them).
#
# Usage: scripts/tinywasm-ab-build-ios.sh <name> <tinywasm-worktree> <ref> [patch...]
#   Checks out <ref> (detached) in <tinywasm-worktree>, applies the patches,
#   builds benchmark-core for aarch64-apple-ios with tinywasm patched to that
#   checkout (same flags as scripts/build-lib.sh), and builds the app into
#   apps/build/DerivedData-tw-<name>. The default iOS lib and Cargo.lock are
#   restored afterwards, and the worktree is left clean at <ref>.
#
#   TW_AB_TARGET_DIR=target/tw-exp  cargo target dir. `target` reuses the
#   default build's dependencies (about 2 GB less disk); the default iOS
#   build then recompiles tinywasm and benchmark-core once.
#   PLATFORM=ios  or tvos: the Apple TV app, into
#   apps/build/DerivedData-tw-<name>-tvos (it links the other runtimes'
#   tvOS libraries, so build those first); or watchos: the watch app with the
#   variant in its arm64_32 slice, into apps/build/DerivedData-tw-<name>-watchos.
#   TW_AB_CPU="-C target-cpu=apple-a12"  codegen flags (tvOS default: the
#   target's own baseline), e.g. "-C target-cpu=apple-a10 -Z tune-cpu=apple-a14".
#
# Use a dedicated worktree (git worktree add --detach <dir> next): the script
# discards uncommitted changes in it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
name="${1:?usage: tinywasm-ab-build-ios.sh <name> <tinywasm-worktree> <ref> [patch...]}"
wt="$(cd "${2:?tinywasm worktree}" && pwd)"
ref="${3:?git ref}"
shift 3
patches=()
for p in "$@"; do
  patches+=("$(cd "$(dirname "$p")" && pwd)/$(basename "$p")")
done

cd "${ROOT}"
if ! git diff --quiet -- Cargo.lock; then
  echo "Cargo.lock has uncommitted changes; commit or stash them first" >&2
  exit 1
fi
( cd "${wt}" && git checkout -q -- . && git checkout -q --detach "${ref}" \
    && for p in ${patches[@]+"${patches[@]}"}; do git apply "$p"; done \
    && echo "[${name}] tinywasm $(git log --oneline -1 | cut -c1-70) $(git diff --shortstat)" )

PLATFORM="${PLATFORM:-ios}"
case "${PLATFORM}" in
  ios)  TRIPLE=aarch64-apple-ios; SCHEME=WasmBenchmarkIOS; DEST="generic/platform=iOS"
        DD="build/DerivedData-tw-${name}"; FEATS="--features nightly-dispatch --features femtovg-e2e"
        BUILD_STD=(); CPU="${TW_AB_CPU:--C target-cpu=apple-a12}" ;;
  tvos) TRIPLE=aarch64-apple-tvos; SCHEME=WasmBenchmarkTV; DEST="generic/platform=tvOS"
        DD="build/DerivedData-tw-${name}-tvos"; FEATS="--features nightly-dispatch"
        BUILD_STD=(-Z build-std=std,panic_abort); CPU="${TW_AB_CPU:-}" ;;
  # The variant goes into the arm64_32 slice (Series 4-8 and SE). The scheme also builds the
  # iOS app, so ARCHS cannot be narrowed; the arm64 slice links the default library.
  watchos) TRIPLE=arm64_32-apple-watchos; SCHEME=WasmBenchmarkWatch; DEST="generic/platform=watchOS"
        DD="build/DerivedData-tw-${name}-watchos"; FEATS="--features nightly-dispatch"
        BUILD_STD=(-Z build-std=std,panic_abort); CPU="${TW_AB_CPU:--C target-cpu=apple-a12}" ;;
  *) echo "PLATFORM must be ios, tvos or watchos" >&2; exit 1 ;;
esac
NIGHTLY_TC="$(grep -m1 '^NIGHTLY_TC=' scripts/build-lib.sh | cut -d'"' -f2)"
export PATH="${HOME}/.rustup/toolchains/${NIGHTLY_TC}-aarch64-apple-darwin/bin:${PATH}"
export RUSTFLAGS="${CPU} --cfg=pulley_tail_calls"
export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
export IPHONEOS_DEPLOYMENT_TARGET=17.0 TVOS_DEPLOYMENT_TARGET=18.0 WATCHOS_DEPLOYMENT_TARGET=11.0
TARGET_DIR="${TW_AB_TARGET_DIR:-target/tw-exp}"
LIB="target/${TRIPLE}/release/libbenchmark_core.a"
SAVED="target/tw-exp/libbenchmark_core.${TRIPLE}.default.a"
mkdir -p target/tw-exp
had_default=0
if [ -f "${LIB}" ]; then cp "${LIB}" "${SAVED}"; had_default=1; fi
restore() {
  git checkout -q -- Cargo.lock
  if [ "${had_default}" = 1 ]; then mv "${SAVED}" "${LIB}"; else rm -f "${LIB}"; fi
  ( cd "${wt}" && git checkout -q -- . )
}
trap restore EXIT

start=$(date +%s)
log="target/tw-exp/cargo-${name}-${PLATFORM}.log"
if ! cargo build --release -p benchmark-core --lib ${FEATS} ${BUILD_STD[@]+"${BUILD_STD[@]}"} \
    --target "${TRIPLE}" --target-dir "${TARGET_DIR}" \
    --config "patch.\"https://github.com/explodingcamera/tinywasm\".tinywasm.path=\"${wt}/crates/tinywasm\"" \
    > "${log}" 2>&1; then
  grep -E '^error' -A5 "${log}" >&2
  echo "[${name}] cargo build failed, see ${log}" >&2
  exit 1
fi
grep -E 'Compiling tinywasm|Finished' "${log}" || true
# The patch must have taken: the lock file then points tinywasm at the checkout.
if grep -A2 '^name = "tinywasm"$' Cargo.lock | grep -q '^source'; then
  echo "[${name}] tinywasm still resolves to its pinned source: is the checkout's version the one Cargo.toml pins?" >&2
  exit 1
fi
mkdir -p "$(dirname "${LIB}")"
built="${TARGET_DIR}/${TRIPLE}/release/libbenchmark_core.a"
[ "${built}" -ef "${LIB}" ] || cp "${built}" "${LIB}"
if ( cd apps && xcodebuild -project WasmBenchmark.xcodeproj -scheme "${SCHEME}" -configuration Release \
      -destination "${DEST}" -derivedDataPath "${DD}" \
      -allowProvisioningUpdates build ) > "target/tw-exp/xcodebuild-${name}-${PLATFORM}.log" 2>&1; then
  echo "[${name}] app built in $(( $(date +%s) - start ))s: apps/${DD}"
else
  echo "[${name}] xcodebuild failed, see target/tw-exp/xcodebuild-${name}-${PLATFORM}.log" >&2
  exit 1
fi
