#!/usr/bin/env bash
# Per-workload PMU capture on iPhone 12.
#
# Unlike `run_fusion_pmu.sh` (which captures ALL workloads in one
# 90 s xctrace window), this captures ONE (workload, runtime) per
# trace. That gives clean per-workload cycle accounting — important
# for the cross-runtime comparison where we want to attribute
# Useful / Processing / Discarded changes to a specific workload.
#
# Usage:
#   COND=phase3 OUTDIR=out/exp-cross-runtime-pmu/phase3 \
#   WORKLOADS_LIST=call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6,graphql-validation \
#   RUNTIME=pulley \
#   scripts/run_per_workload_pmu.sh
#
# Per-call BENCH_TARGET_MS defaults to 12000 so each workload runs
# for ~12 s of pure iteration time, comfortably inside a 20 s xctrace
# attach window after setup overhead.

set -uo pipefail

UDID="${UDID:-B5D4CA48-8949-525C-8E5D-4F661161BD9D}"
XCTRACE_DEV="${XCTRACE_DEV:-00008101-000A044A3C28801E}"
BUNDLE="${BUNDLE:-com.rebeckerspecialties.wasmbench.ios}"
COND="${COND:?need COND}"
OUTDIR="${OUTDIR:?need OUTDIR}"
TEMPLATE="${TEMPLATE:-CPU Counters}"
RUNTIME="${RUNTIME:?need RUNTIME=pulley|wamr}"
WORKLOADS_LIST="${WORKLOADS_LIST:-call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6,graphql-validation}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-12000}"
TIME_LIMIT_MS="${TIME_LIMIT_MS:-20000}"

mkdir -p "${OUTDIR}"

# Note: graphql-validation matches both AS and Porffor variants. The
# loop splits them out so we get separate traces.
EXPANDED=""
IFS=',' read -ra WLS <<< "${WORKLOADS_LIST}"
for w in "${WLS[@]}"; do
  if [ "$w" = "graphql-validation" ]; then
    EXPANDED+=" graphql-validation_AS graphql-validation_Porffor"
  else
    EXPANDED+=" $w"
  fi
done

for wname in $EXPANDED; do
  # Map filter-token form back to the WORKLOADS env var format.
  case "$wname" in
    graphql-validation_AS)      filter="graphql-validation (AS)" ;;
    graphql-validation_Porffor) filter="graphql-validation (Porffor)" ;;
    *)                          filter="$wname" ;;
  esac
  echo "=== ${COND}/${RUNTIME}/${wname} ==="
  trace="${OUTDIR}/${COND}-${RUNTIME}-${wname}.trace"
  xml="${OUTDIR}/${COND}-${RUNTIME}-${wname}.xml"
  log="${OUTDIR}/${COND}-${RUNTIME}-${wname}.log"
  rm -rf "${trace}" "${xml}" "${log}"

  ENV_JSON='{"WORKLOADS":"'"${filter}"'","RUNTIMES":"'"${RUNTIME}"'","BENCH_TARGET_MS":"'"${BENCH_TARGET_MS}"'"}'

  stdbuf -oL xcrun devicectl device process launch \
    --console \
    --device "${UDID}" \
    --terminate-existing \
    --environment-variables "${ENV_JSON}" \
    "${BUNDLE}" > "${log}" 2>&1 &
  LPID=$!

  sleep 3
  PID=$(xcrun devicectl device info processes --device "${UDID}" 2>/dev/null | grep -i wasmbench | awk '{print $1}' | head -1)
  if [ -z "${PID}" ]; then
    echo "  [warn] could not find on-device wasmbench PID, skipping ${wname}"
    wait "${LPID}" 2>/dev/null
    continue
  fi
  echo "  on-device PID=${PID}, attaching xctrace for ${TIME_LIMIT_MS}ms..."

  xcrun xctrace record \
    --device "${XCTRACE_DEV}" \
    --template "${TEMPLATE}" \
    --output "${trace}" \
    --attach "${PID}" \
    --time-limit "${TIME_LIMIT_MS}ms" 2>&1 | tail -3

  APPPID=$(xcrun devicectl device info processes --device "${UDID}" 2>/dev/null | grep -i wasmbench | awk '{print $1}' | head -1)
  [ -n "${APPPID}" ] && xcrun devicectl device process terminate --device "${UDID}" --pid "${APPPID}" >/dev/null 2>&1
  wait "${LPID}" 2>/dev/null

  xcrun xctrace export --input "${trace}" \
    --xpath '//trace-toc/run/data/table[@schema="CounterMetricByThread"]' \
    --output "${xml}" 2>&1 | tail -3

  echo "  -> trace=${trace} xml=${xml}"
  bytes=$(wc -c < "${xml}" 2>/dev/null || echo 0)
  echo "  -> xml size: ${bytes} bytes"
  result=$(grep '^\[\[' "${log}" | head -1)
  echo "  -> bench: ${result}"
  sleep 1
done

echo "[done] ${OUTDIR}"
