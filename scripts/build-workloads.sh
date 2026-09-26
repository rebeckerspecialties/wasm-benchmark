#!/usr/bin/env bash
# Build each `workloads-rs/*.rs` file into a standalone `workloads/*.wasm`
# via rustc. No cargo, no [[lib]] config — each .rs file is a self-
# contained `cdylib` with its own panic handler.
#
# These .wasm files are checked into the repo (per the project brief) so
# the watchOS / iOS / macOS apps don't need a wasm toolchain at app-build
# time; benchmark-core's build.rs simply `include_bytes!`'s them.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${ROOT}/workloads-rs"
OUT="${ROOT}/workloads"

# Use the same stable toolchain as the host benchmark-core crate; wasm32
# is Tier-1, no build-std needed.
TOOLCHAIN="${TOOLCHAIN:-1.93.1}"
TOOLCHAIN_BIN="${HOME}/.rustup/toolchains/${TOOLCHAIN}-aarch64-apple-darwin/bin"
export PATH="${TOOLCHAIN_BIN}:${PATH}"

mkdir -p "${OUT}"

# Workloads that also get a -simd128 build under ${OUT}/scalar/ (see below).
SCALAR_VARIANTS="${SCALAR_VARIANTS:-factorial sieve crc32 convolution bulk_memory}"

for src in "${SRC}"/*.rs; do
  name=$(basename "${src}" .rs)
  out="${OUT}/${name}.wasm"
  echo "==> ${name}.wasm"

  # Canonical feature set: every workload is compiled with the full
  # wasm-3.0-ish proposal stack. Runtimes that can't handle a given
  # feature surface ERROR rows in the harness — that's the
  # cross-runtime signal we want, not a per-workload neutering.
  # (Earlier this script gated `+simd128` to matmul-only so that
  # wasm3 wouldn't choke on auto-vectorizer-emitted v128 locals; wasm3
  # v0.9.0 accepts v128 local slots without executing SIMD ops (our
  # former patches/wasm3/0001, upstreamed as wasm3#559), so the
  # canonical `+simd128 +relaxed-simd` applies to every workload. The
  # scalar variants below are the rows for runtimes that cannot execute
  # the v128 ops the auto-vectorizer emits.)
  #
  # `-C panic=abort` avoids the unwinding personality function.
  rustc \
    --target wasm32-unknown-unknown \
    --edition 2024 \
    --crate-type cdylib \
    --crate-name "${name}" \
    -C opt-level=3 \
    -C lto=fat \
    -C panic=abort \
    -C target-feature=+simd128,+relaxed-simd,+tail-call,+bulk-memory,+multivalue,+reference-types \
    "${src}" \
    -o "${out}"
  size=$(wc -c < "${out}" | tr -d ' ')
  echo "    -> ${out} (${size} bytes)"

  # Scalar variant for workloads whose SIMD is incidental: the
  # auto-vectorizer emits v128 ops into these under the canonical
  # features, which runtimes without an interpreter SIMD-128 path (wasm3,
  # zwasm's interpreter) reject or trap on. The scalar build is the
  # apples-to-apples interpreter comparison across every runtime; the
  # canonical build stays the primary row. matmul_* use SIMD intrinsics
  # by design and have no scalar variant.
  case " ${SCALAR_VARIANTS} " in
    *" ${name} "*)
      mkdir -p "${OUT}/scalar"
      rustc \
        --target wasm32-unknown-unknown \
        --edition 2024 \
        --crate-type cdylib \
        --crate-name "${name}" \
        -C opt-level=3 \
        -C lto=fat \
        -C panic=abort \
        -C target-feature=-simd128,-relaxed-simd,+tail-call,+bulk-memory,+multivalue,+reference-types \
        "${src}" \
        -o "${OUT}/scalar/${name}.wasm"
      echo "    -> ${OUT}/scalar/${name}.wasm (scalar variant)"
      ;;
  esac
done

# Wasm 3.0 feature benchmarks. workloads-wat/gen.py writes the WAT (each
# benchmark and, where the feature can be dropped from the same program,
# its twin); wasm-tools assembles it. Hand-written WAT goes through no
# optimizer, so the check below only guards against a generator change
# that loses the feature's instructions. relaxed_kernels comes from
# workloads-rs/ above and goes through LTO, which is what its check
# guards against.
python3 "${ROOT}/workloads-wat/gen.py"
for wat in "${ROOT}"/workloads-wat/*.wat; do
  name=$(basename "${wat}" .wat)
  wasm-tools parse "${wat}" -o "${OUT}/${name}.wasm"
  echo "==> ${name}.wasm ($(wc -c < "${OUT}/${name}.wasm" | tr -d ' ') bytes, from WAT)"
done

# name|extended regex that must match `wasm-tools print` at least once
FEATURE_OPS=(
  'tailcall_fsm|\breturn_call\b'
  'eh_parser_exnref|\btry_table\b'
  'eh_parser_exnref|\bthrow\b'
  'eh_parser_legacy|^\s*try\b'
  'eh_parser_legacy|^\s*catch\b'
  'gc_trees|\bstruct\.new\b'
  'gc_trees|\bstruct\.get\b'
  'callref_dispatch|\bcall_ref\b'
  'relaxed_kernels|i32x4\.relaxed_dot_i8x16_i7x16_add_s'
  'relaxed_kernels|f32x4\.relaxed_madd'
  'relaxed_kernels|i32x4\.relaxed_trunc_f32x4_s'
  'matmul_fma|f32x4\.relaxed_madd'
  'mem64_chase|\(memory \(;0;\) i64 '
  'multimem_transform|i32\.load \$lut'
  'multimem_transform|i32\.store \$dst'
  'extconst_init|\(offset i32\.const [0-9]+ i32\.const [0-9]+ i32\.mul'
)
echo
echo "Feature-op check (wasm-tools print):"
for entry in "${FEATURE_OPS[@]}"; do
  name="${entry%%|*}"
  re="${entry#*|}"
  n=$(wasm-tools print "${OUT}/${name}.wasm" | grep -cE -- "${re}" || true)
  if [[ "${n}" -eq 0 ]]; then
    echo "    FAIL ${name}: no match for /${re}/" >&2
    exit 1
  fi
  printf '    %-20s %5d × /%s/\n' "${name}" "${n}" "${re}"
done

echo
echo "All workloads built. ${OUT}:"
ls -la "${OUT}"/*.wasm
