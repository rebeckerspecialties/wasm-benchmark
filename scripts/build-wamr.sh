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
PATCH_DIR="${ROOT}/patches/wasm-micro-runtime"
WHICH="${1:-macos}"

# Reset to pinned submodule HEAD then apply our patch series.
# Idempotent — re-runs detect already-applied patches via reverse-check.
# Same pattern as build-wasm3.sh / build-wasmz.sh / build-zwasm.sh.
#
# Currently applies:
#   0001-feat-interpreter-legacy-exception-handling-throw-only-
#     for-fast-interp.patch
#     — lifts the EXCE_HANDLING + FAST_INTERP cmake ban for the
#       throw-only subset of legacy wasm-eh; throw propagates via
#       the existing got_exception path. Enables Porffor-compiled
#       wasm to load on WAMR's fast-interp. Open as
#       rebeckerspecialties/wasm-micro-runtime#1 against the
#       fork; intended for upstream once same-function try/catch
#       lowering lands (see AGENTS.md → Open follow-up).
# Pinned submodule gitlink is the upstream WAMR base; reset to that
# rather than HEAD so that patches 0001-0020 always forward-apply
# cleanly. (HEAD may have feat-branch commits whose content overlaps
# the patch series, breaking apply_patch_series.sh's reverse-check.)
WAMR_PIN="$(cd "${ROOT}" && git ls-tree HEAD wasm-micro-runtime \
  | awk '{print $3}')"
( cd "${WAMR}" && git reset --hard "${WAMR_PIN}" --quiet \
  && git clean -fdq -e 'product-mini' )
if [[ -d "${PATCH_DIR}" ]]; then
  "${ROOT}/scripts/apply_patch_series.sh" "${WAMR}" "${PATCH_DIR}"
fi

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
  # Relaxed-SIMD (wasm 2.0 extension) — same `0xfd` prefix as the
  # legacy SIMD opcodes, plus 20 spec-assigned sub-opcodes at
  # 0x100..0x113. Off by default in upstream WAMR (dormant feature
  # bit `WASM_FEATURE_RELAXED_SIMD` at `aot_runtime.h:32`); our
  # fork's `patches/wasm-micro-runtime/0016..0018` light up the
  # fast-interp dispatch + cmake gate, and we set the flag here
  # so the matmul-relaxed-simd workload + any future relaxed-SIMD
  # benchmark wasm runs on WAMR. Upstreaming work tracked at
  # rebeckerspecialties/wasm-micro-runtime#3.
  -DWAMR_BUILD_RELAXED_SIMD=1
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

# `-DWASM_LINMEM_RESERVATION_CAP=<bytes>` opts into WAMR's PROT_NONE
# linear-memory reservation path
# (`patches/wasm-micro-runtime/0021-…`). Without it, every
# `memory.grow` call goes through `os_mremap_slow` on darwin —
# `mmap(new_size) + memcpy(old, new) + munmap(old)` — which first-
# touch faults every page in the new mapping. For workloads that
# re-instantiate + grow rapidly (Porffor's no-GC graphql-validation:
# ~15 grows per iter, 5 MB → 9 MB), this dominates the wallclock.
# With the reservation on, the OS-level VMA is pre-reserved with
# PROT_NONE up to `RESERVATION_CAP`; grows become `mprotect(extra,
# READ|WRITE)` — no syscall to mmap, no memcpy, no refault.
#
# Empirical (iPhone 12 A14 Icestorm, Porffor graphql-validation):
#   median 63.6 ms → 9.7 ms (6.6×), 43 → 300 iters/3s. WAMR now
#   beats wasmtime/Pulley by 1.6× on this workload.
#
# Cap chosen per-platform:
#   16 MB on arm64_32-apple-watchos (4 GiB total address space —
#     leaves headroom for the Rust side + system frameworks).
#   64 MB on arm64 (iOS / macOS / tvOS) — matches wasmtime's
#     `memory_reservation(64 MB)` for an apples-to-apples
#     comparison; Porffor's working set is ~6-10 MB so 64 MB
#     gives generous headroom.
LINMEM_CAP_64="-DWASM_LINMEM_RESERVATION_CAP=67108864"   # 64 MB
LINMEM_CAP_32="-DWASM_LINMEM_RESERVATION_CAP=16777216"   # 16 MB

build_macos() {
  local DIR="${WAMR}/product-mini/platforms/darwin/build"
  rm -rf "${DIR}" && mkdir -p "${DIR}"
  ( cd "${DIR}" && cmake .. "${COMMON_DEFS[@]}" \
      -DCMAKE_C_FLAGS="${COMMON_CFLAGS} ${LINMEM_CAP_64}"
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

  # 32-bit Apple targets (arm64_32-apple-watchos) get a smaller
  # linmem reservation cap to fit the 4 GiB address space; 64-bit
  # targets get the full 64 MB.
  local LINMEM_CAP
  if [[ "${ARCH}" == "arm64_32" ]]; then
    LINMEM_CAP="${LINMEM_CAP_32}"
  else
    LINMEM_CAP="${LINMEM_CAP_64}"
  fi

  rm -rf "${DIR}" && mkdir -p "${DIR}"
  ( cd "${DIR}" && cmake "${WAMR}/product-mini/platforms/${SUBDIR}" \
      "${COMMON_DEFS[@]}" \
      -DWAMR_BUILD_TARGET="${WAMR_TARGET}" \
      -DCMAKE_SYSTEM_NAME=Darwin \
      -DCMAKE_OSX_SYSROOT="${SYSROOT}" \
      -DCMAKE_OSX_ARCHITECTURES="${ARCH}" \
      -DCMAKE_C_COMPILER="${CC}" \
      -DCMAKE_C_FLAGS="${COMMON_CFLAGS} ${LINMEM_CAP} -arch ${ARCH} -isysroot ${SYSROOT} ${DEPMIN}" \
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
