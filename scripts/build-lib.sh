#!/usr/bin/env bash
# Build the benchmark-core static library for one Apple platform.
#
# Targets:
#   macos          M-series Macs (aarch64-apple-darwin) — host iteration.
#   watchos-sim    Apple Watch simulator on Apple Silicon
#                  (aarch64-apple-watchos-sim) — Tier 3, needs build-std.
#   watchos        Apple Watch Series 6-8, SE (arm64_32-apple-watchos) — Tier 3,
#                  needs build-std.
#   watchos-arm64  Apple Watch Series 9 and later (aarch64-apple-watchos) —
#                  Tier 3, needs build-std. App Store Connect requires this
#                  slice next to arm64_32 since April 2026.
#   ios            iPhone / iPad device (aarch64-apple-ios) — Tier 2, has rust-std.
#   ios-sim        iOS simulator on Apple Silicon (aarch64-apple-ios-sim).
#   tvos           Apple TV (aarch64-apple-tvos) — Tier 3, needs build-std.
#   tvos-sim       Apple TV simulator on Apple Silicon
#                  (aarch64-apple-tvos-sim) — Tier 3, needs build-std.
#   visionos       Apple Vision Pro (aarch64-apple-visionos) — Tier 3, needs
#                  build-std.
#   visionos-sim   Apple Vision Pro simulator (aarch64-apple-visionos-sim) —
#                  Tier 3, needs build-std.
#
# Every target builds with the pinned NIGHTLY_TC below: build-std for the
# Tier-3 targets, and the `become`-based interpreter dispatch
# (--cfg=pulley_tail_calls for Pulley, the `nightly-dispatch` feature for
# tinywasm's tail-call loop) everywhere.
#
# Per the project brief: minimum CPU is apple-a12 (iPhone XS chip; the iOS
# app requires an A12 through UIRequiredDeviceCapabilities). For
# consistency the same -mcpu is set on the macOS dev build too, so any
# instruction-set bug surfaces on the M4 without needing a device cycle.
# tvOS is the exception: tvOS 18 still runs on the Apple TV HD (A8), so the
# tvOS libraries keep the target's ARMv8.0 baseline (apple-a7).
#
# Output: target/<triple>/release/libbenchmark_core.a
#
# Usage:
#   scripts/build-lib.sh macos
#   scripts/build-lib.sh watchos-sim
#   scripts/build-lib.sh watchos
#   scripts/build-lib.sh ios
#   scripts/build-lib.sh visionos
#   scripts/build-lib.sh all      # builds in dependency order
#
# RUSTFLAGS notes:
#   -C target-cpu=apple-a12        — minimum CPU; M4 stays compatible.
#   (no -C linker-plugin-lto / -C embed-bitcode: see the toolchain note
#    below — Xcode 27's LLVM-21 libLTO cannot read LLVM-22 Rust bitcode)

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

WHICH="${1:-macos}"

# Common codegen flags; LTO is Rust-side fat LTO (CARGO_PROFILE_RELEASE_LTO).
#
# Pulley dispatch loop:
#   --cfg=pulley_tail_calls                    nightly, guaranteed TCO via `become`
#   --cfg=pulley_assume_llvm_makes_tail_calls  stable, relies on LLVM TCO
# We override per-target in the build_* functions: nightly for build-std
# targets gets the strong variant; stable targets get the LLVM-assumes one.
LTO_FLAGS="-C target-cpu=apple-a12"

# The apps' deployment targets. rustc and cc-rs (wasmtime's helpers.c) read
# these; without them cc-rs builds for the SDK's version and the app link
# warns that the object was built for a newer OS than the app supports.
export IPHONEOS_DEPLOYMENT_TARGET=17.0
export WATCHOS_DEPLOYMENT_TARGET=11.0
export TVOS_DEPLOYMENT_TARGET=18.0
export XROS_DEPLOYMENT_TARGET=26.0
export CARGO_PROFILE_RELEASE_LTO=fat
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
PULLEY_DISPATCH_NIGHTLY="--cfg=pulley_tail_calls"
PULLEY_DISPATCH_STABLE="--cfg=pulley_assume_llvm_makes_tail_calls"

