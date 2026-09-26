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
#   SIMD=1 + RELAXED_SIMD=1 + BULK_MEMORY=1 + EXTENDED_CONST_EXPR=1
#   + TAIL_CALL=1 + REF_TYPES=1 + EXCE_HANDLING=1 (legacy EH)
#   WAMR_DISABLE_HW_BOUND_CHECK=1          (workaround for the macOS
#                                          touch_pages stack-walk bug —
#                                          we don't need stack guards on
#                                          trusted benchmark workloads)
#   LIBC_BUILTIN/WASI=0                    (we provide no host syscalls)
#
# Usage: scripts/build-wamr.sh {macos|ios|ios-sim|watchos|watchos-arm64|watchos-sim|tvos|tvos-sim|visionos|visionos-sim|all}

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WAMR="${ROOT}/wasm-micro-runtime"
PATCH_DIR="${ROOT}/patches/wasm-micro-runtime"
WHICH="${1:-macos}"

# Reset to pinned submodule HEAD then apply our patch series.
# Idempotent — re-runs detect already-applied patches via reverse-check.
# Same pattern as build-wasm3.sh / build-wasmz.sh / build-zwasm.sh.
#
# Currently applies (on upstream main b70d708d):
#   0001-0017  legacy exception handling for fast-interp: try / catch /
#              catch_all / rethrow / delegate, tag payloads, result-typed
#              try regions (fork PRs #1 + #2)
#   0018-0027  relaxed SIMD for fast-interp (fork PR #3, upstream #4950)
#   0028-0029  opt-in PROT_NONE linear-memory reservation (fork PR #4)
# Pinned submodule gitlink is the upstream WAMR base; check it out
# (detached HEAD) before applying the patch series so that:
#   (a) patches 0001-0029 always forward-apply cleanly. HEAD may have
#       feat-branch commits whose content overlaps the patch series,
#       breaking apply_patch_series.sh's reverse-check.
#   (b) `git reset --hard <pin>` while on a feature branch would move
#       the branch ref to the pin and orphan any local commits — bad
#       UX for anyone iterating on a feat/ branch in the submodule.
#       Detaching HEAD first keeps branch refs untouched.
WAMR_PIN="$(cd "${ROOT}" && git ls-tree HEAD wasm-micro-runtime \
  | awk '{print $3}')"
( cd "${WAMR}" && git checkout --detach "${WAMR_PIN}" --quiet \
  && git reset --hard "${WAMR_PIN}" --quiet \
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
  # 0x100..0x113. Not implemented in upstream WAMR's fast-interp; our
  # `patches/wasm-micro-runtime/0018-0027` add the dispatch cases and
  # the cmake gate (default off), and we set the flag here so the
  # relaxed-SIMD workloads run on WAMR. Upstreaming tracked at
  # rebeckerspecialties/wasm-micro-runtime#3 / upstream #4950.
  -DWAMR_BUILD_RELAXED_SIMD=1
  -DWAMR_BUILD_BULK_MEMORY=1
  # Extended constant expressions (i32/i64 add/sub/mul in initializers):
  # only instantiation-time const-expr evaluation changes, no hot-path
  # cost. GC / typed function references (WAMR_BUILD_GC=1) also work in
  # fast-interp but cost +17-42% on call-heavy workloads (fib, vtable,
  # call_indirect, graphql; M4, 2026-09-22), so the benchmark build
  # leaves GC off.
  -DWAMR_BUILD_EXTENDED_CONST_EXPR=1
  -DWAMR_BUILD_TAIL_CALL=1
  -DWAMR_BUILD_REF_TYPES=1
  # Legacy wasm exceptions — needed for Porffor-compiled wasm, which
  # lowers JS try/catch/throw to legacy EH. WAMR upstream forbids
  # `WAMR_BUILD_EXCE_HANDLING=1` together with `FAST_INTERP=1`
  # (build-scripts/unsupported_combination.cmake); patches 0001-0017
  # implement legacy EH in fast-interp and lift the ban. Limits: an
  # exception payload cannot cross a function boundary (traps, 0014), a
  # br to a loop entry from inside a try region is rejected at load
  # (0015), and exnref (try_table / throw_ref) is not implemented.
  -DWAMR_BUILD_EXCE_HANDLING=1
  -DWAMR_BUILD_MULTI_MODULE=0
  -DWAMR_BUILD_LIB_PTHREAD=0
  -DWAMR_BUILD_MINI_LOADER=0
  -DWAMR_DISABLE_HW_BOUND_CHECK=1
)

