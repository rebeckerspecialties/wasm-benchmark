#!/usr/bin/env bash
# Experiment A: arm64_32-apple-watchos dependency inventory for wasmtime+pulley.
#
# Outputs (under out/exp-a/):
#   wasmtime-head.txt        — wasmtime git rev parsed
#   cargo-tree.txt           — default tree (normal+build+dev edges)
#   cargo-tree-normal.txt    — normal-only tree (what links into the binary)
#   cargo-metadata.json      — raw cargo metadata for build.rs flag lookup
#   cargo-metadata.stderr.txt
#   inventory.txt            — final cross-referenced classification

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASMTIME_DIR="${ROOT}/wasmtime"
OUT="${ROOT}/out/exp-a"
TOOLCHAIN="${TOOLCHAIN:-1.93.1}"
TARGET="${TARGET:-arm64_32-apple-watchos}"

mkdir -p "${OUT}"

if [[ ! -d "${WASMTIME_DIR}" ]]; then
  echo "ERROR: ${WASMTIME_DIR} does not exist. Clone wasmtime first:" >&2
  echo "  git clone --depth 1 https://github.com/bytecodealliance/wasmtime.git ${WASMTIME_DIR}" >&2
  exit 2
fi

cd "${WASMTIME_DIR}"

echo "==> wasmtime HEAD"
git rev-parse HEAD | tee "${OUT}/wasmtime-head.txt"

echo "==> cargo tree (default = normal+build+dev — over-reports for ${TARGET})"
rustup run "${TOOLCHAIN}" cargo tree \
  --target "${TARGET}" \
  -p wasmtime \
  --no-default-features --features pulley \
  > "${OUT}/cargo-tree.txt"

echo "==> cargo tree -e normal (authoritative on-target set)"
rustup run "${TOOLCHAIN}" cargo tree \
  --target "${TARGET}" \
  -p wasmtime \
  --no-default-features --features pulley \
  -e normal \
  > "${OUT}/cargo-tree-normal.txt"

echo "==> cargo metadata"
rustup run "${TOOLCHAIN}" cargo metadata \
  --format-version=1 \
  --filter-platform "${TARGET}" \
  --no-default-features --features pulley \
  --manifest-path crates/wasmtime/Cargo.toml \
  > "${OUT}/cargo-metadata.json" \
  2> "${OUT}/cargo-metadata.stderr.txt"
# (cargo metadata prints crate downloads to stderr; keep streams separate)

echo "==> cross-referencing tree set with metadata for build.rs flags"
python3 - <<PY > "${OUT}/inventory.txt"
import json, re
import os

OUT = os.environ.get("OUT_DIR", "${OUT}")

tree = open(os.path.join(OUT, "cargo-tree-normal.txt")).read()
seen = set()
for line in tree.splitlines():
    m = re.search(r"([a-zA-Z0-9_][\w-]*) v(\d+\.\d+\.\d+(?:[-+][\w.]+)?)", line)
    if m:
        seen.add((m.group(1), m.group(2)))

with open(os.path.join(OUT, "cargo-metadata.json")) as f:
    md = json.load(f)
md_pkgs = {(p["name"], p["version"]): p for p in md["packages"]}

def has_build_script(p):
    return any(t.get("kind") == ["custom-build"] for t in p["targets"])
def is_proc_macro(p):
    return any("proc-macro" in t.get("kind", []) for t in p["targets"])
def src_label(p):
    src = p.get("source") or ""
    return "crates.io" if "crates.io-index" in src else ("workspace" if not src else src)

resolved = [md_pkgs[k] for k in seen if k in md_pkgs]
on_target = [p for p in resolved if not is_proc_macro(p)]
proc_macros = [p for p in resolved if is_proc_macro(p)]

print(f"On-target crates ({len(on_target)}):")
print("-" * 76)
for p in sorted(on_target, key=lambda x: x["name"]):
    flags = []
    if has_build_script(p): flags.append("build.rs")
    if p["name"].endswith("-sys"): flags.append("SYS")
    print(f"  {p['name']:36s} {p['version']:12s} {src_label(p):12s} {' '.join(flags)}")

print(f"\nProc-macros host-only ({len(proc_macros)}):")
for p in sorted(proc_macros, key=lambda x: x["name"]):
    print(f"  {p['name']:36s} {p['version']:12s} {src_label(p):12s}")

bs = [p for p in on_target if has_build_script(p)]
print(f"\nOn-target crates with build.rs: {len(bs)}")
for p in sorted(bs, key=lambda x: x["name"]):
    print(f"  - {p['name']} v{p['version']}")
sys = [p for p in on_target if p["name"].endswith("-sys")]
print(f"\nOn-target -sys crates: {len(sys)}")
PY

echo "==> Done. See ${OUT}/inventory.txt"
ls -la "${OUT}"
