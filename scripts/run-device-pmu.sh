#!/usr/bin/env bash
# Per-(runtime, workload) PMU and sampling profiles on an attached iPhone
# whose PMU xctrace can read (A14 iPhone 12: yes; A12 iPhone XS: no).
#
# For each runtime × workload × mode, xctrace launches the app with only
# that row (RUNTIMES / WORKLOADS filters, passed with --env) and a timed
# window longer than the capture, records CAPTURE_MS, and the trace is
# exported, reduced and deleted. (With Xcode 27 and iOS 26.5, --attach
# no longer finds the app's process, while --launch records both counters
# and samples; with Xcode 26.5 it was the other way round.) The capture
# includes app start and the row's load and warmup; the reductions use
# the busiest thread, the benchmark worker.
#   - guided CPU Counters modes (the Recount event sets for the SoC, as in
#     scripts/run-m4-pmu-pass.sh) -> pmu.jsonl via scripts/pmu_summarize.py
#     (all threads of the app; the benchmark worker is the busiest one)
#   - `timeprofile`: the Time Profiler template -> self and inclusive
#     frame histograms of the benchmark thread (the Release app keeps its
#     symbol table, so frames resolve to tinywasm / WAMR handler names)
#     -> profile-<runtime>-<workload>.txt
# Rates are per 1k cycles of the worker thread (cycles are in every
# capture); IPC comes from a normal run of the same rows (the app's result
# lines, `ipc=`), so per-instruction rates are cycles-rate / IPC.
#
# Usage: scripts/run-device-pmu.sh <out-dir>
#   UDID=00008101-000A044A3C28801E (iPhone 12; devicectl and xctrace IDs match)
#   RUNTIMES_LIST="tinywasm wamr"
#   WORKLOADS_LIST="fib(30);call_indirect (200K;xmrsplayer;graphql-validation (AS);crc32(64KB) [scalar;convolution 256×256 [scalar;audio DSP"
#                    ';'-separated label substrings, one row each
#   MODES="bottleneck:bottlenecks bottleneck:discarded_indirect_sampling metrics:l1d_metrics
#          metrics:instruction_address_translation_metrics characteristics:call_branch_instructions timeprofile"
#   MODES_<runtime>="..."   per-runtime override, e.g. MODES_wamr
#   CAPTURE_MS=12000 BENCH_TARGET_MS=30000
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:?usage: run-device-pmu.sh <out-dir>}"
UDID="${UDID:-00008101-000A044A3C28801E}"
XCTRACE_DEV="${XCTRACE_DEV:-${UDID}}"
BUNDLE="${BUNDLE:-com.rebeckerspecialties.wasmbench}"
RUNTIMES_LIST="${RUNTIMES_LIST:-tinywasm wamr}"
WORKLOADS_LIST="${WORKLOADS_LIST:-fib(30);call_indirect (200K;xmrsplayer;graphql-validation (AS);crc32(64KB) [scalar;convolution 256×256 [scalar;audio DSP}"
MODES="${MODES:-bottleneck:bottlenecks bottleneck:discarded_indirect_sampling metrics:l1d_metrics metrics:instruction_address_translation_metrics characteristics:call_branch_instructions timeprofile}"
CAPTURE_MS="${CAPTURE_MS:-12000}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-30000}"
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

app_pid() {
  xcrun devicectl device info processes --device "${UDID}" 2>/dev/null \
    | grep -i "wasmbench" | awk '{print $1}' | head -1
}

stop_app() {
  local pid
  pid="$(app_pid)"
  [[ -n "${pid}" ]] && xcrun devicectl device process terminate --device "${UDID}" --pid "${pid}" \
    > /dev/null 2>&1
}

slug() { tr -c 'A-Za-z0-9_\n' '_' <<< "$1" | sed 's/__*/_/g; s/_$//'; }

echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ) git: $(git rev-parse --short HEAD) device: ${UDID}" \
  "modes: ${MODES}" > "${OUT}/device-pmu-info.txt"
IFS=';' read -ra WLS <<< "${WORKLOADS_LIST}"
for rt in ${RUNTIMES_LIST}; do
  modes_var="MODES_${rt}"
  rt_modes="${!modes_var:-${MODES}}"
  for wl in "${WLS[@]}"; do
    ws="$(slug "${wl}")"
    for mode in ${rt_modes}; do
      label="${rt}-${ws}-${mode//:/_}"
      log="${OUT}/logs/${label}.log"
      trace="${TMP}/${label}.trace"
      if [[ "${mode}" == timeprofile ]]; then
        tmpl=(--template "Time Profiler")
      else
        tmpl=(--template "CPU Counters" --recording-options "$(opts_for "${mode}")")
      fi
      xcrun xctrace record --device "${XCTRACE_DEV}" "${tmpl[@]}" --time-limit "${CAPTURE_MS}ms" \
        --no-prompt --env "RUNTIMES=${rt}" --env "WORKLOADS=${wl}" \
        --env "BENCH_TARGET_MS=${BENCH_TARGET_MS}" --output "${trace}" --launch -- "${BUNDLE}" \
        > "${log}" 2>&1
      stop_app
      if [[ ! -d "${trace}" ]]; then
        echo "[fail] ${label}: no trace"
        continue
      fi
      if [[ "${mode}" == timeprofile ]]; then
        xcrun xctrace export --input "${trace}" \
          --xpath '//trace-toc/run/data/table[@schema="time-profile"]' \
          --output "${TMP}/${label}.xml" >> "${log}" 2>&1
        python3 scripts/profile_summarize.py "${TMP}/${label}.xml" 60 \
          > "${OUT}/profile-${rt}-${ws}.txt" 2>> "${log}"
      else
        xcrun xctrace export --input "${trace}" \
          --xpath '//trace-toc/run/data/table[@schema="MetricAggregationForThread"]' \
          --output "${TMP}/${label}.xml" >> "${log}" 2>&1
        python3 scripts/pmu_summarize.py "${TMP}/${label}.xml" "${rt}" "${mode}" "${wl}" \
          | python3 -c 'import json,sys
for l in sys.stdin: print(json.dumps({"workload": sys.argv[1], **json.loads(l)}))' "${wl}" \
          >> "${OUT}/pmu.jsonl"
      fi
      echo "[ok] ${label} $(du -sh "${trace}" | cut -f1)"
      rm -rf "${trace}" "${TMP}/${label}.xml"
    done
  done
done
echo "[done] ${OUT}"
