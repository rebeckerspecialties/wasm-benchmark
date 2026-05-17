#!/usr/bin/env bash
# First-time setup for a fresh wasm-benchmark clone.
# Idempotent — safe to run repeatedly.
#
# Does:
#   1. Init submodules. All runtime sources live as submodules now —
#      target-lexicon, mach2, wasmtime, wasm-micro-runtime, wasm3,
#      WasmEdge, wasmz, zwasm, porffor, sightglass — each pinned per
#      .gitmodules to a specific SHA. CI runs `git submodule update
#      --init --recursive` and reproduces the source state exactly.
#
#      Three submodules point at rebeckerspecialties forks because the
#      patches haven't landed upstream yet:
#        - target-lexicon (arm64_32-apple-watchos branch)
#        - mach2 (arm64_32-apple-watchos branch)
#        - wasmtime (accurate-graphql-needs-legacy-exceptions branch
#          — stacks Pulley fusion phases 1-4 + the
#          wasm_legacy_exceptions feature-known fix)
#
#      The rest point at upstream pinned SHAs with our patches applied
#      at build time from `patches/<runtime>/` via
#      `scripts/apply_patch_series.sh` (called by each
#      `scripts/build-<runtime>.sh`). The build scripts are idempotent
#      — they reset the submodule to its pinned HEAD before applying.
#
#   2. Verify rustup toolchains. Pinned:
#        * 1.94.1            — default stable
#        * nightly-2026-01-25 — for arm64_32 build-std + nightly
#                              `become` Pulley dispatch
#
#   3. Verify rustup targets. Adds them if missing.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

echo "==> 1. Init submodules"
git submodule update --init --recursive

echo
echo "==> 2. Verify rustup toolchains"
need_install=()
for tc in 1.94.1 nightly-2026-01-25; do
  if rustup toolchain list 2>/dev/null | grep -q "^${tc}-"; then
    echo "    ${tc} present"
  else
    echo "    ${tc} MISSING"
    need_install+=("${tc}")
  fi
done
if (( ${#need_install[@]} > 0 )); then
  echo
  echo "    Install with:"
  for tc in "${need_install[@]}"; do
    echo "      rustup toolchain install ${tc}"
  done
  exit 1
fi

echo
echo "==> 3. Verify rustup targets"
TARGETS=(
  aarch64-apple-darwin
  aarch64-apple-ios
  aarch64-apple-ios-sim
  wasm32-unknown-unknown
  wasm32-wasip1
)
for t in "${TARGETS[@]}"; do
  if rustup target list --installed 2>/dev/null | grep -qx "${t}"; then
    echo "    ${t} present"
  else
    echo "    ${t} adding..."
    rustup target add "${t}"
  fi
done

# Tier-3 watchOS targets are added against the pinned nightly via
# build-std; nothing to install via rustup target.

echo
echo "==> Setup complete. Next:"
echo "    ./scripts/build-workloads.sh           # builds workloads/*.wasm"
echo "    ./scripts/build-wamr.sh macos          # builds WAMR fast-interp .a"
echo "    ./scripts/build-wasm3.sh macos         # builds wasm3 .a"
echo "    ./scripts/build-wasmedge.sh macos      # builds WasmEdge .a (slow)"
echo "    ./scripts/build-wasmz.sh macos         # builds wasmz .a"
echo "    ./scripts/build-zwasm.sh macos         # builds zwasm .a"
echo "    ./scripts/build-lib.sh macos           # builds benchmark-core .a"
echo "    cargo build --release -p benchmark-core"
echo
echo "    The build scripts above each call apply_patch_series.sh to"
echo "    apply their patches/<runtime>/ stack to the submodule before"
echo "    the upstream build. The scripts are idempotent — they reset"
echo "    the submodule to its pinned HEAD first."
