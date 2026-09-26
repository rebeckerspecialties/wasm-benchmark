#!/usr/bin/env bash
# M4 PMU pass, separate from the timing passes: every runtime x workload
# (the cases.rs matrix plus the femtovg E2E) on the E-cluster under
# `taskpolicy -b`, one xctrace "CPU Counters" capture per (runtime, mode).
#
# Events. The M4 Max's CPU Counters "manual" event lists cannot be set from
# xctrace's command line (its --recording-options JSON only accepts empty
# allEventsAndFormulas), so the pass uses the guided modes, each a fixed
# event set that /System/Library/PrivateFrameworks/Recount.framework/
# Resources/Analysis/{bottleneck,metrics,characteristics}.json defines for
# this SoC (t6041):
#   bottleneck:bottlenecks            useful / processing / delivery / discarded
#                                     slots (MAP_*, RETIRE_UOP, CORE_ACTIVE_CYCLE)
#   bottleneck:discarded_sampling     BRANCH_MISPRED_NONSPEC, BRANCH_COND_MISPRED_NONSPEC,
#                                     ST_MEM_ORDER_VIOL_LD_NONSPEC
#   bottleneck:discarded_indirect_sampling  BRANCH_INDIR/CALL_INDIR/RET_INDIR_MISPRED_NONSPEC
#   metrics:l1d_metrics               L1D_CACHE_MISS_LD / _ST, L1D_CACHE_WRITEBACK,
#                                     LD_UNIT_UOP, ST_UNIT_UOP
#   metrics:instruction_address_translation_metrics
#                                     L1I_CACHE_MISS_DEMAND, L1I_TLB_MISS_DEMAND, L1I_TLB_FILL,
#                                     L2_TLB_MISS_INSTRUCTION, MMU_TABLE_WALK_INSTRUCTION,
#                                     FETCH_RESTART
#   metrics:data_address_translation_metrics
#                                     L1D_TLB_ACCESS / _MISS / _FILL, L2_TLB_MISS_DATA,
#                                     MMU_TABLE_WALK_DATA
#   characteristics:call_branch_instructions
#                                     INST_BRANCH_INDIR, INST_BRANCH_CALL, INST_BRANCH_COND
#                                     (retired indirect branches: an interpreter's
#                                     dispatch density, and the denominator of its
#                                     indirect mispredict rate)
# The M4 core PMU has no L2/SLC miss event and no prefetch event (none in its
# kpep database, /usr/share/kpep/cpu_100000c_2_17d5b93a.plist): misses and
# walks are the proxies.
#
# Attribution. run_matrix runs every case on its own `case:<id>` thread
# (MATRIX_THREAD_PER_CASE=1) and the E2E runs on `femtovg-e2e`, so the
# per-thread table (MetricAggregationForThread) attributes the counters;
# each run_matrix JSON line also records the case's instructions and
# cycles (process rusage over the whole case) as the per-1k-instruction
# denominator. Traces are exported, reduced by scripts/pmu_summarize.py and
# deleted at once; no .trace is kept.
#
# Disk. xctrace writes its raw kernel trace to instruments*.ktrace in the
# user temp dir at ~100 MB/s of recording on this machine (1.5-1.8 GB for
# 15 s, any mode) and leaves it behind; a single capture of Pulley's whole
# matrix (with sqlite3's ~50 s call) reached 11 GB and filled the disk. So
# captures stay short: 100 ms timed windows, audio_dsp in a capture of its
# own, sqlite3 left out (Pulley-only, ~50 s per call), the E2E at 31
# frames, and each capture's .ktrace is deleted after its export.
#
# Usage: scripts/run-m4-pmu-pass.sh <out-dir>
#   RUNTIMES_LIST="pulley wamr wasm3 wasmedge zwasm wasmz tinywasm"
#   MODES="...", BENCH_TARGET_MS=100, PARTS="matrix e2e", E2E_FRAMES=31
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:?usage: run-m4-pmu-pass.sh <out-dir>}"
RUNTIMES_LIST="${RUNTIMES_LIST:-pulley wamr wasm3 wasmedge zwasm wasmz tinywasm}"
MODES="${MODES:-bottleneck:bottlenecks bottleneck:discarded_sampling bottleneck:discarded_indirect_sampling metrics:l1d_metrics metrics:instruction_address_translation_metrics metrics:data_address_translation_metrics characteristics:call_branch_instructions}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-100}"
PARTS=" ${PARTS:-matrix e2e} "
E2E_FRAMES="${E2E_FRAMES:-31}"
KTRACE_DIR="$(getconf DARWIN_USER_TEMP_DIR)"
# Case groups, one capture each: every case but audio_dsp and sqlite3, then
# audio_dsp alone (5-6 s per call on the slowest runtimes).
GROUP_A="$(python3 -c '
import re
src = open("crates/benchmark-core/src/cases.rs").read()
body = src[src.index("pub const CASES"):]
body = body[:body.index("];")]
ids = re.findall(r"(?:c\(\s*|id:\s*)\"([^\"]+)\"", body)
print(",".join("=" + i for i in ids if i not in ("audio_dsp", "sqlite3")))
')"
GROUP_B="=audio_dsp"
MATRIX="${ROOT}/target/release/run_matrix"
E2E_BIN="${ROOT}/target/release/run_femtovg_e2e"
mkdir -p "${OUT}/logs"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

