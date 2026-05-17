#!/usr/bin/env bash
# Build WasmEdge (`libwasmedge.a`) for one Apple target — pure
# interpreter (no LLVM AOT / JIT, App-Store-eligible the same way
# Pulley / WAMR / wasm3 are).
#
# Cross-compile recipe ported from `webgpu-caps`
# (scripts/build_wasmedge_apple_target.sh), where this same WasmEdge
# pin + patch series has been shipping libwasmedge.a for iOS device
# since at least 2026-05. The patch stack in patches/wasmedge/ provides
# the Apple-mobile memory-guard fallbacks + interpreter
# super-instruction fast-paths needed to ship on iOS / watchOS / tvOS;
# without it, WasmEdge's default 4 GiB guard-page allocator hits
# `mmap` failures on iOS's restricted address space.
#
# Output: WasmEdge/build-<triple>/libwasmedge.a
#
# Usage: scripts/build-wasmedge.sh {macos|ios|ios-sim|watchos|watchos-sim|tvos|tvos-sim|all}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WE_SRC="${ROOT}/WasmEdge"
PATCH_DIR="${ROOT}/patches/wasmedge"
APPLY_PATCHES="${ROOT}/scripts/apply_patch_series.sh"
WHICH="${1:-macos}"

# Common cmake defines for the App-Store-eligible static-lib build.
# Matches `webgpu-caps`'s proven combo: USE_LLVM=OFF (pure interp),
# STATIC_LIB=ON (a single .a archive), no plugins, no tools, no tests.
COMMON_DEFS=(
  -DCMAKE_BUILD_TYPE=MinSizeRel
  -DCMAKE_OSX_ARCHITECTURES=arm64
  -DWASMEDGE_USE_LLVM=OFF
  -DWASMEDGE_BUILD_SHARED_LIB=OFF
  -DWASMEDGE_BUILD_STATIC_LIB=ON
  -DWASMEDGE_BUILD_TOOLS=OFF
  -DWASMEDGE_BUILD_PLUGINS=OFF
  -DWASMEDGE_BUILD_TESTS=OFF
  -DWASMEDGE_BUILD_EXAMPLE=OFF
  -DWASMEDGE_ENABLE_WASI_NN_AUTOLOAD=OFF
  -DWASMEDGE_ENABLE_OPTIONAL_PLUGIN_AUTOLOAD=OFF
  -DWASMEDGE_PLUGIN_WASI_NN_GGML_LLAMA_NATIVE=OFF
  -DWASMEDGE_PLUGIN_WASI_NN_GGML_LLAMA_METAL=OFF
  -DWASMEDGE_PLUGIN_WASI_NN_WHISPER_METAL=OFF
  -DWASMEDGE_PLUGIN_STABLEDIFFUSION_METAL=OFF
  -DWASMEDGE_STATIC_LIB_ENABLE_LTO=ON
)

COMMON_CFLAGS="-Os -DNDEBUG -mcpu=apple-a12 -flto=full -fembed-bitcode"

# Debug-build flags for diagnosing the arm64_32 instantiate trap.
# Triggered by the `watchos-debug` target; identical to COMMON_CFLAGS
# minus `-DNDEBUG` and `-flto=full`, plus `-g` for symbol-name backtraces.
# Leaves `assuming(R)` as `assert(expr)` so the first failing predicate
# in `lib/executor/instantiate/*.cpp` prints a useful message to stderr
# instead of dropping straight into `brk #1`.
DEBUG_CFLAGS="-Os -mcpu=apple-a12 -fembed-bitcode"
# WasmEdge requires C++17 (its CMakeLists adds it conditionally for
# Apple). Optional: bump to C++20 once upstream's spdlog version is
# happy with it; the proven recipe sticks with C++17.

apply_patches_once() {
  # Reset the submodule to its pinned HEAD before reapplying the
  # series — a previous run may have left a half-applied state that
  # neither `--check` nor `--reverse --check` matches, which would
  # make apply_patch_series.sh bail. The submodule is purely a build
  # input here; we don't keep edits in it (the source of truth is the
  # tracked `patches/wasmedge/` directory + the submodule SHA in
  # `.gitmodules` / `git submodule status`).
  ( cd "${WE_SRC}" && git reset --hard HEAD --quiet && git clean -fdq -e 'build-*' )
  "${APPLY_PATCHES}" "${WE_SRC}" "${PATCH_DIR}"
}

