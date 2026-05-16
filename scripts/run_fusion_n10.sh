#!/usr/bin/env bash
# N=10 wallclock sweep on iPhone 12 for fusion experiment.
#
# Launches the app, watches the per-rep log for the expected
# result-line count (8 lines: Pulley {call_indirect, xmrsplayer,
# vtable_{mono,bi,poly4,poly6}} + WAMR {call_indirect, xmrsplayer}),
# then terminates the app cleanly. Per AGENTS.md → "Bash launcher
# gotcha — line buffering", devicectl --console is wrapped in
# stdbuf -oL.
#
# Usage:
#   COND=baseline OUTDIR=out/exp-fusion-xband-brif/n10 scripts/run_fusion_n10.sh
#   COND=fusion   OUTDIR=out/exp-fusion-xband-brif/n10 scripts/run_fusion_n10.sh

set -uo pipefail

UDID="${UDID:-B5D4CA48-8949-525C-8E5D-4F661161BD9D}"
BUNDLE="${BUNDLE:-com.rebeckerspecialties.wasmbench.ios}"
COND="${COND:?need COND=baseline|fusion}"
OUTDIR="${OUTDIR:?need OUTDIR}"
N="${N:-10}"
WORKLOADS="${WORKLOADS:-call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6}"
RUNTIMES="${RUNTIMES:-}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-2000}"
# Default matches the default WORKLOADS set above run by BOTH Pulley AND WAMR
# (6 workloads × 2 runtimes = 12 `[[...]]` result lines per launch). Override
# when you add/remove workloads or restrict RUNTIMES: e.g. for the 8-workload
# set including graphql-validation (AS+Porffor variants) → EXPECTED_LINES=16.
EXPECTED_LINES="${EXPECTED_LINES:-12}"
MAX_WAIT_SECS="${MAX_WAIT_SECS:-180}"

mkdir -p "${OUTDIR}"

ENV_JSON='{"WORKLOADS":"'"${WORKLOADS}"'","BENCH_TARGET_MS":"'"${BENCH_TARGET_MS}"'"'
if [ -n "${RUNTIMES}" ]; then
  ENV_JSON+=',"RUNTIMES":"'"${RUNTIMES}"'"'
fi
ENV_JSON+='}'

echo "[start] cond=${COND} N=${N} workloads=${WORKLOADS} runtimes=${RUNTIMES:-(any)} BENCH_TARGET_MS=${BENCH_TARGET_MS}"

for i in $(seq 1 "${N}"); do
  OUT="${OUTDIR}/iphone12-${COND}-r${i}.log"
  echo "[rep ${i}/${N}] -> ${OUT}"

  stdbuf -oL xcrun devicectl device process launch \
    --console \
    --device "${UDID}" \
    --terminate-existing \
    --environment-variables "${ENV_JSON}" \
    "${BUNDLE}" > "${OUT}" 2>&1 &
  LPID=$!

  # Wait for EXPECTED_LINES result lines (or timeout)
  TS=0
  while [ "${TS}" -lt "${MAX_WAIT_SECS}" ]; do
    sleep 2
    TS=$((TS + 2))
    CNT=$(grep -cE '^\[\[' "${OUT}" 2>/dev/null)
    CNT=${CNT:-0}
    if [ "${CNT}" -ge "${EXPECTED_LINES}" ]; then
      break
    fi
  done

  APPPID=$(xcrun devicectl device info processes --device "${UDID}" 2>/dev/null | grep -i wasmbench | awk '{print $1}' | head -1)
  [ -n "${APPPID}" ] && xcrun devicectl device process terminate --device "${UDID}" --pid "${APPPID}" >/dev/null 2>&1
  wait "${LPID}" 2>/dev/null

  CNT=$(grep -cE '^\[\[' "${OUT}" 2>/dev/null)
  CNT=${CNT:-0}
  echo "  -> ${CNT} result lines in ${TS}s"

  if [ "${CNT}" -lt "${EXPECTED_LINES}" ]; then
    echo "  [warn] expected ${EXPECTED_LINES}, got ${CNT}; check ${OUT}"
  fi

  sleep 1
done

echo "[done] ${OUTDIR}/iphone12-${COND}-r*.log"