# Pinned toolchains. wasmtime v49 needs rustc >= 1.96 and tinywasm 0.11
# needs >= 1.98. Every Rust >= 1.95 ships LLVM 22+, but Xcode 27's libLTO
# is LLVM 21 and rejects LLVM-22 Rust bitcode in the app link ("Unknown
# attribute kind (105) (Producer: 'LLVM22.1.8-rust-1.98.0-nightly' Reader:
# 'LLVM APPLE_1_2100.3.34.2_0')"). Cross-language LTO with Xcode's linker
# is therefore not possible at these Rust minimums, so the libraries are
# built as native objects with fat LTO across all Rust crates inside the
# staticlib (CARGO_PROFILE_RELEASE_LTO below) instead of
# -C linker-plugin-lto -C embed-bitcode=yes. The nightly is the newest
# dated 1.98-cycle build (LLVM 22.1.8, same as stable 1.98.0).
# STABLE_TC is the matching stable for non-dispatch-sensitive tooling.
STABLE_TC="1.98"
NIGHTLY_TC="nightly-2026-07-05"
FEATURES="--features nightly-dispatch"
# The femtovg E2E (wgpu on Metal) goes into the iOS and macOS libraries
# only: watchOS has no Metal, and the tvOS and visionOS apps do not run
# the E2E.
E2E_FEATURES="--features femtovg-e2e"

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
    cargo build --release -p benchmark-core --lib ${FEATURES} ${E2E_FEATURES} --target aarch64-apple-darwin
  )
}

build_ios() {
  echo "==> iOS device (aarch64-apple-ios) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} ${E2E_FEATURES} --target aarch64-apple-ios
  )
}

build_watchos_sim() {
  echo "==> watchOS simulator (aarch64-apple-watchos-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-watchos-sim
  )
}

build_watchos() {
  echo "==> watchOS device (arm64_32-apple-watchos) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target arm64_32-apple-watchos
  )
}

build_watchos_arm64() {
  echo "==> watchOS device, arm64 (aarch64-apple-watchos) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-watchos
  )
}

build_ios_sim() {
  echo "==> iOS simulator (aarch64-apple-ios-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} ${E2E_FEATURES} --target aarch64-apple-ios-sim
  )
}

# tvOS: no -C target-cpu=apple-a12 (see the header): the device target's
# baseline is apple-a7, which the Apple TV HD's A8 runs.
build_tvos() {
  echo "==> tvOS device (aarch64-apple-tvos) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-tvos
  )
}

build_tvos_sim() {
  echo "==> tvOS simulator (aarch64-apple-tvos-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-tvos-sim
  )
}

build_visionos() {
  echo "==> visionOS device (aarch64-apple-visionos) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-visionos
  )
}

build_visionos_sim() {
  echo "==> visionOS simulator (aarch64-apple-visionos-sim) [nightly ${NIGHTLY_TC}, +pulley_tail_calls]"
  ( prepend_toolchain_path "${NIGHTLY_TC}"
    export RUSTFLAGS="${LTO_FLAGS} ${PULLEY_DISPATCH_NIGHTLY}"
    cargo build --release -p benchmark-core --lib ${FEATURES} \
      -Z build-std=std,panic_abort \
      --target aarch64-apple-visionos-sim
  )
}

case "${WHICH}" in
  macos)        build_macos ;;
  ios)          build_ios ;;
  ios-sim)      build_ios_sim ;;
  watchos-sim)  build_watchos_sim ;;
  watchos)      build_watchos ;;
  watchos-arm64) build_watchos_arm64 ;;
  tvos)         build_tvos ;;
  tvos-sim)     build_tvos_sim ;;
  visionos)     build_visionos ;;
  visionos-sim) build_visionos_sim ;;
  all)          build_macos && build_ios && build_ios_sim && build_watchos_sim && build_watchos && build_watchos_arm64 && build_tvos && build_tvos_sim && build_visionos && build_visionos_sim ;;
  *) echo "unknown target: ${WHICH}" >&2; exit 2 ;;
esac
