#!/usr/bin/env bash
# M4 per-case memory pass: one process per (runtime, case) under
# `taskpolicy -b`, so the process-lifetime phys_footprint peak that
# run_matrix records (task_vm_info ledger_phys_footprint_peak) is that one
# case's peak. The timing passes run every case of a runtime in one
# process, where both the footprint peak and resident_size_max only ever
# grow; and resident_size_max also keeps pages the allocator has freed but
# not returned (MADV_FREE_REUSABLE), so it cannot tell a leak from a cache.
#
# The window is the timing passes' (BENCH_TARGET_MS=2000), so a runtime
# that leaks per call or per instantiation shows the growth those passes
# ran with.
#
# Usage: scripts/run-m4-memory-pass.sh <out-dir>
#   RUNTIMES_LIST="pulley wamr wasm3 wasmedge zwasm wasmz tinywasm"
#   CASES="..."    case ids (default: every case in cases.rs)
# Output: <out-dir>/memory.jsonl (run_matrix lines).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:?usage: run-m4-memory-pass.sh <out-dir>}"
RUNTIMES_LIST="${RUNTIMES_LIST:-pulley wamr wasm3 wasmedge zwasm wasmz tinywasm}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-2000}"
MATRIX="${ROOT}/target/release/run_matrix"
[[ -x "${MATRIX}" ]] || ./scripts/build-host-cli.sh --bin run_matrix
CASES="${CASES:-$(python3 - <<'EOF'
import re
src = open("crates/benchmark-core/src/cases.rs").read()
body = src[src.index("pub const CASES"):]
body = body[:body.index("];")]
print(" ".join(re.findall(r'(?:c\(\s*|id:\s*)"([^"]+)"', body)))
EOF
)}"
mkdir -p "${OUT}"
echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ) git: $(git rev-parse --short HEAD) BENCH_TARGET_MS=${BENCH_TARGET_MS}" \
  > "${OUT}/memory-pass-info.txt"
for rt in ${RUNTIMES_LIST}; do
  t0=$(date +%s)
  for case in ${CASES}; do
    RUNTIMES="${rt}" WORKLOADS="=${case}" BENCH_TARGET_MS="${BENCH_TARGET_MS}" \
      MATRIX_JSONL="${OUT}/memory.jsonl" taskpolicy -b "${MATRIX}" > /dev/null 2>&1
  done
  echo "[memory] ${rt}: $(( $(date +%s) - t0 ))s"
done
echo "[done] ${OUT}"
