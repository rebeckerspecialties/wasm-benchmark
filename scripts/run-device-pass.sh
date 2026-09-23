#!/usr/bin/env bash
# iPhone measurement pass: N reps × one app launch per runtime, each launch
# with RUNTIMES=<runtime> so every runtime runs in its own process (no
# cross-runtime heap or cache state, and one runtime's crash cannot take
# the others' rows with it). The app runs its worker at .utility QoS
# (E-cores) unless BENCH_QOS says otherwise, and every result line records
# the measured E-core share of its timed window.
#
# The app does not exit by itself: each launch streams its console to a log
# until the completion marker appears (BENCH_DONE for the workload list,
# "FEMTOVG_E2E done" in E2E mode), then the app is terminated. A launch that
# produces no output at all (a dropped device tunnel) is retried.
#
# Usage: scripts/run-device-pass.sh <out-dir>
#   N=10                         reps
#   RUNTIMES_LIST="pulley wamr wasm3 wasmedge zwasm wasmz tinywasm"
#   BENCH_TARGET_MS=2000         timed-window budget per case
#   WORKLOADS=                   optional case-label filter (app semantics)
#   E2E=                         e.g. "0,1": run the femtovg E2E on these
#                                scenes instead of the workload list, one
#                                launch per (runtime, scene) so each
#                                scene's footprint peak is its own
#   FEMTOVG_FRAMES=121 FEMTOVG_PASSES=2
#   UDID=00008020-001C292A2190003A (iPhone XS Max)  DEVICE_NAME=iphonexs
#   BUNDLE=com.rebeckerspecialties.wasmbench.ios
#   MAX_WAIT_SECS=2400           per launch
#   SPLIT_RUNTIMES="zwasm"       runtimes whose heavy rows each get a launch
#   HEAVY_ROWS="xmrsplayer;graphql-validation (porffor);extended-const instantiate;extended-const twin;memory64;tail-call fsm"
#                                ';'-separated label substrings. On the iPhone
#                                zwasm's footprint reaches ~1.3 GB during
#                                xmrsplayer and the next heavy row gets the
#                                app jetsam-killed, which would lose every
#                                later row of the launch.
#
# Logs: <out-dir>/<device>-<runtime>-r<rep>.log (-h<i> for split rows,
# <device>-<runtime>-s<scene>-r<rep>.log in E2E mode). Parse with
# scripts/summarize-pass.py.
set -uo pipefail
OUT="${1:?usage: run-device-pass.sh <out-dir>}"
N="${N:-10}"
RUNTIMES_LIST="${RUNTIMES_LIST:-pulley wamr wasm3 wasmedge zwasm wasmz tinywasm}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-2000}"
WORKLOADS="${WORKLOADS:-}"
E2E="${E2E:-}"
UDID="${UDID:-00008020-001C292A2190003A}"
DEVICE_NAME="${DEVICE_NAME:-iphonexs}"
BUNDLE="${BUNDLE:-com.rebeckerspecialties.wasmbench.ios}"
MAX_WAIT_SECS="${MAX_WAIT_SECS:-2400}"
SPLIT_RUNTIMES=" ${SPLIT_RUNTIMES-zwasm} "
HEAVY_ROWS="${HEAVY_ROWS:-xmrsplayer;graphql-validation (porffor);extended-const instantiate;extended-const twin;memory64;tail-call fsm}"
if [[ -n "${E2E}" ]]; then MARKER="FEMTOVG_E2E done"; else MARKER="BENCH_DONE"; fi
mkdir -p "${OUT}"

env_json() {  # runtime [workloads] [exclude] [e2e scenes]
  local rt="$1" wl="${2:-${WORKLOADS}}" ex="${3:-}" scenes="${4:-${E2E}}" j
  j="{\"RUNTIMES\":\"${rt}\",\"BENCH_TARGET_MS\":\"${BENCH_TARGET_MS}\""
  [[ -n "${wl}" ]] && j+=",\"WORKLOADS\":\"${wl}\""
  [[ -n "${ex}" ]] && j+=",\"WORKLOADS_EXCLUDE\":\"${ex}\""
  if [[ -n "${scenes}" ]]; then
    j+=",\"FEMTOVG_E2E\":\"${scenes}\",\"FEMTOVG_FRAMES\":\"${FEMTOVG_FRAMES:-121}\""
    j+=",\"FEMTOVG_PASSES\":\"${FEMTOVG_PASSES:-2}\""
  fi
  echo "${j}}"
}

