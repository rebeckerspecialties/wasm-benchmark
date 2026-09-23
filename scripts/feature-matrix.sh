#!/usr/bin/env bash
# Runtime × feature support matrix, filled by running a tiny smoke module
# per feature on every runtime in the configuration the harness ships
# (not from documentation).
#
# Core-wasm features: workloads/features/<feature>.wat, compiled with
# wasm-tools, run through `run_matrix --file` on all seven runtimes;
# each module computes run(41) == 42 through the feature's core
# instructions, so a wrong answer is a failure, not a pass.
#
# Component model / WASI 0.3 async: the harness adapters are core-wasm
# APIs, so the components (zwasm's vendored official wasi-testsuite
# wasm32-wasip3 binaries) are run through each runtime's component-aware
# entry point where one exists:
#   wasmtime  — wasmtime CLI, --target pulley64 (Pulley interpreter)
#   zwasm     — zwasm CLI built -Dengine=interp -Dwasi=p3, --engine interp
#   WAMR      — iwasm from the fork's integration/cm-wasip2-all branch
#               (COMPONENT_MODEL=1 + LIBC_WASI=1, fast-interp); not the
#               shipped harness build, which has no component model
# plus each shipped core loader, which must reject the component.
#
# Usage: scripts/feature-matrix.sh [out-dir]
#   WASMTIME_CLI=... ZWASM_P3_CLI=... WAMR_CM_IWASM=... override the tools;
#   missing tools are reported as "not run" in the output, never guessed.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"
OUT="${1:-${ROOT}/out/feature-matrix}"
mkdir -p "${OUT}"
MATRIX="${ROOT}/target/release/run_matrix"
[[ -x "${MATRIX}" ]] || ./scripts/build-host-cli.sh --bin run_matrix
JSONL="${OUT}/feature-matrix.jsonl"
: > "${JSONL}"
WASM_TMP="$(mktemp -d)"
trap 'rm -rf "${WASM_TMP}"' EXIT

echo "== core-wasm features (run(41) must return 42)"
for wat in workloads/features/*.wat; do
  f="$(basename "${wat}" .wat)"
  wasm="${WASM_TMP}/${f}.wasm"
  wasm-tools parse "${wat}" -o "${wasm}"
  BENCH_TARGET_MS=5 MATRIX_JSONL="${JSONL}" MATRIX_REP=0 \
    "${MATRIX}" --file "${wasm}" --func run --arg 41 --expect 42 >/dev/null 2>&1
  echo "   ${f}"
done

echo "== component model / WASI 0.3 async"
COMP="${ROOT}/zwasm/test/component"
TOOLS="${ROOT}/target/cm-tools"   # built by scripts/build-cm-tools.sh
WASMTIME_CLI="${WASMTIME_CLI:-${TOOLS}/wasmtime}"
ZWASM_P3_CLI="${ZWASM_P3_CLI:-${TOOLS}/zwasm-p3}"
WAMR_CM_IWASM="${WAMR_CM_IWASM:-${TOOLS}/iwasm-cm}"
# name|component|expected exit
CM_CASES=(
  "wasip2-cli-stdout|${COMP}/wasip3/cli-stdout.wasm|0"
  "wasip3-multi-clock-wait|${COMP}/wasip3_official/multi-clock-wait.wasm|0"
  "wasip3-cli-exit|${COMP}/wasip3_official/cli-exit.wasm|1"
)
cm_row() {  # runtime case expected cmd...
  local rt="$1" name="$2" expect="$3"; shift 3
  local out rc
  if [[ ! -x "$1" ]]; then
    printf '{"runtime":"%s","case":"%s","ok":null,"error":"not run: %s missing"}\n' \
      "${rt}" "${name}" "$1" >> "${JSONL}"
    return
  fi
  out="$(timeout 60 "$@" 2>&1)"; rc=$?
  local ok=false; [[ "${rc}" == "${expect}" ]] && ok=true
  printf '{"runtime":"%s","case":"%s","ok":%s,"exit":%d,"expected_exit":%d,"output":%s}\n' \
    "${rt}" "${name}" "${ok}" "${rc}" "${expect}" \
    "$(printf '%s' "${out:0:200}" | python3 -c 'import json,sys; print(json.dumps(sys.stdin.read()))')" \
    >> "${JSONL}"
  echo "   ${rt} ${name}: exit ${rc} (want ${expect})"
}
for c in "${CM_CASES[@]}"; do
  IFS='|' read -r name comp expect <<< "${c}"
  cm_row pulley "${name}" "${expect}" "${WASMTIME_CLI}" run --target pulley64 \
    -W component-model-async=y -S p3=y "${comp}"
  cm_row zwasm "${name}" "${expect}" "${ZWASM_P3_CLI}" run --engine interp "${comp}"
  cm_row wamr-cm-fork "${name}" "${expect}" "${WAMR_CM_IWASM}" "${comp}"
  # Shipped core loaders must reject the component outright.
  BENCH_TARGET_MS=5 MATRIX_JSONL="${JSONL}" MATRIX_REP=0 \
    "${MATRIX}" --file "${comp}" --func run --arg 0 >/dev/null 2>&1
done

python3 - "${JSONL}" "${OUT}/feature-matrix.md" <<'EOF'
import json, sys, collections
rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip()]
cells = collections.OrderedDict()
for r in rows:
    cells.setdefault(r["case"], {})[r["runtime"]] = r
rts = ["pulley", "wamr", "wasm3", "wasmedge", "zwasm", "wasmz", "tinywasm", "wamr-cm-fork"]
with open(sys.argv[2], "w") as f:
    f.write("| feature smoke | " + " | ".join(rts) + " |\n|" + "---|" * (len(rts) + 1) + "\n")
    for case, by in cells.items():
        def cell(rt):
            r = by.get(rt)
            if r is None: return ""
            if r.get("ok") is True: return "yes"
            if r.get("ok") is None: return "not run"
            err = r.get("error") or f"exit {r.get('exit')}"
            return "no: " + err.replace("|", "/").replace("\n", " ")[:70]
        f.write(f"| {case} | " + " | ".join(cell(rt) for rt in rts) + " |\n")
print(open(sys.argv[2]).read())
EOF
