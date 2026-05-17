#!/usr/bin/env bash
# Build zwasm (`libzwasm.a`) for one Apple target — pure interpreter
# (`-Djit=false`), App-Store-eligible.
#
# zwasm is the clojurewasm Zig runtime; despite its README only
# listing aarch64-macos / linux / windows as hosts, the underlying
# Zig 0.16 `-target aarch64-ios` / `-target aarch64-tvos` /
# `-target aarch64-watchos` cross-compilation works straight out of
# the box for the static-lib output (`zig build static-lib`). Verified
# 2026-05-16 — see AGENTS.md "Skipped runtimes" table.
#
# Output: zwasm/build-<triple>/libzwasm.a
#
# Usage: scripts/build-zwasm.sh {macos|ios|ios-sim|watchos|watchos-sim|tvos|tvos-sim|all}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ZW="${ROOT}/zwasm"
PATCH_DIR="${ROOT}/patches/zwasm"
WHICH="${1:-macos}"

# Pinned Zig version. zwasm's build.zig.zon declares
# `minimum_zig_version = "0.16.0"`. We expect the binary on PATH OR
# the canonical homebrew install path; override with ZWASM_ZIG to
# point at a specific binary.
ZIG="${ZWASM_ZIG:-/opt/homebrew/bin/zig}"

# Reset to pinned HEAD then apply the zwasm patch series. Same
# pattern as build-wasm3.sh / build-wasmz.sh.
( cd "${ZW}" && git reset --hard HEAD --quiet && git clean -fdq -e 'build*' )
if [[ -d "${PATCH_DIR}" ]]; then
  "${ROOT}/scripts/apply_patch_series.sh" "${ZW}" "${PATCH_DIR}"
fi

# $1 = output dir (e.g. build-aarch64-apple-ios)
# $2 = zig -Dtarget value (e.g. aarch64-ios)
# $3 = extra zig build flags
build_target() {
  local OUTDIR="$1" ZIG_TARGET="$2" EXTRA="${3:-}"
  local DIR="${ZW}/${OUTDIR}"
  rm -rf "${DIR}"
  ( cd "${ZW}" && rm -rf zig-out .zig-cache && \
    "${ZIG}" build static-lib \
      -Djit=false \
      -Doptimize=ReleaseFast \
      -Dtarget="${ZIG_TARGET}" \
      ${EXTRA} )
  mkdir -p "${DIR}"
  # Zig 0.16's static-lib step is broken on aarch64-watchos-ilp32 — it
  # generates the .o but doesn't pack it into the .a, leaving an empty
  # archive (88 bytes, only the SYMDEF). Re-pack from the cache if that
  # happens. Harmless on targets where the .a is already populated.
  local ZIG_A="${ZW}/zig-out/lib/libzwasm.a"
  if [[ $(wc -c < "${ZIG_A}") -lt 1024 ]]; then
    local ZCU_O
    ZCU_O=$(find "${ZW}/.zig-cache/o" -name "libzwasm_zcu.o" -print -quit)
    if [[ -z "${ZCU_O}" ]]; then
      echo "zwasm: empty .a but no libzwasm_zcu.o in cache" >&2
      exit 1
    fi
    /usr/bin/ar rcs "${ZIG_A}" "${ZCU_O}"
  fi
  cp "${ZIG_A}" "${DIR}/libzwasm.a"
  ls -la "${DIR}/libzwasm.a"
}

# Apple-target triples for Zig 0.16. Zig accepts native-style `arch-os`;
# for the static-lib output we don't need to pass an SDK explicitly
# (Zig vendors its own libSystem TBD stubs for the apple targets).
build_macos()       { build_target "build"                            aarch64-macos; }
build_ios()         { build_target "build-aarch64-apple-ios"          aarch64-ios; }
build_ios_sim()     { build_target "build-aarch64-apple-ios-sim"      aarch64-ios-simulator; }
# arm64_32-apple-watchos is ILP32; the Zig 0.16 spelling is
# `aarch64-watchos-ilp32` (the older `arm64_32-` triple was removed in
# ziglang/zig PR #20820). Built via patches/zwasm/0001 + a
# single_threaded static-lib mode + ILP32 narrowing fixes in
# guard.zig / memory.zig / vm.zig / types.zig + self-contained panic
# overrides in c_api.zig — see the patch header for the full
# rationale.
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
