# wasm-benchmark

[![build](https://github.com/rebeckerspecialties/wasm-benchmark/actions/workflows/build.yml/badge.svg)](https://github.com/rebeckerspecialties/wasm-benchmark/actions/workflows/build.yml)

WebAssembly interpreter benchmark harness targeting Apple Silicon
deployment platforms — Apple Watch (arm64_32-apple-watchos), iPhone
(aarch64-apple-ios), Apple TV (aarch64-apple-tvos), and Mac
(aarch64-apple-darwin). Compares
[wasmtime](https://github.com/bytecodealliance/wasmtime)'s **Pulley**
interpreter against six other pure-interpreter runtimes:
[WAMR](https://github.com/bytecodealliance/wasm-micro-runtime) fast-interp,
[wasm3](https://github.com/wasm3/wasm3), [WasmEdge](https://github.com/WasmEdge/WasmEdge)
(`USE_LLVM=OFF`), [zwasm](https://github.com/clojurewasm/zwasm),
[wasmz](https://github.com/Ray-D-Song/wasmz) and
[tinywasm](https://github.com/explodingcamera/tinywasm) — on
dispatch-heavy workloads (synthetic `call_indirect` and vtable dispatch,
the xmrsplayer tracker player, sqlite3 speedtest1, an AssemblyScript
port of graphql-js validation), Wasm 3.0 feature benchmarks
(tail calls, exceptions, GC, typed function references, relaxed SIMD,
memory64, multi-memory, extended-const), a WASI 0.3 component-model
async benchmark, and an end-to-end femtovg-to-wasm vector renderer
drawn by a Metal host.

Latest results: **[docs/runtime-comparison-2026-09-22.md](docs/runtime-comparison-2026-09-22.md)**
(every runtime at its 2026-09 release; M4 E-cores and iPhone XS, N=10;
tinywasm's M4 PMU profile).

The harness drives the
[per-table-mutability optimization stack on the wasmtime fork](https://github.com/rebeckerspecialties/wasmtime/pull/2);
the wasmtime submodule carries its July soundness-fixed split plus the
phase-4 `call_indirectN` arg bundling, rebased onto wasmtime v49.

App-Store constraint: **pure-interpreter only**, no JIT / MAP_JIT /
copy-and-patch. Pulley is the only legal wasmtime runtime in that
space. Every comparison runtime is built with JIT/AOT disabled
(`WAMR_BUILD_AOT=0 JIT=0 FAST_JIT=0`, `WASMEDGE_USE_LLVM=OFF`, zwasm
`-Dengine=interp` with the JIT compiled out; wasm3, wasmz and tinywasm
have no native tier at all).

## Quickstart

```sh
# Clone with submodules (wasmtime, WAMR, wasm3, WasmEdge, wasmz, zwasm,
# femtovg, target-lexicon, mach2, sightglass — all pinned)
git clone --recurse-submodules https://github.com/rebeckerspecialties/wasm-benchmark.git
cd wasm-benchmark

# First-time setup: rustup toolchains + Apple targets
./scripts/setup.sh

# Build .wasm workloads (writes workloads/*.wasm)
./scripts/build-workloads.sh

# Build each comparison runtime's static lib (applies the patch series
# under patches/<runtime>/ idempotently before each build).
./scripts/build-wamr.sh macos       # WAMR libiwasm.a
./scripts/build-wasm3.sh macos      # wasm3 libm3.a
./scripts/build-wasmedge.sh macos   # WasmEdge libwasmedge.a (27 patches)
./scripts/build-wasmz.sh macos      # wasmz libwasmz.a
./scripts/build-zwasm.sh macos      # zwasm libzwasm.a (-Dengine=interp)

# Build benchmark-core static lib for a platform
./scripts/build-lib.sh macos        # M-series host
./scripts/build-lib.sh ios          # iPhone (aarch64-apple-ios)
./scripts/build-lib.sh watchos      # arm64_32-apple-watchos
./scripts/build-lib.sh watchos-arm64  # aarch64-apple-watchos (Series 9+)
./scripts/build-lib.sh tvos         # Apple TV (aarch64-apple-tvos)
./scripts/build-lib.sh visionos     # Apple Vision Pro (aarch64-apple-visionos)
./scripts/build-lib.sh all          # everything

# Host CLIs (pinned nightly, --cfg=pulley_tail_calls, fat LTO)
./scripts/build-host-cli.sh --bin run_matrix
./scripts/build-host-cli.sh --bin run_femtovg_e2e --features femtovg-e2e

# Every case on one runtime, on the E-cores
RUNTIMES=wamr taskpolicy -b ./target/release/run_matrix

# The measurement passes behind the report
./scripts/run-m4-pass.sh out/m4           # M4 E-cores, N=10
./scripts/run-device-pass.sh out/iphone   # attached iPhone, N=10
RUNTIMES_LIST=tinywasm ./scripts/run-m4-pmu-pass.sh out/pmu   # M4 PMU (separately)
./scripts/summarize-pass.py out docs/<report-data-dir>
```

The app (iOS / iPadOS, watchOS, tvOS, visionOS; `apps/`) ranks the
engines on the device it runs on: each engine is an expandable row with
its version, short commit and an aggregate score, and its benchmarks'
scores inside. It builds with `xcodebuild` from `apps/`; see
[AGENTS.md](AGENTS.md) for build, deploy and measurement procedures and
the App Store packaging.

CI (`.github/workflows/build.yml`) reproduces the full submodule init +
patch-series application + per-target build on every PR, so a clean
checkout from any branch should succeed end-to-end without local state.

## Repo layout

```
apps/                    SwiftUI app — iOS / watchOS / tvOS / visionOS / macOS targets
crates/benchmark-core/   Rust library — Pulley + WAMR + wasm3 + WasmEdge +
                         zwasm + wasmz + tinywasm adapters, the case table
                         (cases.rs), the femtovg E2E host, PMU-aware
                         harness
docs/                    project docs (feasibility, fusion phases 1–4,
                         cross-runtime comparisons, IC investigation, ...)
patches/                 upstream-PR-prep patches for each runtime
                         submodule, applied idempotently by build-*.sh
scripts/                 build, setup, PMU analysis
workloads-rs/            standalone Rust sources for *.wasm workloads
workloads-rs-cargo/      cargo-based guests (xmrsplayer-bench, femtovg-guest,
                         cm-async-bench)
workloads-wat/           generator for the Wasm 3.0 feature benchmarks
workloads/               pre-built *.wasm files (checked in)
# --- submodules ---
wasmtime/                rebeckerspecialties/wasmtime, branch
                         `pulley-bench-stack-v49` (v49.0.0 + 9 commits).
                         Only wasmtime/target/ is gitignored.
wasm-micro-runtime/      upstream main b70d708d; patches/wasm-micro-runtime/
                         0001-0012 applied at build time (relaxed
                         SIMD, PROT_NONE linear memory).
wasm3/                   v0.9.0, no patches.
WasmEdge/                0.17.2-rc.3; patches/wasmedge/ (27, Apple-mobile
                         enablement stack) applied at build time.
wasmz/                   v0.1.4; patches/wasmz/0002 (arm64_32 watchOS)
                         applied at build time.
zwasm/                   v2.7.0; patches/zwasm/0001 (JIT compiled out of
                         the C API) + 0002 (arm64_32 watchOS) applied at
                         build time.
femtovg/                 rebeckerspecialties/femtovg, branch wire-renderer
                         (the E2E's Renderer wire stream).
mach2/                   pinned to fork's arm64_32-apple-watchos branch.
target-lexicon/          pinned to fork's arm64_32-apple-watchos branch.
sightglass/              pinned to upstream main (sqlite3.wasm source).
```

## Where to read more

- **[docs/runtime-comparison-2026-09-22.md](docs/runtime-comparison-2026-09-22.md)** —
  the 2026-09 refresh: versions, patches and exact build flags of all
  seven runtimes, the Wasm 3.0 feature matrix with evidence, M4 E-core
  and iPhone XS tables, the femtovg E2E, and tinywasm's M4 PMU profile
  with its bottleneck and next experiments.
- **[docs/tinywasm-iphone12-2026-09-23.md](docs/tinywasm-iphone12-2026-09-23.md)** —
  tinywasm's PMU and Time Profiler profile on the iPhone 12 E-cores, and
  the first two measured contributions (−5.6 % cycles).
- **[docs/femtovg-e2e-abi.md](docs/femtovg-e2e-abi.md)** — the femtovg
  guest/host split and its import ABI.
- **[AGENTS.md](AGENTS.md)** — deep agent context: toolchain pinning
  (and why cross-language LTO is gone), build commands, the case-table
  registration pattern, the measurement passes, xctrace gotchas, QoS,
  the Pulley dispatch loop mode selection, device status, and the
  carried patch series.
- **[docs/feasibility-report.md](docs/feasibility-report.md)** —
  original arm64_32-apple-watchos feasibility analysis.
- **[docs/ic-investigation-results.md](docs/ic-investigation-results.md)** —
  N=10 cross-platform IC measurement closeout. The IC investigation
  is closed; pulley super-op fusion is the next direction (see
  PR #2's description).
- **[docs/opcode-fusion-band-brif.md](docs/opcode-fusion-band-brif.md)** —
  Phase 1 of the opcode-fusion track: `xband_s8 + br_if` fused into
  one Pulley dispatch at call_indirect lazy-init sites. Measurement
  closed out 2026-05-14: wallclock flat, Discarded +7.87 % on iPhone
  12 PMU. **Superseded by phase 2 at the same call site.**
- **[docs/opcode-fusion-funcref-dispatch.md](docs/opcode-fusion-funcref-dispatch.md)** —
  Phase 2 of the opcode-fusion track: `brif + xload code + xload vmctx`
  fused into one `xfuncref_dispatch_*` Pulley dispatch. **Measurement
  closed out 2026-05-14 — call_indirect wallclock −5.0 % on iPhone 12
  (the first measurable win past PR #2's c1-7 ceiling); PMU Discarded
  −1.74 % vs baseline / −8.91 % vs phase 1 (reclaims phase 1's
  predictor-anchor regression).**
- **[docs/opcode-fusion-band-funcref-dispatch.md](docs/opcode-fusion-band-funcref-dispatch.md)** —
  Phase 3 of the opcode-fusion track: `xband + funcref_dispatch` fused
  into one `xband_funcref_dispatch_*` Pulley dispatch (dispatch tail
  now 2 ops vs baseline's 5). **Measurement closed out 2026-05-15 —
  PMU total cycles −4.31 % vs phase 2 / −0.96 % vs baseline; Discarded
  −7.33 % vs phase 2 / −8.95 % vs baseline. Wallclock matches phase 2
  within noise. Ship the full 9-commit stack.**
- **[docs/cross-runtime-pulley-vs-wamr.md](docs/cross-runtime-pulley-vs-wamr.md)** —
  Pulley (phase 3) vs WAMR fast-interp side-by-side, iPhone 12, N=10
  medians, steady-state iteration time (module load excluded).
  **WAMR is 1.37–1.86× faster across call_indirect, xmrsplayer,
  vtable_*, graphql-validation (AS); Pulley wins only on
  graphql-validation (Porffor) because WAMR can't load Porffor's
  wasm-exceptions section in our build.** The fusion track narrowed
  the gap from baseline but can't close it — WAMR's load-time
  register-IR rewrite is structurally fewer dispatches per source
  wasm op.
- **[docs/four-way-baseline-phase3-phase4-wamr.md](docs/four-way-baseline-phase3-phase4-wamr.md)** —
  Phase-4 closeout: cross-device wallclock matrix (iPhone 12 A14 +
  iPhone XS A12 + Watch SE2 S8 + M4 host) across baseline, phase 3,
  phase 4, and WAMR. Phase 4's `PulleyCallIndirect` fused-args ABI
  shrinks the dispatch tail to 1 fused op + 1 `call_indirectN` per
  site; Watch SE2 (the actual deployment target) sees vtable_bi
  −7.68 %, poly4 −4.62 %, poly6 −4.86 % vs phase 3.
- **[docs/archived-ic-branches.md](docs/archived-ic-branches.md)** —
  SHAs for the deleted `pulley-call-indirect-ic*` branches, in case
  a future 2-way-IC or poisoning variant wants that baseline.
- **[patches/README.md](patches/README.md)** — patch-stack workflow
  for the wasmtime + mach2 + target-lexicon upstream PRs.

## Cross-runtime results across Apple silicon E-cores (2026-05)

These are the pre-refresh numbers (fusion phases 1–4 on the old
wasmtime pin, WAMR at `cd390ea0`); the 2026-09 results are in
[docs/runtime-comparison-2026-09-22.md](docs/runtime-comparison-2026-09-22.md).

Driving question: can WAMR fast-interp replace WasmEdge as the wasm
runtime in our interpreter-only App-Store-eligible app? The matrix
below covers Apple's E-core microarchitectures from A12 Tempest
through M4 Sawtooth — the cores that the OS schedules work onto at
`.utility` QoS and where battery-sensitive iOS / watchOS workloads
actually run. macOS measurements pin to E-cluster via
`taskpolicy -b`; iOS / watchOS measurements set the QoS class and
verify lane via `taskinfo` ‑‑ scheduler picks E unless the work
saturates them. wasmtime is configured with
`Config::target("pulley{32,64}")` so its "Pulley" bytecode runs as
data, App-Store-safe.

| Workload (median ms/iter) | Runtime | M4 Lion P | M4 Sawtooth E | A14 Icestorm (iPhone 12) | A12 Tempest (iPhone XS) | S8 (Watch SE2) |
|---|---|---:|---:|---:|---:|---:|
| `call_indirect` (200 K dispatches) | **WAMR** | **4.9** | 41.2 | **16.5** | **27.5** | **31.8** |
| | Pulley | 9.0 | 41.9 | 27.7 | 41.0 | 46.4 |
| `audio_dsp` (1000×512) | **WAMR** | **154** | — | — | **419** | **1060** |
| | Pulley | 231 | — | — | 537 | 1472 |
| `bulk_memory.copy/fill` | **WAMR** | **1.8** | — | — | **5.0** | **15.6** |
| | Pulley | 5.6 | — | — | 10.8 | 31.6 |
| `matmul relaxed-SIMD FMA` ¹ | WAMR | enabled by relaxed-SIMD PR | — | — | — | 4.61 |
| | Pulley | 0.64 | — | — | 1.34 | **4.32** |
| `graphql-validation` (Porffor JS→wasm) ² | WAMR | enabled by legacy-EH PR | — | — | — | 115 ³ |
| | Pulley | — | — | 14.9 | 24.4 | **23.1** |

¹ Requires the relaxed-SIMD fast-interp PR (this PR series). The
small Pulley win on Watch SE2 motivated our diff-fuzz harness
against wasmtime's `relaxed_simd_deterministic` mode (see below).

² Requires the legacy-EH fast-interp PR (this PR series). The
Porffor JS-to-wasm compiler emits `wasm-exceptions` sections that
unmodified WAMR rejects at load with `invalid section id`.

³ WAMR's Porffor median on Watch SE2 is 5× slower than Pulley's
because Porffor re-instantiates per iteration and grows linear
memory through WAMR's mmap-fallback path (~12 K page-faults/iter
on iPhone 12). A separate follow-up PR (PROT_NONE-reservation +
mprotect-commit fast path) closes that gap — measured 6.6× faster
than mmap-fallback on iPhone 12; not part of either upstream PR.

### Test coverage

- **174 conformance checks** across three layers — 32 hand-rolled
  abuse cases, 76 differential comparisons against wasmtime's
  deterministic-relaxed-SIMD mode, 69 upstream
  [WebAssembly/relaxed-simd](https://github.com/WebAssembly/relaxed-simd)
  spec-testsuite assertions (with `(either …)` semantics for
  impl-defined ambiguity).
- **Diff-fuzz against wasmtime caught one of our own bugs**
  pre-ship — see
  [WebAssembly/relaxed-simd#164](https://github.com/WebAssembly/relaxed-simd/pull/164)
  for the spec-test gap that let the bug slip past the canonical
  suite.
- All builds are run through ASan + UBSan locally; integration
  tests linked at https://github.com/rebeckerspecialties/wasm-benchmark/tree/main/crates/benchmark-core/tests
  (`relaxed_simd_abuse.rs`, `relaxed_simd_diff_fuzz.rs`,
  `relaxed_simd_spec_testsuite.rs`).

## Current upstream-PR state (checked 2026-09-22)

Wasmtime:

- **[bytecodealliance/wasmtime#13259](https://github.com/bytecodealliance/wasmtime/pull/13259)**:
  unwinder arm64_32 inline-asm format fix. **Merged.**
- **fork PR [#2](https://github.com/rebeckerspecialties/wasmtime/pull/2)**:
  per-table mutability + `call_indirect` elisions (open). Its July
  soundness-fixed split, plus phase-4 arg bundling and the
  LEGACY_EXCEPTIONS known-feature fix, is what the submodule carries on
  v49 (`pulley-bench-stack-v49`). Fork PR #4 (fusion phases 1–4) and the
  upstream attempts (#13445, #13447, #13909, #13910) are closed.

Runtime PRs:

- **[rebeckerspecialties/wasm-micro-runtime#3–#4](https://github.com/rebeckerspecialties/wasm-micro-runtime/pulls)** —
  relaxed SIMD and PROT_NONE linear memory for fast-interp; carried as
  `patches/wasm-micro-runtime/0001-0012`. The legacy-EH PRs #1 and #2
  are closed: exnref supersedes legacy EH.
  Relaxed SIMD is also upstream as
  [bytecodealliance/wasm-micro-runtime#4950](https://github.com/bytecodealliance/wasm-micro-runtime/pull/4950) (open).
- **[wasm3/wasm3#559](https://github.com/wasm3/wasm3/pull/559)** — v128
  as opaque slot. **Merged** (in v0.9.0; our patch is retired).
- **[Ray-D-Song/wasmz#3](https://github.com/Ray-D-Song/wasmz/pull/3)** —
  Zig 0.16 stdlib port. **Merged** (in v0.1.x; our port patch is retired).
- **[clojurewasm/zwasm#98](https://github.com/clojurewasm/zwasm/pull/98)** —
  arm64_32 watchOS, the maintainers' re-land of our #97. **Merged**;
  v2.7.0 regressed that target again, hence `patches/zwasm/0002`.
- **[WasmEdge/WasmEdge#4802](https://github.com/WasmEdge/WasmEdge/pull/4802)** —
  SIMD superinstruction primitives, part of the Apple-mobile stack.
  **Merged**; the rest is carried in `patches/wasmedge/`.

The **target-lexicon + mach2** patches live on `rebeckerspecialties`
forks (arm64_32-apple-watchos branches); `.gitmodules` already pins
them. See [`patches/README.md`](patches/README.md) for the
patch-stack workflow.

## License

Dual-licensed under MIT or Apache-2.0 at your option — same conventions
as the `bytecodealliance/*` and `gfx-rs/wgpu` projects. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
