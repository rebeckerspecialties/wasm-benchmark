#!/usr/bin/env bash
# Build wasm3 (libm3.a) for one Apple target.
#
# wasm3 is a pure-interpreter wasm runtime (no JIT, no AOT) — so it is
# App-Store-eligible on iOS/watchOS/tvOS the same way Pulley and WAMR are.
# Its source layout is dead simple — 15 .c files in wasm3/source/ with no
# external deps when WASI is disabled — so we skip CMake entirely and
# just shell out to `clang -c` + `ar`, mirroring the per-target output-dir
# layout build-wamr.sh uses.
#
# Output: wasm3/build-<triple>/libm3.a
#
# WASI is NOT compiled in: our workloads are wasm32-unknown-unknown
# cdylibs that do not use WASI, so the API surface (the WASI-related .c
# files: m3_api_wasi.c, m3_api_uvwasi.c, m3_api_meta_wasi.c) would only
# add link-time symbol noise without contributing dispatch behaviour
# we'd actually measure.
#
# Usage: scripts/build-wasm3.sh {macos|ios|ios-sim|watchos|watchos-sim|tvos|tvos-sim|all}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
W3="${ROOT}/wasm3"
PATCH_DIR="${ROOT}/patches/wasm3"
APPLY_PATCHES="${ROOT}/scripts/apply_patch_series.sh"
WHICH="${1:-macos}"

# Reset to pinned HEAD then apply the wasm3 patch series. Idempotent
# via apply_patch_series.sh (skips already-applied patches).
( cd "${W3}" && git reset --hard HEAD --quiet && git clean -fdq -e 'build*' )
if [[ -d "${PATCH_DIR}" ]]; then
  "${APPLY_PATCHES}" "${W3}" "${PATCH_DIR}"
fi

# Source files. The four m3_api_*wasi*.c + m3_api_tracer.c files reference
# host APIs we don't need; omit them so the static lib has no unresolved
# external syscalls.
SOURCES=(
  m3_api_libc.c
  m3_bind.c
  m3_code.c
  m3_compile.c
  m3_core.c
  m3_env.c
  m3_exec.c
  m3_function.c
  m3_info.c
  m3_module.c
  m3_parse.c
)

COMMON_CFLAGS="-O3 -mcpu=apple-a12 -std=c99 -DNDEBUG -fno-exceptions"

# $1 = output dir (e.g. build-aarch64-apple-ios)
# $2 = arch (arm64 / arm64_32)
# $3 = SDK (iphoneos / iphonesimulator / watchos / watchsimulator / appletvos /
#           appletvsimulator / macosx)
# $4 = -m<plat>-version-min flag (or empty for host macOS)
# $5 = extra clang flags (e.g. -target ... for simulator triples; "" for device)
build_target() {
  local OUTDIR="$1"; local ARCH="$2"; local SDK="$3"
  local DEPMIN="$4"; local EXTRA="$5"
  local DIR="${W3}/${OUTDIR}"
  rm -rf "${DIR}" && mkdir -p "${DIR}"
  local SYSROOT
  SYSROOT="$(xcrun --sdk "${SDK}" --show-sdk-path)"
  local CC
  CC="$(xcrun --sdk "${SDK}" --find clang)"
  local OBJS=()
  for src in "${SOURCES[@]}"; do
    local obj="${DIR}/${src%.c}.o"
    "${CC}" -c "${W3}/source/${src}" -o "${obj}" \
      ${COMMON_CFLAGS} -arch "${ARCH}" -isysroot "${SYSROOT}" \
      ${DEPMIN} ${EXTRA}
    OBJS+=("${obj}")
  done
  ar rcs "${DIR}/libm3.a" "${OBJS[@]}"
  ls -la "${DIR}/libm3.a"
}

# macOS uses the bare `build/` dir to match build.rs's per-target
# convention (host = "build", cross = "build-<triple>") shared with WAMR
# and WasmEdge.
build_macos()       { build_target "build"                            arm64    macosx           ""                              ""; }
build_ios()         { build_target "build-aarch64-apple-ios"         arm64    iphoneos         "-miphoneos-version-min=18.0"   ""; }
build_ios_sim()     { build_target "build-aarch64-apple-ios-sim"     arm64    iphonesimulator  "-miphoneos-version-min=18.0"   "-target arm64-apple-ios18.0-simulator"; }
build_watchos()     { build_target "build-arm64_32-apple-watchos"    arm64_32 watchos          "-mwatchos-version-min=11.0"    ""; }
build_watchos_sim() { build_target "build-aarch64-apple-watchos-sim" arm64    watchsimulator   "-mwatchos-version-min=11.0"    "-target arm64-apple-watchos11.0-simulator"; }
build_tvos()        { build_target "build-aarch64-apple-tvos"        arm64    appletvos        "-mtvos-version-min=26.0"       ""; }
build_tvos_sim()    { build_target "build-aarch64-apple-tvos-sim"    arm64    appletvsimulator "-mtvos-version-min=26.0"       "-target arm64-apple-tvos26.0-simulator"; }

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
