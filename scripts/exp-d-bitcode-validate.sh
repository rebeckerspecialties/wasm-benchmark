#!/usr/bin/env bash
# Experiment D: validate that the produced libwasmtime.a (and the .rlibs that
# fed into it) contain LLVM bitcode rather than native Mach-O code.
#
# What we check:
#   1. The final libwasmtime.a — extract every .o, classify each as
#      bitcode / Mach-O / other.
#   2. A sample of .rlibs in target/.../deps — extract their .o files,
#      classify the same way. Specifically include:
#        - liballoc-*.rmeta-paired *.rlib (build-std std)
#        - libwasmtime-*.rlib
#        - libpulley_interpreter-*.rlib
#        - libwasmtime_internal_unwinder-*.rlib (the inline-asm one)
#        - libtarget_lexicon-*.rlib  (our patched crate)
#        - libmach2-*.rlib            (our patched crate)
#   3. For wasmtime's helpers.c output: cc-rs emits a separate static archive
#      (libwasmtime-helpers.a) inside OUT_DIR; locate it and check its .o
#      files. Expectation: native Mach-O unless we passed -flto to cc.
#
# Output: out/exp-d/report.txt  (per-object classification) and
#         out/exp-d/summary.txt (counts by category).

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASMTIME_DIR="${ROOT}/wasmtime"
TARGET="${TARGET:-arm64_32-apple-watchos}"
OUT="${ROOT}/out/exp-d"
WORK="${OUT}/extracted"
TARGET_DIR="${WASMTIME_DIR}/target/${TARGET}/release"
DEPS_DIR="${TARGET_DIR}/deps"
ARTIFACT="${TARGET_DIR}/libwasmtime.a"

rm -rf "${OUT}"
mkdir -p "${OUT}" "${WORK}"

if [[ ! -f "${ARTIFACT}" ]]; then
  echo "ERROR: ${ARTIFACT} not found. Run scripts/exp-b-build.sh realistic first." >&2
  exit 2
fi

# Use llvm-bcanalyzer from the toolchain we built with (nightly-2026-01-25)
# rather than the system one — they should agree on the bitcode format used.
LLVM_BIN="${HOME}/.rustup/toolchains/nightly-2026-01-25-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin"
if [[ -x "${LLVM_BIN}/llvm-bcanalyzer" ]]; then
  BCANALYZER="${LLVM_BIN}/llvm-bcanalyzer"
elif command -v llvm-bcanalyzer >/dev/null; then
  BCANALYZER="$(command -v llvm-bcanalyzer)"
else
  # macOS may have it under xcrun
  BCANALYZER="$(xcrun --find llvm-bcanalyzer 2>/dev/null || true)"
fi
echo "BCANALYZER=${BCANALYZER:-<not found, will rely on file(1) only>}"

REPORT="${OUT}/report.txt"
SUMMARY="${OUT}/summary.txt"
: > "${REPORT}"
: > "${SUMMARY}"

classify_one() {
  # $1 = label (path or archive:member), $2 = file path
  local label="$1" path="$2"
  local base="${path##*/}"
  local kind="other"
  local desc
  desc="$(file -b "${path}" 2>/dev/null)"
  case "${desc}" in
    *"LLVM bitcode"*)                kind="bitcode" ;;
    *"LLVM IR bitcode"*)             kind="bitcode" ;;
    *Mach-O*object*)
      # rustc embeds rmeta as a Mach-O object container (".rmeta" entries
      # inside .rlibs); that's metadata, not executable code.
      if [[ "${base}" == "lib.rmeta" || "${base}" == *.rmeta ]]; then
        kind="rmeta"
      else
        kind="macho-native"
      fi ;;
    "current ar archive"*)           kind="ar-archive" ;;
    *)                               kind="other" ;;
  esac
  printf "%-15s %-9s %s\n" "${kind}" "[${desc:0:30}]" "${label}" >> "${REPORT}"
  printf "%s\n" "${kind}"
}

