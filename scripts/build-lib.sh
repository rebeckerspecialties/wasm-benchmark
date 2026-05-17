#!/usr/bin/env bash
# Build the benchmark-core static library for one Apple platform.
#
# Targets:
#   macos          M-series Macs (aarch64-apple-darwin) — host iteration.
#                  Uses stable 1.93.1; rust-std is shipped.
#   watchos-sim    Apple Watch simulator on Apple Silicon
#                  (aarch64-apple-watchos-sim) — Tier 3, needs build-std
#                  with nightly-2026-01-25.
#   watchos        Apple Watch SE2 device (arm64_32-apple-watchos) — Tier 3,
#                  needs build-std with nightly-2026-01-25.
#   ios            iPhone XS device (aarch64-apple-ios) — Tier 2, has rust-std.
#   tvos           Apple TV 4K (aarch64-apple-tvos) — Tier 3, needs build-std
#                  with nightly-2026-01-25.
#   tvos-sim       Apple TV simulator on Apple Silicon
#                  (aarch64-apple-tvos-sim) — Tier 3, needs build-std with
#                  nightly-2026-01-25.
#
# Per the project brief: minimum CPU is apple-a12 (iPhone XS chip).
# For consistency the same -mcpu is set on the macOS dev build too, so any
# instruction-set bug surfaces on the M4 without needing a device cycle.
#
# Output: target/<triple>/release/libbenchmark_core.a
#
# Usage:
#   scripts/build-lib.sh macos
#   scripts/build-lib.sh watchos-sim
#   scripts/build-lib.sh watchos
#   scripts/build-lib.sh ios
#   scripts/build-lib.sh all      # builds in dependency order
#
# RUSTFLAGS notes:
#   -C target-cpu=apple-a12        — minimum CPU; M4 stays compatible.
#   -C linker-plugin-lto           — emit LLVM bitcode for cross-language LTO.
#   -C embed-bitcode=yes           — keep bitcode in .o so the Apple linker
#                                    can pick it up at app-link time.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

WHICH="${1:-macos}"

# Common LTO/LTO-bitcode flags. -C target-cpu=apple-a12 is set per-target
# below since it doesn't apply to the host-only path.
#
# Pulley dispatch loop:
#   --cfg=pulley_tail_calls                    nightly, guaranteed TCO via `become`
#   --cfg=pulley_assume_llvm_makes_tail_calls  stable, relies on LLVM TCO
# We override per-target in the build_* functions: nightly for build-std
# targets gets the strong variant; stable targets get the LLVM-assumes one.
LTO_FLAGS="-C linker-plugin-lto -C embed-bitcode=yes -C target-cpu=apple-a12"
PULLEY_DISPATCH_NIGHTLY="--cfg=pulley_tail_calls"
PULLEY_DISPATCH_STABLE="--cfg=pulley_assume_llvm_makes_tail_calls"

# Stable for Tier-1/2 targets; pinned nightly for Tier-3 build-std targets.
STABLE_TC="1.93.1"
NIGHTLY_TC="nightly-2026-01-25"

prepend_toolchain_path() {
  # `rustup run` doesn't prepend the toolchain bin to PATH (see
  # docs/feasibility-report.md, "Stage 1.5"); do it explicitly.
  local tc="$1"
  local bin="${HOME}/.rustup/toolchains/${tc}-aarch64-apple-darwin/bin"
  if [[ ! -d "${bin}" ]]; then
    echo "ERROR: missing toolchain bin: ${bin}" >&2
    echo "Install: rustup toolchain install ${tc}" >&2
    return 2
  fi
  export PATH="${bin}:${PATH}"
}

build_macos() {
  echo "==> macOS host (aarch64-apple-darwin) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  # The 'pulley_assume_llvm_makes_tail_calls' (stable) variant
  # stack-overflows on convolution because LLVM's best-effort TCO doesn't
  # cover every dispatch arm reliably. The nightly `become`-based
  # 'pulley_tail_calls' is the only safe tail-call dispatch right now.
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib --target aarch64-apple-darwin
  )
}

build_ios() {
  echo "==> iOS device (aarch64-apple-ios) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib --target aarch64-apple-ios
  )
}

build_watchos_sim() {
  echo "==> watchOS simulator (aarch64-apple-watchos-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-watchos-sim
  )
}

build_watchos() {
  echo "==> watchOS device (arm64_32-apple-watchos) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib \
      -Z build-std=std,panic_abort \
      --target arm64_32-apple-watchos
  )
}

build_ios_sim() {
  echo "==> iOS simulator (aarch64-apple-ios-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib --target aarch64-apple-ios-sim
  )
}

build_tvos() {
  echo "==> tvOS device (aarch64-apple-tvos) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-tvos
  )
}

build_tvos_sim() {
  echo "==> tvOS simulator (aarch64-apple-tvos-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-tvos-sim
  )
}

case "${WHICH}" in
  macos)        build_macos ;;
  ios)          build_ios ;;
  ios-sim)      build_ios_sim ;;
  watchos-sim)  build_watchos_sim ;;
  watchos)      build_watchos ;;
  tvos)         build_tvos ;;
  tvos-sim)     build_tvos_sim ;;
  all)          build_macos && build_ios && build_ios_sim && build_watchos_sim && build_watchos && build_tvos && build_tvos_sim ;;
  *) echo "unknown target: ${WHICH}" >&2; exit 2 ;;
esac
