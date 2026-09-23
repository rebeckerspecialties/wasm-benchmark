#!/usr/bin/env bash
# M4 E-core timing pass: N reps, and in each rep one process per runtime
# (run_matrix with RUNTIMES=<runtime>), all under `taskpolicy -b` so the
# work is scheduled on the E-cluster. Every JSON line records the timed
# window's measured E-core share (e_share), so residency is checked, not
# assumed. Runtimes are interleaved within each rep, so slow drift in the
# machine's background load spreads over all of them.
#
# Parts (PARTS, default all three):
#   matrix  every case in cases.rs on every runtime -> m4-matrix.jsonl
#   e2e     femtovg E2E, scenes 0 and 1, each runtime's best guest build,
#           one process per (runtime, scene) -> m4-e2e.jsonl (+ PNGs of rep 1)
#   cm      component-model async / WASI 0.3 (scripts/run-cm-async-bench.sh)
#           -> cm-async/
#
# Nothing else should run on the machine meanwhile (no builds): the
# E-cores are shared with the system and the numbers are CPU time and
# wall time of the benchmark only.
#
# Usage: scripts/run-m4-pass.sh <out-dir>
#   N=10 BENCH_TARGET_MS=2000 PARTS="matrix e2e cm"
#   RUNTIMES_LIST="pulley wamr wasm3 wasmedge zwasm wasmz tinywasm"
#   E2E_FRAMES=121 E2E_PASSES=2
# Summaries: scripts/summarize-pass.py <out-dir>
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:?usage: run-m4-pass.sh <out-dir>}"
N="${N:-10}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-2000}"
PARTS=" ${PARTS:-matrix e2e cm} "
RUNTIMES_LIST="${RUNTIMES_LIST:-pulley wamr wasm3 wasmedge zwasm wasmz tinywasm}"
E2E_FRAMES="${E2E_FRAMES:-121}"
E2E_PASSES="${E2E_PASSES:-2}"
mkdir -p "${OUT}/logs"

MATRIX="${ROOT}/target/release/run_matrix"
E2E_BIN="${ROOT}/target/release/run_femtovg_e2e"
[[ -x "${MATRIX}" ]] || ./scripts/build-host-cli.sh --bin run_matrix
[[ -x "${E2E_BIN}" ]] || ./scripts/build-host-cli.sh --bin run_femtovg_e2e --features femtovg-e2e

{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host: $(sysctl -n machdep.cpu.brand_string) / $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
  echo "git: $(git rev-parse --short HEAD) $(git status --porcelain | grep -vc '^ m' || true) uncommitted"
  echo "N=${N} BENCH_TARGET_MS=${BENCH_TARGET_MS} PARTS=${PARTS} RUNTIMES=${RUNTIMES_LIST}"
} > "${OUT}/m4-pass-info.txt"

for rep in $(seq 1 "${N}"); do
  for rt in ${RUNTIMES_LIST}; do
    if [[ "${PARTS}" == *" matrix "* ]]; then
      t0=$(date +%s)
      RUNTIMES="${rt}" BENCH_TARGET_MS="${BENCH_TARGET_MS}" MATRIX_JSONL="${OUT}/m4-matrix.jsonl" \
        MATRIX_REP="${rep}" taskpolicy -b "${MATRIX}" > "${OUT}/logs/matrix-${rt}-r${rep}.txt" 2>&1
      echo "[rep ${rep}/${N}] matrix ${rt}: rc=$? $(( $(date +%s) - t0 ))s"
    fi
    if [[ "${PARTS}" == *" e2e "* ]]; then
      for scene in 0 1; do
        png=()
        [[ "${rep}" == 1 ]] && png=(--png "${OUT}/e2e-${rt}-scene${scene}.png")
        t0=$(date +%s)
        taskpolicy -b "${E2E_BIN}" --runtime "${rt}" --scene "${scene}" --frames "${E2E_FRAMES}" \
          --passes "${E2E_PASSES}" --rep "${rep}" --jsonl "${OUT}/m4-e2e.jsonl" ${png[@]+"${png[@]}"} \
          > "${OUT}/logs/e2e-${rt}-s${scene}-r${rep}.txt" 2>&1
        echo "[rep ${rep}/${N}] e2e ${rt} scene ${scene}: rc=$? $(( $(date +%s) - t0 ))s"
      done
    fi
  done
done

if [[ "${PARTS}" == *" cm "* ]]; then
  N="${N}" ./scripts/run-cm-async-bench.sh "${OUT}/cm-async"
fi
echo "[done] ${OUT}"
