#!/usr/bin/env bash
# Build WAMR (libiwasm.a) for one Apple target.
#
# Output layout matches what crates/benchmark-core/build.rs expects:
#
#   wasm-micro-runtime/product-mini/platforms/darwin/build
#   wasm-micro-runtime/product-mini/platforms/darwin/build-arm64_32-apple-watchos
#   wasm-micro-runtime/product-mini/platforms/darwin/build-aarch64-apple-watchos-sim
#   wasm-micro-runtime/product-mini/platforms/ios/build-aarch64-apple-ios
#   wasm-micro-runtime/product-mini/platforms/ios/build-aarch64-apple-ios-sim
#
# All targets share these WAMR cmake settings:
#   WAMR_BUILD_INTERP=1 + FAST_INTERP=1   (the apples-to-apples vs Pulley path)
#   WAMR_BUILD_AOT=0 + JIT=0 + FAST_JIT=0  (no native codegen — App-Store-safe)
#   SIMD=1 + BULK_MEMORY=1 + TAIL_CALL=1 + REF_TYPES=1
#   WAMR_DISABLE_HW_BOUND_CHECK=1          (workaround for the macOS
#                                          touch_pages stack-walk bug —
#                                          we don't need stack guards on
#                                          trusted benchmark workloads)
#   LIBC_BUILTIN/WASI=0                    (we provide no host syscalls)
#
# Usage: scripts/build-wamr.sh {macos|ios|ios-sim|watchos|watchos-sim|all}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WAMR="${ROOT}/wasm-micro-runtime"
WHICH="${1:-macos}"

COMMON_DEFS=(
  -DBUILD_SHARED_LIBS=OFF
  -DCMAKE_BUILD_TYPE=Release
  -DWAMR_BUILD_INTERP=1
  -DWAMR_BUILD_FAST_INTERP=1
  -DWAMR_BUILD_AOT=0
  -DWAMR_BUILD_JIT=0
  -DWAMR_BUILD_FAST_JIT=0
  -DWAMR_BUILD_LIBC_WASI=0
  -DWAMR_BUILD_LIBC_BUILTIN=0
  -DWAMR_BUILD_SIMD=1
  -DWAMR_BUILD_BULK_MEMORY=1
  -DWAMR_BUILD_TAIL_CALL=1
  -DWAMR_BUILD_REF_TYPES=1
  # Wasm-exceptions support — needed for Porffor-compiled wasm, which
  # lowers JS try/catch/throw to the wasm-eh section. WAMR upstream
  # forbids `WAMR_BUILD_EXCE_HANDLING=1` together with `FAST_INTERP=1`
  # (build-scripts/unsupported_combination.cmake:67). Our fork lifts
  # that ban for the *throw-only* subset of legacy-EH — modules that
  # declare tags and execute `throw` but never define a same-function
  # `try`/`catch` handler. Porffor's emit shape is throw-only in our
  # test corpus (561 throws, 0 try/catch in graphql-validation-porf),
  # so this is all we need to make WAMR run the workload correctly.
  # The throw escapes via the existing `got_exception` bailout, same
  # path as any other trap; the host sees the exception via
  # `wasm_runtime_get_exception`. Same-function try/catch lowering is
  # the natural follow-up — see `core/iwasm/interpreter/wasm_interp_
  # fast.c::HANDLE_OP(WASM_OP_THROW)` for status.
  -DWAMR_BUILD_EXCE_HANDLING=1
  -DWAMR_BUILD_MULTI_MODULE=0
  -DWAMR_BUILD_LIB_PTHREAD=0
  -DWAMR_BUILD_MINI_LOADER=0
  -DWAMR_DISABLE_HW_BOUND_CHECK=1
)

# Target-cpu apple-a12 to match the Rust side's `-C target-cpu=apple-a12`.
COMMON_CFLAGS="-O3 -mcpu=apple-a12"

build_macos() {
  local DIR="${WAMR}/product-mini/platforms/darwin/build"
  rm -rf "${DIR}" && mkdir -p "${DIR}"
  ( cd "${DIR}" && cmake .. "${COMMON_DEFS[@]}" \
      -DCMAKE_C_FLAGS="${COMMON_CFLAGS}"
    make -j8 )
}

