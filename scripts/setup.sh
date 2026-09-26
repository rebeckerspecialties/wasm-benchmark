#!/usr/bin/env bash
# First-time setup for a fresh wasm-benchmark clone.
# Idempotent — safe to run repeatedly.
#
# Does:
#   1. Init submodules. All runtime sources live as submodules —
#      target-lexicon, mach2, wasmtime, wasm-micro-runtime, wasm3,
#      WasmEdge, wasmz, zwasm, femtovg, sightglass — each
#      pinned per .gitmodules to a specific SHA. CI runs `git submodule
#      update --init --recursive` and reproduces the source state
#      exactly. (tinywasm is a crates.io dependency.)
#
#      Four submodules point at rebeckerspecialties forks:
#        - target-lexicon, mach2 (arm64_32-apple-watchos branches)
#        - wasmtime (pulley-bench-stack-v49: v49.0.0 + the Pulley stack)
#        - femtovg (wire-renderer: the E2E's Renderer wire stream; a
#          path dependency of benchmark-core, so it must be initialized
#          even though the femtovg-e2e feature is optional)
#
#      The rest point at upstream pinned SHAs with our patches applied
#      at build time from `patches/<runtime>/` via
#      `scripts/apply_patch_series.sh` (called by each
#      `scripts/build-<runtime>.sh`). The build scripts are idempotent
#      — they reset the submodule to its pinned gitlink before applying.
#
#   2. Verify rustup toolchains. Pinned:
#        * nightly-2026-07-05 — every harness build (build-lib.sh,
#                               build-host-cli.sh): Tier-3 build-std,
#                               --cfg=pulley_tail_calls, tinywasm's
#                               nightly-tail-calls
#        * 1.93.1             — the wasm32 guest builds
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
for tc in nightly-2026-07-05 1.93.1; do
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
    echo "      rustup toolchain install ${tc} --component rust-src"
  done
  exit 1
fi

echo
echo "==> 3. Verify rustup targets"
add_targets() {  # toolchain targets...
  local tc="$1"; shift
  for t in "$@"; do
    if rustup target list --toolchain "${tc}" --installed 2>/dev/null | grep -qx "${t}"; then
      echo "    ${tc}: ${t} present"
    else
      echo "    ${tc}: ${t} adding..."
      rustup target add --toolchain "${tc}" "${t}"
    fi
  done
}
add_targets nightly-2026-07-05 aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim
add_targets 1.93.1 wasm32-unknown-unknown wasm32-wasip2 wasm32-wasip1

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
echo "    ./scripts/build-host-cli.sh --bin run_matrix   # host CLI"
echo
echo "    The build scripts above each call apply_patch_series.sh to"
echo "    apply their patches/<runtime>/ stack to the submodule before"
echo "    the upstream build. The scripts are idempotent — they reset"
echo "    the submodule to its pinned HEAD first."