scan_archive() {
  # $1 = archive path
  local arch="$1"
  local name; name="$(basename "${arch}")"
  local d="${WORK}/${name}.d"
  rm -rf "${d}"; mkdir -p "${d}"
  ( cd "${d}" && ar -x "${arch}" 2>/dev/null )
  local total=0 bc=0 native=0 rmeta=0 other=0
  shopt -s nullglob
  # Include .rmeta files too so we account for them in the summary.
  for o in "${d}"/*.o "${d}"/*.lto.o "${d}"/*.bc "${d}"/*.rmeta; do
    [[ -f "${o}" ]] || continue
    # Skip ar's symbol-index entries.
    [[ "$(basename "${o}")" == "__.SYMDEF" ]] && continue
    total=$((total+1))
    k=$(classify_one "${name}::$(basename "${o}")" "${o}")
    case "${k}" in
      bitcode)      bc=$((bc+1)) ;;
      macho-native) native=$((native+1)) ;;
      rmeta)        rmeta=$((rmeta+1)) ;;
      *)            other=$((other+1)) ;;
    esac
  done
  shopt -u nullglob
  printf "%-58s  total=%-4d  bitcode=%-4d  native=%-4d  rmeta=%-4d  other=%d\n" \
         "${name}" "${total}" "${bc}" "${native}" "${rmeta}" "${other}" \
         | tee -a "${SUMMARY}"
}

echo "==> Final static library"
echo "==> Final static library" >> "${SUMMARY}"
scan_archive "${ARTIFACT}"

echo
echo "==> Sample of dependent rlibs"
echo "==> Sample of dependent rlibs" >> "${SUMMARY}"
PATTERNS=(
  "libwasmtime-*.rlib"
  "libpulley_interpreter-*.rlib"
  "libwasmtime_internal_unwinder-*.rlib"
  "libwasmtime_internal_core-*.rlib"
  "libtarget_lexicon-*.rlib"
  "libmach2-*.rlib"
  "libstd-*.rlib"
  "liballoc-*.rlib"
  "libcore-*.rlib"
)
for p in "${PATTERNS[@]}"; do
  for f in "${DEPS_DIR}"/${p}; do
    [[ -f "${f}" ]] || continue
    scan_archive "${f}"
  done
done

echo
echo "==> wasmtime-helpers C output (helpers.c via cc-rs)"
echo "==> wasmtime-helpers C output (helpers.c via cc-rs)" >> "${SUMMARY}"
HELPERS_AS=$(find "${TARGET_DIR}/build" -name "libwasmtime-helpers*.a" 2>/dev/null | head -3)
if [[ -n "${HELPERS_AS}" ]]; then
  for h in ${HELPERS_AS}; do
    scan_archive "${h}"
  done
else
  echo "(no libwasmtime-helpers*.a found under build/ — helpers.c may not have been compiled this run)" \
    | tee -a "${SUMMARY}"
fi

echo
echo "==> Aggregate verdict"
echo "==> Aggregate verdict" >> "${SUMMARY}"
python3 - <<PY | tee -a "${SUMMARY}"
import re
totals = {"bitcode":0, "native":0, "rmeta":0, "other":0}
with open("${SUMMARY}") as f:
    for line in f:
        m = re.search(r"bitcode=(\d+)\s+native=(\d+)\s+rmeta=(\d+)\s+other=(\d+)", line)
        if m:
            totals["bitcode"] += int(m.group(1))
            totals["native"]  += int(m.group(2))
            totals["rmeta"]   += int(m.group(3))
            totals["other"]   += int(m.group(4))
print(f"TOTAL  bitcode={totals['bitcode']}  native={totals['native']}  rmeta={totals['rmeta']}  other={totals['other']}")
verdict = "PASS — pure-bitcode" if totals["native"] == 0 and totals["other"] == 0 else \
          ("PARTIAL — native objects present" if totals["native"] > 0 else "ATTENTION — uncategorised entries")
print(f"VERDICT  {verdict}")
PY

echo
echo "Wrote ${REPORT} and ${SUMMARY}"