# Cross-compile for an Apple non-host target.
#
# $1 = output dir name (e.g. build-arm64_32-apple-watchos)
# $2 = which CMakeLists subdir (darwin or ios)
# $3 = arch (arm64_32 / arm64)
# $4 = SDK (watchos / watchsimulator / iphoneos / iphonesimulator)
# $5 = WAMR_BUILD_TARGET (AARCH64 / AARCH64_ILP32 / etc)
# $6 = deployment-target flag (e.g. -mwatchos-version-min=11.0)
build_target() {
  local OUTDIR="$1"; local SUBDIR="$2"; local ARCH="$3"
  local SDK="$4"; local WAMR_TARGET="$5"; local DEPMIN="$6"

  local DIR="${WAMR}/product-mini/platforms/${SUBDIR}/${OUTDIR}"
  local SYSROOT
  SYSROOT="$(xcrun --sdk "${SDK}" --show-sdk-path)"
  local CC
  CC="$(xcrun --sdk "${SDK}" --find clang)"

  rm -rf "${DIR}" && mkdir -p "${DIR}"
  ( cd "${DIR}" && cmake "${WAMR}/product-mini/platforms/${SUBDIR}" \
      "${COMMON_DEFS[@]}" \
      -DWAMR_BUILD_TARGET="${WAMR_TARGET}" \
      -DCMAKE_SYSTEM_NAME=Darwin \
      -DCMAKE_OSX_SYSROOT="${SYSROOT}" \
      -DCMAKE_OSX_ARCHITECTURES="${ARCH}" \
      -DCMAKE_C_COMPILER="${CC}" \
      -DCMAKE_C_FLAGS="${COMMON_CFLAGS} -arch ${ARCH} -isysroot ${SYSROOT} ${DEPMIN}" \
      -DCMAKE_EXE_LINKER_FLAGS="-arch ${ARCH} -isysroot ${SYSROOT} ${DEPMIN}"
    make -j8 iwasm_static 2>/dev/null || make -j8 vmlib 2>/dev/null || make -j8 )
  ls -la "${DIR}/libiwasm.a" 2>/dev/null || \
    (echo "ERROR: ${DIR}/libiwasm.a missing"; ls "${DIR}"; exit 2)
}

# NB: WAMR's product-mini/platforms/ios/CMakeLists.txt hard-codes
# `add_library(iwasm SHARED ...)` and overrides BUILD_SHARED_LIBS, which
# produces a `.dylib` we can't link into a Rust staticlib. The
# `darwin/CMakeLists.txt` builds `vmlib` as a respect-BUILD_SHARED_LIBS
# library and works for every Apple target including iOS / watchOS, so
# we route both iOS variants through it.
build_ios()         { build_target "build-aarch64-apple-ios"          darwin  "arm64"    iphoneos        AARCH64 "-miphoneos-version-min=18.0"; }
build_ios_sim()     { build_target "build-aarch64-apple-ios-sim"      darwin  "arm64"    iphonesimulator AARCH64 "-miphoneos-version-min=18.0 -target arm64-apple-ios18.0-simulator"; }
build_watchos()     { build_target "build-arm64_32-apple-watchos"     darwin  "arm64_32" watchos         AARCH64 "-mwatchos-version-min=11.0"; }
build_watchos_sim() { build_target "build-aarch64-apple-watchos-sim"  darwin  "arm64"    watchsimulator  AARCH64 "-mwatchos-version-min=11.0 -target arm64-apple-watchos11.0-simulator"; }
build_tvos()        { build_target "build-aarch64-apple-tvos"         darwin  "arm64"    appletvos       AARCH64 "-mtvos-version-min=26.0"; }
build_tvos_sim()    { build_target "build-aarch64-apple-tvos-sim"     darwin  "arm64"    appletvsimulator AARCH64 "-mtvos-version-min=26.0 -target arm64-apple-tvos26.0-simulator"; }

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
