#!/usr/bin/env bash
# Interleaved A/B of tinywasm builds on an attached iPhone's E-cores. Every rep
# installs and launches each variant once, in an order rotated per rep, so
# drift during the run (thermal state, background work) hits all variants
# alike. Build the variants first with scripts/tinywasm-ab-build-ios.sh.
#
# Usage: scripts/tinywasm-ab-iphone.sh <out-dir> <variant>...
#   REPS=5  UDID=00008101-000A044A3C28801E (iPhone 12)  DEVICE_NAME=iphone12
#   WORKLOADS=<the 16 tinywasm rows below>  BENCH_TARGET_MS=2000
# Logs: <out-dir>/<variant>/<device>-r<rep>-tinywasm-r1.log; summarize with
# scripts/tinywasm_ab_summary.py <out-dir> <variant>...
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
O="${1:?usage: tinywasm-ab-iphone.sh <out-dir> <variant>...}"
shift
VARIANTS=("$@")
[ ${#VARIANTS[@]} -gt 0 ] || { echo "no variants" >&2; exit 1; }
REPS="${REPS:-5}"
UDID="${UDID:-00008101-000A044A3C28801E}"
DEVICE_NAME="${DEVICE_NAME:-iphone12}"
W="${WORKLOADS:-fib(30),call_indirect (200k,xmrsplayer,graphql-validation (as),crc32(64kb) [scalar,convolution 256×256 [scalar,audio dsp,vtable_poly4,sieve(10000) [scalar,bulk_memory (memory.copy/fill) [scalar,tail-call fsm,exnref (4096,gc binary trees,matmul relaxed-simd fma,graphql-validation (porffor)}"
for v in "${VARIANTS[@]}"; do
  app="apps/build/DerivedData-tw-${v}/Build/Products/Release-iphoneos/WasmBenchmarkIOS.app"
  [ -d "${app}" ] || { echo "missing ${app}: build it with scripts/tinywasm-ab-build-ios.sh" >&2; exit 1; }
done
mkdir -p "${O}"
n=${#VARIANTS[@]}
for rep in $(seq 1 "${REPS}"); do
  for i in $(seq 0 $((n - 1))); do
    v="${VARIANTS[$(( (i + rep - 1) % n ))]}"
    app="apps/build/DerivedData-tw-${v}/Build/Products/Release-iphoneos/WasmBenchmarkIOS.app"
    ok=0
    for _ in 1 2 3 4 5; do
      if xcrun devicectl device install app --device "${UDID}" "${app}" > /dev/null 2>&1; then ok=1; break; fi
      sleep 3
    done
    [ "${ok}" = 1 ] || { echo "[rep ${rep}] install of ${v} failed"; continue; }
    N=1 RUNTIMES_LIST=tinywasm WORKLOADS="${W}" UDID="${UDID}" DEVICE_NAME="${DEVICE_NAME}-r${rep}" \
      ./scripts/run-device-pass.sh "${O}/${v}" | grep -v '^\[start\]\|^\[done\]' | sed "s/^/[rep ${rep}] ${v}: /"
  done
done
echo "[done] ${O} $(date +%H:%M:%S)"
