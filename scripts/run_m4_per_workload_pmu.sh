#!/usr/bin/env bash
# Per-workload M4 host PMU capture, equivalent of run_per_workload_pmu.sh
# but for the macOS host's E-core via `taskpolicy -b` instead of an
# attached iPhone.
#
# Pre-conditions:
#   - ./target/release/run_dispatch_workloads is built with the wasmtime
#     branch under test (the binary path is the integration point).
#   - macOS host has Xcode 26+ and the `CPU Counters` template available.
#
# Usage:
#   COND=phase4 OUTDIR=out/exp-3way-m4/pmu-phase4 \
#   WORKLOADS_LIST=call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6,graphql-validation-as,graphql-validation-porf \
#   scripts/run_m4_per_workload_pmu.sh

set -uo pipefail

HOST_DEV="${HOST_DEV:-2A4B80BE-7BC1-53FB-8FBF-84FA73BD5206}"
COND="${COND:?need COND}"
OUTDIR="${OUTDIR:?need OUTDIR}"
TEMPLATE="${TEMPLATE:-CPU Counters}"
BIN="${BIN:-./target/release/run_dispatch_workloads}"
WORKLOADS_LIST="${WORKLOADS_LIST:-call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6,graphql-validation-as,graphql-validation-porf}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-12000}"
TIME_LIMIT_MS="${TIME_LIMIT_MS:-25000}"

BIN_ABS="$(cd "$(dirname "${BIN}")" && pwd)/$(basename "${BIN}")"
if [ ! -x "${BIN_ABS}" ]; then
  echo "[error] binary not executable: ${BIN_ABS}" >&2
  exit 1
fi

mkdir -p "${OUTDIR}"

IFS=',' read -ra WLS <<< "${WORKLOADS_LIST}"
for wname in "${WLS[@]}"; do
  echo "=== ${COND}/m4-ecore/${wname} ==="
  trace="${OUTDIR}/${COND}-m4-${wname}.trace"
  xml="${OUTDIR}/${COND}-m4-${wname}.xml"
  log="${OUTDIR}/${COND}-m4-${wname}.log"
  rm -rf "${trace}" "${xml}" "${log}"

  # `xctrace record --launch` runs the process under the trace. We
  # wrap with /usr/sbin/taskpolicy -b for E-core scheduling. The
  # WORKLOADS env var filters the runner to a single case so the
  # PMU window covers only that workload's iterations.
  WORKLOADS="${wname}" BENCH_TARGET_MS="${BENCH_TARGET_MS}" \
    xcrun xctrace record \
      --device "${HOST_DEV}" \
      --template "${TEMPLATE}" \
      --output "${trace}" \
      --time-limit "${TIME_LIMIT_MS}ms" \
      --launch -- \
      /usr/sbin/taskpolicy -b "${BIN_ABS}" > "${log}" 2>&1

  if [ ! -d "${trace}" ]; then
    echo "  [warn] no trace produced for ${wname}"
    continue
  fi

  xcrun xctrace export \
    --input "${trace}" \
    --xpath '//trace-toc/run/data/table[@schema="CounterMetricByThread"]' \
    --output "${xml}" >> "${log}" 2>&1

  if [ ! -s "${xml}" ]; then
    echo "  [retry] xml empty, re-exporting"
    sleep 1
    xcrun xctrace export \
      --input "${trace}" \
      --xpath '//trace-toc/run/data/table[@schema="CounterMetricByThread"]' \
      --output "${xml}" >> "${log}" 2>&1
  fi

  size=$(stat -f%z "${xml}" 2>/dev/null || echo 0)
  echo "  -> trace=${trace} xml=${xml}"
  echo "  -> xml size:  ${size} bytes"
  if [ -f "${log}" ]; then
    last_bench=$(grep -E '^[a-z_-]+ +[0-9]' "${log}" | tail -1)
    [ -n "${last_bench}" ] && echo "  -> bench: ${last_bench}"
  fi
done

echo "[done] ${OUTDIR}"