# Extra -D flags for experiment builds (e.g. feature-matrix probes):
#   WAMR_EXTRA_DEFS="-DWAMR_BUILD_GC=1" scripts/build-wamr.sh macos
# shellcheck disable=SC2206
COMMON_DEFS+=( ${WAMR_EXTRA_DEFS:-} )

# Target-cpu apple-a12 to match the Rust side's `-C target-cpu=apple-a12`;
# tvOS keeps the Apple TV HD's (A8) ARMv8.0 baseline, as the Rust side does.
COMMON_CFLAGS="-O3"
CPU_A12="-mcpu=apple-a12"
CPU_TVOS="-mcpu=apple-a7"

# `-DWASM_LINMEM_RESERVATION_CAP=<bytes>` opts into WAMR's PROT_NONE
# linear-memory reservation path
# (`patches/wasm-micro-runtime/0028-…`). Without it, every
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
#   64 MB on arm64 (iOS / macOS / tvOS) — Porffor's working set is
#     ~6-10 MB, so 64 MB gives generous headroom. (Pulley reserves
#     256 MiB per memory; see pulley_engine() in benchmark-core.)
LINMEM_CAP_64="-DWASM_LINMEM_RESERVATION_CAP=67108864"   # 64 MB
LINMEM_CAP_32="-DWASM_LINMEM_RESERVATION_CAP=16777216"   # 16 MB

# WAMR compiles every core/iwasm/common/*.c, so libiwasm.a always carries
# the standard wasm-c-api layer (wasm_c_api.c.o: wasm_engine_new,
# wasm_store_new, wasm_func_call, ... 265 symbols). zwasm v2's libzwasm.a
# exports the same standard names, and the linker silently binds whichever
# archive member it meets first: zwasm_instance_new_ex was handed WAMR's
# store and module and crashed. The harness drives WAMR only through
# wasm_runtime_* (wasm_export.h), so drop WAMR's c-api member. Its one
# remaining reference, wasm_trap_delete in the c-api native-call bridge
# (wasm_runtime_invoke_c_api_native, reachable only for host functions
# registered through wasm-c-api, which the harness never does), gets a
# weak no-op so WAMR-only links still resolve.
# $1 = libiwasm.a, $2 = C compiler, remaining args = target C flags
drop_wasm_c_api() {
  local LIB="$1" CC="$2"; shift 2
  local STUB_DIR
  STUB_DIR="$(dirname "${LIB}")/wamr-c-api-stub"
  mkdir -p "${STUB_DIR}"
  ar d "${LIB}" wasm_c_api.c.o 2>/dev/null || true
  printf '%s\n' \
    '/* weak stand-in for the dropped wasm-c-api layer; see build-wamr.sh */' \
    '__attribute__((weak)) void wasm_trap_delete(void *trap) { (void)trap; }' \
    > "${STUB_DIR}/wamr_c_api_stub.c"
  "${CC}" "$@" -c "${STUB_DIR}/wamr_c_api_stub.c" -o "${STUB_DIR}/wamr_c_api_stub.o"
  ar rs "${LIB}" "${STUB_DIR}/wamr_c_api_stub.o"
}

build_macos() {
  local DIR="${WAMR}/product-mini/platforms/darwin/build"
  rm -rf "${DIR}" && mkdir -p "${DIR}"
  ( cd "${DIR}" && cmake .. "${COMMON_DEFS[@]}" \
      -DCMAKE_C_FLAGS="${COMMON_CFLAGS} ${CPU_A12} ${LINMEM_CAP_64}"
    make -j8 )
  drop_wasm_c_api "${DIR}/libiwasm.a" "$(xcrun --sdk macosx --find clang)" ${COMMON_CFLAGS} ${CPU_A12}
}