app_pid() {
  xcrun devicectl device info processes --device "${UDID}" 2>/dev/null \
    | grep -i "wasmbench" | awk '{print $1}' | head -1
}

launch_once() {  # runtime log [workloads] [exclude] [e2e scenes] -> 0 if the marker arrived
  local rt="$1" log="$2" lpid ts=0
  stdbuf -oL xcrun devicectl device process launch --console --device "${UDID}" \
    --terminate-existing --environment-variables "$(env_json "${rt}" "${3:-}" "${4:-}" "${5:-}")" \
    "${BUNDLE}" > "${log}" 2>&1 &
  lpid=$!
  while (( ts < MAX_WAIT_SECS )); do
    sleep 3
    ts=$((ts + 3))
    grep -q "^${MARKER}" "${log}" 2>/dev/null && break
    # The launch ended without the marker (app crash, tunnel drop).
    kill -0 "${lpid}" 2>/dev/null || break
  done
  local pid
  pid="$(app_pid)"
  [[ -n "${pid}" ]] && xcrun devicectl device process terminate --device "${UDID}" --pid "${pid}" \
    > /dev/null 2>&1
  kill "${lpid}" 2> /dev/null
  wait "${lpid}" 2> /dev/null
  echo "${ts}" > "${log}.secs"
  grep -q "^${MARKER}" "${log}"
}

echo "[start] ${DEVICE_NAME} N=${N} runtimes=(${RUNTIMES_LIST}) BENCH_TARGET_MS=${BENCH_TARGET_MS}" \
  "${E2E:+E2E scenes=${E2E}}"
run_launch() {  # runtime log [workloads] [exclude] [e2e scenes]
  local rt="$1" log="$2" ok=1
  for attempt in 1 2 3; do
    if launch_once "$@"; then ok=0; break; fi
    # Retry only a launch that produced no result lines at all, and not
    # one the OS killed (jetsam's SIGKILL is a result, not a tunnel drop).
    if grep -qE '^\[\[|^FEMTOVG_E2E \{' "${log}"; then break; fi
    if grep -q "terminated due to signal" "${log}"; then break; fi
    echo "   $(basename "${log}"): no output (attempt ${attempt}), retrying"
    sleep 5
  done
  lines=$(grep -cE '^\[\[|^FEMTOVG_E2E \{' "${log}" || true)
  echo "[rep ${rep}/${N}] $(basename "${log}" .log): ${lines} result lines in $(cat "${log}.secs")s$([[ ${ok} -ne 0 ]] && echo ' (no completion marker)')"
  sleep 2
}

for rep in $(seq 1 "${N}"); do
  for rt in ${RUNTIMES_LIST}; do
    base="${OUT}/${DEVICE_NAME}-${rt}-r${rep}"
    if [[ -n "${E2E}" ]]; then
      IFS=',' read -ra scenes <<< "${E2E}"
      for sc in "${scenes[@]}"; do
        run_launch "${rt}" "${OUT}/${DEVICE_NAME}-${rt}-s${sc}-r${rep}.log" "" "" "${sc}"
      done
    elif [[ -z "${WORKLOADS}" && "${SPLIT_RUNTIMES}" == *" ${rt} "* ]]; then
      run_launch "${rt}" "${base}.log" "" "$(tr ';' ',' <<< "${HEAVY_ROWS}")"
      i=0
      IFS=';' read -ra heavy <<< "${HEAVY_ROWS}"
      for row in "${heavy[@]}"; do
        i=$((i + 1))
        run_launch "${rt}" "${base}-h${i}.log" "${row}"
      done
    else
      run_launch "${rt}" "${base}.log"
    fi
  done
done
echo "[done] ${OUT}"