# $1 = output dir (e.g. build-aarch64-apple-ios)
# $2 = CMAKE_SYSTEM_NAME (Darwin / iOS / watchOS / tvOS)
# $3 = SDK (iphoneos / iphonesimulator / watchos / watchsimulator /
#           appletvos / appletvsimulator / macosx)
# $4 = CMAKE_OSX_DEPLOYMENT_TARGET (e.g. 18.0)
# $5 = extra c/cxx flag string (e.g. "-target arm64-apple-ios18.0-simulator")
# $6 = CMAKE_OSX_ARCHITECTURES override (e.g. arm64 / arm64_32)
build_target() {
  local OUTDIR="$1" SYSNAME="$2" SDK="$3" DEPMIN="$4" EXTRA="$5" ARCH="$6"
  local FLAGS_OVERRIDE="${7:-}"
  apply_patches_once
  local DIR="${WE_SRC}/${OUTDIR}"
  local SYSROOT
  SYSROOT="$(xcrun --sdk "${SDK}" --show-sdk-path)"
  rm -rf "${DIR}"
  local CFLAGS_USE="${FLAGS_OVERRIDE:-${COMMON_CFLAGS}}"
  # Debug builds use BUILD_TYPE=Debug so cmake picks Debug compile
  # flags (no -DNDEBUG inserted automatically), and skip the LTO
  # interproc opt that defeats inlined assertion messages.
  local BUILD_TYPE="MinSizeRel"
  local IPO_FLAG=ON
  if [[ -n "${FLAGS_OVERRIDE}" ]]; then
    BUILD_TYPE="Debug"
    IPO_FLAG=OFF
  fi
  cmake -S "${WE_SRC}" -B "${DIR}" -G Ninja \
    "${COMMON_DEFS[@]}" \
    -DCMAKE_BUILD_TYPE="${BUILD_TYPE}" \
    -DCMAKE_SYSTEM_NAME="${SYSNAME}" \
    -DCMAKE_OSX_SYSROOT="${SYSROOT}" \
    -DCMAKE_OSX_DEPLOYMENT_TARGET="${DEPMIN}" \
    -DCMAKE_OSX_ARCHITECTURES="${ARCH}" \
    -DCMAKE_C_FLAGS_MINSIZEREL="${CFLAGS_USE} ${EXTRA}" \
    -DCMAKE_CXX_FLAGS_MINSIZEREL="${CFLAGS_USE} ${EXTRA}" \
    -DCMAKE_OBJCXX_FLAGS_MINSIZEREL="${CFLAGS_USE} ${EXTRA}" \
    -DCMAKE_C_FLAGS_DEBUG="${CFLAGS_USE} ${EXTRA}" \
    -DCMAKE_CXX_FLAGS_DEBUG="${CFLAGS_USE} ${EXTRA}" \
    -DCMAKE_OBJCXX_FLAGS_DEBUG="${CFLAGS_USE} ${EXTRA}" \
    -DCMAKE_INTERPROCEDURAL_OPTIMIZATION_MINSIZEREL=${IPO_FLAG}
  # The proven webgpu-caps recipe builds the default target (the patch
  # series wires the static lib in as a default product) and then
  # copies lib/api/libwasmedge.a into the top of the build dir for the
  # adapter to link against.
  cmake --build "${DIR}" -j8
  cp "${DIR}/lib/api/libwasmedge.a" "${DIR}/libwasmedge.a"
  ls -la "${DIR}/libwasmedge.a"
}

# macOS uses the bare `build/` dir to match build.rs's per-target
# convention (host = "build", cross = "build-<triple>") shared with
# WAMR and wasm3.
build_macos()       { build_target "build"                            Darwin   macosx            13.4 ""                                               arm64; }
build_ios()         { build_target "build-aarch64-apple-ios"         iOS      iphoneos          18.0 ""                                               arm64; }
build_ios_sim()     { build_target "build-aarch64-apple-ios-sim"     iOS      iphonesimulator   18.0 "-target arm64-apple-ios18.0-simulator"          arm64; }
# watchOS arm64_32 is gnarly for WasmEdge (its allocator does
# pointer-tagged 64-bit math); when the build hits an arm64_32-specific
# wall, we punt to "wasmedge unavailable on this target" rather than
# blocking the watch app's main flow.
build_watchos()     { build_target "build-arm64_32-apple-watchos"    watchOS  watchos           11.0 ""                                               arm64_32; }
# Debug variant of watchos build for diagnosing the arm64_32
# WasmEdge_VMInstantiate trap. Drops -DNDEBUG so `assuming(R)` prints
# the first failing predicate via `assert()` instead of falling into
# `__builtin_unreachable() → brk #1`. Larger / slower / no LTO.
build_watchos_debug() { build_target "build-arm64_32-apple-watchos-debug" watchOS watchos 11.0 "" arm64_32 "${DEBUG_CFLAGS}"; }
build_watchos_sim() { build_target "build-aarch64-apple-watchos-sim" watchOS  watchsimulator    11.0 "-target arm64-apple-watchos11.0-simulator"      arm64; }
build_tvos()        { build_target "build-aarch64-apple-tvos"        tvOS     appletvos         26.0 ""                                               arm64; }
build_tvos_sim()    { build_target "build-aarch64-apple-tvos-sim"    tvOS     appletvsimulator  26.0 "-target arm64-apple-tvos26.0-simulator"         arm64; }

case "${WHICH}" in
  macos)        build_macos ;;
  ios)          build_ios ;;
  ios-sim)      build_ios_sim ;;
  watchos)        build_watchos ;;
  watchos-debug)  build_watchos_debug ;;
  watchos-sim)  build_watchos_sim ;;
  tvos)         build_tvos ;;
  tvos-sim)     build_tvos_sim ;;
  all)          build_macos && build_ios && build_ios_sim && build_watchos_sim && build_tvos && build_tvos_sim ;;
  *) echo "unknown target: ${WHICH}" >&2; exit 2 ;;
esac