# Cross-compile for an Apple non-host target.
#
# $1 = output dir name (e.g. build-arm64_32-apple-watchos)
# $2 = which CMakeLists subdir (darwin or ios)
# $3 = arch (arm64_32 / arm64)
# $4 = SDK (watchos / watchsimulator / iphoneos / iphonesimulator)
# $5 = WAMR_BUILD_TARGET (AARCH64 / AARCH64_ILP32 / etc)
# $6 = deployment-target flag (e.g. -mwatchos-version-min=11.0)
# $7 = -mcpu flag (default ${CPU_A12})
build_target() {
  local OUTDIR="$1"; local SUBDIR="$2"; local ARCH="$3"
  local SDK="$4"; local WAMR_TARGET="$5"; local DEPMIN="$6"
  local CPU="${7:-${CPU_A12}}"

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
      -DCMAKE_C_FLAGS="${COMMON_CFLAGS} ${CPU} ${LINMEM_CAP} -arch ${ARCH} -isysroot ${SYSROOT} ${DEPMIN}" \
      -DCMAKE_ASM_FLAGS="${CPU} -arch ${ARCH} -isysroot ${SYSROOT} ${DEPMIN}" \
      -DCMAKE_EXE_LINKER_FLAGS="-arch ${ARCH} -isysroot ${SYSROOT} ${DEPMIN}"
    make -j8 iwasm_static 2>/dev/null || make -j8 vmlib 2>/dev/null || make -j8 )
  ls -la "${DIR}/libiwasm.a" 2>/dev/null || \
    (echo "ERROR: ${DIR}/libiwasm.a missing"; ls "${DIR}"; exit 2)
  drop_wasm_c_api "${DIR}/libiwasm.a" "${CC}" ${COMMON_CFLAGS} ${CPU} -arch "${ARCH}" \
    -isysroot "${SYSROOT}" ${DEPMIN}
}

# NB: WAMR's product-mini/platforms/ios/CMakeLists.txt hard-codes
# `add_library(iwasm SHARED ...)` and overrides BUILD_SHARED_LIBS, which
# produces a `.dylib` we can't link into a Rust staticlib. The
# `darwin/CMakeLists.txt` builds `vmlib` as a respect-BUILD_SHARED_LIBS
# library and works for every Apple target including iOS / watchOS, so
# we route both iOS variants through it.
build_ios()           { build_target "build-aarch64-apple-ios"          darwin  "arm64"    iphoneos         AARCH64 "-miphoneos-version-min=17.0"; }
build_ios_sim()       { build_target "build-aarch64-apple-ios-sim"      darwin  "arm64"    iphonesimulator  AARCH64 "-miphoneos-version-min=17.0 -target arm64-apple-ios17.0-simulator"; }
build_watchos()       { build_target "build-arm64_32-apple-watchos"     darwin  "arm64_32" watchos          AARCH64 "-mwatchos-version-min=11.0"; }
build_watchos_arm64() { build_target "build-aarch64-apple-watchos"      darwin  "arm64"    watchos          AARCH64 "-mwatchos-version-min=11.0"; }
build_watchos_sim()   { build_target "build-aarch64-apple-watchos-sim"  darwin  "arm64"    watchsimulator   AARCH64 "-mwatchos-version-min=11.0 -target arm64-apple-watchos11.0-simulator"; }
build_tvos()          { build_target "build-aarch64-apple-tvos"         darwin  "arm64"    appletvos        AARCH64 "-mtvos-version-min=18.0" "${CPU_TVOS}"; }
build_tvos_sim()      { build_target "build-aarch64-apple-tvos-sim"     darwin  "arm64"    appletvsimulator AARCH64 "-mtvos-version-min=18.0 -target arm64-apple-tvos18.0-simulator" "${CPU_TVOS}"; }
build_visionos()      { build_target "build-aarch64-apple-visionos"     darwin  "arm64"    xros             AARCH64 "-target arm64-apple-xros26.0"; }
build_visionos_sim()  { build_target "build-aarch64-apple-visionos-sim" darwin  "arm64"    xrsimulator      AARCH64 "-target arm64-apple-xros26.0-simulator"; }

case "${WHICH}" in
  macos)        build_macos ;;
  ios)          build_ios ;;
  ios-sim)      build_ios_sim ;;
  watchos)      build_watchos ;;
  watchos-arm64) build_watchos_arm64 ;;
  watchos-sim)  build_watchos_sim ;;
  tvos)         build_tvos ;;
  tvos-sim)     build_tvos_sim ;;
  visionos)     build_visionos ;;
  visionos-sim) build_visionos_sim ;;
  all)          build_macos && build_ios && build_ios_sim && build_watchos && build_watchos_arm64 && build_watchos_sim && build_tvos && build_tvos_sim && build_visionos && build_visionos_sim ;;
  *) echo "unknown target: ${WHICH}" >&2; exit 2 ;;
esac
