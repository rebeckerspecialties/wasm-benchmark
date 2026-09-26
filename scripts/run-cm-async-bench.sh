#!/usr/bin/env bash
# Component-model async / WASI 0.3 benchmark pass (M4 host only: the iOS /
# watchOS app has no component runner — neither wasmtime's component API
# nor zwasm's P3 host is linked into it).
#
# N process runs per runtime of workloads/cm_async_bench.wasm, each under
# `taskpolicy -b` (E-cluster) and wrapped by rusage_exec, which records the
# process's measured E-core share, CPU time, instructions and cycles. The
# guest times its own phases (see workloads-rs-cargo/cm-async-bench), so
# CLI startup and compilation are not in the per-op numbers.
#
#   pulley        target/cm-tools/wasmtime run --target pulley64
#                 -W component-model-async=y -S p3=y
#   zwasm         target/cm-tools/zwasm-p3 run --engine interp
#   wamr-cm-fork  target/cm-tools/iwasm-cm (WASIp2-only lineage; run once,
#                 expected to reject the WASI 0.3 component)
#
# stdin is /dev/null: zwasm's P3 runner does not finish while host stdin is
# open, even when the guest never reads it. stdout must be exactly the
# guest's REPS × 2 MiB byte pattern (checked by SHA-256), so a runtime that
# drops stream bytes fails instead of reporting a fast number.
#
# Usage: scripts/run-cm-async-bench.sh [out-dir]    (N=10 by default)
# Tools: scripts/build-cm-tools.sh; component: scripts/build-cm-async-bench.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:-${ROOT}/out/cm-async}"
N="${N:-10}"
mkdir -p "${OUT}"
T="${ROOT}/target/cm-tools"
WASM="${ROOT}/workloads/cm_async_bench.wasm"
RUSAGE="${ROOT}/target/release/rusage_exec"
[[ -x "${RUSAGE}" ]] || ./scripts/build-host-cli.sh --bin rusage_exec
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT
: > "${OUT}/rusage.jsonl"
: > "${OUT}/checks.txt"

# The guest writes, per rep, chunks i = 0..8191 of bytes (i*31 + j) & 0xFF.
EXPECT_SHA=$(python3 - <<'PY'
import hashlib
blk = bytearray()
for i in range(8192):
    blk += bytes(((i * 31 + j) & 0xFF) for j in range(256))
print(hashlib.sha256(bytes(blk) * 5).hexdigest())
PY
)

run_one() {  # runtime rep cmd...
  local rt="$1" rep="$2"; shift 2
  local log="${OUT}/${rt}-r${rep}.log"
  taskpolicy -b "${RUSAGE}" --jsonl "${OUT}/rusage.jsonl" --label "${rt} rep=${rep}" -- \
    "$@" "${WASM}" < /dev/null > "${TMP}/stdout.bin" 2> "${log}" || true
  local sha size
  sha=$(shasum -a 256 "${TMP}/stdout.bin" | cut -d' ' -f1)
  size=$(wc -c < "${TMP}/stdout.bin" | tr -d ' ')
  if [[ "${sha}" == "${EXPECT_SHA}" ]]; then
    echo "${rt} rep=${rep} stdout ok (${size} bytes)" >> "${OUT}/checks.txt"
  else
    echo "${rt} rep=${rep} stdout MISMATCH (${size} bytes, sha256 ${sha})" >> "${OUT}/checks.txt"
  fi
  echo "   ${rt} rep ${rep}: $(grep -c '^cm_async .* ns_per_op' "${log}" || true) phase lines, stdout ${size} B"
}

for rep in $(seq 1 "${N}"); do
  echo "== rep ${rep}/${N}"
  run_one pulley "${rep}" "${T}/wasmtime" run --target pulley64 -W component-model-async=y -S p3=y
  run_one zwasm "${rep}" "${T}/zwasm-p3" run --engine interp
  if [[ "${rep}" == 1 && -x "${T}/iwasm-cm" ]]; then
    run_one wamr-cm-fork 1 "${T}/iwasm-cm"
  fi
done

python3 - "${OUT}" <<'PY'
import glob, json, os, re, statistics, sys
out = sys.argv[1]
rows = []
for log in sorted(glob.glob(os.path.join(out, "*-r*.log"))):
    rt, rep = re.match(r"(.+)-r(\d+)\.log", os.path.basename(log)).groups()
    for line in open(log, errors="replace"):
        m = re.match(r"cm_async (\S+) rep=(\d+) n=(\d+) total_ns=(\d+) ns_per_op=([\d.]+)", line)
        if m:
            rows.append(dict(runtime=rt, run=int(rep), phase=m[1], inner_rep=int(m[2]),
                             n=int(m[3]), total_ns=int(m[4]), ns_per_op=float(m[5])))
with open(os.path.join(out, "cm-async.jsonl"), "w") as f:
    for r in rows:
        f.write(json.dumps(r) + "\n")
usage = [json.loads(l) for l in open(os.path.join(out, "rusage.jsonl"))]
lines = ["| runtime | phase | runs | median ns/op | min | max |", "|---|---|---:|---:|---:|---:|"]
for rt in sorted({r["runtime"] for r in rows}):
    for ph in ["wait_for_0", "concurrent_wait", "stdout_stream"]:
        # per process run: median over its inner reps; then median/range over runs
        per_run = []
        for run in sorted({r["run"] for r in rows if r["runtime"] == rt}):
            v = [r["ns_per_op"] for r in rows if r["runtime"] == rt and r["run"] == run and r["phase"] == ph]
            if v:
                per_run.append(statistics.median(v))
        if per_run:
            lines.append(f"| {rt} | {ph} | {len(per_run)} | {statistics.median(per_run):.1f} | "
                         f"{min(per_run):.1f} | {max(per_run):.1f} |")
lines += ["", "| run | exit | e_share | cpu (ms) | wall (ms) | IPC |", "|---|---:|---:|---:|---:|---:|"]
for u in usage:
    ipc = u["instructions"] / u["cycles"] if u.get("cycles") else float("nan")
    lines.append(f"| {u['label']} | {u['exit']} | {u.get('e_share', float('nan')):.4f} | "
                 f"{u.get('cpu_ns', 0)/1e6:.1f} | {u['wall_ns']/1e6:.1f} | {ipc:.2f} |")
open(os.path.join(out, "summary.md"), "w").write("\n".join(lines) + "\n")
print("\n".join(lines))
PY
cat "${OUT}/checks.txt"
