#!/usr/bin/env bash
# First-time setup for a fresh wasm-benchmark clone.
# Idempotent — safe to run repeatedly.
#
# Does:
#   1. Init submodules (mach2, target-lexicon, wasm-micro-runtime,
#      porffor, sightglass) — pinned per .gitmodules. mach2 and
#      target-lexicon are on the fork's arm64_32-apple-watchos
#      branch so they ship with our patches already applied. The
#      `patches/` directory in this repo is the same content in
#      patch-file form for upstream-PR drafting; you do NOT need to
#      apply them.
#
#   2. Verify rustup toolchains. Pinned:
#        * 1.94.1            — default stable
#        * nightly-2026-01-25 — for arm64_32 build-std + nightly
#                              `become` Pulley dispatch
#
#   3. Verify rustup targets. Adds them if missing.
#
#   4. Optionally clone the wasmtime working clone if absent. It's
#      gitignored (47 GB, multiple active branches — see AGENTS.md →
#      wasmtime section). Skip with NO_WASMTIME_CLONE=1.

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
echo "==> 4. wasmtime working clone"
if [[ -d "${ROOT}/wasmtime/.git" || -f "${ROOT}/wasmtime/.git" ]]; then
  echo "    wasmtime/ present"
elif [[ -n "${NO_WASMTIME_CLONE:-}" ]]; then
  echo "    wasmtime/ not present; NO_WASMTIME_CLONE set, skipping"
else
  echo "    wasmtime/ not present, cloning fork at PR #2 branch..."
  echo "    (set NO_WASMTIME_CLONE=1 to skip; you can also clone manually:"
  echo "       git clone --branch table-mutability-tracking \\"
  echo "         git@github.com:rebeckerspecialties/wasmtime.git wasmtime"
  echo "    then add bytecodealliance upstream as 'origin' for fetches)"
  git clone --branch table-mutability-tracking \
    https://github.com/rebeckerspecialties/wasmtime.git wasmtime
fi

echo
echo "==> Setup complete. Next:"
echo "    ./scripts/build-workloads.sh     # builds workloads/*.wasm"
echo "    ./scripts/build-lib.sh macos     # builds benchmark-core .a"
echo "    cargo build --release --bin run_dispatch_workloads"
