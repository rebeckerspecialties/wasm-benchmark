#!/usr/bin/env bash
# Build the iOS benchmark app against a local tinywasm revision, for A/Bs of
# tinywasm changes (scripts/tinywasm-ab-iphone.sh runs them).
#
# Usage: scripts/tinywasm-ab-build-ios.sh <name> <tinywasm-worktree> <ref> [patch...]
#   Checks out <ref> (detached) in <tinywasm-worktree>, applies the patches,
#   builds benchmark-core for aarch64-apple-ios with tinywasm patched to that
#   checkout (same flags as scripts/build-lib.sh), and builds the app into
#   apps/build/DerivedData-tw-<name>. The default iOS lib and Cargo.lock are
#   restored afterwards, and the worktree is left clean at <ref>.
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

NIGHTLY_TC="$(grep -m1 '^NIGHTLY_TC=' scripts/build-lib.sh | cut -d'"' -f2)"
export PATH="${HOME}/.rustup/toolchains/${NIGHTLY_TC}-aarch64-apple-darwin/bin:${PATH}"
export RUSTFLAGS="-C target-cpu=apple-a12 --cfg=pulley_tail_calls"
export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
LIB=target/aarch64-apple-ios/release/libbenchmark_core.a
SAVED=target/tw-exp/libbenchmark_core.default.a
mkdir -p target/tw-exp
restore() {
  git checkout -q -- Cargo.lock
  [ -f "${SAVED}" ] && mv "${SAVED}" "${LIB}"
  ( cd "${wt}" && git checkout -q -- . )
}
trap restore EXIT
[ -f "${LIB}" ] && cp "${LIB}" "${SAVED}"

start=$(date +%s)
cargo build --release -p benchmark-core --lib --features nightly-dispatch --features femtovg-e2e \
  --target aarch64-apple-ios --target-dir target/tw-exp \
  --config "patch.crates-io.tinywasm.path=\"${wt}/crates/tinywasm\"" 2>&1 \
  | grep -E '^error|Compiling tinywasm|Finished' || true
# The patch must have taken: the lock file then points tinywasm at the checkout.
if grep -A2 '^name = "tinywasm"$' Cargo.lock | grep -q '^source'; then
  echo "[${name}] tinywasm still resolves to crates.io: is the checkout's version the one Cargo.toml pins?" >&2
  exit 1
fi
mkdir -p "$(dirname "${LIB}")"
cp target/tw-exp/aarch64-apple-ios/release/libbenchmark_core.a "${LIB}"
if ( cd apps && xcodebuild -project WasmBenchmark.xcodeproj -scheme WasmBenchmarkIOS -configuration Release \
      -destination "generic/platform=iOS" -derivedDataPath "build/DerivedData-tw-${name}" \
      -allowProvisioningUpdates build ) > "target/tw-exp/xcodebuild-${name}.log" 2>&1; then
  echo "[${name}] app built in $(( $(date +%s) - start ))s: apps/build/DerivedData-tw-${name}"
else
  echo "[${name}] xcodebuild failed, see target/tw-exp/xcodebuild-${name}.log" >&2
  exit 1
fi
