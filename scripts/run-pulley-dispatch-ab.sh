#!/usr/bin/env bash
# Pulley dispatch A/B on the M4 E-cores: the harness build
# (--cfg=pulley_tail_calls, the nightly `become` dispatch loop) against the
# same build without the cfg (upstream's default `match` loop). Interleaved
# reps of the given cases, each in its own process under `taskpolicy -b`.
# Re-run it on every wasmtime bump to check the cfg still earns its place.
#
# Usage: scripts/run-pulley-dispatch-ab.sh <out-dir>
#   N=5 BENCH_TARGET_MS=2000
#   CASES="fib,call_indirect,vtable_poly4,xmrsplayer,graphql_as,convolution.scalar,audio_dsp"
#                    run_matrix WORKLOADS= substrings of case ids
# Output: <out-dir>/ab.jsonl (run_matrix lines, `rep` = rep, plus
#         "dispatch": "tail" | "match"). The match-loop binary is built
#         into target/ab-match-loop (delete it afterwards: ~3 GB).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:?usage: run-pulley-dispatch-ab.sh <out-dir>}"
N="${N:-5}"
BENCH_TARGET_MS="${BENCH_TARGET_MS:-2000}"
CASES="${CASES:-fib,call_indirect,vtable_poly4,xmrsplayer,graphql_as,convolution.scalar,audio_dsp}"
mkdir -p "${OUT}"

TAIL_BIN="${ROOT}/target/release/run_matrix"
MATCH_DIR="${ROOT}/target/ab-match-loop"
MATCH_BIN="${MATCH_DIR}/release/run_matrix"
[[ -x "${TAIL_BIN}" ]] || ./scripts/build-host-cli.sh --bin run_matrix
if [[ ! -x "${MATCH_BIN}" ]]; then
  NIGHTLY_TC="$(grep -m1 '^NIGHTLY_TC=' scripts/build-lib.sh | cut -d'"' -f2)"
  ( export PATH="${HOME}/.rustup/toolchains/${NIGHTLY_TC}-aarch64-apple-darwin/bin:${PATH}"
    export RUSTFLAGS="-C target-cpu=apple-a12"
    export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
    cargo build --release -p benchmark-core --features nightly-dispatch --bin run_matrix \
      --target-dir "${MATCH_DIR}" )
fi

echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ) git: $(git rev-parse --short HEAD) N=${N} cases=${CASES}" \
  > "${OUT}/ab-info.txt"
for rep in $(seq 1 "${N}"); do
  for variant in tail match; do
    bin="${TAIL_BIN}"; [[ "${variant}" == match ]] && bin="${MATCH_BIN}"
    RUNTIMES=pulley WORKLOADS="${CASES}" BENCH_TARGET_MS="${BENCH_TARGET_MS}" \
      MATRIX_JSONL="${OUT}/ab-${variant}.tmp" MATRIX_REP="${rep}" \
      taskpolicy -b "${bin}" > /dev/null 2>&1
    sed "s/^{/{\"dispatch\":\"${variant}\",/" "${OUT}/ab-${variant}.tmp" >> "${OUT}/ab.jsonl"
    rm -f "${OUT}/ab-${variant}.tmp"
    echo "[rep ${rep}/${N}] ${variant}"
  done
done
echo "[done] ${OUT}"
