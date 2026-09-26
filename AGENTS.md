# AGENTS.md — wasm-benchmark project guide

This is a WebAssembly-interpreter benchmark harness targeting Apple Silicon
deployment platforms (App-Store-eligible: no JIT, no MAP_JIT, no
copy-and-patch) — primarily arm64_32-apple-watchos, aarch64-apple-ios,
aarch64-apple-tvos, and aarch64-apple-darwin. The harness compares seven
interpreters: **Pulley** (wasmtime's interpreter), **WAMR** (WebAssembly
Micro Runtime fast-interp), **wasm3** (the m3 pure C interpreter),
**WasmEdge** (`WASMEDGE_USE_LLVM=OFF` with the Apple-mobile patch
stack), **zwasm** (clojurewasm's Zig runtime, interpreter engine only),
**wasmz** (Ray-D-Song's Zig interpreter) and **tinywasm** (a pure-Rust
interpreter). It was set up to drive a per-table-mutability optimization
stack upstream (see
[PR #2](https://github.com/rebeckerspecialties/wasmtime/pull/2)).

## Next session starting points

Pick this up cold without re-deriving state:

- **Latest results**: [`docs/runtime-comparison-2026-09-22.md`](docs/runtime-comparison-2026-09-22.md)
  (every runtime at its 2026-09 release, the Wasm 3.0 feature matrix and
  benchmarks, the femtovg E2E, M4 E-core + iPhone XS N=10, tinywasm's M4
  PMU profile).
  Raw per-rep data is in `docs/runtime-comparison-2026-09-22/`.
- **Working branch**: `runtime-refresh-2026-09` on
  `rebeckerspecialties/wasm-benchmark`, on top of the WAMR relaxed-SIMD
  work of open PR #4 (`claude/wasm-benchmark-continue-wuuPd`) and
  `claude/relaxed-simd-diff-fuzz`.
- **Runtime pins** (details, flags and caveats in *Cross-runtime
  comparison* below):

  | runtime | pin | carried patches |
  |---|---|---|
  | Pulley | wasmtime v49.0.0 + 9 commits, fork branch `pulley-bench-stack-v49` (`0d9aebd66d`) | fork commits (no patch files) |
  | WAMR | upstream `main` `b70d708d` (2026-09-21, WAMR-2.4.1-364) | `patches/wasm-micro-runtime/0001-0029` |
  | wasm3 | v0.9.0 `0cd38327` | none |
  | WasmEdge | 0.17.2-rc.3 `16ea4c45` | `patches/wasmedge/` (26) |
  | zwasm | v2.7.0 `d09d9248` | `patches/zwasm/0001-0002` |
  | wasmz | v0.1.4 `0796998b` | `patches/wasmz/0002` |
  | tinywasm | 0.11.0 (crates.io) | none |
  | femtovg (E2E guest + host) | fork branch `wire-renderer` `074050a` on upstream master 0.27.0 | fork commits |

- **WAMR on `main`, not the release**: WAMR-2.4.5 sits on
  `release/2.4.x` (cut from 2.4.1, 2025-07), lacks 364 main-line commits,
  and none of our patches apply to it. The WAMR fork branches remain the
  authoritative copies of the patch series (PR #2 legacy EH, PR #3
  relaxed SIMD = upstream #4950, PR #4 PROT_NONE linear memory).
- **WASIp2 / component-model WAMR work (2026-06-14)** — a SEPARATE
  airbus-`cm_wasip2`-based lineage, NOT the EH/relaxed-SIMD branches
  above. Fork PRs (all branch from airbus `dev/cm_wasip2_complete @
  2815b698`): [#10](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/10)
  `feat/wasip2-apple-port` (Apple host-layer port: kqueue/getentropy/
  openat O_NOFOLLOW_ANY/SO_NOSIGPIPE/etc. + 8 base-CI fixes),
  [#9](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/9)
  `integration/cm-wasip2-all` (everything merged — the integration PR),
  [#8](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/8)
  conformance traps, [#7](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/7)
  parser fuzz. **The fork has its own `wasm-micro-runtime/AGENTS.md`
  with the full dev context — read it first.**
  - **CI**: macOS fully green (128/0); ubuntu green except 3 `test aot`
    jobs (one shared `align.wast` "stack size does not match block type"
    wamrc-only quirk from the airbus base). Accepted/deferred 2026-06-14.
  - `scripts/build-cm-tools.sh` builds this lineage's `iwasm`
    (`integration/cm-wasip2-all`, FAST_INTERP + COMPONENT_MODEL +
    LIBC_WASI) as `target/cm-tools/iwasm-cm` for the feature matrix. It
    runs WASIp2 components and rejects every WASIp3 one (exit 255).
  - **Swap WAMR builds for perf testing (non-destructive)**: `build.rs`
    links `libiwasm.a` ONLY (FFI is hand-declared in
    `crates/benchmark-core/src/wamr.rs`, no headers). Build the wanted
    branch in a git worktree, copy its `libiwasm.a` over
    `wasm-micro-runtime/product-mini/platforms/darwin/build/`, then
    rebuild the host CLI (rerun-if-changed relinks). Remove the
    `wasm_c_api.c.o` member first (see *WAMR* below) or zwasm crashes.
- **tinywasm contributions**: a three-PR stack, upstream as
  [explodingcamera/tinywasm#57](https://github.com/explodingcamera/tinywasm/pull/57)
  (value-stack growth out of line),
  [#58](https://github.com/explodingcamera/tinywasm/pull/58) (inlined fused
  binop / compare helpers) and
  [#59](https://github.com/explodingcamera/tinywasm/pull/59) (per-function
  operand-stack reservation). All three target `next` (the maintainer's
  working branch; `main` lags it).
  - GitHub's native stacked PRs don't work across forks and we can't
    push upstream, so it is a manual stack: #58 and #59 contain the
    commits below them, and each description says which commits are new.
    After a squash-merge, rebase the next branch onto `next` and drop the
    merged commit.
  - Branches live in the fork `rebeckerspecialties/tinywasm`, where the
    same stack is fork PRs #1-#3 (`perf/value-stack-cold-growth` →
    `perf/inline-fused-binop-helpers` → `perf/reserve-operand-stack`).
    They were rebased 2026-09-23 onto `next` `785be0e`: #2's attributes
    moved onto the `impl_value_ops!` macro, and #3 no longer touches the
    removed `examples/rust` archive fixture.
  - Measured on `next` `b45a98a`, before the rebase: −4.1 % / −5.3 % /
    −8.0 % cycles on the iPhone 12 E-cores; #3 matches the no-growth
    upper bound. See
    [`docs/tinywasm-iphone12-2026-09-23.md`](docs/tinywasm-iphone12-2026-09-23.md).
  - #59 adds `pub max_stack` to `tinywasm_types::WasmFunction` and bumps
    the archive to `06`.
  - Local checkout `~/src/tinywasm`: `origin` is upstream, `fork` is
    ours; worktrees in `~/src/tinywasm-worktrees/`. A/B tooling:
    `scripts/tinywasm-ab-build-ios.sh`, `scripts/tinywasm-ab-iphone.sh`,
    `scripts/tinywasm_ab_summary.py`.
  - tinywasm's CONTRIBUTING requires the issue / PR text to be the
    contributor's own. The PR descriptions are the user's.
  - Running tinywasm's CI matrix locally: rustfmt comes from the
    floating `nightly`, clippy from `1.98` (the pinned nightly has
    neither). The matrix needs ~10 GB of target dir per toolchain; build
    one toolchain at a time with `CARGO_INCREMENTAL=0` when disk is
    tight.
- **Open follow-ups** found by the 2026-09 refresh (evidence in the
  report):
  - wasmz v0.1.4 bugs to file upstream: wrong result on extended-const
    initializers; active data segments not re-applied on a second
    instantiation of the same module; typed-table `elem` expression
    segments not applied (`call_ref` then null-traps); `catch_ref` /
    `throw_ref` stack underflow.
  - zwasm v2.7.0: never reclaims GC structs (a `gc_trees` call grows the
    heap by >100 MB, segfault near 2.3 GB); memory per instantiation is
    not returned when the instance is deleted (extended-const: 2.8 GB
    peak over 1663 instantiations, 0.96 GB still held); `return_call` is
    not constant-space (~50 B per call; likely cause: the tail-call
    trampoline allocates callee locals from the per-instance
    `ArenaAllocator`, `src/api/instance.zig`, whose `free` only reclaims
    the latest allocation); and the interpreter has no SIMD-128. On the
    iPhone its xmrsplayer footprint reaches ~1.2 GB and
    graphql-validation (Porffor) gets the app jetsam-killed even in a
    launch of its own.
  - tinywasm's Porffor row holds 1.03 GB after 103 samples on macOS
    (each sample drops its `Store`) but peaks at 52 MB RSS on the
    iPhone. Undiagnosed: a macOS allocator effect or a macOS-only leak.
  - WasmEdge 0.17.2-rc.3 and wasmz never collect GC structs either (the
    gc_trees heap grows ~15 MB / ~4.8 MB per call); Pulley (DRC) and
    tinywasm stay flat.
  - `--cfg=pulley_tail_calls` is still opt-in upstream in v49 (same three
    dispatch modes in `pulley/src/interp.rs`), so it stays.
- **Open fork PRs** (state checked 2026-09-22):
  - [`rebeckerspecialties/wasmtime#2`](https://github.com/rebeckerspecialties/wasmtime/pull/2) — table-mutability tracking (open; `#4`, the phase 1–4 fusion PR, is closed)
  - [`rebeckerspecialties/wasm-micro-runtime#1`–`#4`](https://github.com/rebeckerspecialties/wasm-micro-runtime/pulls) — throw-only EH, full legacy EH, relaxed SIMD, PROT_NONE linear memory (open; relaxed SIMD also upstream as [bytecodealliance/wasm-micro-runtime#4950](https://github.com/bytecodealliance/wasm-micro-runtime/pull/4950), open)
  - [`rebeckerspecialties/wasm3#1`](https://github.com/rebeckerspecialties/wasm3/pull/1) — v128 opaque slot; superseded by upstream [wasm3#559](https://github.com/wasm3/wasm3/pull/559) (merged, in v0.9.0)
  - Upstream wasmtime [#13445](https://github.com/bytecodealliance/wasmtime/pull/13445) / [#13447](https://github.com/bytecodealliance/wasmtime/pull/13447) and the July split [#13909](https://github.com/bytecodealliance/wasmtime/pull/13909) / [#13910](https://github.com/bytecodealliance/wasmtime/pull/13910) are closed; [#13259](https://github.com/bytecodealliance/wasmtime/pull/13259) (arm64_32 unwinder) is merged.
- **Hot tools**:
  - `./scripts/run-m4-pass.sh <out>` — M4 E-core N=10 (matrix + femtovg E2E + CM async)
  - `./scripts/run-device-pass.sh <out>` — iPhone N=10, one runtime per launch (`E2E=0,1` for the femtovg E2E)
  - `./scripts/run-m4-pmu-pass.sh <out>` — M4 PMU per runtime × workload (`RUNTIMES_LIST=` / `MODES=` narrow it; the 2026-09 report profiles tinywasm only), never alongside a timing pass
  - `./scripts/run-device-pmu.sh <out>` — iPhone 12 PMU + Time Profiler per (runtime, row), xctrace launch mode
  - `./scripts/run-m4-memory-pass.sh <out>` — per-case phys_footprint peak, one process per (runtime, case)
  - `./scripts/run-pulley-dispatch-ab.sh <out>` — Pulley `pulley_tail_calls` vs match-loop dispatch
  - `./scripts/tinywasm-ab-build-ios.sh <name> <tinywasm-worktree> <ref> [patch...]` + `./scripts/tinywasm-ab-iphone.sh <out> <names...>` + `./scripts/tinywasm_ab_summary.py <out> <names...>` — interleaved A/B of tinywasm revisions on the iPhone
  - `./scripts/summarize-pass.py <pass-root> <data-dir>` — per-rep CSVs into the report's data dir, median/range tables into `<pass-root>/report-tables/`; `./scripts/build-report.py <report.md> <pass-root>/report-tables` fills the report
  - `./scripts/feature-matrix.sh [out]` — runtime × feature smoke matrix
  - `target/release/run_matrix` (`RUNTIMES=`, `WORKLOADS=`, `--case`, `--file`) — any case on any runtime
  - older: `./scripts/run_fusion_n10.sh`, `./scripts/aggregate_4way.py`, `./scripts/run_per_workload_pmu.sh` (iPhone 12 PMU)

## Project goal & current state

**Motivation**: A WatchOS audio app (incumbent: WasmEdge with custom
patches) runs at ~25 % CPU on iPhone XS for a WASI-audio music
player; S8 Apple Watch is 2–3× slower per core. Dispatch density on
narrow E-cores is the binding performance constraint. wasmtime's
Pulley interpreter is the only App-Store-legal runtime in that space.
Goal: identify and ship dispatch optimizations that move the needle
on these targets.

**Status** (2026-09-22): every runtime was moved to its latest release
(WAMR: latest `main`), tinywasm was added as the seventh runtime, the
workload set gained Wasm 3.0 feature benchmarks with featureless twins,
a WASI 0.3 component-model async benchmark and a femtovg-to-wasm E2E
rendered by a Metal host, and the three measurement passes were rerun.
See the report for numbers.

The Pulley stack carried on v49 is the July **soundness-fixed split** of
PR #2 plus phase 4 of the fusion work: per-table mutability tracking, the
constant table bound, the signature-check and null-check elisions on
immutable funcref tables, the `call_indirect{1,2,3,4}` arg-bundling ops,
and the LEGACY_EXCEPTIONS known-feature fix. **Not carried**: eager table
init, the constant-index `call_indirect` lowering and fusion phases 1–3
(they only fire with eager init). The split replaced them along with the
deferred-elem-segment soundness fix.

History (2026-05): an IC investigation in `pulley-call-indirect-ic*`
was abandoned after PMU evidence showed the IC's back-end savings cancel
against new front-end / mispredict pressure on Apple E-cores
(`docs/ic-investigation-results.md`, `docs/archived-ic-branches.md`).
Opcode fusion phases 1–4 were measured on iPhone 12, iPhone XS,
Watch SE2 and the M4 E-cores: phase 2 was the first wallclock win
(call_indirect −5.0 %), phase 3 cut Discarded 7–9 %, and phase 4's arg
bundling cut vtable_bi −7.68 % on Watch SE2. See
`docs/opcode-fusion-*.md` and `docs/four-way-baseline-phase3-phase4-wamr.md`.

## Toolchain pinning

| Toolchain | Use for | rustc |
|---|---|---|
| **`nightly-2026-07-05`** (`NIGHTLY_TC` in `scripts/build-lib.sh`) | every harness build: the device libs (`-Z build-std` for arm64_32-apple-watchos / tvOS), the host CLIs (`scripts/build-host-cli.sh`), and the CM-tools wasmtime CLI. Needed for `--cfg=pulley_tail_calls` and tinywasm's `nightly-tail-calls` | `1.98.0-nightly (c397dae80)`, LLVM 22.1.8 |
| **`1.98`** (`STABLE_TC`) | non-dispatch-sensitive tooling | `1.98.0 (88d9e12ae 2026-08-18)` |
| **`1.93.1`** | wasm32 guest builds: `build-workloads.sh`, `build-femtovg-guest.sh`, `build-cm-async-bench.sh`. 1.98's rust-lld cannot load its `libLLVM.dylib` on this host, so wasm links abort | `1.93.1 (01f6ddf75 2026-02-11)` |
| `1.94.1` | the machine default on PATH (no harness build uses it) | |

Minimums that forced the re-pin: wasmtime v49 needs rustc ≥ 1.96 and
tinywasm 0.11 needs ≥ 1.98. Zig **0.16.0** builds zwasm and wasmz.
Xcode **27.0** (27A266a), Apple clang 21.0.0.

**Cross-language LTO is no longer possible.** Every Rust ≥ 1.95 ships
LLVM 22, and Xcode 27's libLTO (LLVM 21) rejects its bitcode ("Unknown
attribute kind (105) (Producer: 'LLVM22.1.8-rust-1.98.0-nightly' Reader:
'LLVM APPLE_1_2100.3.34.2_0')"). `build-lib.sh` therefore drops
`-C linker-plugin-lto -C embed-bitcode=yes` and builds native objects
with Rust-side fat LTO across all Rust crates
(`CARGO_PROFILE_RELEASE_LTO=fat`, `CODEGEN_UNITS=1`). The only way back
to cross-language LTO would be a Rust whose LLVM major matches Xcode's
(≤ 1.94 for LLVM 21), which cannot build wasmtime v49 or tinywasm 0.11.

**Footgun**: `rustup run nightly` resolves to a newer floating nightly.
Always name the dated one (`rustup run nightly-2026-07-05`, or the
scripts' `NIGHTLY_TC`). The PATH `rustc` is 1.94.1, so `rustc +x` does
not switch toolchains here; use `rustup run`.

**Rustup targets**: `aarch64-apple-darwin`, `aarch64-apple-ios`,
`aarch64-apple-ios-sim`, `wasm32-unknown-unknown` and `wasm32-wasip2`
(on 1.93.1, for the guests), `wasm32-wasip1` (wasmtime's own
`cargo test --test disas`). `aarch64-apple-tvos` and
`arm64_32-apple-watchos` are Tier 3: `-Z build-std` on the nightly.

## Pulley dispatch loop selection (critical for perf)

Pulley ships three dispatch loop implementations in
`wasmtime/pulley/src/interp/` (unchanged in v49):

| mode | flag | safety | use |
|---|---|---|---|
| 1. Default `match` in `loop {}` | none | safe (stable Rust) | baseline; ~30-50 % slower than (3) on narrow E-cores |
| 2. LLVM-best-effort TCO | `--cfg=pulley_assume_llvm_makes_tail_calls` | **UNSAFE** — stack-overflows on `convolution` workload | do not use |
| 3. nightly `become` guaranteed TCO | `--cfg=pulley_tail_calls` | safe (requires nightly) | **standard** |

`scripts/build-lib.sh` and `scripts/build-host-cli.sh` set
`--cfg=pulley_tail_calls` for every target (before 2026-09 the host CLI
was a plain stable build, i.e. mode 1). Measured wins vs default-loop
dispatch (2026-05): M4 22-34 %, iPhone XS A12 33-46 %, Apple Watch SE2
S8 30-50 % across the workload set. This is the largest single perf
decision in the project. Don't change it without re-measuring across
all three E-core platforms. tinywasm's `nightly-tail-calls` (its own
`become` loop) is enabled with it through the `nightly-dispatch` cargo
feature.

## Repo layout

```
apps/                    iOS / watchOS / tvOS / macOS SwiftUI app
                         (Metal + QuartzCore linked for the femtovg E2E)
crates/benchmark-core/   Rust library — the seven runtime adapters
                         (lib.rs = Pulley, wamr.rs, wasm3.rs, wasmedge.rs,
                         zwasm.rs, wasmz.rs, tinywasm.rs), the case table
                         (cases.rs), the femtovg E2E host (femtovg_e2e.rs),
                         the residency / PMU-counter helpers (residency.rs)
                         and the CLIs in src/bin/
workloads-rs/            one .rs per workload (cdylib, no_std)
workloads-rs-cargo/      cargo-built guests: xmrsplayer-bench,
                         femtovg-guest (E2E), cm-async-bench (WASI 0.3)
workloads-wat/           gen.py (feature-benchmark WAT generator) and
                         reference.py (independent expected results)
workloads/               pre-built *.wasm (checked in — apps don't
                         need a wasm toolchain at build time);
                         workloads/scalar/ (-simd128 twins),
                         workloads/features/ (smoke modules),
                         workloads/femtovg/ (SVG scenes + LICENSES.md)
scripts/                 build-*.sh, the measurement passes, analysis
out/                     experiment outputs (gitignored)
docs/                    project docs and reports
patches/                 out-of-tree patch series per runtime
# --- submodules ---
wasmtime/                rebeckerspecialties/wasmtime, branch
                         pulley-bench-stack-v49. Only wasmtime/target/ is
                         gitignored.
wasm-micro-runtime/      upstream main b70d708d; patches applied at build
wasm3/                   v0.9.0
WasmEdge/                0.17.2-rc.3; patches applied at build
zwasm/                   v2.7.0; patches applied at build
wasmz/                   v0.1.4; patches applied at build
femtovg/                 rebeckerspecialties/femtovg, branch wire-renderer
                         (path dependency of benchmark-core and of the
                         guest — must be initialized even though the
                         `femtovg-e2e` feature is optional)
target-lexicon/, mach2/  forks with arm64_32-apple-watchos support
porffor/, sightglass/    workload sources
```

tinywasm is a crates.io dependency (`=0.11.0`), not a submodule.

## Building

```sh
# wasm workloads (rustc 1.93.1, wasm32-unknown-unknown). Also runs
# workloads-wat/gen.py and assembles the feature benchmarks, then checks
# with wasm-tools print that each feature's ops survived.
./scripts/build-workloads.sh
./scripts/build-femtovg-guest.sh     # workloads/femtovg-{simd128,relaxed,scalar}.wasm
./scripts/build-cm-async-bench.sh    # workloads/cm_async_bench.wasm (component)

# Cross-runtime static libs (each applies its patches/<runtime>/ series
# idempotently; per-target output dirs under the submodule).
./scripts/build-wamr.sh all          # WAMR libiwasm.a (minus wasm_c_api.c.o)
./scripts/build-wasm3.sh all         # wasm3 libm3.a
./scripts/build-wasmedge.sh all      # WasmEdge libwasmedge.a
./scripts/build-zwasm.sh all         # zwasm libzwasm.a (-Dengine=interp)
./scripts/build-wasmz.sh all         # wasmz libwasmz.a

# benchmark-core static lib for a platform (nightly, pulley_tail_calls,
# nightly-dispatch, fat LTO; femtovg-e2e on macos / ios / ios-sim)
./scripts/build-lib.sh macos | ios | ios-sim | watchos | watchos-sim | tvos | tvos-sim | all

# macOS host CLIs, same flags as the device libs
./scripts/build-host-cli.sh --bin run_matrix
./scripts/build-host-cli.sh --bin run_femtovg_e2e --features femtovg-e2e
./scripts/build-host-cli.sh --bin rusage_exec

# Component-aware CLIs for the feature matrix and the WASI 0.3 benchmark
./scripts/build-cm-tools.sh          # target/cm-tools/{wasmtime,zwasm-p3,iwasm-cm}

# iOS app (iPhone XS: DerivedData-xs-ios)
cd apps && xcodebuild -project WasmBenchmark.xcodeproj \
  -scheme WasmBenchmarkIOS -configuration Release \
  -destination "generic/platform=iOS" \
  -derivedDataPath build/DerivedData-xs-ios \
  -allowProvisioningUpdates build

# watchOS app (Watch SE2 S8). MUST pass ARCHS=arm64_32 + ONLY_ACTIVE_ARCH=NO
# since Xcode defaults to arm64 (S9+) but the Rust lib is arm64_32-only.
xcodebuild -project apps/WasmBenchmark.xcodeproj \
  -scheme WasmBenchmarkWatch -configuration Release \
  -destination "generic/platform=watchOS" \
  -derivedDataPath apps/build/DerivedData-se2-watch \
  -allowProvisioningUpdates \
  ARCHS=arm64_32 ONLY_ACTIVE_ARCH=NO \
  build

# tvOS app (Apple TV 4K, tvOS 26+).
xcodebuild -project apps/WasmBenchmark.xcodeproj \
  -scheme WasmBenchmarkTV -configuration Release \
  -destination "generic/platform=tvOS" \
  -derivedDataPath apps/build/DerivedData-tv \
  -allowProvisioningUpdates build
```

The Xcode project is generated from `apps/project.yml` (XcodeGen); keep
both in sync.

## Workload registration pattern

Workloads are rows of the runtime-independent case table in
`crates/benchmark-core/src/cases.rs`: `Case { id, label, wasm, func,
arg, expected, shape }`. `expected` is the cross-runtime consensus
result (a runtime that returns anything else fails the row), and
`shape` says how the case is driven: `I32ToI32` (`export(i32) -> i32`,
no imports), `PorfforMain`, `Sqlite3` (Pulley only), or
`InstantiateEach` (a fresh instance per sample, for features whose hot
path runs at instantiation). `run_case` dispatches a case to any
runtime; `KNOWN_CRASHES` lists (runtime, case) pairs that would take
the process down, which become N/A rows with the reason.

To add a workload:
1. Write `workloads-rs/<name>.rs` (see `call_indirect.rs` for the
   `panic_handler` + entry-point pattern), or generate WAT in
   `workloads-wat/gen.py` for feature benchmarks (and add an
   independent expected value to `workloads-wat/reference.py`).
2. Run `./scripts/build-workloads.sh`; add a `FEATURE_OPS` check there
   if the workload exists to exercise particular instructions.
3. In `crates/benchmark-core/src/lib.rs`: `pub const <NAME>_WASM` and
   `EXPECTED_<NAME>`.
4. In `cases.rs`: a `c("<id>", "<label>", ...)` entry. The CLIs
   (`run_matrix`, the passes) pick it up from there.
5. In `apps/Shared/BenchmarkContentView.swift`: one row per runtime
   calling `bench_run_case(runtime, "<id>")`, labeled
   `"<prefix> <label>"` with the label identical to the case table's.
   Prefixes: `[Pulley]`, `[ WAMR ]`, `[wasm3 ]`, `[WE    ]`, `[zwasm ]`,
   `[wasmz ]`, `[tinywm]` — the RUNTIMES filter and
   `scripts/summarize-pass.py` both match on them.

The older per-workload `bench_run_<name>` FFI functions still exist for
the original rows; new rows don't need them.

## Measurement methodology

Every adapter's timed loop goes through one `Window` helper (lib.rs):
per-call samples until `BENCH_TARGET_MS` of timed work, reporting min /
median / p99, CPU time, and — from `proc_pid_rusage(RUSAGE_INFO_V6)` —
the window's P-core CPU time (so **measured E-core residency**
`e_share = 1 - P-core time / CPU time`), instructions and cycles (IPC).
The kernel keeps those from the always-on fixed counters, so they work
on every device, including the A12 and S8 where xctrace exposes no PMU.
Teardown of instantiate-per-sample cases runs outside the clock.

### The three passes (2026-09 refresh)

- **M4 E-cores**: `scripts/run-m4-pass.sh <out>` — N reps; in each rep
  one process per runtime (`RUNTIMES=<rt> taskpolicy -b run_matrix`,
  interleaved), then the femtovg E2E per (runtime, scene), then the CM
  async benchmark. Nothing else may run meanwhile (no builds).
  **Caveat**: M4 E-cores are shared with macOS system services and
  per-call wallclock can have 30× outliers from preemption. Report CPU
  time next to wallclock; the iPhone gives tighter ranges.
- **iPhone** (XS Max, A12): `scripts/run-device-pass.sh <out>` — N reps
  × one app launch per runtime (`RUNTIMES=<rt>`), `.utility` QoS,
  `BENCH_TARGET_MS=2000`. The app never exits by itself: the launcher
  streams the console until the `BENCH_DONE` (or `FEMTOVG_E2E done`)
  marker, then terminates it. A launch with no output (tunnel drop) is
  retried; one the OS killed (jetsam SIGKILL) is a result, not retried.
  zwasm's memory-heavy rows run in launches of their own
  (`SPLIT_RUNTIMES` / `HEAVY_ROWS`), so one row's footprint cannot lose
  the rest of the launch. `E2E=0,1` runs the femtovg E2E instead.
- **M4 PMU**: `scripts/run-m4-pmu-pass.sh <out>` — separate from the
  timing passes. The 2026-09 report profiles tinywasm only
  (`RUNTIMES_LIST=tinywasm`). See *PMU on M4* below.
- `scripts/summarize-pass.py <pass-root> <data-dir>` turns the logs into
  the per-rep CSVs that the report's data dir keeps, and the derived
  "median [min–max]" tables (errors become footnoted codes with the error
  text) into `<pass-root>/report-tables/` under `out/`, which
  `scripts/build-report.py` fills into the report. Only the raw per-rep
  data is committed.

### Per-device notes

- **iPhone 12 (A14 Icestorm) / iPhone XS (A12 Tempest)**: launch via
  `devicectl device process launch --console --terminate-existing
  --environment-variables ...`. `.utility` QoS pins to E-cores (set in
  `BenchmarkContentView.swift`). Threads the harness spawns (the Zig
  runtimes' and the E2E's big-stack threads) must carry the caller's
  QoS: `run_on_thread` in lib.rs does that. A bare
  `std::thread::spawn` starts at the default QoS and moved the E2E onto
  the P-cores (e_share 0.004) before the fix.
- **Auto-lock**: a devicectl launch into an awake phone does not reset
  its idle timer, so an unattended run can hit auto-lock mid-launch and
  iOS suspends the app (the launch then waits on one row until the
  launcher times out). The app therefore disables the idle timer for the
  duration of a run (iOS / tvOS).
- **iPhone XS Max** used in 2026-09: devicectl UDID
  `00008020-001C292A2190003A`, iOS 18.7.10 (the A12 is not supported by
  iOS 26). Crash logs: `xcrun devicectl device info files --domain-type
  systemCrashLogs` + `copy from` (libimobiledevice does not see it).
- **iOS memory**: jetsam SIGKILLs the app when its footprint crosses the
  per-app limit (the XS has 4 GB RAM; zwasm rows with a ~1.5 GB peak RSS
  survived, its Porffor row did not). iOS also refuses two 4 GiB virtual
  reservations in one process: Pulley uses 256 MiB memory / GC-heap
  reservations everywhere (`pulley_engine()` in lib.rs), otherwise
  modules with a GC heap or several memories fail to instantiate.
- **Apple TV 4K (A12 / A15)**: same `devicectl` flow, bundle ID
  `com.rebeckerspecialties.wasmbench.tv`. tvOS reads
  `devicectl --environment-variables` into Swift `ProcessInfo` the same
  way iOS does (unlike watchOS). tvOS 26+ deployment target.
- **Apple Watch SE2 (S8)**: same `devicectl` launch flow, bundle ID
  `com.rebeckerspecialties.wasmbench.watch`. **xcodebuild requires
  `ARCHS=arm64_32 ONLY_ACTIVE_ARCH=NO`**. The watch app's workload
  filter is the hardcoded `WATCHOS_WORKLOADS_FILTER` constant in
  `apps/Shared/BenchmarkContentView.swift` —
  `devicectl --environment-variables` does **not** propagate to Swift
  `ProcessInfo` on watchOS, though it does propagate to Rust
  `std::env::var` (so `BENCH_TARGET_MS=2000` works). Watch BLE tunnel
  drops mid-session are common; wrap installs in a 5-attempt retry loop
  with `sleep 3`. If a run stalls on `Network.NWError` 60, wake the
  watch and retry. (Not re-measured in 2026-09: no watch attached.)
- **iOS scheduler stickiness**: at every QoS tier tested (`.utility`,
  `.userInitiated`, `.userInteractive`), iPhone 12 keeps sustained
  dispatch loops on E-cores. P-core PMU on iPhone 12 is structurally
  unobtainable from outside the app.

### PMU on M4 (2026-09)

- The M4 Max (t6041) CPU Counters **manual event lists cannot be set
  from xctrace's command line**: `--recording-options` only accepts an
  empty `allEventsAndFormulas`, and every element form of a manual event
  list is rejected. The pass therefore uses the **guided modes**, fixed
  event sets defined per SoC in
  `/System/Library/PrivateFrameworks/Recount.framework/Resources/Analysis/{bottleneck,metrics,characteristics}.json`,
  selected with `selectedCountingMode {analysisMode, countingMode}` in a
  JSON generated from `--show-recording-options`. One capture per
  (runtime, mode).
- The M4 core PMU (kpep database `as6`, `/usr/share/kpep/cpu_100000c_2_17d5b93a.plist`,
  104 events, 8 configurable counters) has **no L2/SLC miss event and no
  prefetch event**. L1D/L1I misses, TLB misses and table walks are the
  proxies.
- Attribution: `run_matrix` with `MATRIX_THREAD_PER_CASE=1` runs each
  case on a thread named `case:<id>` (and `run_on_thread` gives a
  runtime's own big-stack thread the same name), the E2E runs on
  `femtovg-e2e`, and `scripts/pmu_summarize.py` reduces the
  `MetricAggregationForThread` export per thread. That table holds each
  interval twice (precise rows cut at context switches and 10 ms
  imprecise buckets, identical integer totals); ratio metrics are sums
  of per-segment ratios, so bucket shares are normalized per interval
  and cycle-weighted. Per-1k-instruction denominators come from the
  same capture's `case_instructions` (process rusage over the case).
- Traces are exported and deleted immediately (a matrix capture is
  ~0.5 GB); no `.trace` goes into git.
- Not used yet: the guided modes `bottleneck:delivery` (delivery latency
  from icache / iTLB vs bandwidth lost to taken branches),
  `bottleneck:processing` (memory miss vs execution latency) and
  `bottleneck:l1d_miss_sampling` (samples L1D misses) break the
  level-1 buckets down further.

### PMU / xctrace gotchas on iPhone

- **Xcode 27 + iOS 26.5 (iPhone 12, 2026-09): the reverse of 26.5.**
  `--attach` fails ("Cannot find process for provided pid" / "matching
  name") while `--launch -- <bundle id>` with `--env KEY=VALUE` records
  both CPU Counters (guided modes, `MetricAggregationForThread`) and the
  Time Profiler. `scripts/run-device-pmu.sh` uses launch mode, one row
  per capture, and `scripts/profile_summarize.py` reduces Time Profiler
  exports (the Release app keeps its symbol table, so handler names
  resolve).
- **xctrace leaves its raw kernel trace behind**: every recording writes
  `instruments*.ktrace` into `$(getconf DARWIN_USER_TEMP_DIR)` at up to
  ~100 MB/s of recording (system-wide kdebug; worse while devicectl
  streams) and never deletes it. A 2-minute M4 capture reached 11 GB and
  filled the disk mid-pass. Keep captures short and delete the `.ktrace`
  after each export (both PMU scripts do), and keep a free-space
  watchdog on long passes.
- **Xcode 26.5 (historical)**: `--launch` was broken for iPhone 12 /
  iOS 26.3+ (the trace held only `RunIssues.storedata`, no counter
  data), and `--attach <pid>` worked:
  ```sh
  xcrun devicectl device process launch --device <UDID> \
    --terminate-existing \
    --environment-variables '{"WORKLOADS":"vtable","BENCH_TARGET_MS":"15000"}' \
    com.rebeckerspecialties.wasmbench.ios &
  sleep 3
  PID=$(xcrun devicectl device info processes --device <UDID> \
        | grep -i wasmbench | awk '{print $1}' | head -1)
  xcrun xctrace record --device <DEV-ID> --template "CPU Counters" \
    --output capture.trace --attach $PID --time-limit 15000ms
  ```
- **Two different device IDs**: `devicectl` uses the UDID; `xctrace`
  uses Apple's device ID. Map with `xcrun xctrace list devices`.
- **Template name**: "CPU Bottlenecks" became "CPU Counters" in 26.5.
- **Per-platform counter availability**: A12 Tempest (iPhone XS) and S8
  (Watch SE2) do NOT expose `CounterMetricByThread` — wallclock and the
  rusage counters only. A14 Icestorm (iPhone 12) does (attach mode;
  `scripts/run_per_workload_pmu.sh`). M4: launch mode works.
- **Export quirk**: `xctrace export --xpath '...' > file.xml` may
  silently produce 0-byte output. Use `--output file.xml`.
- **XPath**: `//trace-toc/run/data/table[@schema="..."]` works across
  trace variants; index-based paths do not.

### Bucket analysis tools

- `scripts/analyze_pmu.py LABEL_A a.xml LABEL_B b.xml` — pairwise
  `CounterMetricByThread` diff (iPhone 12 captures).
- `scripts/pmu_summarize.py` + `scripts/summarize-pass.py` — the M4
  guided-mode pass (above).
- `scripts/aggregate_3way.py`, `scripts/aggregate_4way.py`,
  `scripts/parse_n10.py`, `scripts/m4_phase4_bucket_shares.py` — the
  2026-05 fusion-phase analyses.

### Bash launcher gotchas

- `echo` from inside a `nohup`'d bash script is block-buffered to the
  output file; launch with `stdbuf -oL` (`nohup stdbuf -oL ./script ...
  > log 2>&1 &`) or watch the per-rep output files instead.
- macOS bash is 3.2: no `declare -A`, and `"${arr[@]}"` of an empty
  array fails under `set -u` (use `${arr[@]+"${arr[@]}"}`).
- The Bash tool's shell is zsh: run bash-isms through `bash -c`.

## QoS env-var override

The iOS app reads `BENCH_QOS` to override the default `.utility`
dispatch QoS:
- `BENCH_QOS=user-initiated` → `DispatchQoS.userInitiated`
- `BENCH_QOS=user-interactive` → `DispatchQoS.userInteractive`
- unset / anything else → `.utility` (E-core-preferred)

## wasmtime submodule

`./wasmtime/` points at `rebeckerspecialties/wasmtime`, branch
`pulley-bench-stack-v49`: upstream v49.0.0 plus nine commits,
cherry-picked from the July soundness-fixed split of PR #2 and the
phase-4 fusion:

  environ: track which tables can be mutated after instantiation
  cranelift: use a constant table bound when the table can never grow
  tests: runtime coverage for call_indirect over immutable and mutated tables
  cranelift: elide the call_indirect signature check on uniform immutable tables
  cranelift: drop the call_indirect null check when no table slot can be null
  pulley: add call_indirect{1,2,3,4} fused indirect-call ops
  cranelift/pulley: pass first 4 indirect-call args via call_indirectN
  tests: runtime coverage for call_indirectN arg bundling
  config: add LEGACY_EXCEPTIONS to known-features list

Gates on v49: disas 2590/2590, wasmtime-environ table_mutability 16/16
under two feature sets, wast 1382/1382 (CraneliftNative + Winch) and
691/691 with CraneliftPulley. Pulley still cannot compile legacy EH
(`try`: "Unsupported feature: operator Try"), so the legacy-EH rows are
N/A on Pulley; exnref (`try_table`) works. The older branches
(`table-mutability-tracking`, `claude/pulley-fusion-xband-brif`,
`accurate-graphql-needs-legacy-exceptions`) remain on the fork as
history; `patches/pulley-fusion-*` are their format-patch exports.

The harness engine (`pulley_engine()` in lib.rs): `Config::target
("pulley64")`, the DRC collector (`gc-drc`; with only `gc-null` the GC
heap is never reclaimed), 256 MiB memory and GC-heap reservations with
no growth reservation, explicit bounds checks (Pulley never uses guard
pages or signal-based traps).

### Patch-stack discipline across all runtime submodules

Every runtime submodule pins an UPSTREAM SHA (target-lexicon, mach2,
wasmtime and femtovg are the only ones pointing at our forks). Local
fixes we carry without bumping the pin live as `.patch` files in
`patches/<runtime>/`, applied at build time by each
`scripts/build-<runtime>.sh` via `scripts/apply_patch_series.sh`. The
apply step is idempotent — reset to the pinned gitlink first, then
forward-apply each patch in the series, skipping any that are already
applied. **Never drop a patch silently**: a patch that is upstream gets
retired in a commit that says where it landed.

Current series (2026-09):

  * `wasm-micro-runtime/0001-0029` on upstream main `b70d708d`:
    0001-0017 legacy EH for fast-interp (fork PR #2), 0018-0027 relaxed
    SIMD (fork PR #3 = upstream #4950), 0028-0029 opt-in PROT_NONE
    linear-memory reservation (fork PR #4). All cherry-pick cleanly.
  * `wasmedge/` (26: 0001-0003, 0006-0028) on 0.17.2-rc.3 — Apple-mobile
    guarded-memory fallbacks, interpreter super-instructions, arm64_32
    fixes. 0005 retired (upstream). 0006, 0009, 0011, 0012, 0014-0016,
    0023, 0024 were rebased; each patch's message records its conflict
    resolution (0023's cached-default-locals fast path now only covers
    functions whose locals are all numeric).
  * `zwasm/0001` compiles the JIT out of the C API when
    `-Dengine=interp` (upstream v2.7.0 only reads the flag for the CLI's
    `--version`, so libzwasm.a otherwise carries the JIT and imports
    `pthread_jit_write_protect_np`); `zwasm/0002` restores the
    arm64_32-apple-watchos ILP32 static-lib build (upstream has no CI
    job for it). The old arm64_32 patch landed upstream as zwasm#98.
  * `wasmz/0002` arm64_32-apple-watchos support, reworked for v0.1.4.
    The Zig 0.16 port (old 0001) landed upstream as wasmz#3.
  * wasm3: none — the v128-as-opaque-slot patch is upstream (wasm3#559,
    in v0.9.0).
  * `pulley-fusion-*` — historical exports of the 2026-05 fork branches
    (not applied by any build).

CI (`.github/workflows/build.yml`) reproduces a clean checkout +
submodule init + every patch series + cross-target builds.

## Cross-runtime comparison

All seven are built interpreter-only; exact flags:

1. **Pulley** — see *wasmtime submodule*. RUSTFLAGS
   `--cfg=pulley_tail_calls -C target-cpu=apple-a12`, nightly-2026-07-05,
   fat LTO, one codegen unit. Cranelift runs at load time as a compiler
   to Pulley *bytecode* (data, not executable pages).
2. **WAMR** fast-interp (`wasm-micro-runtime/`, `libiwasm.a`) — cmake
   Release, `-O3 -mcpu=apple-a12`; `WAMR_BUILD_INTERP=1 FAST_INTERP=1
   AOT=0 JIT=0 FAST_JIT=0`, `SIMD=1 RELAXED_SIMD=1 BULK_MEMORY=1
   EXTENDED_CONST_EXPR=1 TAIL_CALL=1 REF_TYPES=1 EXCE_HANDLING=1`,
   `LIBC_WASI=0 LIBC_BUILTIN=0 MULTI_MODULE=0 LIB_PTHREAD=0
   MINI_LOADER=0`, `WAMR_DISABLE_HW_BOUND_CHECK=1`,
   `-DWASM_LINMEM_RESERVATION_CAP` 64 MB (16 MB on arm64_32). GC /
   typed function references (`WAMR_BUILD_GC=1`) work in fast-interp but
   cost +17-42 % on call-heavy workloads, so the benchmark build leaves
   them off (GC, function-references, memory64, multi-memory and exnref
   are therefore "no" for WAMR in the matrix). `build-wamr.sh` deletes
   `wasm_c_api.c.o` from every `libiwasm.a`: zwasm v2 exports the same
   standard `wasm_*` names and the linker otherwise binds zwasm's calls
   to WAMR's (SIGBUS on macOS, `bh_vector_destroy` crash on iOS); a weak
   no-op `wasm_trap_delete` keeps WAMR-only links resolving.
3. **wasm3** v0.9.0 (`libm3.a`) — `-O3 -mcpu=apple-a12 -std=c99 -DNDEBUG
   -fno-exceptions`, no WASI sources, plus v0.9's `m3_validate.c`. No
   SIMD (v128 locals parse, v128 ops don't), no exceptions, no GC, one
   memory: the SIMD-canonical rows are N/A and the `[scalar build]`
   twins are its comparison rows.
4. **WasmEdge** 0.17.2-rc.3 (`libwasmedge.a`) — the incumbent production
   recipe from webgpu-caps: cmake `MinSizeRel`, `-Os -DNDEBUG
   -mcpu=apple-a12 -flto=full -fembed-bitcode`,
   `WASMEDGE_USE_LLVM=OFF`, static lib with
   `WASMEDGE_STATIC_LIB_ENABLE_LTO=ON`, tools / plugins / tests off.
   Host functions have the 4-argument signature
   `(data, calling_frame, params, returns)`. On arm64_32-apple-watchos
   `WasmEdge_VMInstantiate` BRKs (an `assuming()` predicate in
   `lib/executor/instantiate/*`), so the adapter returns a clean error
   there.
5. **zwasm** v2.7.0 (`libzwasm.a`) — Zig 0.16.0, `-Dengine=interp
   -Doptimize=ReleaseFast` (`-Dwasm=3.0`, `-Dwasi=p2` defaults) plus
   `patches/zwasm/0001` (no JIT in the library: 17.6 MB → 5.1 MB, 0
   JIT-named symbols). The adapter creates every instance with
   `zwasm_instance_new_ex(.., ZWASM_ENGINE_INTERP)` and fails if the
   resolved engine is not the interpreter. No interpreter SIMD-128 (v128
   ops trap `unreachable`). Needs an 8 MiB-stack thread and a weak
   `_dyld_get_image_header_containing_address` stub on iOS/tvOS/watchOS.
   Its P3 CLI waits for stdin EOF: run it with `</dev/null`.
6. **wasmz** v0.1.4 (`libwasmz.a`) — Zig 0.16.0, `-Doptimize=ReleaseFast`,
   `zig build static-lib`. No JIT/AOT tier exists. 8 MiB-stack thread,
   same dyld stub.
7. **tinywasm** 0.11.0 — crates.io, `default-features = false`,
   features `std`, `parser`, `validate` (its `archive` serializer off),
   `nightly-tail-calls` via `nightly-dispatch`. No native codegen at all
   (`#![forbid(unsafe_code)]` outside opt-in x86 intrinsics).

Feature support in the shipped builds (smoke modules,
`scripts/feature-matrix.sh`, evidence in
`docs/runtime-comparison-2026-09-22/feature-matrix/`): tail calls on all
seven; extended-const on all but wasmz (wrong result); relaxed SIMD on
Pulley, WAMR, WasmEdge, wasmz, tinywasm; memory64, GC and typed function
references on Pulley, WasmEdge, zwasm, wasmz, tinywasm; multi-memory on
Pulley, WasmEdge, zwasm, tinywasm; exnref on Pulley, WasmEdge, zwasm,
tinywasm (wasmz: basic `try_table` only); legacy EH on WAMR and wasmz;
component model / WASI 0.3 async on wasmtime-Pulley and zwasm only (both
through their CLIs; the WAMR cm_wasip2 lineage is WASIp2-only).

The femtovg E2E ([docs/femtovg-e2e-abi.md](docs/femtovg-e2e-abi.md)):
the wasm guest does femtovg's CPU work per frame and a native host
replays its Renderer command stream through femtovg's wgpu renderer on
Metal. Every runtime runs its best guest build (simd128 on Pulley, WAMR,
WasmEdge, tinywasm; scalar on wasm3, zwasm, wasmz) and every runtime's
frames hash identically.

### Device-side status

2026-09-22, iPhone XS Max (A12, iOS 18.7.10): all seven runtimes init
and complete their rows in per-runtime launches; zwasm's
graphql-validation (Porffor) launch is jetsam-killed (recorded as such).
The watch, Apple TV, iPhone 12 and iPhone 16 were not re-validated with
the 2026-09 builds; their last status (2026-05-16/17, six runtimes):

| device | hardware | runtimes that completed the set (2026-05) |
|---|---|---|
| iPhone 12 | A14 Icestorm | all 6 |
| iPhone XS Max | A12 Tempest | all 6 |
| iPhone 16 Pro Max | A18 Pro | all 6 (fib-only check) |
| Watch SE2 | S8, arm64_32 | Pulley, WAMR, wasm3 full watch filter; wasmz + zwasm init ok; WasmEdge clean ERROR (instantiate BRK) |
| Apple TV 4K | A12, tvOS 26 | all 6 init; wasmz crashed mid-run on a `factorial(20)` miscompile of its old pin |

The 2026-09 builds link all seven runtimes into the watch app
(arm64_32), but that app was not run on a watch in this refresh.

### WAMR fast-interp legacy exception handling

**Status**: complete and carried as `patches/wasm-micro-runtime/0001-0017`
(fork PR #2, full legacy EH: try / catch / catch_all / rethrow /
delegate, tag payloads, result-typed try regions, plus the 2026-08-16
correctness fixes). Porffor's graphql-validation and the legacy-EH
parser benchmark run on WAMR. Remaining limits, both trapping cleanly:
exception payloads crossing a function boundary (patch 0014 traps —
the benchmark's legacy EH parser passes its error position through a
global for this reason) and `br` to a loop entry from inside a try
region (0015 rejects at load). `try_table` / `throw_ref` (exnref) are
not implemented anywhere in WAMR (`b70d708d` has no `TRY_TABLE` /
`THROW_REF` in `core/`).

Land-mines from that work (keep them in mind when touching the loader
or the fast-interp EH paths):

  1. **Loader pass-1 / pass-2 size accounting must match.** Any
     `emit_*` must run in both traverses or pass 2 overruns the
     `code_compiled` buffer sized by pass 1. Gate the *populate* on
     `p_code_compiled != NULL`, never the *emit*.
  2. **IR encoding under `WASM_ENABLE_LABELS_AS_VALUES`**: each opcode
     in the rewritten IR is an 8-byte handler pointer; constants move to
     the per-function const pool. Don't reason about IR layout by
     counting source bytes.
  3. **`scripts/build-wamr.sh` resets the submodule** to the pinned
     gitlink before applying the series, wiping uncommitted WAMR
     changes. Commit on a fork branch, or run cmake/make directly in
     the build dir while iterating.
  4. **`frame->exception_raised` is not zero-initialized by
     `ALLOC_FRAME`** in fast-interp; the return-path hook reads it.
  5. **`wasm_runtime_load` does not copy the wasm bytes** — keep the
     buffer alive for the module's lifetime.

Tests: `cargo test -p benchmark-core --test eh_correctness` (60+ cases),
`src/bin/probe_eh_void.rs` as a smoke check, and WAMR's own spec runner
with `--eh` (the legacy-EH `.wast` files run on fast-interp since patch
0012). Cost-model rule upstream reviewers will apply: EH must not tax
`CALL` / `LOAD` / `STORE` handlers on the success path.

### Skipped runtimes (App Store / Apple-platform feasibility)

| runtime | reason skipped |
|---|---|
| **wasmer** | All backends (Singlepass, Cranelift, LLVM) are JIT — emit native code at runtime and require MAP_JIT. No pure-interpreter backend. |
| **Silverfir-nano** (`mbbill/Silverfir-nano`) | Self-describes as a "compact optimizing WebAssembly 3.0 **JIT**"; JIT is mandatory, no interpreter mode. |

### Historical: phase-4 Pulley/WAMR wallclock ratios (2026-05, pre-refresh builds)

| workload | iPhone 12 A14 | iPhone XS A12 | Watch SE2 S8 | Apple TV 4K A12 |
|---|---:|---:|---:|---:|
| xmrsplayer | 1.33× | 1.22× | **1.16×** | 1.28× |
| graphql-validation (AS) | 1.56× | 1.58× | 1.24× | 1.91× |
| call_indirect | 1.67× | 1.49× | 1.46× | 1.78× |
| vtable_bi | 1.65× | 1.48× | 1.48× | 1.77× |
| vtable_poly4 | 1.58× | 1.42× | 1.36× | 1.54× |
| vtable_poly6 | 1.61× | 1.48× | 1.42× | 1.65× |
| vtable_mono | 1.74× | 1.45× | 1.54× | 1.81× |

Apple TV 4K runs the same A12 as the iPhone XS Max but sustains higher
clocks (plugged in, no thermal envelope): `fib(30)` on Pulley took 71 ms
on the TV vs 128 ms on the XS. The Pulley-vs-WAMR gap is structural —
WAMR's load-time register IR has fewer dispatches per source wasm op.
The 2026-09 numbers (v49 stack, WAMR main) are in the report.
