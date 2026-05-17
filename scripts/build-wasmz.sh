#!/usr/bin/env bash
# Build wasmz (`libwasmz.a`) for one Apple target — pure interpreter,
# App-Store-eligible.
#
# Upstream wasmz (Ray-D-Song/wasmz) requires Zig 0.15.2, but Zig 0.15's
# build runner segfaults on macOS 26 Tahoe (its bundled libSystem TBDs
# lack symbols like `_realpath$DARWIN_EXTSN`, `_sigaction`). We apply
# `patches/wasmz/0001-zig-0.16-stdlib-port.patch` to port the sources
# to the 0.16 stdlib (intToEnum→fromInt, ArrayListUnmanaged.empty,
# @FieldType, std.c.mprotect, dropped Thread.Mutex shared-memory
# paths, etc.) and to add a `static-lib` build step. See AGENTS.md
# "Skipped runtimes" → wasmz row for the rationale.
#
# Output: wasmz/build-<triple>/libwasmz.a
#
# Usage: scripts/build-wasmz.sh {macos|ios|ios-sim|watchos|watchos-sim|tvos|tvos-sim|all}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WMZ="${ROOT}/wasmz"
PATCH_DIR="${ROOT}/patches/wasmz"
WHICH="${1:-macos}"

# Pinned Zig version. Patched wasmz tracks Zig 0.16.x.
ZIG="${WASMZ_ZIG:-/opt/homebrew/bin/zig}"

# Apply the upstream-incompatible patch series. Idempotent — re-runs
# detect already-applied patches via reverse-check.
"${ROOT}/scripts/apply_patch_series.sh" "${WMZ}" "${PATCH_DIR}"

# $1 = output dir (e.g. build-aarch64-apple-ios)
# $2 = zig -Dtarget value (e.g. aarch64-ios)
# $3 = extra zig build flags
build_target() {
  local OUTDIR="$1" ZIG_TARGET="$2" EXTRA="${3:-}"
  local DIR="${WMZ}/${OUTDIR}"
  rm -rf "${DIR}"
  ( cd "${WMZ}" && rm -rf zig-out .zig-cache && \
    "${ZIG}" build static-lib \
      -Doptimize=ReleaseFast \
      -Dtarget="${ZIG_TARGET}" \
      ${EXTRA} )
  mkdir -p "${DIR}"
  cp "${WMZ}/zig-out/lib/libwasmz.a" "${DIR}/libwasmz.a"
  ls -la "${DIR}/libwasmz.a"
}

# Apple-target triples for Zig 0.16. Zig accepts native-style `arch-os`;
# for the static-lib output we don't need to pass an SDK explicitly
# (Zig vendors its own libSystem TBD stubs for the apple targets).
build_macos()       { build_target "build"                            aarch64-macos; }
build_ios()         { build_target "build-aarch64-apple-ios"          aarch64-ios; }
build_ios_sim()     { build_target "build-aarch64-apple-ios-sim"      aarch64-ios-simulator; }
# wasmz uses 64-bit pointers throughout; arm64_32-apple-watchos is
# ILP32 and Zig 0.16 has no arm64_32 target, so watchOS device builds
# are not supported. watchOS sim (aarch64) still works for parity.
build_watchos_sim() { build_target "build-aarch64-apple-watchos-sim"  aarch64-watchos-simulator; }
build_tvos()        { build_target "build-aarch64-apple-tvos"         aarch64-tvos; }
build_tvos_sim()    { build_target "build-aarch64-apple-tvos-sim"     aarch64-tvos-simulator; }

case "${WHICH}" in
  macos)        build_macos ;;
  ios)          build_ios ;;
  ios-sim)      build_ios_sim ;;
  watchos)
    echo "wasmz: watchOS arm64_32 unsupported (wasmz assumes 64-bit ptrs; Zig 0.16 has no arm64_32 target)" >&2
    exit 0
    ;;
  watchos-sim)  build_watchos_sim ;;
  tvos)         build_tvos ;;
  tvos-sim)     build_tvos_sim ;;
  all)          build_macos && build_ios && build_ios_sim && build_watchos_sim && build_tvos && build_tvos_sim ;;
  *) echo "unknown target: ${WHICH}" >&2; exit 2 ;;
esac
