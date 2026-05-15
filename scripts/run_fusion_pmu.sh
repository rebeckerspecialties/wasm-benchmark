#!/usr/bin/env bash
# PMU attach-mode capture on iPhone 12 for the fusion experiment.
# Uses the xctrace 26.5 attach workaround from AGENTS.md →
# "PMU / xctrace gotchas".
#
# Usage:
#   COND=baseline OUTDIR=out/exp-fusion-xband-brif/pmu scripts/run_fusion_pmu.sh
#   COND=fusion   OUTDIR=out/exp-fusion-xband-brif/pmu scripts/run_fusion_pmu.sh

set -uo pipefail

UDID="${UDID:-B5D4CA48-8949-525C-8E5D-4F661161BD9D}"
XCTRACE_DEV="${XCTRACE_DEV:-00008101-000A044A3C28801E}"
BUNDLE="${BUNDLE:-com.rebeckerspecialties.wasmbench.ios}"
COND="${COND:?need COND=baseline|fusion}"
OUTDIR="${OUTDIR:?need OUTDIR}"
TEMPLATE="${TEMPLATE:-CPU Counters}"
# Filter to Pulley only — task brief: "filter to Pulley only via RUNTIMES=pulley"
WORKLOADS="${WORKLOADS:-call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6}"
RUNTIMES="${RUNTIMES:-pulley}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-15000}"
TIME_LIMIT_MS="${TIME_LIMIT_MS:-90000}"  # enough for 6 workloads × 15s budget + load

mkdir -p "${OUTDIR}"

TRACE="${OUTDIR}/iphone12-${COND}.trace"
LOGOUT="${OUTDIR}/iphone12-${COND}.log"
XMLOUT="${OUTDIR}/iphone12-${COND}.xml"

rm -rf "${TRACE}" "${LOGOUT}" "${XMLOUT}"

ENV_JSON='{"WORKLOADS":"'"${WORKLOADS}"'","BENCH_TARGET_MS":"'"${BENCH_TARGET_MS}"'"'
if [ -n "${RUNTIMES}" ]; then
  ENV_JSON+=',"RUNTIMES":"'"${RUNTIMES}"'"'
fi
ENV_JSON+='}'

echo "[start] cond=${COND} workloads=${WORKLOADS} runtimes=${RUNTIMES:-(any)} BENCH_TARGET_MS=${BENCH_TARGET_MS}"
echo "[start] xctrace template=\"${TEMPLATE}\" time-limit=${TIME_LIMIT_MS}ms"

# 1. Launch app via devicectl with long target so it's still running when xctrace attaches
stdbuf -oL xcrun devicectl device process launch \
  --console \
  --device "${UDID}" \
  --terminate-existing \
  --environment-variables "${ENV_JSON}" \
  "${BUNDLE}" > "${LOGOUT}" 2>&1 &
LPID=$!

# 2. Wait for app to actually start
sleep 4

# 3. Find on-device PID
PID=$(xcrun devicectl device info processes --device "${UDID}" 2>/dev/null | grep -i wasmbench | awk '{print $1}' | head -1)
if [ -z "${PID}" ]; then
  echo "[err] could not find on-device wasmbench PID after sleep 4"
  kill "${LPID}" 2>/dev/null
  exit 2
fi
echo "[start] on-device PID=${PID}, attaching xctrace..."

# 4. Attach xctrace
xcrun xctrace record \
  --device "${XCTRACE_DEV}" \
  --template "${TEMPLATE}" \
  --output "${TRACE}" \
  --attach "${PID}" \
  --time-limit "${TIME_LIMIT_MS}ms" 2>&1 | tail -10

# 5. Terminate app
APPPID=$(xcrun devicectl device info processes --device "${UDID}" 2>/dev/null | grep -i wasmbench | awk '{print $1}' | head -1)
[ -n "${APPPID}" ] && xcrun devicectl device process terminate --device "${UDID}" --pid "${APPPID}" >/dev/null 2>&1
wait "${LPID}" 2>/dev/null

# 6. Export CounterMetricByThread XML
xcrun xctrace export --input "${TRACE}" \
  --xpath '//trace-toc/run/data/table[@schema="CounterMetricByThread"]' \
  --output "${XMLOUT}" 2>&1 | tail -5

echo "[done] trace=${TRACE}"
echo "[done] xml=${XMLOUT} ($(wc -c < "${XMLOUT}" 2>/dev/null || echo 0) bytes)"
echo "[done] device-stderr=${LOGOUT}"
echo "[done] benchmark output:"
grep -E '^\[\[' "${LOGOUT}" | sed 's/^/    /'
