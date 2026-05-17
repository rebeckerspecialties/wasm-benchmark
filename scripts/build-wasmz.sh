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

# Reset to pinned HEAD then apply the wasmz patch series. Without
# the reset, apply_patch_series.sh's per-patch reverse-check can't
# handle a series where later patches modify lines added by earlier
# ones (the reverse no longer matches the tree state) — same pattern
# as scripts/build-wasm3.sh.
( cd "${WMZ}" && git reset --hard HEAD --quiet && git clean -fdq -e 'build*' )
if [[ -d "${PATCH_DIR}" ]]; then
  "${ROOT}/scripts/apply_patch_series.sh" "${WMZ}" "${PATCH_DIR}"
fi

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
  # Zig 0.16's static-lib step is broken on aarch64-watchos-ilp32 — it
  # generates the .o but doesn't pack it into the .a, leaving an empty
  # archive (88 bytes, only the SYMDEF). Re-pack from the cache if that
  # happens. Harmless on targets where the .a is already populated.
  local ZIG_A="${WMZ}/zig-out/lib/libwasmz.a"
  if [[ $(wc -c < "${ZIG_A}") -lt 1024 ]]; then
    local ZCU_O
    ZCU_O=$(find "${WMZ}/.zig-cache/o" -name "libwasmz_zcu.o" -print -quit)
    if [[ -z "${ZCU_O}" ]]; then
      echo "wasmz: empty .a but no libwasmz_zcu.o in cache" >&2
      exit 1
    fi
    /usr/bin/ar rcs "${ZIG_A}" "${ZCU_O}"
  fi
  cp "${ZIG_A}" "${DIR}/libwasmz.a"
  ls -la "${DIR}/libwasmz.a"
}

# Apple-target triples for Zig 0.16. Zig accepts native-style `arch-os`;
# for the static-lib output we don't need to pass an SDK explicitly
# (Zig vendors its own libSystem TBD stubs for the apple targets).
build_macos()       { build_target "build"                            aarch64-macos; }
build_ios()         { build_target "build-aarch64-apple-ios"          aarch64-ios; }
build_ios_sim()     { build_target "build-aarch64-apple-ios-sim"      aarch64-ios-simulator; }
# arm64_32-apple-watchos is ILP32; the Zig 0.16 spelling is
# `aarch64-watchos-ilp32` (the older `arm64_32-` triple was removed in
# ziglang/zig PR #20820). The static-lib build is enabled via
# `single_threaded = true` in build.zig + a fully self-contained
# `panic` namespace in src/capi.zig (both shipped via patch series)
# so we never pull in std.Io.Threaded — which doesn't compile on
# arm64_32 (32-bit `usize` vs Darwin's 64-bit syscall returns).
build_watchos()     { build_target "build-arm64_32-apple-watchos"     aarch64-watchos-ilp32; }
build_watchos_sim() { build_target "build-aarch64-apple-watchos-sim"  aarch64-watchos-simulator; }
build_tvos()        { build_target "build-aarch64-apple-tvos"         aarch64-tvos; }
build_tvos_sim()    { build_target "build-aarch64-apple-tvos-sim"     aarch64-tvos-simulator; }

case "${WHICH}" in
  macos)        build_macos ;;
  ios)          build_ios ;;
  ios-sim)      build_ios_sim ;;
  watchos)      build_watchos ;;
  watchos-sim)  build_watchos_sim ;;
  tvos)         build_tvos ;;
  tvos-sim)     build_tvos_sim ;;
  all)          build_macos && build_ios && build_ios_sim && build_watchos && build_watchos_sim && build_tvos && build_tvos_sim ;;
  *) echo "unknown target: ${WHICH}" >&2; exit 2 ;;
esac
