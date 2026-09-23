#!/usr/bin/env bash
# Build the component-aware CLIs that scripts/feature-matrix.sh (and the
# WASI 0.3 async benchmark) drive, into target/cm-tools/:
#   wasmtime   wasmtime v49 CLI from the submodule, with Pulley + component
#              model + component-model-async (run with --target pulley64)
#   zwasm-p3   zwasm CLI, -Dengine=interp -Dwasi=p3, patches/zwasm applied
#   iwasm-cm   WAMR iwasm from the fork's integration/cm-wasip2-all branch
#              (airbus cm_wasip2 lineage + Apple WASIp2 host port),
#              FAST_INTERP + COMPONENT_MODEL + LIBC_WASI, no AOT / JIT
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${ROOT}/target/cm-tools"
mkdir -p "${OUT}"
STABLE_TC="$(grep -m1 '^STABLE_TC=' "${ROOT}/scripts/build-lib.sh" | cut -d'"' -f2)"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"; git -C "${ROOT}/zwasm" worktree prune; git -C "${ROOT}/wasm-micro-runtime" worktree prune' EXIT

echo "==> wasmtime CLI (Pulley + component model async)"
( cd "${ROOT}/wasmtime"
  PATH="${HOME}/.rustup/toolchains/${STABLE_TC}-aarch64-apple-darwin/bin:${PATH}" \
    cargo build --release -p wasmtime-cli --no-default-features \
      --features run,cranelift,pulley,component-model,component-model-async,gc,gc-drc,wat,parallel-compilation )
cp "${ROOT}/wasmtime/target/release/wasmtime" "${OUT}/wasmtime"

echo "==> zwasm CLI (-Dengine=interp -Dwasi=p3)"
git -C "${ROOT}/zwasm" worktree add -q --detach "${TMP}/zwasm" HEAD
"${ROOT}/scripts/apply_patch_series.sh" "${TMP}/zwasm" "${ROOT}/patches/zwasm"
( cd "${TMP}/zwasm" && "${ZWASM_ZIG:-/opt/homebrew/bin/zig}" build \
    -Dengine=interp -Dwasi=p3 -Doptimize=ReleaseFast )
cp "${TMP}/zwasm/zig-out/bin/zwasm" "${OUT}/zwasm-p3"

echo "==> WAMR iwasm (fork integration/cm-wasip2-all, component model)"
git -C "${ROOT}/wasm-micro-runtime" fetch -q fork integration/cm-wasip2-all
git -C "${ROOT}/wasm-micro-runtime" worktree add -q --detach "${TMP}/wamr" FETCH_HEAD
mkdir -p "${TMP}/wamr/product-mini/platforms/darwin/build-cm"
( cd "${TMP}/wamr/product-mini/platforms/darwin/build-cm" \
  && cmake .. -DCMAKE_BUILD_TYPE=Release -DWAMR_BUILD_INTERP=1 -DWAMR_BUILD_FAST_INTERP=1 \
       -DWAMR_BUILD_AOT=0 -DWAMR_BUILD_JIT=0 -DWAMR_BUILD_FAST_JIT=0 \
       -DWAMR_BUILD_COMPONENT_MODEL=1 -DWAMR_BUILD_LIBC_WASI=1 -DWAMR_BUILD_LIBC_BUILTIN=1 \
       -DBISON_EXECUTABLE="$(brew --prefix bison 2>/dev/null || echo /opt/homebrew/opt/bison)/bin/bison" \
  && make -j8 iwasm )
cp -L "${TMP}/wamr/product-mini/platforms/darwin/build-cm/iwasm" "${OUT}/iwasm-cm"
ls -la "${OUT}"