opts_for() {  # analysis:counting -> options JSON path
  local f="${TMP}/${1//:/_}.json"
  if [[ ! -f "${f}" ]]; then
    xcrun xctrace record --template "CPU Counters" --show-recording-options 2>/dev/null \
      | python3 -c 'import json,sys
d=json.load(sys.stdin); a,c=sys.argv[1].split(":")
d["CPU Counters"]["selectedCountingMode"]={"analysisMode":a,"countingMode":c}
json.dump(d, open(sys.argv[2],"w"))' "$1" "${f}"
  fi
  echo "${f}"
}

capture() {  # label mode e2e_case cmd... ; exports + summarizes, deletes the trace
  local label="$1" mode="$2" e2e_case="$3"; shift 3
  local trace="${TMP}/${label}.trace" xml="${TMP}/${label}.xml" t0
  t0=$(date +%s)
  rm -rf "${trace}" "${xml}"
  xcrun xctrace record --template "CPU Counters" --recording-options "$(opts_for "${mode}")" \
    --output "${trace}" --time-limit 60m --no-prompt "$@" > "${OUT}/logs/${label}.txt" 2>&1
  xcrun xctrace export --input "${trace}" \
    --xpath '//trace-toc/run/data/table[@schema="MetricAggregationForThread"]' --output "${xml}" \
    >> "${OUT}/logs/${label}.txt" 2>&1
  python3 scripts/pmu_summarize.py "${xml}" "${rt}" "${mode}" "${e2e_case}" >> "${OUT}/pmu.jsonl"
  echo "[pmu] ${label}: $(( $(date +%s) - t0 ))s, trace $(du -sh "${trace}" 2>/dev/null | cut -f1)," \
    "ktrace $(du -shc "${KTRACE_DIR}"/instruments*.ktrace 2>/dev/null | tail -1 | cut -f1), free $(df -h / | awk 'NR==2{print $4}')"
  rm -rf "${trace}" "${xml}" "${KTRACE_DIR}"/instruments*.ktrace
}

echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ) git: $(git rev-parse --short HEAD) modes: ${MODES}" \
  > "${OUT}/pmu-pass-info.txt"
mi=0
for mode in ${MODES}; do
  mi=$((mi + 1))
  for rt in ${RUNTIMES_LIST}; do
    if [[ "${PARTS}" == *" matrix "* ]]; then
      for g in A B; do
        wl="GROUP_${g}"
        capture "matrix${g}-${rt}-${mode//:/_}" "${mode}" "" \
          --env RUNTIMES="${rt}" --env WORKLOADS="${!wl}" --env BENCH_TARGET_MS="${BENCH_TARGET_MS}" \
          --env MATRIX_THREAD_PER_CASE=1 --env MATRIX_JSONL="${OUT}/pmu-matrix.jsonl" \
          --env MATRIX_REP="${mi}" --launch -- /usr/sbin/taskpolicy -b "${MATRIX}"
      done
    fi
    if [[ "${PARTS}" == *" e2e "* ]]; then
      for scene in 0 1; do
        capture "e2e-${rt}-s${scene}-${mode//:/_}" "${mode}" "femtovg_e2e.scene${scene}" \
          --launch -- /usr/sbin/taskpolicy -b "${E2E_BIN}" --runtime "${rt}" --scene "${scene}" \
          --frames "${E2E_FRAMES}" --passes 1 --rep "${mi}" --jsonl "${OUT}/pmu-e2e.jsonl"
      done
    fi
  done
done
echo "[done] ${OUT}"
