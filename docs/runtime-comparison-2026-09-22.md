# Runtime comparison, 2026-09-22

Seven App-Store-eligible WebAssembly interpreters at their latest
releases, measured on the M4 Max E-cores and on an iPhone XS Max (A12),
plus Wasm 3.0 feature benchmarks, a WASI 0.3 component-model async
benchmark, a femtovg-to-wasm E2E rendered by a Metal host, and a PMU
profile of tinywasm on the M4 E-cores.

## Summary

CPU time per call relative to the fastest runtime on each case,
geometric mean over the 17 cases every runtime runs (N=10 per platform):

| | wasm3 | wasmz | WAMR | Pulley | tinywasm | WasmEdge | zwasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| M4 Max E-cores | 1.01 | 1.59 | 1.63 | 2.09 | 3.20 | 8.42 | 8.47 |
| iPhone XS (A12) E-cores | 1.04 | 1.63 | 1.38 | 1.68 | 2.98 | 9.28 | 8.98 |

- **wasm3 is the fastest interpreter on nearly every case it runs**,
  including xmrsplayer, graphql (AssemblyScript) and audio DSP. The
  exception is instantiation: the extended-const rows cost it 62-122×
  Pulley's time. Of the Wasm 3.0 features tested it has only tail calls
  and extended-const.
- **Features.** Pulley, WasmEdge and tinywasm pass every core Wasm 3.0
  smoke module except legacy EH. The others fall short:
  - zwasm has no SIMD in its interpreter;
  - wasmz gets extended-const and `catch_ref` / `throw_ref` wrong and
    has no multi-memory;
  - the shipped WAMR build runs relaxed SIMD and legacy EH (our
    patches), but has no GC, typed references, memory64, multi-memory or
    exnref.

  WASI 0.3 async components run only on wasmtime-Pulley and zwasm, and
  Pulley is 1.7-3.3× faster there.
- **The incumbent, WasmEdge**, is 8-9× the fastest runtime. xmrsplayer
  is the workload closest to the production audio app. On it, WasmEdge
  spends 159 ms of A12 E-core CPU per 1024-frame buffer; wasm3, wasmz,
  WAMR and Pulley spend 19-27 ms, and tinywasm 49 ms.
- **femtovg E2E.** All seven runtimes draw bit-identical frames. The
  guest's CPU work dominates every frame, so frame rate ranks the
  runtimes as the MVP workloads do: on the A12, scene 0 runs at 7.0 fps
  on wasm3, down to 1.1-1.2 fps on zwasm and WasmEdge. Pulley compiles
  the 1 MB guest with Cranelift at load, which takes ~2.7 s on the A12.
- **tinywasm is instruction-bound**: 69 % useful slots on the M4 and
  62-90 % on the iPhone 12. It retires 2.55× WAMR's instructions for
  1.97× WAMR's CPU time. Three changes, stacked as PRs on tinywasm's
  `next` in our fork, cut its cycles by 8.0 % on the iPhone 12. Two are
  small safe-Rust changes (−5.3 %). The third reserves each function's
  operand stack on entry, for another −2.8 %, which is the whole
  upper-bound gain.
- **Build.** Cross-language LTO is impossible at these Rust minimums:
  Rust ≥ 1.95 emits LLVM 22 bitcode, and Xcode 27's libLTO is LLVM 21.
  The Rust side is built with fat LTO instead, and the C, C++ and Zig
  runtimes link as native objects. `--cfg=pulley_tail_calls` stays,
  because Pulley's default dispatch loop costs 1.3-1.9× the cycles.
- **Bugs found** (none fixed here):
  - wasmz fails six rows: wrong results, traps and crashes.
  - zwasm, WasmEdge and wasmz never collect GC structs while an
    instance lives.
  - zwasm also keeps memory per call and per instance. Porffor gets it
    jetsam-killed on the iPhone.

Raw per-rep data: [`runtime-comparison-2026-09-22/`](runtime-comparison-2026-09-22/)
(CSV / JSONL, one row per rep × runtime × case; the tables below are
generated from it by `scripts/summarize-pass.py`).

## Contents

1. [Setup](#setup)
2. [Runtimes: versions, patches, build flags](#runtimes-versions-patches-build-flags)
3. [Methodology](#methodology)
4. [Feature matrix](#feature-matrix)
5. [Workload coverage](#workload-coverage)
6. [M4 Max E-cores](#m4-max-e-cores)
7. [iPhone XS Max](#iphone-xs-max)
8. [Feature benchmarks against their twins](#feature-benchmarks-against-their-twins)
   and [Per-case memory (M4)](#per-case-memory-m4)
9. [Component model async (WASI 0.3)](#component-model-async-wasi-03)
10. [femtovg E2E](#femtovg-e2e)
11. [tinywasm PMU (M4 E-cores)](#tinywasm-pmu-m4-e-cores)
12. [Bugs and limits found](#bugs-and-limits-found)
13. [Reproducing](#reproducing)

## Setup

| | |
|---|---|
| Host | MacBook Pro, Apple M4 Max (12 P + 4 E cores), macOS 27.0 (26A428) |
| Device | iPhone XS Max, A12 Bionic (2 Vortex P + 4 Tempest E), iOS 18.7.10 |
| Xcode | 27.0 (27A266a), Apple clang 21.0.0 |
| Rust | nightly-2026-07-05 (1.98.0-nightly, LLVM 22.1.8) for every harness build; 1.93.1 for the wasm32 guests |
| Zig | 0.16.0 (zwasm, wasmz) |
| wasm-tools | 1.247.0 |

**Cross-language LTO is not possible at these Rust minimums.** wasmtime
v49 needs rustc ≥ 1.96 and tinywasm 0.11 needs ≥ 1.98; every Rust ≥ 1.95
uses LLVM 22, and Xcode 27's libLTO (LLVM 21) rejects its bitcode
("Unknown attribute kind (105) (Producer: 'LLVM22.1.8-rust-1.98.0-nightly'
Reader: 'LLVM APPLE_1_2100.3.34.2_0')"). The Rust side is built with fat
LTO across all Rust crates instead, and the C/C++/Zig runtimes are linked
as native objects. Getting cross-language LTO back would mean a Rust on
LLVM 21 (≤ 1.94), which cannot build wasmtime v49 or tinywasm 0.11.

## Runtimes: versions, patches, build flags

| runtime | version | pin | carried patches | build |
|---|---|---|---|---|
| **Pulley** (wasmtime) | v49.0.0 + 9 commits | fork branch `pulley-bench-stack-v49` `0d9aebd66d` | fork commits: the soundness-fixed split of the table-mutability stack (5), `call_indirect{1,2,3,4}` arg bundling (3), LEGACY_EXCEPTIONS known-feature (1) | cargo, nightly-2026-07-05, `RUSTFLAGS="--cfg=pulley_tail_calls -C target-cpu=apple-a12"`, fat LTO, 1 CGU; features `pulley runtime std cranelift gc gc-drc gc-null`; `Config::target("pulley64")`, DRC collector, 256 MiB memory and GC-heap reservations, no growth reservation |
| **WAMR** fast-interp | `main` (WAMR-2.4.1-364) | `b70d708d` (2026-09-21) | 29: 0001-0017 legacy EH for fast-interp, 0018-0027 relaxed SIMD, 0028-0029 PROT_NONE linear-memory reservation | cmake Release, `-O3 -mcpu=apple-a12`; `INTERP=1 FAST_INTERP=1 AOT=0 JIT=0 FAST_JIT=0 SIMD=1 RELAXED_SIMD=1 BULK_MEMORY=1 EXTENDED_CONST_EXPR=1 TAIL_CALL=1 REF_TYPES=1 EXCE_HANDLING=1 LIBC_WASI=0 LIBC_BUILTIN=0 MULTI_MODULE=0 LIB_PTHREAD=0 MINI_LOADER=0`, `WAMR_DISABLE_HW_BOUND_CHECK=1`, `WASM_LINMEM_RESERVATION_CAP` 64 MB; `wasm_c_api.c.o` removed from the archive |
| **wasm3** | v0.9.0 | `0cd38327` | none | `-O3 -mcpu=apple-a12 -std=c99 -DNDEBUG -fno-exceptions`, no WASI sources |
| **WasmEdge** interpreter | 0.17.2-rc.3 | `16ea4c45` | 26 (Apple-mobile stack: 0001-0003, 0006-0028) | cmake MinSizeRel, `-Os -DNDEBUG -mcpu=apple-a12 -flto=full -fembed-bitcode`, `WASMEDGE_USE_LLVM=OFF`, static lib with `WASMEDGE_STATIC_LIB_ENABLE_LTO=ON`, no tools / plugins / tests (the incumbent production app's recipe) |
| **zwasm** | v2.7.0 | `d09d9248` | 0001 compile the JIT out of the C API under `-Dengine=interp`; 0002 arm64_32 ILP32 build | zig 0.16.0 `-Dengine=interp -Doptimize=ReleaseFast` (defaults `-Dwasm=3.0 -Dwasi=p2`); every instance created with `ZWASM_ENGINE_INTERP` and the resolved engine checked |
| **wasmz** | v0.1.4 | `0796998b` | 0002 arm64_32 watchOS | zig 0.16.0 `-Doptimize=ReleaseFast`, `zig build static-lib` |
| **tinywasm** | 0.11.0 | crates.io | none | `default-features = false`, features `std parser validate`, plus `nightly-tail-calls` (its `become` dispatch loop) |

Interpreter-only, checked per runtime:

- Pulley: Cranelift runs at load time and emits Pulley *bytecode* (data);
  never Cranelift-native or Winch.
- WAMR: `AOT=0 JIT=0 FAST_JIT=0`.
- WasmEdge: `WASMEDGE_USE_LLVM=OFF` (no AOT compiler in the library).
- zwasm v2 ships a JIT and an AOT loader, and upstream's `-Dengine=interp`
  only labels the CLI's `--version`, so `libzwasm.a` still contained the
  JIT and imported `pthread_jit_write_protect_np` and
  `sys_icache_invalidate`. Patch 0001 compiles the JIT out of the C API:
  the library goes from 17.6 MB to 5.1 MB with no JIT-named symbols.
- wasmz, wasm3 and tinywasm have no native tier at all.

**Upstream versions not used, and why.**

- WAMR: the latest release, WAMR-2.4.5, is on `release/2.4.x`, cut from
  2.4.1 in 2025-07. It lacks 364 `main` commits, and none of the patches
  apply to it.
- WasmEdge: 0.17.2-rc.3 is the newest pre-release; 0.17.1 is the latest
  stable.
- wasm3: v0.9.1-beta.1 exists, but it is a pre-release.

**Patches retired, and why.**

- wasm3: `0001` (v128 locals as opaque slots) is upstream as wasm3#559,
  in v0.9.0.
- wasmz: `0001` (the Zig 0.16 port) is upstream as wasmz#3.
- zwasm: the old arm64_32 patch is upstream as zwasm#98. v2.7.0 broke that
  target again (it now depends on `std.Io.Threaded`, and the interpreter
  indexes slices with `u64`), hence the new `0002`.
- WasmEdge: `0005` (uint128 `is_class`) is upstream. Nine patches needed a
  rebase; each patch's message records the resolution. In the largest one,
  `0023`'s cached-default-locals fast path now covers only functions whose
  locals are all numeric, because upstream now initializes ref-typed locals
  as bottom-typed nulls.
- Pulley: not carried from the old pin are eager table init, the
  constant-index `call_indirect` lowering and fusion phases 1–3 (they fire
  only with eager init). The July soundness-fixed split replaced them.

Every WAMR patch cherry-picks cleanly onto `b70d708d`.

**`--cfg=pulley_tail_calls` stays.** v49 still ships the same three
dispatch loops (`pulley/src/interp.rs`: default `match` loop, the unsafe
LLVM-best-effort variant, and the nightly `become` loop), and the
default is still the `match` loop, and it is much slower. Measured with
`scripts/run-pulley-dispatch-ab.sh`: the same v49 build with and without
the cfg, interleaved, 5 reps, M4 E-cores:

<!-- T:pulley-ab -->
| case | tail (`--cfg=pulley_tail_calls`) CPU ms/call | match loop CPU ms/call | match ÷ tail, CPU | match ÷ tail, cycles | tail wall ms | match wall ms |
|---|---:|---:|---:|---:|---:|---:|
| fib | 83.46 [63.8–95.93] | 135.4 [122.3–150.6] | 1.62× | 1.52× | 95.2 [82.28–106.6] | 155.6 [141.7–171.6] |
| fib_tail | 0.4894 [0.4009–0.5182] | 0.6963 [0.6451–0.7718] | 1.42× | 1.46× | 0.6307 [0.3762–0.6322] | 0.8915 [0.734–0.9322] |
| audio_dsp | 1084 [1035–1237] | 2022 [1890–2205] | 1.87× | 1.80× | 1359 [1219–1434] | 2263 [2229–2481] |
| call_indirect | 30.81 [22.26–34.42] | 45.6 [42.18–49.1] | 1.48× | 1.48× | 35.94 [21.39–41.91] | 52.59 [49.38–56.02] |
| convolution.scalar | 10.88 [8.691–12.4] | 20.05 [17.89–21.57] | 1.84× | 1.81× | 12.18 [9.794–15.43] | 22.64 [21.36–25.26] |
| xmrsplayer | 15.88 [12.26–17.69] | 20.95 [14.84–22.01] | 1.32× | 1.34× | 18.64 [14.61–22.07] | 23.34 [13.37–26.51] |
| vtable_poly4 | 54.21 [39.44–61.23] | 80.6 [79.96–92.94] | 1.49× | 1.46× | 68.08 [46.46–72.95] | 95.48 [90.4–106] |
| graphql_as | 9.326 [8.235–10.79] | 13.28 [12.52–14.48] | 1.42× | 1.37× | 10.56 [9.845–12.33] | 14.89 [14.48–17.11] |
<!-- /T:pulley-ab -->

The default loop costs 1.3-1.9× the cycles of the tail-call dispatch, in
line with the 22-34 % (M4) to 30-50 % (Watch SE2) savings measured in
2026-05.

## Methodology

**Harness.** One case table (`crates/benchmark-core/src/cases.rs`, 39
cases) drives all seven adapters. Each case is an export called with a
fixed argument, and its result is checked against the cross-runtime
consensus (independently re-derived in Python for the feature
benchmarks), so a fast wrong answer is an error, not a number. Timing:

- Load and instantiation are outside the timed window.
- The export is called repeatedly until `BENCH_TARGET_MS` (2000 ms) of
  timed work; the window reports per-call min, median and p99, and CPU
  time.
- `proc_pid_rusage(RUSAGE_INFO_V6)` gives the window's P-core CPU time,
  instructions and cycles. From those, **E-core residency is measured,
  not assumed**: `e_share = 1 − P-core time / CPU time`. IPC is also
  recorded.
- Instantiate-per-sample cases (extended-const) time instantiate + call;
  teardown happens outside the clock.

**Passes.**

- **M4 E-cores:** `scripts/run-m4-pass.sh`, N = 10.
  - Every rep runs each runtime in its own process under
    `taskpolicy -b`, runtimes interleaved.
  - After the matrix, each rep runs the femtovg E2E per (runtime, scene).
  - The component-model async benchmark runs at the end.
  - Nothing else ran on the host.
- **iPhone XS Max:** `scripts/run-device-pass.sh`, N = 10.
  - One app launch per runtime (`RUNTIMES=<rt>`), `.utility` QoS,
    `BENCH_TARGET_MS=2000`, launched with devicectl.
  - zwasm's six memory-heavy rows run in launches of their own. Its
    footprint reaches ~1.2 GB in xmrsplayer, and one jetsam kill would
    otherwise lose every later row of the launch.
- **tinywasm PMU (M4):** `scripts/run-m4-pmu-pass.sh` with
  `RUNTIMES_LIST=tinywasm`, run separately and never alongside a timing
  pass (see [tinywasm PMU](#tinywasm-pmu-m4-e-cores)).

**Tables.** Cells are the median over reps of each rep's own median,
with the [min–max] over reps. Wall time is per call. CPU time is
`cpu_user / iterations`. Failed cells carry a code whose text is listed
under the table.

## Feature matrix

Every cell below comes from running a smoke module, not from
documentation (`scripts/feature-matrix.sh`; the raw results and error
texts are in
[`runtime-comparison-2026-09-22/feature-matrix/`](runtime-comparison-2026-09-22/feature-matrix/)).

- **Core features.** Each smoke module
  (`workloads/features/<feature>.wat`) computes `run(41) == 42` through
  the feature's instructions, e.g. `return_call` ping-pong a million deep,
  `struct.new` / `array.set`, `call_ref` through `br_on_null`, an `i64`
  memory at offset 65528. A wrong answer counts as a failure. The modules
  run through `run_matrix --file` in exactly the build each runtime ships.
- **Component model / WASI 0.3.** The harness adapters are core-wasm
  APIs, so components run through each runtime's own component-aware
  entry point: the wasmtime v49 CLI with `--target pulley64`, the zwasm
  CLI built `-Dengine=interp -Dwasi=p3`, and, for WAMR, the `iwasm` of the
  fork's cm_wasip2 lineage (component model + WASI, fast-interp; not the
  shipped harness build). The components are zwasm's vendored official
  wasi-testsuite `wasm32-wasip3` binaries plus our
  `cm_async_bench.wasm`. Every shipped core loader must reject them, and
  every one does.

| feature (smoke) | Pulley | WAMR | wasm3 | WasmEdge | zwasm | wasmz | tinywasm |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| tail calls (`return_call`) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| extended-const | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ wrong result (40) | ✓ |
| typed function references | ✓ | ✗ ¹ | ✗ | ✓ | ✓ | ✓ | ✓ |
| GC (structs, arrays) | ✓ | ✗ ¹ | ✗ | ✓ | ✓ | ✓ | ✓ |
| memory64 | ✓ | ✗ | ✗ | ✓ | ✓ | ✓ | ✓ |
| multi-memory | ✓ | ✗ | ✗ | ✓ | ✓ | ✗ | ✓ |
| relaxed SIMD | ✓ | ✓ ² | ✗ | ✓ | ✗ ³ | ✓ | ✓ |
| EH, exnref: `try_table` + `catch` | ✓ | ✗ | ✗ | ✓ | ✓ | ✓ | ✓ |
| EH, exnref: `catch_ref` + `throw_ref` | ✓ | ✗ | ✗ | ✓ | ✓ | ✗ stack underflow | ✓ |
| EH, legacy `try` / `catch` | ✗ ⁴ | ✓ ² | ✗ | ✗ | ✗ | ✓ | ✗ ⁵ |
| component model, WASIp2 CLI | ✓ | ✓ ⁶ | — | — | ✓ | — | — |
| WASIp3 async (4 wasi-testsuite components + cm_async_bench) | ✓ | ✗ ⁶ | — | — | ✓ | — | — |

✓ passes, ✗ fails (load error, trap or wrong result), — no
component-aware entry point (the core loader rejects the component).

1. WAMR has GC and typed function references in fast-interp
   (`WAMR_BUILD_GC=1`), but that build costs +17-42 % on call-heavy
   workloads (fib, vtable, call_indirect, graphql; M4, 2026-09-22). The
   shipped build leaves them off, and the loader then rejects the types
   ("invalid local type", "invalid type flag").
2. With our patch series: relaxed SIMD 0018-0027, legacy EH 0001-0017.
   Upstream WAMR fast-interp has neither.
3. zwasm has SIMD-128 only in its JIT. In the interpreter the v128 ops
   trap `unreachable`.
4. Cranelift's Pulley backend has no legacy `try`: "Unsupported
   feature: operator Try".
5. tinywasm 0.11 validates with wasmparser's legacy-exceptions feature
   off, so `try` / `catch` is rejected ("legacy exceptions support is
   not enabled"). A bare `throw`, which is all Porffor's production
   build uses, runs.
6. The WAMR cm_wasip2 fork (not the shipped build) runs the WASIp2 CLI
   component and exits 255 on every WASIp3 one.

**Which features got a benchmark.** A feature got one when at least two
shipped runtimes pass its smoke test, which is every row above.
Component-model async counts too: zwasm supports it as well as wasmtime,
so it is not wasmtime-only. Each benchmark stresses the feature's hot
path. Wherever a featureless formulation exists there is a **twin** with
the same work, so the feature's own cost can be read off per runtime
(see [Feature benchmarks against their twins](#feature-benchmarks-against-their-twins)):

| case | feature | what it does | twin |
|---|---|---|---|
| `tailcall_fsm` | tail calls | 8-state machine over 64 KiB; every transition a `return_call` chosen by `br_table` (65 536 per call) | — |
| `eh_parser_exnref` / `eh_parser_legacy` | exceptions | recursive-descent parser over 4096 statements, ~25 % malformed. Throws from up to several frames deep and catches per statement. The same program in both encodings (the tag carries no payload; WAMR's patch 0014 traps on a payload crossing a function boundary) | each other |
| `gc_trees` | GC | binary trees of depth 4-10 (~130K `struct.new` per call) with a long-lived depth-12 tree in a global | — |
| `callref_dispatch` | typed function references | 200K `call_ref` through a typed `(ref null $ft)` table | `.indirect`: `call_indirect` through a funcref table |
| `relaxed_dot`, `relaxed_madd` | relaxed SIMD | 64×64×256 int8 dot product (`i32x4.relaxed_dot_i8x16_i7x16_add_s`), degree-8 Horner over 16K points (`f32x4.relaxed_madd`, `i32x4.relaxed_trunc_f32x4_s`) | `matmul_fma` vs `matmul_simd` |
| `mem64_chase` | memory64 | 256K dependent loads over a 64 MiB `i64` memory (LCG permutation) | `.mem32`: same layout, 32-bit memory |
| `multimem_transform` | multi-memory | table lookup from `$lut`, reading `$src`, writing `$dst` | `.single`: one memory at offsets |
| `extconst_init` | extended-const | instantiate a module with 2560 globals and 256 data segments whose initializers are add/sub/mul trees (timed per instantiation) | `.mvp`: the same constants pre-folded |
| `cm_async_bench` | WASIp3 async | `wait-for(0)` × 20 000, 1000 concurrent waits in a waitable set, 8192 × 256 B through a `stream<u8>` to stdout | — |

`wasm-tools print` confirms each feature's opcodes are present in the
built module (the `FEATURE_OPS` checks in `scripts/build-workloads.sh`).
The expected results are re-derived independently in Python
(`workloads-wat/reference.py`) and match the cross-runtime consensus.


## Workload coverage

Every case ran on every runtime, or the row says why not. ✓ means all N
reps passed the consensus check. "M4: …; iPhone: …" marks the one row
where the platforms differ. The reasons match the feature matrix:
- the SIMD-canonical builds need SIMD-128 (wasm3 has none, zwasm only
  in its JIT);
- the feature benchmarks need their feature;
- sqlite3 needs the WASI preview 1 + `bench.*` host imports, which only
  the Pulley adapter implements;
- the rest are the runtime bugs listed in
  [Bugs and limits found](#bugs-and-limits-found).

<!-- T:coverage -->
| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| fib | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| fib_tail | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| factorial | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| sieve | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | N/A: v128 ops crash the process | ✓ |
| crc32 | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | N/A: v128 ops crash the process | ✓ |
| matmul_simd | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| matmul_fma | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| convolution | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| audio_dsp | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| bulk_memory | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| call_indirect | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| factorial.scalar | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| sieve.scalar | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| crc32.scalar | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| convolution.scalar | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| bulk_memory.scalar | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| xmrsplayer | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| vtable_mono | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| vtable_bi | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| vtable_poly4 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| vtable_poly6 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| graphql_as | ✓ | ✓ | ✓ | ✓ | ✓ | traps `unreachable` | ✓ |
| tailcall_fsm | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| eh_parser_exnref | ✓ | no exnref | no exnref | ✓ | ✓ | ✓ | ✓ |
| eh_parser_legacy | no legacy EH | ✓ | no legacy EH | no legacy EH | no legacy EH | ✓ | no legacy EH |
| gc_trees | ✓ | no GC | no GC | ✓ | N/A: GC heap growth crashes the process | ✓ | ✓ |
| callref_dispatch | ✓ | no typed func refs | no typed func refs | ✓ | ✓ | traps: typed elem segment not applied | ✓ |
| callref_dispatch.indirect | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| relaxed_dot | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| relaxed_madd | ✓ | ✓ | no SIMD | ✓ | no SIMD in interpreter | ✓ | ✓ |
| mem64_chase | ✓ | no memory64 | no memory64 | ✓ | ✓ | ✓ | ✓ |
| mem64_chase.mem32 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| multimem_transform | ✓ | no multi-memory | no multi-memory | ✓ | ✓ | no multi-memory | ✓ |
| multimem_transform.single | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| extconst_init | ✓ | ✓ | ✓ | ✓ | ✓ | wrong result | ✓ |
| extconst_init.mvp | ✓ | ✓ | ✓ | ✓ | M4: ✓ 9/10; iPhone: ✓ | wrong result | ✓ |
| graphql_porf | ✓ | ✓ | no exceptions | ✓ | M4: ✓; iPhone: killed by jetsam | N/A: C API has no multi-value | ✓ |
| graphql_porf_trycatch | no legacy try/catch | ✓ | no legacy try/catch | no legacy try/catch | no legacy try/catch | N/A: C API has no multi-value | no legacy try/catch |
| sqlite3 | ✓ | N/A: no WASI p1 shim in harness | N/A: no WASI p1 shim in harness | N/A: no WASI p1 shim in harness | N/A: no WASI p1 shim in harness | N/A: no WASI p1 shim in harness | N/A: no WASI p1 shim in harness |
<!-- /T:coverage -->

`workloads/graphql-validation.wasm`, an early AssemblyScript build from
the initial commit, is not referenced by any harness code; its
successor `graphql-validation-as.wasm` is the `graphql_as` row. The
Porffor try/catch variant (`graphql_porf_trycatch`) was compiled into
the library but missing from the case table and the app. It was added
during this refresh and measured in supplementary N=10 runs on both
platforms.

## M4 Max E-cores

`taskpolicy -b`, N=10. Every row's timed window ran ≥ 99.6 % on E-cores.
First, each runtime's CPU time relative to the fastest runtime on that
case, with the geometric mean over the 17 cases all seven runtimes run
(factorial excluded: its ~1 µs calls measure call overhead):

<!-- T:m4-relative -->
**M4 relative CPU time** — CPU time per call ÷ the fastest runtime's (1.00 = fastest)

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | 1.13 | 1.38 | **1.00** | 6.80 | 8.14 | 1.08 | 2.43 |
| fib_tail | **1.00** | 1.68 | 1.05 | 10.98 | 9.15 | 1.10 | 2.70 |
| factorial | 1.00 | **1.00** | — | 9.42 | — | 3.22 | 1.50 |
| sieve | 1.38 | **1.00** | — | 7.21 | — | — | 2.39 |
| crc32 | 1.18 | **1.00** | — | 7.42 | — | — | 2.34 |
| matmul_simd | 1.18 | **1.00** | — | 6.75 | — | 1.64 | 2.54 |
| matmul_fma | 1.23 | **1.00** | — | 7.03 | — | 1.58 | 2.82 |
| convolution | 1.50 | **1.00** | — | 6.29 | — | 1.88 | 2.70 |
| audio_dsp | 2.35 | 1.82 | **1.00** | 10.40 | 9.33 | 1.75 | 4.16 |
| bulk_memory | 2.61 | 1.23 | — | 7.78 | — | **1.00** | 2.14 |
| call_indirect | 2.70 | 1.58 | **1.00** | 7.27 | 8.07 | 2.07 | 3.47 |
| factorial.scalar | 1.81 | 1.23 | **1.00** | 11.15 | 8.78 | 3.39 | 1.80 |
| sieve.scalar | 2.85 | 2.12 | **1.00** | 15.05 | 13.32 | 2.07 | 5.06 |
| crc32.scalar | 1.93 | 1.75 | **1.00** | 13.11 | 10.85 | 1.93 | 4.41 |
| convolution.scalar | 1.97 | 1.90 | **1.00** | 9.71 | 8.91 | 1.85 | 4.26 |
| bulk_memory.scalar | 3.45 | 1.69 | **1.00** | 10.88 | 9.94 | 1.28 | 3.13 |
| xmrsplayer | 2.26 | 1.72 | **1.00** | 10.52 | 9.91 | 1.43 | 3.42 |
| vtable_mono | 2.73 | 1.42 | **1.00** | 6.43 | 8.04 | 1.87 | 3.21 |
| vtable_bi | 2.47 | 1.47 | **1.00** | 7.25 | 8.28 | 1.86 | 3.29 |
| vtable_poly4 | 2.55 | 1.61 | **1.00** | 7.37 | 8.83 | 1.88 | 3.35 |
| vtable_poly6 | 2.50 | 1.55 | **1.00** | 7.17 | 8.17 | 1.80 | 3.38 |
| graphql_as | 1.76 | 1.41 | **1.00** | 8.61 | 7.91 | — | 3.24 |
| tailcall_fsm | 1.11 | 2.07 | 1.04 | 7.45 | 9.46 | **1.00** | 1.99 |
| eh_parser_exnref | 1.46 | — | — | 4.35 | 5.64 | **1.00** | 2.31 |
| eh_parser_legacy | — | 1.00 | — | — | — | **1.00** | — |
| gc_trees | 3.44 | — | — | 4.12 | — | **1.00** | 4.89 |
| callref_dispatch | **1.00** | — | — | 3.18 | 3.58 | — | 1.32 |
| callref_dispatch.indirect | 2.21 | 1.56 | **1.00** | 7.17 | 7.68 | 1.95 | 3.08 |
| relaxed_dot | 2.77 | **1.00** | — | 2.71 | — | 1.25 | 1.98 |
| relaxed_madd | **1.00** | 1.11 | — | 7.81 | — | 2.06 | 3.73 |
| mem64_chase | 1.39 | — | — | 2.60 | 2.35 | **1.00** | 1.48 |
| mem64_chase.mem32 | 1.06 | 1.07 | 1.01 | 2.12 | 1.95 | **1.00** | 1.18 |
| multimem_transform | **1.00** | — | — | 5.49 | 4.34 | — | 1.81 |
| multimem_transform.single | 4.08 | 1.58 | **1.00** | 15.42 | 12.37 | 1.88 | 4.67 |
| extconst_init | **1.00** | 4.42 | 122.20 | 54.01 | 14.80 | — | 6.29 |
| extconst_init.mvp | **1.00** | 1.38 | 115.68 | 85.75 | 12.36 | — | 2.52 |
| graphql_porf | 1.27 | **1.00** | — | 4.66 | 4.83 | — | 1.87 |
| graphql_porf_trycatch | — | **1.00** | — | — | — | — | — |
| sqlite3 | **1.00** | — | — | — | — | — | — |
| **geomean, 17 cases all ran (not factorial)** | **2.09** | **1.63** | **1.01** | **8.42** | **8.47** | **1.59** | **3.20** |
<!-- /T:m4-relative -->

- **wasm3 is the fastest interpreter** on nearly every case it can
  run: MVP and scalar code, calls, `call_indirect` dispatch and the real
  workloads (xmrsplayer, graphql AS, audio DSP). Instantiation is its
  weak spot: the extended-const rows cost it 116-122× Pulley's time. It
  cannot run SIMD, exceptions, GC, typed references, memory64 or
  multi-memory.
- **WAMR fast-interp** leads the SIMD-canonical builds, relaxed int8 dot
  products and Porffor.
- **Pulley** leads where Cranelift's lowering pays off: typed `call_ref`,
  multi-memory, `return_call` chains, relaxed FMA and extended-const
  instantiation. It is also the only runtime with a WASI preview 1
  harness for sqlite3. On dispatch-heavy scalar code it runs 2-4× wasm3.
- **wasmz** is close to WAMR overall and fastest on memory64, the EH
  parser and GC. The GC win comes from never collecting (see
  [Per-case memory](#per-case-memory-m4)). It fails 6 rows on bugs: 2
  wrong results, 2 traps and 2 crashes.
- **tinywasm** is ~3× the fastest, and runs every Wasm 3.0 feature but
  legacy EH.
- **WasmEdge** (the incumbent) and **zwasm** are 6-15× the fastest on
  MVP code.

CPU time per call (ms, median [min–max] over the 10 reps) and wall time
per call:

<!-- T:m4 -->
**CPU time per call, ms (cpu_user / iterations)**

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | 92.13 [69.33–96.43] | 113 [70.6–121.5] | 81.74 [74.99–90.62] | 555.4 [512.7–635] | 665.2 [575.8–706] | 88.41 [51.54–102.7] | 198.9 [145.8–225.6] |
| fib_tail | 0.4699 [0.3556–0.5555] | 0.7889 [0.3924–0.8564] | 0.4928 [0.4623–0.5581] | 5.158 [4.714–5.777] | 4.299 [3.611–4.511] | 0.5179 [0.2752–0.5942] | 1.269 [0.7451–1.421] |
| factorial | 0.0005064 [0.0003818–0.0007371] | 0.0005047 [0.0002689–0.0006178] | E1 | 0.004753 [0.003958–0.00536] | E2 | 0.001624 [0.0008337–0.001777] | 0.0007552 [0.0004512–0.001148] |
| sieve | 0.7082 [0.5374–0.7758] | 0.5128 [0.2849–0.5822] | E3 | 3.698 [3.498–3.916] | E2 | N4 | 1.224 [0.8157–1.372] |
| crc32 | 4.535 [3.443–4.938] | 3.842 [1.989–4.259] | E1 | 28.5 [26.71–32.07] | E2 | N4 | 8.998 [5.624–10.78] |
| matmul_simd | 3.158 [2.507–3.742] | 2.679 [1.649–3.042] | E5 | 18.09 [16.97–20.33] | E2 | 4.395 [2.4–5.165] | 6.816 [5.11–7.947] |
| matmul_fma | 2.4 [1.831–2.709] | 1.943 [1.716–2.198] | E5 | 13.66 [11.68–14.7] | E2 | 3.068 [1.746–3.578] | 5.484 [3.776–6.047] |
| convolution | 5.883 [4.627–6.621] | 3.928 [3.086–4.737] | E3 | 24.7 [20.08–27.75] | E2 | 7.373 [4.958–9.117] | 10.59 [8.384–11.69] |
| audio_dsp | 1169 [913.9–1323] | 904.5 [744.2–1064] | 496.9 [430.2–589.6] | 5166 [4918–5724] | 4634 [3922–5121] | 871.7 [584.5–1049] | 2067 [1667–2298] |
| bulk_memory | 22.14 [19.54–25.84] | 10.41 [9.315–11.12] | E6 | 66.1 [54.31–73.29] | E2 | 8.493 [5.273–10.15] | 18.19 [14.4–20.17] |
| call_indirect | 31.53 [25.21–34.46] | 18.48 [16.5–21.63] | 11.67 [10.38–12.74] | 84.91 [69.04–92.4] | 94.17 [71.75–100.8] | 24.2 [13.27–26.34] | 40.5 [31.87–44.66] |
| factorial.scalar | 0.0007597 [0.0003667–0.0008594] | 0.000516 [0.0002961–0.0006893] | 0.0004186 [0.0001646–0.0004272] | 0.004667 [0.002721–0.005996] | 0.003677 [0.00267–0.004114] | 0.00142 [0.0007312–0.001652] | 0.0007537 [0.0005219–0.0008584] |
| sieve.scalar | 0.7273 [0.6047–0.8118] | 0.5425 [0.4641–0.6985] | 0.2555 [0.238–0.2867] | 3.845 [3.259–4.204] | 3.404 [2.67–3.693] | 0.5289 [0.2691–0.5581] | 1.293 [0.8829–1.44] |
| crc32.scalar | 4.226 [2.86–5.05] | 3.823 [2.34–4.845] | 2.187 [2.058–2.395] | 28.66 [23.21–32.36] | 23.74 [18.86–25.77] | 4.22 [2.247–4.592] | 9.648 [4.969–10.94] |
| convolution.scalar | 11.99 [6.459–13.22] | 11.6 [10.18–13.29] | 6.095 [5.536–6.796] | 59.16 [53.37–66.32] | 54.29 [44.11–59.12] | 11.28 [7.012–12.13] | 25.94 [19.79–29.08] |
| bulk_memory.scalar | 21.97 [12.37–25.2] | 10.76 [9.706–11.68] | 6.368 [5.769–7.316] | 69.27 [68.48–74.3] | 63.3 [54.91–72.7] | 8.156 [4.686–8.856] | 19.96 [13.63–21.06] |
| xmrsplayer | 15.7 [8.586–18.49] | 11.94 [9.79–12.56] | 6.938 [6.313–7.652] | 73.02 [69.21–80.87] | 68.73 [54.73–71.72] | 9.949 [5.401–10.68] | 23.74 [18.61–24.73] |
| vtable_mono | 52.29 [31.49–63.08] | 27.17 [23.59–30.84] | 19.13 [17.1–21.85] | 123 [96.27–141.7] | 153.8 [127.4–167] | 35.8 [26.61–40.31] | 61.32 [48.79–64.72] |
| vtable_bi | 54.67 [39.21–68.12] | 32.53 [27.46–36.22] | 22.17 [20.26–23.25] | 160.7 [115.9–180.1] | 183.6 [144.7–195.9] | 41.27 [32.39–44.32] | 73.02 [58.43–81.05] |
| vtable_poly4 | 56.81 [33.7–63.67] | 35.82 [29.2–41.02] | 22.28 [20.62–23.86] | 164.3 [127–185.9] | 196.7 [156.2–202] | 41.92 [33.45–46.84] | 74.71 [60.23–83.87] |
| vtable_poly6 | 63.51 [44.12–71.62] | 39.35 [26.58–44.1] | 25.44 [22.76–28.11] | 182.5 [158.7–213.1] | 207.9 [161.1–229.1] | 45.76 [37.62–51.77] | 86.01 [69.33–95.91] |
| graphql_as | 9.808 [7.581–11.62] | 7.88 [4.928–9.217] | 5.58 [5.047–6.433] | 48.02 [38.52–53.04] | 44.13 [37.17–48.41] | E7 | 18.1 [14.78–19.85] |
| tailcall_fsm | 4.809 [3.671–5.398] | 8.923 [6.954–10.48] | 4.491 [4.151–5.044] | 32.13 [27.17–35.31] | 40.8 [31.09–44.64] | 4.314 [3.467–4.941] | 8.606 [6.69–10.48] |
| eh_parser_exnref | 17.38 [13.29–19.27] | E8 | E9 | 51.66 [30.56–61.24] | 66.96 [55.61–69.8] | 11.87 [9.268–13.25] | 27.42 [20.88–31.77] |
| eh_parser_legacy | E10 | 12.34 [9.638–14.8] | E9 | E11 | E12 | 12.33 [9.934–13.02] | E13 |
| gc_trees | 131.1 [98.09–140.1] | E14 | E15 | 157.1 [120.4–176.1] | N16 | 38.16 [31.46–42.93] | 186.6 [143.3–204.7] |
| callref_dispatch | 28.02 [21.09–31.27] | E17 | E18 | 89.05 [60.57–102.6] | 100.2 [82.38–106.5] | E19 | 36.99 [30.7–42.37] |
| callref_dispatch.indirect | 26.31 [20.22–30.16] | 18.64 [12.04–21.6] | 11.93 [11.09–13.69] | 85.53 [57.67–98.53] | 91.55 [76.8–105.2] | 23.28 [17.3–24.79] | 36.75 [22.15–44.07] |
| relaxed_dot | 6.143 [4.68–6.957] | 2.215 [1.874–2.543] | E5 | 6.009 [4.518–6.83] | E2 | 2.766 [2.1–3.094] | 4.395 [3.389–5.167] |
| relaxed_madd | 0.3339 [0.2344–0.3591] | 0.37 [0.3235–0.4384] | E5 | 2.609 [2.427–3.002] | E2 | 0.6866 [0.555–0.7721] | 1.246 [0.932–1.422] |
| mem64_chase | 78.04 [41.31–89.79] | E20 | E15 | 145.8 [116.1–167.7] | 131.7 [78.22–147.1] | 56.08 [33.17–77.47] | 82.84 [43.19–96.84] |
| mem64_chase.mem32 | 69.87 [38.28–82.02] | 70.16 [33.66–78.62] | 66.47 [33.22–76.03] | 139.4 [92.79–153.7] | 128.1 [75.44–143.5] | 65.7 [33.66–77.46] | 77.25 [42.85–92.37] |
| multimem_transform | 6.504 [4.935–7.819] | E21 | E22 | 35.72 [22.88–41.71] | 28.2 [21.71–33.02] | E23 | 11.75 [9.152–12.82] |
| multimem_transform.single | 9.705 [6.897–10.35] | 3.769 [3.169–4.395] | 2.38 [2.244–2.723] | 36.7 [25.99–42.02] | 29.44 [25.69–32.8] | 4.473 [3.561–4.872] | 11.12 [8.285–13.41] |
| extconst_init | 0.1127 [0.083–0.1237] | 0.4976 [0.4249–0.5772] | 13.77 [10.16–14.95] | 6.086 [4.733–7.24] | 1.667 [1.586–1.864] | E24 | 0.7087 [0.5319–0.8013] |
| extconst_init.mvp | 0.06286 [0.05016–0.07096] | 0.08656 [0.07232–0.1007] | 7.271 [4.914–7.696] | 5.39 [4.519–6.275] | 0.7767 [0.7271–0.861] (9/10 ok; E25) | E26 | 0.1585 [0.1226–0.1787] |
| graphql_porf | 9.502 [7.529–10.85] | 7.482 [5.817–8.345] | E9 | 34.84 [31.59–39.58] | 36.12 [32.14–41.26] | E27 | 13.97 [10.95–15.47] |
| graphql_porf_trycatch | E28 | 6.99 [6.776–7.156] | E9 | E29 | E12 | E27 | E30 |
| sqlite3 | 4.468e+04 [3.206e+04–4.942e+04] | N31 | N32 | N33 | N34 | N35 | N36 |

- E1 (wasm3; factorial, crc32): wasm3 m3_FindFunction failed: compiling function underran the stack
- E2 (zwasm; factorial, sieve, crc32, matmul_simd, matmul_fma, convolution, bulk_memory, relaxed_dot, relaxed_madd): init warmup failed: call trapped: trap: unreachable
- E3 (wasm3; sieve, convolution): wasm3 m3_FindFunction failed: incorrect type on stack
- N4 (wasmz; sieve, crc32): N/A: wasmz v0.1.4 segfaults on this module's v128 ops (the scalar build runs)
- E5 (wasm3; matmul_simd, matmul_fma, relaxed_dot, relaxed_madd): wasm3 m3_FindFunction failed: unknown type
- E6 (wasm3; bulk_memory): wasm3 m3_FindFunction failed: unknown label
- E7 (wasmz; graphql_as): wasmz init-warmup failed: wasmz_instance_call trap: trap: wasm `unreachable` instruction executed
- E8 (wamr; eh_parser_exnref): wasm_runtime_load failed: WASM module load failed: unsupported opcode 1f
- E9 (wasm3; eh_parser_exnref, eh_parser_legacy, graphql_porf, graphql_porf_trycatch): wasm3 m3_ParseModule failed: out of order Wasm section
- E10 (pulley; eh_parser_legacy): Module::from_binary failed — invalid wasm or unsupported feature?: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E11 (wasmedge; eh_parser_legacy): WasmEdge VMLoadWasmFromBytes: illegal opcode loading failed: illegal opcode, Code: 0x117
- E12 (zwasm; eh_parser_legacy, graphql_porf_trycatch): wasm_module_new failed (invalid wasm or unsupported feature)
- E13 (tinywasm; eh_parser_legacy): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0x171) at offset 369
- E14 (wamr; gc_trees): wasm_runtime_load failed: WASM module load failed: invalid type flag
- E15 (wasm3; gc_trees, mem64_chase): wasm3 m3_ParseModule failed: malformed Wasm binary
- N16 (zwasm; gc_trees): N/A: zwasm v2.7.0 never reclaims GC structs and grows its GC heap far faster than the allocation rate (124 MB for 31K 24-byte structs); the process segfaults at ~2.3 GB
- E17 (wamr; callref_dispatch): wasm_runtime_load failed: WASM module load failed: incompatible import type
- E18 (wasm3; callref_dispatch): wasm3 m3_ParseModule failed: unknown value_type
- E19 (wasmz; callref_dispatch): wasmz init-warmup failed: wasmz_instance_call trap: trap: null reference dereference
- E20 (wamr; mem64_chase): wasm_runtime_load failed: WASM module load failed: invalid limits flags
- E21 (wamr; multimem_transform): wasm_runtime_load failed: WASM module load failed: multiple memories
- E22 (wasm3; multimem_transform): wasm3 m3_ParseModule failed: only one memory per module is supported
- E23 (wasmz; multimem_transform): wasmz_module_new failed: module compilation failed: MultipleMemories
- E24 (wasmz; extconst_init): wrong result: got -1669113347, expected -1152068691 (cross-runtime consensus)
- E25 (zwasm; extconst_init.mvp): zwasm_instance_new_ex failed (missing imports or unsupported feature)
- E26 (wasmz; extconst_init.mvp): wrong result: got 1844351884, expected -1152068691 (cross-runtime consensus)
- E27 (wasmz; graphql_porf, graphql_porf_trycatch): init warmup failed: wasmz m() trap: multi-value results are not supported by the C API (results_len = 2)
- E28 (pulley; graphql_porf_trycatch): graphql-validation-porf: Module::from_binary failed: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E29 (wasmedge; graphql_porf_trycatch): WasmEdge LoaderParseFromBytes: illegal opcode loading failed: illegal opcode, Code: 0x117
- E30 (tinywasm; graphql_porf_trycatch): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0xfa34) at offset 64052
- N31 (wamr; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N32 (wasm3; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N33 (wasmedge; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N34 (zwasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N35 (wasmz; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N36 (tinywasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley

**Wall time per call, ms: median [range] over reps**

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | 104.9 [76.45–110.6] | 130 [108.5–143] | 95.6 [84.71–105.3] | 623.3 [541.6–691.8] | 754.9 [596.5–851.5] | 98.9 [55.48–116.9] | 224.4 [171.5–240.2] |
| fib_tail | 0.6223 [0.3055–0.6347] | 0.9948 [0.487–1.001] | 0.6396 [0.5137–0.6412] | 6.035 [5.422–6.826] | 4.865 [3.58–5.474] | 0.6837 [0.3098–0.6898] | 1.612 [0.6837–1.629] |
| factorial | 0.000437 [0.000291–0.000666] | 0.000437 [0.000209–0.000583] | E1 | 0.007791 [0.004416–0.007833] | E2 | 0.008292 [0.003708–0.008666] | 0.0007705 [0.000417–0.001042] |
| sieve | 0.9083 [0.4773–0.9143] | 0.6597 [0.3162–0.6674] | E3 | 4.326 [3.939–4.696] | E2 | N4 | 1.561 [0.7618–1.576] |
| crc32 | 5.193 [3.515–5.726] | 4.495 [3.111–4.995] | E1 | 32.71 [29.19–35.93] | E2 | N4 | 10.54 [6.159–12] |
| matmul_simd | 3.623 [2.514–4.193] | 3.309 [2.272–3.448] | E5 | 20.99 [18.39–22.34] | E2 | 5.011 [2.687–5.934] | 7.879 [5.183–9.1] |
| matmul_fma | 2.873 [1.793–3.116] | 2.328 [1.797–2.563] | E5 | 16.02 [13.7–17.17] | E2 | 3.518 [1.824–4.065] | 6.152 [3.723–6.983] |
| convolution | 6.921 [4.698–7.609] | 4.43 [2.709–5.575] | E3 | 28.12 [24.13–29.69] | E2 | 8.568 [4.877–10.36] | 12.13 [9.156–12.96] |
| audio_dsp | 1338 [1065–1425] | 1043 [854.8–1260] | 550 [470.3–637.7] | 5910 [5482–6323] | 5265 [4221–5618] | 968.2 [680.2–1142] | 2363 [1848–2600] |
| bulk_memory | 26.58 [22.94–28.11] | 11.77 [10.81–13.56] | E6 | 74.6 [65.43–88.98] | E2 | 9.787 [5.353–11.31] | 20.88 [16.12–22.68] |
| call_indirect | 35.34 [28.93–38.11] | 20.9 [18.16–24.57] | 13.54 [11.87–14.77] | 95.41 [84.03–103.6] | 103.8 [77.01–112.1] | 28.07 [15.27–30.33] | 46.07 [36.51–49.74] |
| factorial.scalar | 0.000833 [0.000333–0.000834] | 0.000583 [0.00025–0.000584] | 0.000333 [0.000125–0.000334] | 0.005104 [0.0035–0.008917] | 0.00425 [0.00225–0.004291] | 0.007895 [0.0035–0.0085] | 0.00075 [0.000375–0.000791] |
| sieve.scalar | 0.9424 [0.5605–0.945] | 0.7032 [0.402–0.8358] | 0.3277 [0.2486–0.3284] | 4.424 [3.433–4.931] | 3.891 [2.787–4.338] | 0.6713 [0.3023–0.6755] | 1.614 [0.8725–1.652] |
| crc32.scalar | 4.789 [3.128–5.852] | 4.455 [3.255–5.32] | 2.677 [2.285–2.847] | 33.27 [29.55–35.17] | 26.99 [20.29–29.93] | 4.914 [2.51–5.315] | 10.95 [5.98–12.5] |
| convolution.scalar | 13.63 [7.271–14.7] | 13.29 [11.97–15.28] | 7.024 [6.264–7.783] | 66.63 [62.25–71.17] | 61.08 [46.57–65.72] | 12.72 [7.086–14.45] | 30.61 [24.16–31.99] |
| bulk_memory.scalar | 25.43 [14–28.58] | 12.3 [11.34–12.98] | 7.334 [6.141–8.33] | 79.47 [75.25–84.12] | 71.33 [56.52–79.91] | 9.328 [4.963–10.01] | 22.71 [17.42–24.35] |
| xmrsplayer | 18.14 [10.03–20.25] | 13.61 [11.03–14.3] | 7.834 [7.2–8.974] | 82.7 [73.15–89.14] | 78.6 [61.11–80.24] | 11.3 [6.263–12.25] | 26.68 [23.34–28.46] |
| vtable_mono | 61.83 [34.83–68.49] | 30.52 [24.57–35.28] | 21.98 [18.61–24.67] | 142.1 [109.3–157.6] | 170.5 [133.9–183.8] | 41.24 [30.74–44.58] | 69.15 [53.55–73.8] |
| vtable_bi | 62.14 [54.17–72.02] | 37.23 [30.2–40.44] | 24.89 [21.82–27.37] | 183 [153.4–191.8] | 205.2 [154.6–217.8] | 46.01 [36.7–50.52] | 82.29 [72.45–87.75] |
| vtable_poly4 | 64.52 [48.31–72.83] | 40.39 [33.47–47.37] | 25.91 [22.21–26.59] | 194.3 [173.8–223.5] | 218.7 [163.5–239.3] | 46.89 [37.07–53.1] | 83.48 [73.29–93.36] |
| vtable_poly6 | 71.92 [49.34–79.54] | 44.59 [25.3–50.22] | 29.46 [25.39–31.82] | 210.3 [185.8–226.6] | 241 [171.3–254.8] | 52.77 [41.55–56.97] | 93.85 [86.21–106] |
| graphql_as | 11.17 [8.329–12.65] | 8.561 [4.653–9.452] | 6.425 [5.483–7.467] | 53.1 [51.09–60.33] | 48.66 [39.64–57.87] | E7 | 20.23 [16.82–28.33] |
| tailcall_fsm | 5.472 [3.831–6.064] | 10.44 [6.103–12] | 5.21 [4.705–5.867] | 37.46 [33.47–41.28] | 46.63 [35.36–52.81] | 4.956 [3.699–5.766] | 9.653 [7.034–11.88] |
| eh_parser_exnref | 20.18 [14.74–21.46] | E8 | E9 | 58.35 [32.35–66.5] | 73.21 [59.3–81.42] | 13.34 [10.41–15.18] | 30.31 [25.37–34.78] |
| eh_parser_legacy | E10 | 14.31 [9.877–16.6] | E9 | E11 | E12 | 14.25 [11.38–14.8] | E13 |
| gc_trees | 145.1 [111.4–161.7] | E14 | E15 | 187 [154.8–201.3] | N16 | 45.51 [35.73–49.12] | 204.1 [159.1–221.7] |
| callref_dispatch | 32.45 [23.51–35.21] | E17 | E18 | 101.6 [60.19–110.6] | 109.1 [87.57–121] | E19 | 42.68 [33.62–50.23] |
| callref_dispatch.indirect | 30.46 [22.28–33.42] | 21.7 [11.37–23.81] | 14.13 [12.18–16.04] | 94.85 [57.57–107.8] | 105.4 [82.63–116.9] | 26.75 [19.4–29.13] | 42.17 [20.35–47.78] |
| relaxed_dot | 7.195 [5.085–8.16] | 2.61 [1.825–2.958] | E5 | 6.907 [4.115–7.947] | E2 | 3.189 [1.984–3.656] | 5.137 [3.435–5.904] |
| relaxed_madd | 0.413 [0.184–0.4137] | 0.4901 [0.343–0.494] | E5 | 3.052 [2.695–3.499] | E2 | 0.907 [0.5412–0.9205] | 1.584 [0.8792–1.607] |
| mem64_chase | 88.82 [46.89–113] | E20 | E15 | 170.8 [133.8–185.2] | 146 [84.67–173.2] | 59.1 [36.25–91.18] | 90.63 [48.07–113.7] |
| mem64_chase.mem32 | 75.06 [39.95–105.4] | 77.72 [36.81–90.82] | 79.04 [34.9–88.38] | 161.4 [107.4–184.2] | 139.8 [80.9–159.9] | 75.86 [36.77–86.83] | 94.99 [46.82–113.1] |
| multimem_transform | 7.351 [5.335–8.76] | E21 | E22 | 40.45 [25.1–50.01] | 32.41 [22.87–36.72] | E23 | 13.57 [10.15–16.47] |
| multimem_transform.single | 10.68 [7.695–12.47] | 4.354 [3.344–5.042] | 2.813 [2.339–3.151] | 41.89 [30.89–46.79] | 32.96 [28.38–36.95] | 5.315 [3.785–5.648] | 13.14 [9.189–15.34] |
| extconst_init | 0.1438 [0.06646–0.1446] | 0.6477 [0.4032–0.6542] | 15.35 [11.25–17.55] | 12.07 [9.172–14.28] | 2.357 [2.176–2.523] | E24 | 0.864 [0.5003–0.9674] |
| extconst_init.mvp | 0.08825 [0.04854–0.0895] | 0.1176 [0.06683–0.1185] | 7.898 [5.631–8.498] | 11.59 [9.422–13.41] | 1.115 [1.076–1.248] (9/10 ok; E25) | E26 | 0.1891 [0.108–0.1918] |
| graphql_porf | 12.89 [9.337–14.47] | 11.02 [7.746–11.88] | E9 | 42.42 [39.04–45.86] | 43.96 [37.78–51.15] | E27 | 18.67 [13.66–25.85] |
| graphql_porf_trycatch | E28 | 10.25 [9.511–10.58] | E9 | E29 | E12 | E27 | E30 |
| sqlite3 | 5.026e+04 [3.999e+04–6.6e+04] | N31 | N32 | N33 | N34 | N35 | N36 |

- E1 (wasm3; factorial, crc32): wasm3 m3_FindFunction failed: compiling function underran the stack
- E2 (zwasm; factorial, sieve, crc32, matmul_simd, matmul_fma, convolution, bulk_memory, relaxed_dot, relaxed_madd): init warmup failed: call trapped: trap: unreachable
- E3 (wasm3; sieve, convolution): wasm3 m3_FindFunction failed: incorrect type on stack
- N4 (wasmz; sieve, crc32): N/A: wasmz v0.1.4 segfaults on this module's v128 ops (the scalar build runs)
- E5 (wasm3; matmul_simd, matmul_fma, relaxed_dot, relaxed_madd): wasm3 m3_FindFunction failed: unknown type
- E6 (wasm3; bulk_memory): wasm3 m3_FindFunction failed: unknown label
- E7 (wasmz; graphql_as): wasmz init-warmup failed: wasmz_instance_call trap: trap: wasm `unreachable` instruction executed
- E8 (wamr; eh_parser_exnref): wasm_runtime_load failed: WASM module load failed: unsupported opcode 1f
- E9 (wasm3; eh_parser_exnref, eh_parser_legacy, graphql_porf, graphql_porf_trycatch): wasm3 m3_ParseModule failed: out of order Wasm section
- E10 (pulley; eh_parser_legacy): Module::from_binary failed — invalid wasm or unsupported feature?: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E11 (wasmedge; eh_parser_legacy): WasmEdge VMLoadWasmFromBytes: illegal opcode loading failed: illegal opcode, Code: 0x117
- E12 (zwasm; eh_parser_legacy, graphql_porf_trycatch): wasm_module_new failed (invalid wasm or unsupported feature)
- E13 (tinywasm; eh_parser_legacy): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0x171) at offset 369
- E14 (wamr; gc_trees): wasm_runtime_load failed: WASM module load failed: invalid type flag
- E15 (wasm3; gc_trees, mem64_chase): wasm3 m3_ParseModule failed: malformed Wasm binary
- N16 (zwasm; gc_trees): N/A: zwasm v2.7.0 never reclaims GC structs and grows its GC heap far faster than the allocation rate (124 MB for 31K 24-byte structs); the process segfaults at ~2.3 GB
- E17 (wamr; callref_dispatch): wasm_runtime_load failed: WASM module load failed: incompatible import type
- E18 (wasm3; callref_dispatch): wasm3 m3_ParseModule failed: unknown value_type
- E19 (wasmz; callref_dispatch): wasmz init-warmup failed: wasmz_instance_call trap: trap: null reference dereference
- E20 (wamr; mem64_chase): wasm_runtime_load failed: WASM module load failed: invalid limits flags
- E21 (wamr; multimem_transform): wasm_runtime_load failed: WASM module load failed: multiple memories
- E22 (wasm3; multimem_transform): wasm3 m3_ParseModule failed: only one memory per module is supported
- E23 (wasmz; multimem_transform): wasmz_module_new failed: module compilation failed: MultipleMemories
- E24 (wasmz; extconst_init): wrong result: got -1669113347, expected -1152068691 (cross-runtime consensus)
- E25 (zwasm; extconst_init.mvp): zwasm_instance_new_ex failed (missing imports or unsupported feature)
- E26 (wasmz; extconst_init.mvp): wrong result: got 1844351884, expected -1152068691 (cross-runtime consensus)
- E27 (wasmz; graphql_porf, graphql_porf_trycatch): init warmup failed: wasmz m() trap: multi-value results are not supported by the C API (results_len = 2)
- E28 (pulley; graphql_porf_trycatch): graphql-validation-porf: Module::from_binary failed: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E29 (wasmedge; graphql_porf_trycatch): WasmEdge LoaderParseFromBytes: illegal opcode loading failed: illegal opcode, Code: 0x117
- E30 (tinywasm; graphql_porf_trycatch): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0xfa34) at offset 64052
- N31 (wamr; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N32 (wasm3; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N33 (wasmedge; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N34 (zwasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N35 (wasmz; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N36 (tinywasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley

**Measured E-core residency and IPC**

| runtime | rows | e_share min | e_share median | IPC median |
|---|---:|---:|---:|---:|
| pulley | 37 | 0.997 | 1.000 | 2.76 |
| wamr | 33 | 0.996 | 1.000 | 2.87 |
| wasm3 | 21 | 1.000 | 1.000 | 3.17 |
| wasmedge | 36 | 0.999 | 1.000 | 3.47 |
| zwasm | 26 | 0.999 | 1.000 | 3.50 |
| wasmz | 29 | 0.996 | 1.000 | 2.97 |
| tinywasm | 36 | 0.998 | 1.000 | 3.47 |
<!-- /T:m4 -->

## iPhone XS Max

A12, `.utility` QoS, one launch per runtime, N=10. Every row ran ≥ 98 %
on E-cores except `factorial(20)`, whose whole window is 10-35 ms of CPU
(the 16 384-iteration cap), where a few ms of P-core time at setup show.
Rep-to-rep ranges are far tighter than on the M4. Per-call wall times in
the device log have 1 µs resolution, so use CPU time for sub-10 µs rows.

<!-- T:iphone-relative -->
**iPhone relative CPU time** — CPU time per call ÷ the fastest runtime's (1.00 = fastest)

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | **1.00** | 1.66 | 1.36 | 8.38 | 10.10 | 1.14 | 2.60 |
| fib_tail | 1.16 | 1.86 | **1.00** | 16.19 | 14.26 | 1.77 | 4.34 |
| factorial | **1.00** | 1.07 | — | 17.77 | — | 14.62 | 1.61 |
| sieve | 1.19 | **1.00** | — | 8.46 | — | — | 2.27 |
| crc32 | 1.23 | **1.00** | — | 9.62 | — | — | 2.94 |
| matmul_simd | 1.13 | **1.00** | — | 7.79 | — | 1.79 | 2.58 |
| matmul_fma | 1.18 | **1.00** | — | 8.43 | — | 1.65 | 2.73 |
| convolution | 1.56 | **1.00** | — | 6.24 | — | 2.04 | 2.71 |
| audio_dsp | 1.97 | 1.26 | **1.00** | 13.50 | 11.04 | 1.63 | 3.78 |
| bulk_memory | 1.99 | **1.00** | — | 8.82 | — | 1.03 | 1.96 |
| call_indirect | 2.02 | 1.35 | **1.00** | 7.94 | 7.90 | 2.24 | 2.91 |
| factorial.scalar | 1.11 | **1.00** | 1.16 | 20.77 | 6.20 | 13.72 | 1.68 |
| sieve.scalar | 2.22 | 1.59 | **1.00** | 15.41 | 12.21 | 1.91 | 4.32 |
| crc32.scalar | 1.79 | 1.45 | **1.00** | 13.89 | 12.56 | 1.88 | 4.29 |
| convolution.scalar | 1.84 | 1.41 | **1.00** | 9.25 | 10.04 | 1.91 | 4.00 |
| bulk_memory.scalar | 2.67 | 1.38 | **1.00** | 11.73 | 11.00 | 1.33 | 2.80 |
| xmrsplayer | 1.48 | 1.33 | **1.00** | 8.56 | 7.22 | 1.10 | 2.66 |
| vtable_mono | 1.73 | 1.13 | **1.00** | 5.03 | 5.99 | 1.92 | 2.35 |
| vtable_bi | 1.82 | 1.17 | **1.00** | 6.75 | 7.25 | 1.98 | 2.65 |
| vtable_poly4 | 1.70 | 1.14 | **1.00** | 6.52 | 6.70 | 1.81 | 2.56 |
| vtable_poly6 | 1.80 | 1.15 | **1.00** | 6.38 | 6.52 | 1.72 | 2.54 |
| graphql_as | 1.46 | 1.11 | **1.00** | 6.64 | 5.96 | — | 2.54 |
| tailcall_fsm | **1.00** | 2.21 | 1.48 | 11.62 | 12.72 | 1.09 | 2.47 |
| eh_parser_exnref | 1.08 | — | — | 4.03 | 5.20 | **1.00** | 1.92 |
| eh_parser_legacy | — | **1.00** | — | — | — | 1.10 | — |
| gc_trees | 2.74 | — | — | 3.86 | — | **1.00** | 3.39 |
| callref_dispatch | **1.00** | — | — | 5.73 | 5.93 | — | 1.88 |
| callref_dispatch.indirect | 1.29 | 1.22 | **1.00** | 7.62 | 7.90 | 2.09 | 2.46 |
| relaxed_dot | 3.44 | **1.00** | — | 2.96 | — | 1.57 | 2.02 |
| relaxed_madd | **1.00** | 1.19 | — | 9.75 | — | 1.79 | 3.82 |
| mem64_chase | 1.27 | — | — | 4.10 | 3.45 | **1.00** | 1.58 |
| mem64_chase.mem32 | 1.15 | 1.06 | **1.00** | 4.06 | 3.72 | 1.09 | 1.54 |
| multimem_transform | **1.00** | — | — | 7.27 | 5.66 | — | 1.83 |
| multimem_transform.single | 3.56 | 1.48 | **1.00** | 18.98 | 14.77 | 1.79 | 4.52 |
| extconst_init | **1.00** | 3.74 | 85.17 | 84.63 | 13.35 | — | 4.71 |
| extconst_init.mvp | **1.00** | 1.20 | 62.27 | 112.71 | 10.94 | — | 1.87 |
| graphql_porf | 1.21 | **1.00** | — | 4.03 | — | — | 1.70 |
| graphql_porf_trycatch | — | **1.00** | — | — | — | — | — |
| sqlite3 | **1.00** | — | — | — | — | — | — |
| **geomean, 17 cases all ran (not factorial)** | **1.68** | **1.38** | **1.04** | **9.28** | **8.98** | **1.63** | **2.98** |
<!-- /T:iphone-relative -->

The ranking matches the M4's. The gaps are narrower, and Pulley moves
closer:

| | Pulley | WAMR | wasmz | tinywasm | WasmEdge | zwasm |
|---|---:|---:|---:|---:|---:|---:|
| geomean vs fastest, A12 | 1.68 | 1.38 | 1.63 | 2.98 | 9.28 | 8.98 |
| geomean vs fastest, M4 E-cores | 2.09 | 1.63 | 1.59 | 3.20 | 8.42 | 8.47 |

For the production question, xmrsplayer (the tracker player closest to
the WASI audio app) costs, in CPU ms per 1024-frame buffer on the A12:

| WasmEdge | zwasm | tinywasm | Pulley | WAMR | wasmz | wasm3 |
|---:|---:|---:|---:|---:|---:|---:|
| 159 | 134 | 49 | 27 | 25 | 20 | 19 |

<!-- T:iphone -->
**CPU time per call, ms (cpu_user / iterations)**

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | 124.9 [124.5–128.4] | 206.8 [206–207.6] | 170.4 [159–178.6] | 1047 [1042–1054] | 1262 [1252–1271] | 142.7 [142.2–143.9] | 324.8 [323.4–326.8] |
| fib_tail | 0.5763 [0.5752–0.5769] | 0.9216 [0.9183–0.9255] | 0.4951 [0.4937–0.497] | 8.016 [7.923–8.078] | 7.062 [7.015–7.119] | 0.8743 [0.8711–0.8825] | 2.148 [2.142–2.159] |
| factorial | 0.001091 [0.0006128–0.001417] | 0.001164 [0.0006573–0.001776] | E1 | 0.01939 [0.01873–0.01991] | E2 | 0.01595 [0.01518–0.01628] | 0.00176 [0.001501–0.002037] |
| sieve | 0.9604 [0.9271–1.014] | 0.8102 [0.7984–0.8135] | E3 | 6.851 [6.799–6.888] | E2 | N4 | 1.842 [1.828–1.859] |
| crc32 | 6.617 [6.589–6.829] | 5.386 [5.273–5.501] | E1 | 51.8 [51.53–52.04] | E2 | N4 | 15.85 [15.72–16.09] |
| matmul_simd | 4.808 [4.793–6.211] | 4.265 [4.237–4.308] | E5 | 33.21 [32.87–33.36] | E2 | 7.649 [7.567–7.714] | 11.02 [10.96–11.05] |
| matmul_fma | 3.715 [3.693–3.754] | 3.161 [3.154–3.182] | E5 | 26.64 [26.45–26.77] | E2 | 5.221 [5.201–5.249] | 8.619 [8.585–8.673] |
| convolution | 10.1 [9.949–10.37] | 6.465 [6.412–6.549] | E3 | 40.32 [40.09–40.47] | E2 | 13.18 [13.13–13.25] | 17.55 [17.49–17.74] |
| audio_dsp | 1699 [1670–1793] | 1089 [1080–1096] | 864.7 [688.9–890.6] | 1.167e+04 [1.164e+04–1.171e+04] | 9548 [9533–9557] | 1410 [1391–1420] | 3268 [3185–3390] |
| bulk_memory | 30.11 [29.49–32.09] | 15.13 [15.03–15.21] | E6 | 133.4 [132.2–134.1] | E2 | 15.58 [15.4–17.18] | 29.59 [29.44–29.67] |
| call_indirect | 41.34 [41.01–41.55] | 27.69 [27.48–27.77] | 20.52 [20.03–20.69] | 163 [162.6–163.7] | 162.1 [160.9–162.6] | 46.05 [45.95–46.49] | 59.61 [59.45–62.18] |
| factorial.scalar | 0.001133 [0.0007422–0.001456] | 0.001017 [0.0008447–0.001559] | 0.00118 [0.000556–0.001353] | 0.02113 [0.02068–0.02142] | 0.006309 [0.006044–0.006388] | 0.01395 [0.0135–0.0143] | 0.001707 [0.001152–0.002138] |
| sieve.scalar | 1.029 [1.023–1.063] | 0.739 [0.7345–0.7435] | 0.4646 [0.4607–0.4667] | 7.158 [7.098–7.251] | 5.673 [5.662–5.689] | 0.8863 [0.8719–0.891] | 2.006 [2.001–2.009] |
| crc32.scalar | 6.641 [6.63–6.664] | 5.383 [5.341–5.456] | 3.719 [3.703–3.748] | 51.64 [51.01–52.28] | 46.72 [46.62–46.81] | 6.978 [6.932–7.003] | 15.96 [15.82–16.02] |
| convolution.scalar | 19.27 [19.2–20.54] | 14.73 [14.67–14.79] | 10.45 [10.41–11.2] | 96.7 [96.29–100.6] | 104.9 [104.5–105.4] | 19.95 [19.82–20.03] | 41.84 [41.76–41.97] |
| bulk_memory.scalar | 30.26 [29.68–32.3] | 15.67 [15.55–15.77] | 11.32 [11.31–11.36] | 132.8 [132.3–133.4] | 124.5 [123.9–125] | 15.08 [14.92–16.78] | 31.73 [31.62–31.86] |
| xmrsplayer | 27.4 [26.79–30.68] | 24.75 [24.44–25.2] | 18.57 [18.31–19.16] | 159 [157.8–160.2] | 134 [133.1–135] | 20.4 [19.86–20.87] | 49.35 [47.92–50.08] |
| vtable_mono | 76.05 [71.82–78.6] | 49.77 [49.49–53.36] | 43.95 [43.68–44.4] | 221.1 [219.7–222.5] | 263.4 [261.5–264.9] | 84.21 [83.75–84.43] | 103.1 [102.2–113.9] |
| vtable_bi | 83.15 [79.11–89.65] | 53.54 [53.4–53.81] | 45.72 [45.26–47.12] | 308.5 [307.3–310.2] | 331.4 [330.4–338.7] | 90.34 [89.81–90.94] | 121.3 [120.5–122.7] |
| vtable_poly4 | 86.9 [81.33–93.32] | 58.19 [57.47–58.69] | 51.26 [50.51–55.71] | 334.2 [332.5–335.5] | 343.5 [340.7–347.2] | 92.65 [92.13–94.23] | 131.2 [127.9–135.6] |
| vtable_poly6 | 102.7 [94.89–109.5] | 65.59 [64.55–66.44] | 57 [53.45–58.15] | 363.7 [361.3–366.2] | 371.4 [370–373.8] | 98 [96.13–99.16] | 144.9 [140.6–150.3] |
| graphql_as | 21.53 [21.31–21.74] | 16.32 [16.13–16.51] | 14.76 [14.6–14.85] | 98.01 [97.75–98.18] | 88.01 [87.69–88.29] | E7 | 37.44 [37.15–37.83] |
| tailcall_fsm | 5.946 [5.93–5.972] | 13.14 [13.05–13.86] | 8.813 [8.781–8.848] | 69.07 [68.8–70.27] | 75.63 [75.21–76.05] | 6.46 [6.379–6.511] | 14.68 [14.59–14.75] |
| eh_parser_exnref | 24.55 [24.15–25.44] | E8 | E9 | 91.75 [91.45–92.13] | 118.3 [117.6–119.3] | 22.75 [22.62–22.88] | 43.63 [43.27–43.96] |
| eh_parser_legacy | E10 | 21.6 [21.46–21.84] | E9 | E11 | E12 | 23.7 [23.43–23.83] | E13 |
| gc_trees | 210.3 [208.9–214.4] | E14 | E15 | 296.1 [292.9–297.7] | N16 | 76.67 [75.97–76.81] | 259.7 [259–264.4] |
| callref_dispatch | 29.53 [29.39–29.89] | E17 | E18 | 169.1 [167.7–170.5] | 175.1 [174.3–175.6] | E19 | 55.44 [55.02–58.18] |
| callref_dispatch.indirect | 27.84 [27.71–28.31] | 26.41 [26.28–26.93] | 21.67 [21.61–22] | 165.2 [164.2–166.8] | 171.1 [170.9–171.8] | 45.36 [44.92–45.53] | 53.21 [52.98–54.5] |
| relaxed_dot | 11.44 [11.37–12.32] | 3.327 [3.292–3.379] | E5 | 9.863 [9.831–9.893] | E2 | 5.219 [5.158–5.233] | 6.706 [6.668–7.58] |
| relaxed_madd | 0.5675 [0.5632–0.5761] | 0.6755 [0.6681–0.6877] | E5 | 5.536 [5.509–5.633] | E2 | 1.017 [1.008–1.022] | 2.167 [2.15–2.174] |
| mem64_chase | 73.69 [73.04–75.07] | E20 | E15 | 238.4 [209.5–241.8] | 200.9 [198.4–202] | 58.2 [57.99–58.64] | 91.78 [76.6–94.42] |
| mem64_chase.mem32 | 61.37 [60.53–61.48] | 56.58 [56.37–56.66] | 53.36 [53.03–53.59] | 216.4 [192.4–218] | 198.6 [197.3–199.7] | 58.01 [57.77–58.24] | 82.07 [80.68–84.2] |
| multimem_transform | 10.09 [10.04–10.3] | E21 | E22 | 73.38 [72.64–73.76] | 57.1 [56.38–57.47] | E23 | 18.42 [18.37–18.48] |
| multimem_transform.single | 13.78 [13.73–14.28] | 5.717 [5.699–5.736] | 3.868 [3.85–3.881] | 73.43 [72.78–73.97] | 57.15 [56.89–57.41] | 6.938 [6.881–6.963] | 17.5 [17.4–17.58] |
| extconst_init | 0.2332 [0.2296–0.2466] | 0.8716 [0.8523–0.9016] | 19.87 [19.74–20.04] | 19.74 [19.65–19.85] | 3.113 [3.095–3.116] | E24 | 1.099 [1.081–1.164] |
| extconst_init.mvp | 0.1642 [0.1619–0.1693] | 0.1978 [0.1969–0.199] | 10.22 [10.19–10.31] | 18.51 [18.42–18.59] | 1.796 [1.785–1.801] | E25 | 0.3065 [0.3045–0.3078] |
| graphql_porf | 24.25 [23.71–24.75] | 20 [19.32–20.29] | E9 | 80.55 [80.35–81.17] | E26 | E27 | 34.09 [33.81–34.57] |
| graphql_porf_trycatch | E28 | 20.38 [19.19–20.49] | E9 | E29 | E12 | E27 | E30 |
| sqlite3 | 8.559e+04 [8.52e+04–8.621e+04] | N31 | N32 | N33 | N34 | N35 | N36 |

- E1 (wasm3; factorial, crc32): wasm3 m3_FindFunction failed: compiling function underran the stack
- E2 (zwasm; factorial, sieve, crc32, matmul_simd, matmul_fma, convolution, bulk_memory, relaxed_dot, relaxed_madd): init warmup failed: call trapped: trap: unreachable
- E3 (wasm3; sieve, convolution): wasm3 m3_FindFunction failed: incorrect type on stack
- N4 (wasmz; sieve, crc32): N/A: wasmz v0.1.4 segfaults on this module's v128 ops (the scalar build runs)
- E5 (wasm3; matmul_simd, matmul_fma, relaxed_dot, relaxed_madd): wasm3 m3_FindFunction failed: unknown type
- E6 (wasm3; bulk_memory): wasm3 m3_FindFunction failed: unknown label
- E7 (wasmz; graphql_as): wasmz init-warmup failed: wasmz_instance_call trap: trap: wasm `unreachable` instruction executed
- E8 (wamr; eh_parser_exnref): wasm_runtime_load failed: WASM module load failed: unsupported opcode 1f
- E9 (wasm3; eh_parser_exnref, eh_parser_legacy, graphql_porf, graphql_porf_trycatch): wasm3 m3_ParseModule failed: out of order Wasm section
- E10 (pulley; eh_parser_legacy): Module::from_binary failed — invalid wasm or unsupported feature?: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E11 (wasmedge; eh_parser_legacy): WasmEdge VMLoadWasmFromBytes: illegal opcode / loading failed: illegal opcode, Code: 0x117 /
- E12 (zwasm; eh_parser_legacy, graphql_porf_trycatch): wasm_module_new failed (invalid wasm or unsupported feature)
- E13 (tinywasm; eh_parser_legacy): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0x171) at offset 369
- E14 (wamr; gc_trees): wasm_runtime_load failed: WASM module load failed: invalid type flag
- E15 (wasm3; gc_trees, mem64_chase): wasm3 m3_ParseModule failed: malformed Wasm binary
- N16 (zwasm; gc_trees): N/A: zwasm v2.7.0 never reclaims GC structs and grows its GC heap far faster than the allocation rate (124 MB for 31K 24-byte structs); the process segfaults at ~2.3 GB
- E17 (wamr; callref_dispatch): wasm_runtime_load failed: WASM module load failed: incompatible import type
- E18 (wasm3; callref_dispatch): wasm3 m3_ParseModule failed: unknown value_type
- E19 (wasmz; callref_dispatch): wasmz init-warmup failed: wasmz_instance_call trap: trap: null reference dereference
- E20 (wamr; mem64_chase): wasm_runtime_load failed: WASM module load failed: invalid limits flags
- E21 (wamr; multimem_transform): wasm_runtime_load failed: WASM module load failed: multiple memories
- E22 (wasm3; multimem_transform): wasm3 m3_ParseModule failed: only one memory per module is supported
- E23 (wasmz; multimem_transform): wasmz_module_new failed: module compilation failed: MultipleMemories
- E24 (wasmz; extconst_init): wrong result: got -1669113347, expected -1152068691 (cross-runtime consensus)
- E25 (wasmz; extconst_init.mvp): wrong result: got 1844351884, expected -1152068691 (cross-runtime consensus)
- E26 (zwasm; graphql_porf): app killed by signal 9 (jetsam) during this row's launch
- E27 (wasmz; graphql_porf, graphql_porf_trycatch): init warmup failed: wasmz m() trap: multi-value results are not supported by the C API (results_len = 2)
- E28 (pulley; graphql_porf_trycatch): graphql-validation-porf: Module::from_binary failed: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E29 (wasmedge; graphql_porf_trycatch): WasmEdge LoaderParseFromBytes: illegal opcode / loading failed: illegal opcode, Code: 0x117 /
- E30 (tinywasm; graphql_porf_trycatch): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0xfa34) at offset 64052
- N31 (wamr; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N32 (wasm3; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N33 (wasmedge; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N34 (zwasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N35 (wasmz; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N36 (tinywasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley

**Wall time per call, ms: median [range] over reps**

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | 126.8 [126.2–130.4] | 210.6 [209.1–212.2] | 174.2 [161.5–182.2] | 1060 [1058–1078] | 1297 [1290–1306] | 144.2 [143.7–146.9] | 331.6 [327.9–333.9] |
| fib_tail | 0.57 [0.569–0.571] | 0.9095 [0.908–0.915] | 0.488 [0.488–0.488] | 8.116 [8.091–8.137] | 7.186 [7.171–7.21] | 0.862 [0.862–0.87] | 2.127 [2.12–2.131] |
| factorial | 0.001 [0.001–0.001] | 0.001 [0.001–0.001] | E1 | 0.018 [0.018–0.018] | E2 | 0.013 [0.013–0.013] | 0.002 [0.001–0.002] |
| sieve | 0.9675 [0.964–1.043] | 0.8115 [0.81–0.827] | E3 | 6.84 [6.816–6.862] | E2 | N4 | 1.855 [1.854–1.857] |
| crc32 | 6.613 [6.604–6.65] | 5.338 [5.24–5.435] | E1 | 52.33 [51.93–52.82] | E2 | N4 | 15.97 [15.79–16.14] |
| matmul_simd | 4.792 [4.78–6.359] | 4.258 [4.226–4.282] | E5 | 33.46 [33.41–33.52] | E2 | 7.8 [7.786–7.856] | 11.09 [11.08–11.15] |
| matmul_fma | 3.713 [3.692–3.843] | 3.139 [3.131–3.15] | E5 | 26.73 [26.62–27] | E2 | 5.223 [5.212–5.236] | 8.644 [8.616–8.694] |
| convolution | 10.16 [9.991–10.46] | 6.475 [6.415–6.557] | E3 | 40.55 [40.43–41.03] | E2 | 13.24 [13.2–13.29] | 17.66 [17.6–17.89] |
| audio_dsp | 1713 [1694–1816] | 1097 [1091–1105] | 877.1 [714.8–899.7] | 1.177e+04 [1.174e+04–1.181e+04] | 9628 [9612–9644] | 1425 [1407–1479] | 3317 [3230–3425] |
| bulk_memory | 30.19 [29.54–32.53] | 15.22 [15.1–15.29] | E6 | 134.6 [132.9–134.9] | E2 | 15.66 [15.5–17.22] | 29.77 [29.66–29.85] |
| call_indirect | 41.5 [41.12–42.02] | 27.87 [27.62–27.94] | 20.65 [20.14–20.83] | 164.5 [163.8–164.9] | 163.1 [162–164.1] | 46.37 [46.17–46.58] | 59.93 [59.78–62.61] |
| factorial.scalar | 0.001 [0.001–0.001] | 0.001 [0.001–0.001] | 0.001 [0.001–0.001] | 0.02 [0.02–0.02] | 0.006 [0.006–0.006] | 0.013 [0.013–0.013] | 0.001 [0.001–0.001] |
| sieve.scalar | 1.018 [1.016–1.072] | 0.7285 [0.726–0.738] | 0.462 [0.461–0.464] | 7.122 [7.07–7.244] | 5.668 [5.65–5.677] | 0.882 [0.861–0.884] | 1.986 [1.982–1.989] |
| crc32.scalar | 6.645 [6.634–6.659] | 5.303 [5.288–5.366] | 3.737 [3.732–3.743] | 52.24 [51.63–52.77] | 46.99 [46.82–47.1] | 6.996 [6.97–7.01] | 16.02 [15.95–16.14] |
| convolution.scalar | 19.35 [19.31–20.78] | 14.77 [14.74–14.8] | 10.48 [10.46–11.3] | 97.31 [96.67–98.3] | 105.5 [105.3–106.4] | 20.04 [19.87–20.09] | 42.04 [41.9–42.12] |
| bulk_memory.scalar | 30.44 [29.97–32.79] | 15.73 [15.63–15.82] | 11.39 [11.36–11.42] | 133.7 [133.5–134.5] | 125.3 [125–127.3] | 15.18 [14.93–16.86] | 31.9 [31.83–32.01] |
| xmrsplayer | 27.71 [26.9–30.8] | 24.9 [24.51–25.22] | 18.97 [18.47–19.38] | 160.1 [159.4–162.1] | 136 [135.7–136.3] | 20.51 [19.96–20.89] | 49.72 [48.13–50.33] |
| vtable_mono | 76.75 [72.24–79.33] | 50.12 [49.86–53.82] | 44.29 [44–44.59] | 223.2 [222.6–224.7] | 265 [264.1–267.1] | 84.95 [84.23–85.81] | 104 [103–114.3] |
| vtable_bi | 83.97 [80.23–91] | 53.95 [53.73–54.09] | 46.05 [45.74–47.56] | 310.8 [310.2–312] | 334.7 [333–339.7] | 90.94 [90.44–92.44] | 122.2 [121.1–123.8] |
| vtable_poly4 | 87.73 [82.3–94.5] | 58.52 [57.91–59.06] | 51.69 [50.99–56.46] | 336.8 [335.4–337.4] | 346.6 [344.2–350.6] | 93.42 [92.81–94.86] | 132.2 [128.4–137.5] |
| vtable_poly6 | 103.6 [95.59–111] | 66.11 [64.89–66.76] | 57.61 [53.88–58.72] | 366.1 [365.2–368.1] | 374.2 [373.3–376.2] | 98.37 [97.13–100] | 146.1 [141.1–151.6] |
| graphql_as | 21.39 [21.22–22.7] | 15.11 [14.46–15.3] | 14.71 [14.53–14.82] | 97.5 [96.86–98.9] | 87 [86.36–87.18] | E7 | 37.3 [37.03–37.65] |
| tailcall_fsm | 5.934 [5.919–5.945] | 13.2 [13.06–13.91] | 8.832 [8.803–8.897] | 69.6 [69.16–70.5] | 76.88 [76.55–77.61] | 6.456 [6.375–6.502] | 14.75 [14.65–14.87] |
| eh_parser_exnref | 24.73 [24.27–25.56] | E8 | E9 | 92.48 [91.89–92.71] | 118.9 [118.2–120.3] | 22.8 [22.56–22.95] | 43.85 [43.45–44.16] |
| eh_parser_legacy | E10 | 21.7 [21.56–21.94] | E9 | E11 | E12 | 23.84 [23.46–23.91] | E13 |
| gc_trees | 211.8 [210.2–216.6] | E14 | E15 | 298.4 [297–300.8] | N16 | 73.62 [73.01–73.88] | 262.7 [261.3–267.4] |
| callref_dispatch | 29.69 [29.59–30.11] | E17 | E18 | 170.4 [169.8–171.6] | 176.4 [175.9–176.9] | E19 | 55.75 [55.47–58.53] |
| callref_dispatch.indirect | 27.96 [27.89–28.51] | 26.53 [26.49–27.09] | 21.78 [21.72–22.04] | 166.9 [165.4–168.6] | 172.7 [172.1–173.4] | 45.57 [45.04–45.66] | 53.54 [53.35–54.94] |
| relaxed_dot | 11.48 [11.43–12.54] | 3.306 [3.279–3.348] | E5 | 9.893 [9.867–9.931] | E2 | 5.213 [5.159–5.229] | 6.706 [6.687–7.609] |
| relaxed_madd | 0.581 [0.58–0.585] | 0.697 [0.696–0.697] | E5 | 5.518 [5.503–5.614] | E2 | 1.006 [1.001–1.008] | 2.155 [2.138–2.157] |
| mem64_chase | 74.19 [73.92–75.63] | E20 | E15 | 242.6 [215.7–245] | 204.9 [204.2–206.5] | 58.68 [58.36–59.39] | 92.37 [85.81–96.16] |
| mem64_chase.mem32 | 62.02 [61.85–62.15] | 57.04 [56.99–57.12] | 53.74 [53.58–54.09] | 219 [192.6–220.6] | 201.5 [198.4–202.5] | 58.66 [58.32–58.85] | 82.62 [81.77–84.76] |
| multimem_transform | 10.12 [10.09–10.31] | E21 | E22 | 74.09 [72.84–74.47] | 57.57 [57.31–57.83] | E23 | 18.49 [18.48–18.59] |
| multimem_transform.single | 13.83 [13.78–14.43] | 5.71 [5.702–5.715] | 3.846 [3.84–3.848] | 74.16 [73.4–74.49] | 57.51 [57.35–57.77] | 6.935 [6.874–6.96] | 17.56 [17.53–17.59] |
| extconst_init | 0.192 [0.189–0.206] | 0.809 [0.786–0.831] | 19.79 [19.74–19.99] | 19.36 [19.28–19.47] | 3.095 [3.087–3.102] | E24 | 1.027 [1.014–1.092] |
| extconst_init.mvp | 0.124 [0.123–0.127] | 0.174 [0.172–0.175] | 10.15 [10.12–10.23] | 18.12 [18.02–18.19] | 1.786 [1.769–1.79] | E25 | 0.2665 [0.264–0.268] |
| graphql_porf | 24.01 [23.52–24.51] | 19.11 [18.45–19.29] | E9 | 80.16 [79.81–80.52] | E26 | E27 | 33.91 [33.59–34.18] |
| graphql_porf_trycatch | E28 | 19.6 [18.76–19.74] | E9 | E29 | E12 | E27 | E30 |
| sqlite3 | 8.642e+04 [8.601e+04–8.737e+04] | N31 | N32 | N33 | N34 | N35 | N36 |

- E1 (wasm3; factorial, crc32): wasm3 m3_FindFunction failed: compiling function underran the stack
- E2 (zwasm; factorial, sieve, crc32, matmul_simd, matmul_fma, convolution, bulk_memory, relaxed_dot, relaxed_madd): init warmup failed: call trapped: trap: unreachable
- E3 (wasm3; sieve, convolution): wasm3 m3_FindFunction failed: incorrect type on stack
- N4 (wasmz; sieve, crc32): N/A: wasmz v0.1.4 segfaults on this module's v128 ops (the scalar build runs)
- E5 (wasm3; matmul_simd, matmul_fma, relaxed_dot, relaxed_madd): wasm3 m3_FindFunction failed: unknown type
- E6 (wasm3; bulk_memory): wasm3 m3_FindFunction failed: unknown label
- E7 (wasmz; graphql_as): wasmz init-warmup failed: wasmz_instance_call trap: trap: wasm `unreachable` instruction executed
- E8 (wamr; eh_parser_exnref): wasm_runtime_load failed: WASM module load failed: unsupported opcode 1f
- E9 (wasm3; eh_parser_exnref, eh_parser_legacy, graphql_porf, graphql_porf_trycatch): wasm3 m3_ParseModule failed: out of order Wasm section
- E10 (pulley; eh_parser_legacy): Module::from_binary failed — invalid wasm or unsupported feature?: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E11 (wasmedge; eh_parser_legacy): WasmEdge VMLoadWasmFromBytes: illegal opcode / loading failed: illegal opcode, Code: 0x117 /
- E12 (zwasm; eh_parser_legacy, graphql_porf_trycatch): wasm_module_new failed (invalid wasm or unsupported feature)
- E13 (tinywasm; eh_parser_legacy): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0x171) at offset 369
- E14 (wamr; gc_trees): wasm_runtime_load failed: WASM module load failed: invalid type flag
- E15 (wasm3; gc_trees, mem64_chase): wasm3 m3_ParseModule failed: malformed Wasm binary
- N16 (zwasm; gc_trees): N/A: zwasm v2.7.0 never reclaims GC structs and grows its GC heap far faster than the allocation rate (124 MB for 31K 24-byte structs); the process segfaults at ~2.3 GB
- E17 (wamr; callref_dispatch): wasm_runtime_load failed: WASM module load failed: incompatible import type
- E18 (wasm3; callref_dispatch): wasm3 m3_ParseModule failed: unknown value_type
- E19 (wasmz; callref_dispatch): wasmz init-warmup failed: wasmz_instance_call trap: trap: null reference dereference
- E20 (wamr; mem64_chase): wasm_runtime_load failed: WASM module load failed: invalid limits flags
- E21 (wamr; multimem_transform): wasm_runtime_load failed: WASM module load failed: multiple memories
- E22 (wasm3; multimem_transform): wasm3 m3_ParseModule failed: only one memory per module is supported
- E23 (wasmz; multimem_transform): wasmz_module_new failed: module compilation failed: MultipleMemories
- E24 (wasmz; extconst_init): wrong result: got -1669113347, expected -1152068691 (cross-runtime consensus)
- E25 (wasmz; extconst_init.mvp): wrong result: got 1844351884, expected -1152068691 (cross-runtime consensus)
- E26 (zwasm; graphql_porf): app killed by signal 9 (jetsam) during this row's launch
- E27 (wasmz; graphql_porf, graphql_porf_trycatch): init warmup failed: wasmz m() trap: multi-value results are not supported by the C API (results_len = 2)
- E28 (pulley; graphql_porf_trycatch): graphql-validation-porf: Module::from_binary failed: failed to compile: wasm[0]::function[N]: WebAssembly translation error: Unsupported feature: operator Try { blockty: Empty }
- E29 (wasmedge; graphql_porf_trycatch): WasmEdge LoaderParseFromBytes: illegal opcode / loading failed: illegal opcode, Code: 0x117 /
- E30 (tinywasm; graphql_porf_trycatch): tinywasm::parse_bytes failed: error parsing module: legacy exceptions support is not enabled (at offset 0xfa34) at offset 64052
- N31 (wamr; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N32 (wasm3; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N33 (wasmedge; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N34 (zwasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N35 (wasmz; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley
- N36 (tinywasm; sqlite3): N/A: the harness only has a WASI preview-1 + bench.* import shim for Pulley

**Measured E-core residency and IPC**

| runtime | rows | e_share min | e_share median | IPC median |
|---|---:|---:|---:|---:|
| pulley | 37 | 0.823 | 1.000 | 1.56 |
| wamr | 33 | 0.794 | 1.000 | 1.55 |
| wasm3 | 21 | 0.763 | 1.000 | 1.40 |
| wasmedge | 36 | 0.992 | 1.000 | 1.54 |
| zwasm | 25 | 0.978 | 1.000 | 1.66 |
| wasmz | 29 | 0.990 | 1.000 | 1.38 |
| tinywasm | 36 | 0.734 | 1.000 | 1.73 |
<!-- /T:iphone -->

## Feature benchmarks against their twins

CPU time per call of each feature benchmark divided by its featureless
twin's, same runtime. The two platforms agree:

- **`call_ref` vs `call_indirect`:** `call_ref` is 1-9 % slower on every
  runtime that has both. On Pulley, the v49 stack's elisions make
  immutable-table `call_indirect` cheaper than a typed `call_ref`.
- **memory64:** costs Pulley 12-20 % over a 32-bit memory (64-bit
  bounds arithmetic on explicit checks), WasmEdge and tinywasm 5-12 %,
  zwasm and wasmz nothing.
- **Multi-memory:** is *faster* than the one-memory twin on Pulley
  (0.67-0.73: the twin's per-access offset adds are extra Pulley ops) and
  neutral elsewhere.
- **Extended-const:** makes instantiation 1.1-5.8× slower than
  pre-folded constants (WAMR 4.4-5.8×, tinywasm 3.6-4.5×). Constant
  expressions are evaluated by each runtime's slow path.
- **Relaxed SIMD `f32x4.relaxed_madd`:** beats simd128 mul+add by 20-32 %
  on every runtime that has it.
- **Rust's auto-vectorized builds:** matter only for convolution (1.5-3×
  faster than the scalar build) and are neutral for the others.
- **exnref vs legacy EH:** only wasmz runs both; exnref is 4 % faster.

<!-- T:twins -->
**M4 E-cores** — CPU time per call, feature ÷ twin (< 1: the feature's build is faster)

| feature | twin | measures | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| callref_dispatch | callref_dispatch.indirect | call_ref vs call_indirect | 1.06 | — | — | 1.04 | 1.09 | — | 1.01 |
| mem64_chase | mem64_chase.mem32 | memory64 vs 32-bit memory | 1.12 | — | — | 1.05 | 1.03 | 0.85 | 1.07 |
| multimem_transform | multimem_transform.single | 3 memories vs 1 (offsets) | 0.67 | — | — | 0.97 | 0.96 | — | 1.06 |
| extconst_init | extconst_init.mvp | extended-const vs pre-folded consts | 1.79 | 5.75 | 1.89 | 1.13 | 2.15 | — | 4.47 |
| eh_parser_exnref | eh_parser_legacy | exnref vs legacy EH encoding | — | — | — | — | — | 0.96 | — |
| matmul_fma | matmul_simd | relaxed madd vs simd128 mul+add | 0.76 | 0.73 | — | 0.75 | — | 0.70 | 0.80 |
| factorial | factorial.scalar | auto-vectorized vs scalar build | 0.67 | 0.98 | — | 1.02 | — | 1.14 | 1.00 |
| sieve | sieve.scalar | auto-vectorized vs scalar build | 0.97 | 0.95 | — | 0.96 | — | — | 0.95 |
| crc32 | crc32.scalar | auto-vectorized vs scalar build | 1.07 | 1.01 | — | 0.99 | — | — | 0.93 |
| convolution | convolution.scalar | auto-vectorized vs scalar build | 0.49 | 0.34 | — | 0.42 | — | 0.65 | 0.41 |
| bulk_memory | bulk_memory.scalar | auto-vectorized vs scalar build | 1.01 | 0.97 | — | 0.95 | — | 1.04 | 0.91 |

**iPhone XS E-cores** — CPU time per call, feature ÷ twin (< 1: the feature's build is faster)

| feature | twin | measures | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| callref_dispatch | callref_dispatch.indirect | call_ref vs call_indirect | 1.06 | — | — | 1.02 | 1.02 | — | 1.04 |
| mem64_chase | mem64_chase.mem32 | memory64 vs 32-bit memory | 1.20 | — | — | 1.10 | 1.01 | 1.00 | 1.12 |
| multimem_transform | multimem_transform.single | 3 memories vs 1 (offsets) | 0.73 | — | — | 1.00 | 1.00 | — | 1.05 |
| extconst_init | extconst_init.mvp | extended-const vs pre-folded consts | 1.42 | 4.41 | 1.94 | 1.07 | 1.73 | — | 3.58 |
| eh_parser_exnref | eh_parser_legacy | exnref vs legacy EH encoding | — | — | — | — | — | 0.96 | — |
| matmul_fma | matmul_simd | relaxed madd vs simd128 mul+add | 0.77 | 0.74 | — | 0.80 | — | 0.68 | 0.78 |
| factorial | factorial.scalar | auto-vectorized vs scalar build | 0.96 | 1.14 | — | 0.92 | — | 1.14 | 1.03 |
| sieve | sieve.scalar | auto-vectorized vs scalar build | 0.93 | 1.10 | — | 0.96 | — | — | 0.92 |
| crc32 | crc32.scalar | auto-vectorized vs scalar build | 1.00 | 1.00 | — | 1.00 | — | — | 0.99 |
| convolution | convolution.scalar | auto-vectorized vs scalar build | 0.52 | 0.44 | — | 0.42 | — | 0.66 | 0.42 |
| bulk_memory | bulk_memory.scalar | auto-vectorized vs scalar build | 0.99 | 0.97 | — | 1.00 | — | 1.03 | 0.93 |
<!-- /T:twins -->

## Per-case memory (M4)

`scripts/run-m4-memory-pass.sh`. The timing passes run all of a
runtime's cases in one process, where the footprint and RSS peaks only
grow, and macOS RSS also counts pages the allocator freed but kept
(MADV_FREE_REUSABLE). So each case also ran once in a process of its
own, recording the `phys_footprint` ledger peak and the footprint left
after the case.

<!-- T:memory -->
Peak phys_footprint in MB (task_vm_info ledger peak) over load, warmup and a 2000 ms window, one process per runtime × case; in parentheses the footprint still held after the case when it is at least 20 MB. Failed rows are blank.

| case | pulley | wamr | wasm3 | wasmedge | zwasm | wasmz | tinywasm |
|---|---:|---:|---:|---:|---:|---:|---:|
| fib | 2.7 | 11 | 3.5 | 3.0 | 4.0 | 4.2 | 3.3 |
| fib_tail | 2.9 | 11 | 3.5 | 3.0 | 4.1 | 4.3 | 3.4 |
| factorial | 2.9 | 11 |  | 3.2 |  | 4.3 | 3.5 |
| sieve | 3.7 | 11 |  | 3.1 |  |  | 3.5 |
| crc32 | 3.3 | 11 |  | 3.1 |  |  | 3.5 |
| matmul_simd | 3.7 | 11 |  | 3.2 |  | 4.3 | 3.5 |
| matmul_fma | 3.7 | 11 |  | 3.2 |  | 4.3 | 3.5 |
| convolution | 3.6 | 11 |  | 3.2 |  | 4.4 | 3.5 |
| audio_dsp | 3.3 | 11 | 3.6 | 3.0 | 4.1 | 4.5 | 3.4 |
| bulk_memory | 3.9 | 11 |  | 3.2 |  | 4.7 | 3.6 |
| call_indirect | 4.1 | 11 | 3.5 | 3.1 | 4.1 | 5.2 | 3.4 |
| factorial.scalar | 2.9 | 11 | 3.6 | 3.2 | 4.2 | 4.2 | 3.5 |
| sieve.scalar | 3.8 | 11 | 3.6 | 3.1 | 4.1 | 4.5 | 3.4 |
| crc32.scalar | 3.2 | 11 | 3.6 | 3.1 | 4.1 | 4.3 | 3.4 |
| convolution.scalar | 3.2 | 11 | 3.6 | 3.2 | 4.2 | 4.4 | 3.5 |
| bulk_memory.scalar | 3.9 | 11 | 3.7 | 3.2 | 4.2 | 4.7 | 3.6 |
| xmrsplayer | 32 | 27 | 22 (22) | 22 | 22 | 24 | 21 (21) |
| vtable_mono | 4.7 | 11 | 3.6 | 3.2 | 4.1 | 5.6 | 3.5 |
| vtable_bi | 4.7 | 11 | 3.6 | 3.3 | 4.1 | 5.6 | 3.5 |
| vtable_poly4 | 4.7 | 11 | 3.6 | 3.3 | 4.1 | 5.6 | 3.5 |
| vtable_poly6 | 4.7 | 11 | 3.6 | 3.3 | 4.2 | 5.6 | 3.5 |
| graphql_as | 11 | 11 | 3.2 | 5.4 | 5.4 |  | 3.7 |
| tailcall_fsm | 3.6 | 11 | 2.6 | 3.3 | 159 (158) | 4.0 | 2.5 |
| eh_parser_exnref | 4.2 |  |  | 3.4 | 18 | 7.2 | 4.5 |
| eh_parser_legacy |  | 11 |  |  |  | 8.8 |  |
| gc_trees | 5.1 |  |  | 295 (145) |  | 316 | 6.6 |
| callref_dispatch | 3.6 |  |  | 3.0 | 4.0 |  | 2.3 |
| callref_dispatch.indirect | 3.3 | 2.3 | 2.4 | 3.0 | 4.1 | 3.6 | 2.3 |
| relaxed_dot | 4.2 | 11 |  | 3.2 |  | 4.5 | 3.5 |
| relaxed_madd | 4.3 | 11 |  | 3.3 |  | 4.5 | 3.6 |
| mem64_chase | 70 |  |  | 70 (70) | 71 (71) | 70 | 70 (70) |
| mem64_chase.mem32 | 70 | 78 | 70 (70) | 70 | 71 | 70 | 69 (69) |
| multimem_transform | 3.4 |  |  | 3.4 | 4.4 |  | 2.7 |
| multimem_transform.single | 3.3 | 11 | 2.8 | 3.4 | 4.4 | 3.6 | 2.7 |
| extconst_init | 47 (47) | 4.1 | 3.7 | 6.4 | 2781 (957) |  | 4.2 |
| extconst_init.mvp | 8.7 | 2.9 | 3.7 | 5.2 | 2084 (273) |  | 3.3 |
| graphql_porf | 29 | 19 |  | 17 | 614 (138) |  | 1051 (1034) |
| graphql_porf_trycatch |  | 19 |  |  |  |  |  |
| sqlite3 | 77 (58) |  |  |  |  |  |  |
<!-- /T:memory -->

- **zwasm** keeps what it allocates:
  - The extended-const rows peak at 2.1-2.8 GB and still hold 0.3-1.0 GB
    after the case (each sample instantiates the module).
  - Porffor peaks at 614 MB.
  - `tailcall_fsm` holds 158 MB after the case, so `return_call` is not
    constant-space.
  - On the iPhone this gets the Porffor launch jetsam-killed.
- **WasmEdge** never collects GC structs: `gc_trees` peaks at 295 MB and
  still holds 145 MB after the case.
- **wasmz** peaks at 316 MB on `gc_trees` and frees it only when the
  instance is torn down.
- **Pulley** (DRC collector) stays at 5 MB and tinywasm at 7 MB.
- **tinywasm's Porffor row** holds 1.03 GB after 103 samples, although
  each sample drops its `Store`. The same row peaks at 52 MB RSS on the
  iPhone, so it is either a macOS-allocator effect or a leak that only
  shows on macOS. Not yet diagnosed.

## Component model async (WASI 0.3)

The component `workloads/cm_async_bench.wasm` is a no_std Rust guest
on `wasip3` 0.9.0. It times itself with `wasi:clocks/monotonic-clock@0.3.0`,
so runtime startup and compilation stay out of the per-op numbers. It
has three phases, each a hot path of the async canonical ABI, and runs
each 5 times per process:

- `wait_for_0`: 20 000 sequential `wait-for(0)` calls. This is an
  async-lowered host import that is ready immediately: subtask start,
  return and waitable bookkeeping per call.
- `concurrent_wait`: 1000 `wait-for(0)` subtasks in flight at once, then
  joined. This exercises a waitable set with many members and guest task
  switching.
- `stdout_stream`: 2 MiB through `stdout.write-via-stream` as 8192
  `stream<u8>` writes of 256 B. The runner checks the exact bytes by
  SHA-256, so a runtime that drops bytes fails instead of scoring.

Only the M4 runs it: the iOS app links no component runner. Runners:

- Pulley: the wasmtime v49 CLI from the submodule, built like the
  harness (nightly, `--cfg=pulley_tail_calls`), run as `run --target
  pulley64 -W component-model-async=y -S p3=y`.
- zwasm: its CLI built `-Dengine=interp -Dwasi=p3`, run with
  `--engine interp`.

Each of N = 10 processes runs under `taskpolicy -b`, wrapped by
`rusage_exec`, which records the E-core share, CPU time, instructions and
cycles. The WAMR cm_wasip2 fork's `iwasm` was run once and rejects the
component (exit 255), as in the feature matrix.

<!-- T:cm -->
| runtime | phase | runs | median ns/op | min | max |
|---|---|---:|---:|---:|---:|
| pulley | wait_for_0 | 10 | 2702.4 | 1543.8 | 2965.5 |
| pulley | concurrent_wait | 10 | 6469.3 | 3280.6 | 7765.0 |
| pulley | stdout_stream | 10 | 45868.9 | 30437.6 | 47789.6 |
| zwasm | wait_for_0 | 10 | 6251.1 | 5447.7 | 6964.7 |
| zwasm | concurrent_wait | 10 | 21252.3 | 19021.2 | 23108.6 |
| zwasm | stdout_stream | 10 | 76708.6 | 65521.7 | 78680.5 |

| run | exit | e_share | cpu (ms) | wall (ms) | IPC |
|---|---:|---:|---:|---:|---:|
| pulley rep=1 | 0 | 0.9781 | 2306.2 | 2314.8 | 2.63 |
| zwasm rep=1 | 0 | 0.9955 | 3591.8 | 3864.9 | 3.86 |
| wamr-cm-fork rep=1 | 255 | 0.9465 | 8.1 | 14.3 | 2.29 |
| pulley rep=2 | 0 | 0.9991 | 2332.3 | 2341.8 | 2.73 |
| zwasm rep=2 | 0 | 0.9954 | 3646.5 | 3898.2 | 3.87 |
| pulley rep=3 | 0 | 0.9993 | 2278.3 | 2282.7 | 2.75 |
| zwasm rep=3 | 0 | 0.9963 | 3772.1 | 3977.9 | 3.88 |
| pulley rep=4 | 0 | 0.9955 | 2197.0 | 2194.4 | 2.75 |
| zwasm rep=4 | 0 | 0.9951 | 3437.8 | 3661.6 | 3.87 |
| pulley rep=5 | 0 | 0.9971 | 2331.0 | 2346.2 | 2.75 |
| zwasm rep=5 | 0 | 0.9985 | 3573.6 | 3866.3 | 3.85 |
| pulley rep=6 | 0 | 0.9989 | 2208.9 | 2211.7 | 2.76 |
| zwasm rep=6 | 0 | 0.9998 | 3606.7 | 3882.4 | 3.87 |
| pulley rep=7 | 0 | 0.9955 | 1988.1 | 2367.2 | 2.50 |
| zwasm rep=7 | 0 | 0.9992 | 2621.4 | 3273.0 | 3.80 |
| pulley rep=8 | 0 | 0.9996 | 1680.2 | 2067.2 | 2.64 |
| zwasm rep=8 | 0 | 0.9967 | 3166.1 | 4186.9 | 3.78 |
| pulley rep=9 | 0 | 0.9942 | 1390.7 | 1468.1 | 2.70 |
| zwasm rep=9 | 0 | 0.9959 | 2754.1 | 3862.7 | 3.74 |
| pulley rep=10 | 0 | 0.9946 | 2122.1 | 2254.5 | 2.71 |
| zwasm rep=10 | 0 | 0.9989 | 3609.7 | 3869.2 | 3.87 |
<!-- /T:cm -->

Both runtimes run all three phases correctly, and every run's stdout
matched the expected bytes. Pulley is 2.3× faster on
`wait-for(0)` subtasks, 3.3× on a 1000-member waitable set and 1.7× on
the `stream<u8>` writes. zwasm's P3 runner does not finish while host
stdin is open, so every run uses `</dev/null`.


## femtovg E2E

femtovg compiled to wasm draws an SVG per frame; a native host renders
the result with femtovg's own wgpu renderer on Metal. The ABI is in
[femtovg-e2e-abi.md](femtovg-e2e-abi.md). In short:

- The guest parses the SVG once with usvg, outside the timed loop. Per
  frame it does all of femtovg's CPU work: transforms, path flattening,
  fill and stroke tessellation, and building the vertex and command
  buffers.
- At flush, the fork's `WireRenderer` (femtovg's `Renderer` trait)
  passes the vertices and an encoded command stream to the host through
  five `i32`-only imports, identical on every runtime.
- The host copies them out, decodes them into real `Command`s
  (`WireReplayer`), and calls the stock `WGPURenderer::render` on an
  offscreen 1024×1024 RGBA8 texture.

The scenes are femtovg's own example SVG (the Ghostscript tiger: 240
paths) and a heavier real-world one, the component-model design repo's
`combined-linking.svg`: 189 paths, 9341 segments of glyph outlines, and
a root `clip-path`. Provenance and licenses are in
`workloads/femtovg/LICENSES.md`. The tiger's artwork descends from
Ghostscript's AGPL-3.0 `tiger.eps`, so check that term before
redistributing it outside this benchmark.

**Schedule and timing.**

- Every runtime draws the same deterministic zoom schedule: 121 frames,
  1× → 16× → 1× in equal log steps about the canvas center. It runs two
  passes, and the second is measured.
- Per frame, `guest` is the wall time of `fvg_frame` (all of femtovg's
  CPU work inside the runtime). `encode` is decode + wgpu encode +
  submit. `GPU` is the wait on that submission. The host waits on the
  GPU every frame, so a frame is complete when it is counted.
- FPS mean is frames / measured-pass time; FPS median is 1 / median
  frame time.
- Peak memory is the process's `phys_footprint` high-water mark (the
  `task_vm_info` ledger) and the peak wasm linear memory
  (`memory.size` × 64 KiB, sampled every frame).
- One runtime per process on the M4, and one launch per runtime × scene
  on the iPhone, so peaks never mix.

**Guest builds.** The guest was built three ways with rustc 1.93.1:
`+simd128,+relaxed-simd`, `+simd128`, and scalar.

- **The relaxed build contains no relaxed-SIMD instruction**
  (`wasm-tools print` shows 3832 SIMD ops, 0 relaxed). rustc emits
  relaxed SIMD only through intrinsics and never contracts `a*b+c`
  without fast-math. The relaxed build is therefore effectively the
  simd128 build: identical code, only the target-features section
  differs.
- Each runtime runs the best build it supports. simd128 runs on Pulley,
  WAMR, WasmEdge and tinywasm. The scalar build runs on wasm3 and zwasm
  (no interpreter SIMD) and on wasmz, whose v128 execution crashes.

**Correctness.**

- FNV-1a-64 over each frame's vertices and command stream: all seven
  runtimes and all three builds produce bit-identical frames, and the
  same final-texture hash on the M4 Max and on the A12's GPU.
- A native (non-wasm) build of the same guest matches the command
  stream and the vertex counts on every frame. Its vertices differ by
  1 ulp on a few dozen of ~275K floats per frame: femtovg calls `sin_cos`,
  `tan`, `hypot`, `acos` and `atan2`, and the wasm guest carries Rust's
  libm port where the native build uses Apple's libm.

### M4 Max E-cores

`taskpolicy -b`, one process per (runtime, scene), N=10 (median [range]
over reps; the measured pass is the second of two).

<!-- T:e2e-m4 -->
**M4 Max E-cores: frame rate and frame time**

| runtime | build | scene | FPS mean | FPS median | frame p50 ms | p95 | p99 | guest ms/frame | encode ms | GPU wait ms | E-share (min) | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| pulley | simd128 | 0 | 8.48 [7.842–9.25] | 8.848 [8.163–11.89] | 113 [84.07–122.5] | 160.5 [140–420.4] | 176.6 [154.6–656.4] | 110.7 [101.7–119.4] | 2.577 [2.318–4.042] | 4.06 [4.029–5.305] | 1.000 | 10/10 |
| wamr | simd128 | 0 | 10.92 [10.36–11.74] | 11.34 [10.85–11.97] | 88.22 [83.54–92.19] | 126.2 [111.6–128.6] | 133.8 [125.6–147.1] | 84.72 [78.66–89.82] | 2.657 [2.232–2.722] | 4.049 [3.971–4.204] | 1.000 | 10/10 |
| wasm3 | scalar | 0 | 15.59 [13.84–18.32] | 16.15 [14.74–18.39] | 61.92 [54.37–67.82] | 87.4 [77.55–105.6] | 95.27 [85.48–121.5] | 57.59 [50.07–65.45] | 2.505 [1.817–2.732] | 4.068 [2.634–4.177] | 1.000 | 10/10 |
| wasmedge | simd128 | 0 | 2.05 [1.921–2.177] | 2.18 [2.042–2.351] | 458.7 [425.4–489.8] | 674.3 [609–719.3] | 740.2 [679.3–837.7] | 481.4 [452.8–513.7] | 2.511 [2.245–2.741] | 4.05 [3.86–4.28] | 1.000 | 10/10 |
| zwasm | scalar | 0 | 2.309 [2.132–2.391] | 2.462 [2.263–2.554] | 406.3 [391.5–441.9] | 589.3 [552.9–671.1] | 646.9 [594.8–734.8] | 426.3 [411.7–462.5] | 2.563 [2.292–2.79] | 4.018 [3.994–4.264] | 1.000 | 10/10 |
| wasmz | scalar | 0 | 9.983 [9.346–16.44] | 10.37 [9.477–16.04] | 96.45 [62.36–105.5] | 137.8 [75.35–161.1] | 149.5 [79.24–211.4] | 93.58 [55.75–99.15] | 2.558 [1.423–3.607] | 4.066 [3.637–4.294] | 1.000 | 10/10 |
| tinywasm | simd128 | 0 | 4.827 [1.533–6.464] | 5.185 [4.809–6.855] | 192.9 [145.9–208] | 293 [216.1–394.4] | 319.1 [225.3–1.806e+04] | 200.6 [149.4–622.5] | 2.46 [1.604–5.94] | 4.058 [3.693–23.75] | 1.000 | 10/10 |
| pulley | simd128 | 1 | 15.59 [14.61–18.76] | 17.06 [16.28–21.12] | 58.63 [47.35–61.41] | 103.5 [91.14–109.7] | 115.7 [100.5–161] | 60.87 [50.67–64.82] | 1.733 [1.25–1.947] | 1.544 [1.38–1.702] | 1.000 | 10/10 |
| wamr | simd128 | 1 | 19.23 [17.89–20.83] | 21 [19.14–22.98] | 47.62 [43.53–52.25] | 83.28 [75.74–90.29] | 88.77 [82.95–101.8] | 48.68 [45.12–52.34] | 1.784 [1.434–1.938] | 1.593 [1.459–1.761] | 1.000 | 10/10 |
| wasm3 | scalar | 1 | 26.33 [22.73–28.2] | 29.1 [23.83–31.86] | 34.37 [31.38–41.97] | 61.47 [57.25–69.42] | 65.69 [62.14–83.35] | 34.58 [32.23–39.87] | 1.821 [1.679–2.367] | 1.609 [1.464–1.763] | 1.000 | 10/10 |
| wasmedge | simd128 | 1 | 3.466 [3.255–4.425] | 3.962 [3.646–4.869] | 252.4 [205.4–274.3] | 427.5 [344.4–463.6] | 458.1 [378.1–515.9] | 285.2 [223.6–303.5] | 1.81 [1.157–1.993] | 1.562 [1.206–1.735] | 1.000 | 10/10 |
| zwasm | scalar | 1 | 3.936 [3.517–4.121] | 4.393 [4.119–4.579] | 227.6 [218.4–242.8] | 409.2 [374.8–427.3] | 440.5 [397.3–557.1] | 250.7 [239.3–281] | 1.829 [1.646–3.887] | 1.602 [1.453–1.652] | 1.000 | 10/10 |
| wasmz | scalar | 1 | 17.93 [16.09–31.27] | 19.63 [17.69–35.01] | 50.94 [28.57–56.53] | 87.26 [45.78–92.37] | 91.98 [58.96–120.9] | 52.56 [29.83–58.28] | 1.788 [0.925–2.089] | 1.567 [1.223–1.774] | 1.000 | 10/10 |
| tinywasm | simd128 | 1 | 8.364 [3.458–10.36] | 9.313 [8.809–10.45] | 107.4 [95.67–113.5] | 172 [127.1–186.7] | 185.5 [134.7–3477] | 116.2 [93.94–260.5] | 1.808 [1.167–2.841] | 1.555 [1.411–25.81] | 1.000 | 10/10 |

**M4 Max E-cores: startup and memory**

| runtime | scene | load ms | init ms | peak footprint MB | peak linear memory MB |
|---|---:|---:|---:|---:|---:|
| pulley | 0 | 1677 [1338–2313] | 152.3 [70.02–189.8] | 540.4 [539.8–553] | 12.39 [12.39–12.39] |
| wamr | 0 | 56.82 [27.73–62.11] | 101.7 [61.9–125] | 522.4 [521.7–534.7] | 12.39 [12.39–12.39] |
| wasm3 | 0 | 2.3 [1.423–2.491] | 99.06 [72.18–111.1] | 523 [522.2–539.4] | 12.39 [12.39–12.39] |
| wasmedge | 0 | 61.85 [33.1–70.93] | 619.7 [458.1–690.5] | 539.9 [534.2–554.8] | 12.39 [12.39–12.39] |
| zwasm | 0 | 28.54 [21.78–33.98] | 626.7 [527.5–714.3] | 532.8 [527.3–545.6] | 12.39 [12.39–12.39] |
| wasmz | 0 | 28.38 [21.7–33.89] | 203.1 [149.9–234.8] | 566.5 [565.9–582.9] | 12.39 [12.39–12.39] |
| tinywasm | 0 | 38.66 [31.23–47.43] | 211.1 [174.1–330] | 525.2 [520.8–542.1] | 12.39 [12.39–12.39] |
| pulley | 1 | 1725 [1158–1906] | 179.5 [104.5–192.1] | 530.8 [530.7–546.6] | 6.291 [6.291–6.291] |
| wamr | 1 | 52.42 [38.76–58.54] | 135.7 [100.6–152.4] | 512.7 [512.4–529.7] | 6.291 [6.291–6.291] |
| wasm3 | 1 | 1.98 [1.38–2.833] | 110.5 [92.85–124.2] | 515.3 [515–533.5] | 6.291 [6.291–6.291] |
| wasmedge | 1 | 65.24 [56.62–76.5] | 767.6 [575.4–917.5] | 529.4 [529.2–529.6] | 6.291 [6.291–6.291] |
| zwasm | 1 | 26.05 [20.37–29.51] | 838.1 [741.5–1020] | 522.5 [522.3–540.6] | 6.291 [6.291–6.291] |
| wasmz | 1 | 24.45 [18.96–34.65] | 235.2 [126.6–258.2] | 554.8 [554.4–571.6] | 6.291 [6.291–6.291] |
| tinywasm | 1 | 41.24 [38.51–68.52] | 254.9 [160.9–313.9] | 515.7 [512.7–531.6] | 6.291 [6.291–6.291] |

Scene 0: 1 distinct all-frames hash(es) across runtimes and reps (f9fa7399f9cbc21b); 1 distinct final-texture hash(es) (d8670619b1c3d306).
Scene 1: 1 distinct all-frames hash(es) across runtimes and reps (a4d95f880db7a22b); 1 distinct final-texture hash(es) (33e4bacd203e3c72).
<!-- /T:e2e-m4 -->

### iPhone XS Max (A12) E-cores

One launch per (runtime, scene), N=10.

<!-- T:e2e-iphonexs -->
**iPhone XS E-cores: frame rate and frame time**

| runtime | build | scene | FPS mean | FPS median | frame p50 ms | p95 | p99 | guest ms/frame | encode ms | GPU wait ms | E-share (min) | runs |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| pulley | simd128 | 0 | 4.186 [4.15–4.339] | 4.442 [4.41–4.546] | 225.1 [220–226.8] | 307.9 [296.2–315.9] | 322.9 [307.6–330.6] | 196.9 [188.3–199.1] | 4.174 [4.14–4.211] | 37.82 [37.54–38.43] | 0.998 | 10/10 |
| wamr | simd128 | 0 | 5.569 [5.537–5.598] | 5.755 [5.731–5.836] | 173.8 [171.3–174.5] | 233.5 [228.8–235.7] | 241 [237.9–243] | 137.5 [136.2–138.6] | 4.204 [4.127–4.25] | 37.87 [37.69–39.21] | 0.997 | 10/10 |
| wasm3 | scalar | 0 | 7.008 [6.888–7.048] | 7.183 [7.037–7.245] | 139.2 [138–142.1] | 175.9 [175.5–178.2] | 181.7 [179.9–188.3] | 100.9 [99.88–103.1] | 4.245 [4.159–4.26] | 37.77 [37.54–37.85] | 0.997 | 10/10 |
| wasmedge | simd128 | 0 | 1.106 [1.099–1.109] | 1.197 [1.188–1.202] | 835.2 [832.1–841.9] | 1235 [1233–1245] | 1294 [1283–1313] | 861.5 [860–867.3] | 4.194 [4.138–4.229] | 37.98 [37.66–38.32] | 0.999 | 10/10 |
| zwasm | scalar | 0 | 1.241 [1.237–1.243] | 1.335 [1.331–1.351] | 749.4 [740.4–751.2] | 1091 [1089–1100] | 1140 [1136–1151] | 763.8 [762.4–766] | 4.216 [4.169–4.269] | 37.84 [37.42–38.05] | 0.999 | 10/10 |
| wasmz | scalar | 0 | 5.646 [5.384–5.678] | 5.872 [5.828–5.9] | 170.3 [169.5–171.6] | 224.2 [222.4–238.2] | 234.7 [227.7–460.1] | 135.1 [134.3–143.9] | 4.267 [4.247–4.347] | 37.72 [37.51–38.11] | 0.997 | 10/10 |
| tinywasm | simd128 | 0 | 2.756 [2.744–2.776] | 2.964 [2.949–2.974] | 337.4 [336.3–339.1] | 484.1 [480.8–491.8] | 509.4 [498.8–514.4] | 320.8 [318.1–322.7] | 4.154 [4.082–4.245] | 37.87 [37.57–38.06] | 0.999 | 10/10 |
| pulley | simd128 | 1 | 7.886 [7.824–8.094] | 8.899 [8.768–9.199] | 112.4 [108.7–114] | 179.9 [175.1–180.5] | 181.3 [179.1–182.7] | 113.2 [109.9–114.2] | 3.002 [2.973–3.049] | 10.62 [10.6–10.78] | 0.997 | 10/10 |
| wamr | simd128 | 1 | 10.11 [9.98–10.18] | 11.18 [11.04–11.37] | 89.44 [87.94–90.59] | 134.8 [133.9–136] | 137.3 [136.2–142.8] | 85.25 [84.63–86.53] | 3.026 [2.971–3.07] | 10.62 [10.57–10.65] | 0.996 | 10/10 |
| wasm3 | scalar | 1 | 12.77 [12.56–12.91] | 14.94 [14.41–15.22] | 66.95 [65.72–69.38] | 113.4 [112.1–115.8] | 115.8 [114.7–117.6] | 64.67 [63.85–66.01] | 2.99 [2.955–3.032] | 10.6 [10.58–10.63] | 0.995 | 10/10 |
| wasmedge | simd128 | 1 | 1.932 [1.924–1.938] | 2.181 [2.164–2.189] | 458.6 [456.9–462.2] | 745.9 [743.9–748.4] | 754.4 [752.3–757.4] | 503.9 [502.2–506.1] | 3.08 [3.065–3.161] | 10.63 [10.59–10.65] | 0.999 | 10/10 |
| zwasm | scalar | 1 | 2.132 [2.128–2.135] | 2.462 [2.393–2.47] | 406.2 [404.9–417.9] | 684.1 [683.2–686.1] | 692.4 [690.1–696.3] | 455.3 [454.6–456.3] | 3.112 [3.082–3.143] | 10.62 [10.6–10.66] | 0.999 | 10/10 |
| wasmz | scalar | 1 | 10.39 [10.3–10.45] | 12.07 [11.71–12.33] | 82.85 [81.11–85.42] | 139.4 [135.9–139.7] | 140.8 [139.5–157] | 82.62 [82.07–83.43] | 3.011 [2.951–3.069] | 10.63 [10.53–10.65] | 0.996 | 10/10 |
| tinywasm | simd128 | 1 | 4.894 [4.812–4.936] | 5.478 [5.38–5.546] | 182.6 [180.3–185.9] | 286 [283.8–287.6] | 289.3 [287.6–307.6] | 190.6 [189–194.1] | 3.014 [2.974–3.077] | 10.63 [10.6–10.69] | 0.998 | 10/10 |

**iPhone XS E-cores: startup and memory**

| runtime | scene | load ms | init ms | peak footprint MB | peak linear memory MB |
|---|---:|---:|---:|---:|---:|
| pulley | 0 | 2692 [2684–2704] | 254.9 [252.4–260.6] | 50.91 [50.86–51.23] | 12.39 [12.39–12.39] |
| wamr | 0 | 79.94 [78.32–84.6] | 201.6 [197.2–205.3] | 53.72 [53.33–53.84] | 12.39 [12.39–12.39] |
| wasm3 | 0 | 4.799 [4.483–8.522] | 181.2 [177.6–183.8] | 55.13 [54.9–55.23] | 12.39 [12.39–12.39] |
| wasmedge | 0 | 121.6 [119.2–129.1] | 1092 [1080–1108] | 67.84 [67.72–68.06] | 12.39 [12.39–12.39] |
| zwasm | 0 | 43.72 [41.94–45.26] | 1038 [1035–1046] | 232 [231.9–232.1] | 12.39 [12.39–12.39] |
| wasmz | 0 | 48.72 [47.28–73.26] | 363.3 [356.8–371.7] | 97.24 [97.04–97.35] | 12.39 [12.39–12.39] |
| tinywasm | 0 | 75.06 [72.11–83.21] | 413.3 [409.7–442.9] | 54.63 [54.41–54.92] | 12.39 [12.39–12.39] |
| pulley | 1 | 2702 [2676–2727] | 297.1 [293.5–299.8] | 41.26 [41.16–41.32] | 6.291 [6.291–6.291] |
| wamr | 1 | 82.22 [80.27–88.81] | 241.8 [238.6–250.7] | 44.12 [44.01–44.35] | 6.291 [6.291–6.291] |
| wasm3 | 1 | 4.705 [4.663–5.349] | 219.8 [215.7–227.4] | 44.12 [44.02–44.25] | 6.291 [6.291–6.291] |
| wasmedge | 1 | 121.2 [119.9–129] | 1454 [1449–1471] | 58.59 [58.46–58.74] | 6.291 [6.291–6.291] |
| zwasm | 1 | 43.77 [42.15–53.27] | 1460 [1454–1469] | 133.2 [133.1–133.3] | 6.291 [6.291–6.291] |
| wasmz | 1 | 47.05 [46.05–84.53] | 414.3 [407–426.3] | 86.7 [86.49–86.8] | 6.291 [6.291–6.291] |
| tinywasm | 1 | 75.15 [72.93–78.83] | 494.7 [487.9–504.3] | 43.66 [43.5–43.81] | 6.291 [6.291–6.291] |

Scene 0: 1 distinct all-frames hash(es) across runtimes and reps (f9fa7399f9cbc21b); 1 distinct final-texture hash(es) (d8670619b1c3d306).
Scene 1: 1 distinct all-frames hash(es) across runtimes and reps (a4d95f880db7a22b); 1 distinct final-texture hash(es) (33e4bacd203e3c72).
<!-- /T:e2e-iphonexs -->

### Reading the E2E

- **The guest dominates every frame.** GPU wait is ~4 ms (scene 0) and
  ~1.6 ms (scene 1) on the M4, and ~38 / 11 ms on the A12's GPU.
  Decoding plus encoding the command stream is 2-4 ms. The rest is the
  interpreter running femtovg, so frame rate ranks runtimes the way the
  MVP workloads do.
- **Order on both devices:** wasm3 (scalar build) first, then WAMR and
  wasmz within a few percent of each other (WAMR ahead on the M4, wasmz
  on the A12), then Pulley, tinywasm, and zwasm ≈ WasmEdge last. On the
  A12, scene 0 runs at 7.0 fps on wasm3, 5.6 on wasmz and WAMR, 4.2 on
  Pulley, 2.8 on tinywasm and 1.1-1.2 on zwasm and WasmEdge. wasm3 wins
  without SIMD: femtovg's hot paths (tessellation, flattening) are scalar
  float code, and simd128 auto-vectorization buys the SIMD builds
  little.
- **Startup (`load ms`):** Pulley compiles the 1 MB guest to Pulley
  bytecode with Cranelift at load. That takes 1.2-2.3 s on the M4
  E-cores and ~2.7 s on the A12, where the other runtimes take
  2-130 ms. For self-contained `.wasm` deployment this is a real
  first-launch cost. The in-guest SVG parse (`init ms`) is 0.1-0.8 s on
  the M4 and 0.2-1.5 s on the A12, slowest on WasmEdge and zwasm.
- **Peak memory:** on the M4 every runtime lands at 513-570 MB, but only
  ~8 MB exist before the guest loads. The host records the footprint
  after its GPU and renderer setup, and tinywasm's E2E runs in the PMU
  capture show 7.8-8.6 MB there. The ~510 MB appear once rendering
  starts, on the wgpu / Metal side, and are the same for every runtime.
  The runtime-attributable part is the difference between rows, ≤ 50 MB
  (wasmz highest). On the A12 the whole process peaks at 41-68 MB,
  except wasmz (87-97 MB) and zwasm (133-232 MB); one extra launch per
  scene (wasm3) shows 10.6-10.8 MB before the guest loads. Peak linear
  memory is the same everywhere: 12.4 MB (scene 0) and 6.3 MB
  (scene 1).
- **Correctness:** every run hashes identically (see the notes under
  each table), including the final texture on both GPUs.


## tinywasm PMU (M4 E-cores)

tinywasm is the runtime the follow-up work targets. It passes every core
Wasm 3.0 smoke module except legacy EH, it is safe Rust with no native
tier, and it runs at about 3× the fastest runtime's CPU time. This section
profiles it on the M4 Max's E-cores over every matrix workload and the
femtovg E2E. The same profile on the iPhone 12's E-cores, with the first
measured contributions, is in
[tinywasm-iphone12-2026-09-23.md](tinywasm-iphone12-2026-09-23.md).

**Events.** The M4 Max's CPU Counters instrument cannot take a manual
event list from the command line: xctrace's `--recording-options` only
accepts an empty `allEventsAndFormulas`, and every form of a manual list
is rejected. The captures therefore use the instrument's guided modes,
the fixed event sets that Apple's Recount framework defines for this SoC
(t6041). There is one capture per mode, each mode within the 8
configurable counters:

| mode | events |
|---|---|
| bottlenecks | useful / processing / delivery / discarded slots (`RETIRE_UOP`, `MAP_*`, `CORE_ACTIVE_CYCLE`) |
| discarded_sampling | `BRANCH_MISPRED_NONSPEC`, `BRANCH_COND_MISPRED_NONSPEC`, `ST_MEM_ORDER_VIOL_LD_NONSPEC` |
| discarded_indirect_sampling | `BRANCH_INDIR_MISPRED_NONSPEC`, `BRANCH_CALL_INDIR_MISPRED_NONSPEC`, `BRANCH_RET_INDIR_MISPRED_NONSPEC` |
| l1d_metrics | `L1D_CACHE_MISS_LD`, `L1D_CACHE_MISS_ST`, `L1D_CACHE_WRITEBACK`, `LD_UNIT_UOP`, `ST_UNIT_UOP` |
| instruction_address_translation_metrics | `L1I_CACHE_MISS_DEMAND`, `L1I_TLB_MISS_DEMAND`, `L1I_TLB_FILL`, `L2_TLB_MISS_INSTRUCTION`, `MMU_TABLE_WALK_INSTRUCTION`, `FETCH_RESTART` |
| data_address_translation_metrics | `L1D_TLB_ACCESS`, `L1D_TLB_MISS`, `L1D_TLB_FILL`, `L2_TLB_MISS_DATA`, `MMU_TABLE_WALK_DATA` |
| call_branch_instructions | `INST_BRANCH_INDIR`, `INST_BRANCH_CALL`, `INST_BRANCH_COND` |

**No L2/SLC and no prefetch events exist on the M4 core PMU.** Its kpep
database (`as6`, 104 events, 8 configurable counters) has no L2, SLC or
prefetcher event. L1D / L1I misses, TLB misses and page-table walks are
the proxies: a miss that goes past L1 shows up as a miss plus, when it
misses the L2 as well, as processing-bucket stalls.

**Attribution and capture.**

- `scripts/run-m4-pmu-pass.sh` with `RUNTIMES_LIST=tinywasm`, on the
  E-cluster under `taskpolicy -b`, never alongside a timing pass.
- Each capture covers every matrix case but sqlite3 (Pulley-only) with a
  100 ms timed window, or one measured pass of the E2E (31 frames).
- Every case runs on its own thread named `case:<id>`, and the E2E runs
  on `femtovg-e2e`. `scripts/pmu_summarize.py` reduces xctrace's
  per-thread table.
- Bucket shares average each interval's four ratios, weighted by the
  interval's cycles.
- Event rates are per 1000 instructions of the same capture's case (the
  process's retired instructions over the case). For the E2E the
  denominator is the whole process, but the `femtovg-e2e` thread has
  over 99 % of its cycles.
- Indirect branches per 1000 instructions is the interpreter's dispatch
  density. In tinywasm's `become` dispatch every handler ends in one
  indirect branch, so `instr / indir branch` approximates the
  instructions per dispatched op.
- The first four modes come from a pass over every runtime that was
  stopped once the scope narrowed to tinywasm; only tinywasm's captures
  are kept. The last three modes are a tinywasm-only rerun with the same
  binaries (`pmu/pmu-pass-info.txt` in the data directory).

<!-- T:pmu -->
**tinywasm: pipeline slots (% of slots), IPC and control side (events per 1k instructions)**

| case | useful | processing | delivery | discarded | IPC | instr / indir branch | indir mispred % | br mispred | cond mispred | fetch restart | e_share min |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| fib | 69.3 | 21.4 | 2.1 | 7.1 | 3.58 | 38.4 | 0.0 | 0.768 | 0.763 | 2.379 | 1.00 |
| fib_tail | 82.9 | 15.1 | 1.5 | 0.5 | 4.28 | 35.6 | 0.0 | 0.008 | 0.005 | 1.808 | 1.00 |
| factorial | 64.8 | 24.5 | 6.9 | 3.8 | 3.55 | 49.4 | 0.1 | 0.063 | 0.043 | 9.043 | 1.00 |
| sieve | 74.1 | 23.3 | 2.2 | 0.4 | 3.85 | 42.5 | 0.0 | 0.026 | 0.016 | 0.556 | 1.00 |
| crc32 | 73.2 | 25.1 | 1.4 | 0.3 | 3.76 | 37.9 | 0.0 | 0.018 | 0.007 | 4.273 | 1.00 |
| matmul_simd | 73.5 | 23.3 | 2.5 | 0.7 | 3.78 | 37.2 | 0.1 | 0.060 | 0.039 | 0.420 | 1.00 |
| matmul_fma | 71.6 | 25.0 | 2.6 | 0.9 | 3.70 | 43.3 | 0.1 | 0.081 | 0.054 | 5.355 | 1.00 |
| convolution | 68.8 | 26.7 | 3.0 | 1.6 | 3.56 | 41.9 | 0.6 | 0.139 | 0.016 | 3.200 | 1.00 |
| audio_dsp | 73.8 | 22.1 | 3.1 | 1.1 | 3.85 | 36.9 | 0.0 | 0.068 | 0.053 | 14.6 | 1.00 |
| bulk_memory | 75.1 | 21.3 | 3.1 | 0.4 | 3.93 | 35.2 | 0.0 | 0.031 | 0.021 | 6.159 | 1.00 |
| call_indirect | 54.3 | 30.1 | 3.2 | 12.3 | 2.92 | 55.2 | 9.7 | 1.799 | 0.024 | 8.719 | 1.00 |
| factorial.scalar | 68.6 | 20.2 | 6.7 | 4.5 | 3.93 | 39.2 | 0.0 | 0.337 | 0.328 | 1.750 | 1.00 |
| sieve.scalar | 71.8 | 24.9 | 2.4 | 0.9 | 3.88 | 40.2 | 0.1 | 0.020 | 0.013 | 0.938 | 1.00 |
| crc32.scalar | 71.6 | 25.5 | 2.1 | 0.8 | 3.79 | 38.0 | 0.1 | 0.034 | 0.024 | 4.303 | 1.00 |
| convolution.scalar | 69.6 | 27.6 | 2.2 | 0.6 | 3.70 | 45.6 | 0.0 | 0.014 | 0.006 | 1.409 | 1.00 |
| bulk_memory.scalar | 75.0 | 21.6 | 2.8 | 0.7 | 3.96 | 35.8 | 0.0 | 0.062 | 0.048 | 5.792 | 1.00 |
| xmrsplayer | 67.1 | 24.9 | 5.8 | 2.3 | 3.71 | 42.5 | 0.3 | 0.160 | 0.082 | 12.7 | 1.00 |
| vtable_mono | 64.7 | 32.7 | 2.2 | 0.4 | 3.33 | 61.5 | 0.0 | 0.011 | 0.005 | 5.343 | 1.00 |
| vtable_bi | 65.1 | 31.3 | 3.0 | 0.6 | 3.39 | 53.5 | 0.0 | 0.012 | 0.005 | 6.482 | 1.00 |
| vtable_poly4 | 65.8 | 30.9 | 2.7 | 0.7 | 3.44 | 51.9 | 0.0 | 0.018 | 0.006 | 9.605 | 1.00 |
| vtable_poly6 | 63.2 | 28.9 | 3.9 | 4.0 | 3.33 | 50.2 | 1.9 | 0.393 | 0.005 | 11.0 | 1.00 |
| graphql_as | 60.7 | 26.9 | 6.7 | 5.7 | 3.35 | 41.8 | 1.6 | 0.492 | 0.178 | 16.8 | 1.00 |
| tailcall_fsm | 71.8 | 21.2 | 3.6 | 3.4 | 3.84 | 41.8 | 0.8 | 0.381 | 0.190 | 3.280 | 1.00 |
| eh_parser_exnref | 59.3 | 29.7 | 4.3 | 6.6 | 3.12 | 52.5 | 2.5 | 0.828 | 0.349 | 8.936 | 1.00 |
| gc_trees | 46.1 | 44.0 | 3.8 | 6.1 | 2.52 | 111.9 | 5.6 | 0.980 | 0.488 | 7.578 | 1.00 |
| callref_dispatch | 56.5 | 26.1 | 4.4 | 13.0 | 3.02 | 47.9 | 8.1 | 1.695 | 0.004 | 5.560 | 1.00 |
| callref_dispatch.indirect | 54.8 | 28.2 | 3.9 | 13.1 | 2.89 | 49.4 | 8.8 | 1.780 | 0.004 | 5.817 | 1.00 |
| relaxed_dot | 63.8 | 32.6 | 1.5 | 2.1 | 3.25 | 53.0 | 1.2 | 0.241 | 0.011 | 1.702 | 1.00 |
| relaxed_madd | 72.0 | 25.6 | 1.7 | 0.7 | 3.60 | 39.1 | 0.0 | 0.014 | 0.006 | 8.231 | 1.00 |
| mem64_chase | 49.9 | 48.6 | 1.2 | 0.3 | 2.65 | 35.0 | 0.0 | 0.005 | 0.002 | 12.1 | 1.00 |
| mem64_chase.mem32 | 55.2 | 42.9 | 1.7 | 0.2 | 2.87 | 34.1 | 0.0 | 0.005 | 0.002 | 1.527 | 1.00 |
| multimem_transform | 76.2 | 22.4 | 1.2 | 0.2 | 3.93 | 34.9 | 0.0 | 0.009 | 0.004 | 7.570 | 1.00 |
| multimem_transform.single | 76.1 | 21.5 | 2.0 | 0.4 | 3.99 | 34.2 | 0.0 | 0.010 | 0.004 | 7.779 | 1.00 |
| extconst_init | 73.0 | 21.3 | 4.2 | 1.6 | 3.94 | 53.3 | 0.4 | 0.092 | 0.031 | 10.6 | 1.00 |
| extconst_init.mvp | 73.7 | 22.0 | 3.6 | 0.7 | 3.77 | 43.8 | 0.1 | 0.041 | 0.031 | 7.415 | 1.00 |
| graphql_porf | 52.4 | 31.8 | 7.7 | 8.1 | 3.92 | 57.2 | 2.5 | 0.665 | 0.228 | 12.7 | 1.00 |
| femtovg_e2e.scene0 | 61.6 | 26.7 | 5.0 | 6.8 | 3.30 | 42.6 | 2.7 | 0.853 | 0.215 | 13.8 | 1.00 |
| femtovg_e2e.scene1 | 57.6 | 26.4 | 6.7 | 9.3 | 3.17 | 43.4 | 3.8 | 1.201 | 0.308 | 15.2 | 0.99 |
| **median, matrix cases** | **69.1** | **25.3** | **2.9** | **0.9** | **3.70** | **42.2** | **0.1** | **0.062** | **0.019** | **5.988** | **1.00** |

**tinywasm: memory side, events per 1k instructions**

| case | L1D ld miss | L1D st miss | L1I miss | iTLB miss | iTLB L2 miss | i-walk | dTLB miss | dTLB L2 miss | d-walk |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| fib | 0.038 | 0.021 | 0.003 | 0.013 | 0.032 | 0.033 | 0.015 | 0.003 | 0.003 |
| fib_tail | 0.032 | 0.044 | 0.004 | 0.008 | 0.009 | 0.010 | 0.011 | 0.002 | 0.003 |
| factorial | 0.277 | 0.524 | 0.031 | 0.062 | 0.171 | 0.173 | 0.077 | 0.025 | 0.028 |
| sieve | 0.112 | 0.071 | 0.007 | 0.023 | 0.018 | 0.019 | 0.013 | 0.003 | 0.004 |
| crc32 | 0.104 | 0.120 | 0.004 | 0.015 | 0.011 | 0.012 | 0.012 | 0.002 | 0.003 |
| matmul_simd | 0.127 | 0.048 | 0.007 | 0.016 | 0.020 | 0.021 | 0.014 | 0.004 | 0.005 |
| matmul_fma | 0.244 | 0.063 | 0.007 | 0.019 | 0.018 | 0.019 | 0.016 | 0.004 | 0.005 |
| convolution | 0.139 | 0.059 | 0.008 | 0.040 | 0.023 | 0.024 | 0.015 | 0.004 | 0.005 |
| audio_dsp | 0.063 | 0.004 | 0.006 | 0.019 | 0.147 | 0.148 | 0.012 | 0.003 | 0.005 |
| bulk_memory | 4.476 | 2.284 | 0.005 | 0.018 | 0.016 | 0.016 | 0.020 | 0.004 | 0.005 |
| call_indirect | 0.161 | 0.066 | 0.011 | 0.053 | 0.043 | 0.045 | 0.026 | 0.005 | 0.006 |
| factorial.scalar | 0.318 | 0.627 | 0.026 | 0.035 | 0.141 | 0.144 | 0.072 | 0.014 | 0.017 |
| sieve.scalar | 0.101 | 0.066 | 0.008 | 0.027 | 0.022 | 0.023 | 0.025 | 0.005 | 0.007 |
| crc32.scalar | 0.079 | 0.050 | 0.005 | 0.018 | 0.014 | 0.015 | 0.016 | 0.004 | 0.005 |
| convolution.scalar | 0.063 | 0.050 | 0.003 | 0.011 | 0.010 | 0.011 | 0.019 | 0.004 | 0.006 |
| bulk_memory.scalar | 4.230 | 2.108 | 0.005 | 0.018 | 0.016 | 0.017 | 0.029 | 0.006 | 0.007 |
| xmrsplayer | 0.516 | 0.218 | 0.028 | 0.073 | 0.140 | 0.142 | 0.098 | 0.015 | 0.017 |
| vtable_mono | 0.095 | 0.061 | 0.005 | 0.011 | 0.016 | 0.017 | 0.024 | 0.005 | 0.007 |
| vtable_bi | 0.070 | 0.039 | 0.009 | 0.014 | 0.031 | 0.032 | 0.031 | 0.006 | 0.008 |
| vtable_poly4 | 0.087 | 0.038 | 0.012 | 0.021 | 0.037 | 0.038 | 0.031 | 0.007 | 0.009 |
| vtable_poly6 | 0.253 | 0.053 | 0.008 | 0.021 | 0.109 | 0.110 | 0.114 | 0.091 | 0.093 |
| graphql_as | 1.817 | 0.232 | 0.030 | 0.123 | 0.157 | 0.159 | 0.083 | 0.032 | 0.034 |
| tailcall_fsm | 0.111 | 0.030 | 0.004 | 0.014 | 0.013 | 0.013 | 0.029 | 0.007 | 0.009 |
| eh_parser_exnref | 7.489 | 0.616 | 0.015 | 0.056 | 0.058 | 0.060 | 0.053 | 0.011 | 0.013 |
| gc_trees | 3.132 | 0.339 | 0.015 | 0.043 | 0.230 | 0.232 | 0.126 | 0.024 | 0.029 |
| callref_dispatch | 0.160 | 0.055 | 0.005 | 0.097 | 0.024 | 0.025 | 0.410 | 0.383 | 0.386 |
| callref_dispatch.indirect | 0.159 | 0.051 | 0.005 | 0.093 | 0.020 | 0.021 | 0.381 | 0.384 | 0.386 |
| relaxed_dot | 0.180 | 0.061 | 0.005 | 0.024 | 0.015 | 0.015 | 0.042 | 0.008 | 0.011 |
| relaxed_madd | 0.221 | 0.063 | 0.005 | 0.015 | 0.013 | 0.014 | 0.028 | 0.007 | 0.008 |
| mem64_chase | 0.937 | 0.373 | 0.001 | 0.008 | 0.008 | 0.008 | 0.644 | 0.187 | 0.190 |
| mem64_chase.mem32 | 0.948 | 0.390 | 0.002 | 0.009 | 0.008 | 0.009 | 0.544 | 0.216 | 0.219 |
| multimem_transform | 0.174 | 0.028 | 0.002 | 0.008 | 0.007 | 0.008 | 0.012 | 0.003 | 0.004 |
| multimem_transform.single | 0.193 | 0.044 | 0.002 | 0.007 | 0.006 | 0.007 | 0.013 | 0.003 | 0.004 |
| extconst_init | 6.319 | 0.562 | 0.017 | 0.017 | 0.235 | 0.236 | 0.135 | 0.020 | 0.025 |
| extconst_init.mvp | 9.168 | 1.997 | 0.029 | 0.019 | 0.064 | 0.065 | 0.122 | 0.020 | 0.023 |
| graphql_porf | 2.434 | 4.680 | 0.037 | 0.110 | 0.168 | 0.171 | 1.822 | 0.082 | 0.084 |
| femtovg_e2e.scene0 | 1.223 | 0.683 | 0.046 | 0.135 | 0.100 | 0.105 | 0.194 | 0.058 | 0.062 |
| femtovg_e2e.scene1 | 1.470 | 0.522 | 0.054 | 0.150 | 0.122 | 0.128 | 0.211 | 0.074 | 0.077 |
| **median, matrix cases** | **0.177** | **0.063** | **0.007** | **0.019** | **0.021** | **0.022** | **0.029** | **0.006** | **0.008** |
<!-- /T:pmu -->

**Reading.**

- **tinywasm is instruction-bound on the M4 as well.** In the median
  matrix case it retires useful work in 69 % of pipeline slots at an IPC
  of 3.7, and loses 0.9 % of slots to wrong-path work and 3 % to
  instruction delivery. L1I misses stay under 0.06 and iTLB misses under
  0.16 per 1k instructions on every row, so its handlers fit the
  E-core's instruction cache. The iPhone 12 profile had the same shape
  (62-90 % useful on the A14 E-cores).
- **The cost is the number of instructions per op.** The median case
  runs 42 instructions per indirect branch. The M4 counts returns
  separately, and every `become` handler ends in one indirect branch, so
  that is about 42 instructions per dispatched op. Over the 17 cases
  every runtime runs, tinywasm retires 2.55× WAMR's instructions per
  call on the M4 and 2.69× on the A12 (the timing passes' rusage
  counters; per rep and case as `insns_per_iter` in `m4-matrix.csv` and
  `iphonexs-matrix.csv` in the data directory). Its IPC is
  higher than WAMR's, so its CPU time is 1.97× and 2.17× WAMR's.
- **Where the pipeline does stall, the workload explains it.**
  - Processing reaches 43-49 % on the `mem64_chase` pair (256K dependent
    loads over 64 MiB).
  - Processing reaches 44 % on `gc_trees`, which runs 112 instructions
    per indirect branch: allocation work outside the dispatch loop.
  - Discarded reaches 12-13 % on `call_indirect` and the two
    `callref_dispatch` rows. Their call targets change from call to call,
    and 8-10 % of indirect branches mispredict.
  - Among the matrix rows, L1D load misses pass 1 per 1k instructions
    only on bulk memory, the EH parser, `gc_trees`, the two graphql
    validators and the extended-const rows (a fresh instance per
    sample).
- **The femtovg frame** looks like the matrix: 58-62 % useful and an IPC
  of 3.2-3.3. It has more discarded slots (7-9 %), presumably because
  femtovg's data-dependent control flow reaches the interpreter as less
  predictable dispatch: 3-4 % of indirect branches mispredict.

### Bottleneck and next experiments

The bottleneck is the instruction count per wasm op, not stalls, so the
levers are the code paths that the iPhone 12 profile located:
- the dispatch sequence (~12 instructions);
- `Vec::push` growth inlined into the pushing handlers;
- memory re-resolution on every load and store;
- calls that touch all three value stacks.

- **Measured: a stack of three PRs** on `next` in the fork
  ([#1](https://github.com/rebeckerspecialties/tinywasm/pull/1),
  [#2](https://github.com/rebeckerspecialties/tinywasm/pull/2),
  [#3](https://github.com/rebeckerspecialties/tinywasm/pull/3)). They cut
  cycles by 8.0 % on the iPhone 12 E-cores (geomean of 16 rows,
  interleaved launches):
  - growing the value stack out of line, −4.1 %;
  - inlining the fused binop / compare helpers, −1.3 % more;
  - reserving each function's operand stack on entry, −2.8 % more.

  The reservation was this report's next experiment. It keeps the
  whole gain of the no-growth upper bound (−0.1 % against it), at the
  cost of a public `WasmFunction` field and a new archive version, so
  upstream it starts as an issue. All three pass tinywasm's CI matrix.
  The A/B, the checks and the patches are in the
  [iPhone 12 doc](tinywasm-iphone12-2026-09-23.md).
- **Next**, in the order of the iPhone 12 doc:
  - a memory-0 fast path for loads and stores (`I32Load8U` alone is 35 %
    of convolution's time on the A14);
  - carrying the instruction slice through the `become` handlers;
  - lighter calls and returns.

## Bugs and limits found

Correctness is checked on every row: a runtime that returns anything
other than the cross-runtime consensus fails the row. Every runtime
agrees on every case it runs, with the exceptions below. Found in this
refresh, none of them fixed here:

**wasmz v0.1.4**
- `extconst_init` returns -1669113347 instead of -1152068691: extended
  constant expressions evaluate wrongly. The smoke module gets 40 instead
  of 42 for `40 + 1·2`, so the `i32.mul` inside the initializer is lost.
- `extconst_init.mvp` (plain constants) returns 1844351884 on the second
  and later instantiations of the same module. The Python reference
  reproduces that exact value with the active data segments zeroed, so
  the segments are not re-applied when a module is instantiated again.
- `callref_dispatch` traps "null reference dereference": the typed
  table's `elem` segment (expression form, `ref.func`) is not applied,
  so `call_ref` meets null.
- `catch_ref` / `throw_ref` fail with a stack underflow. Plain
  `try_table` + `catch` works.
- The auto-vectorized `sieve` and `crc32` builds segfault inside wasmz
  (listed in `KNOWN_CRASHES`, so they are N/A rather than a crash). The
  scalar builds run.
- `graphql-validation (AS)` traps `unreachable`, which no other runtime
  does.
- The C API cannot return multi-value results, so Porffor's `m() ->
  (f64, i32)` cannot be called (a harness limit through wasmz's C API).

**zwasm v2.7.0**
- **GC structs are never reclaimed**, and the GC heap grows far faster
  than the allocation rate: 124 MB for 31K 24-byte structs. The process
  segfaults at ~2.3 GB, so `gc_trees` is N/A (`KNOWN_CRASHES`).
- Memory per instantiation is not returned when the instance is
  deleted (the harness deletes each instance and reuses the store). In a
  process of its own, `extconst_init` peaks at 2.8 GB over 1663
  instantiations and still holds 0.96 GB after the case. On the iPhone
  the two extended-const rows peak at 1.0 and 1.5 GB RSS.
- `return_call` does not run in constant space: `tailcall_fsm`'s
  one-frame-deep machine still holds 158 MB after 47 calls, about 50
  bytes per `return_call` (117 MB RSS on the iPhone). The likely cause
  is in the source. The interpreter's tail-call trampoline allocates each
  callee's locals from the instance's runtime allocator, which is a
  per-instance `std.heap.ArenaAllocator` (`src/api/instance.zig`). It
  then frees the previous callee's locals (`src/interp/dispatch.zig`),
  and an arena only reclaims its latest allocation.
- The interpreter has no SIMD-128 (v128 ops trap `unreachable`). All
  SIMD-canonical rows are N/A; the scalar twins and the scalar femtovg
  guest run.
- On the iPhone, graphql-validation (Porffor) gets the app
  **jetsam-killed** even in a launch of its own (it peaks at 614 MB in a
  process of its own on the M4), and xmrsplayer peaks at ~1.16 GB
  RSS.
- The WASI 0.3 CLI does not finish while host stdin is open, even when
  the guest never reads it (run it with `</dev/null`).

**WasmEdge 0.17.2-rc.3**
- GC structs are never collected: `gc_trees` grows the heap by ~15 MB
  per call. The row still completes within the window.
- No legacy EH ("illegal opcode"); exnref works.
- (Known, arm64_32 only) `WasmEdge_VMInstantiate` BRKs on
  arm64_32-apple-watchos. The adapter returns a clean error there.

**WAMR (main + our patches)**
- No exnref (`try_table` is opcode 0x1f: "unsupported opcode 1f"),
  memory64 or multi-memory in fast-interp. GC and typed function
  references are off in the shipped build (see the feature matrix).
- Legacy EH limits, both reported cleanly: an exception payload cannot
  cross a function boundary (patch 0014 traps), and a `br` to a loop
  entry from inside a try region is rejected at load (0015).

**wasm3 v0.9.0**
- No SIMD, exceptions, GC, typed references, memory64 or multi-memory.
  Its v128 errors are indirect ("compiling function underran the
  stack", "incorrect type on stack", "unknown label"): v0.9 parses
  v128 locals but not the ops.

**tinywasm 0.11.0**
- Legacy `try` / `catch` is rejected by its validator configuration.
  Everything else passes.

**Pulley (wasmtime v49)**
- No legacy `try` in Cranelift's Pulley backend. exnref works.
- iOS refuses two 4 GiB virtual reservations in one process, so
  wasmtime's default 4 GiB memory reservation fails to instantiate a
  module with a GC heap or a second memory ("mmap failed to reserve
  0x100000000"). The harness configures 256 MiB memory and GC-heap
  reservations on every platform.


## Reproducing

```sh
git clone --recurse-submodules https://github.com/rebeckerspecialties/wasm-benchmark.git
cd wasm-benchmark && git checkout runtime-refresh-2026-09
./scripts/setup.sh
./scripts/build-workloads.sh && ./scripts/build-femtovg-guest.sh && ./scripts/build-cm-async-bench.sh
for r in wamr wasm3 wasmedge zwasm wasmz; do ./scripts/build-$r.sh macos; ./scripts/build-$r.sh ios; done
./scripts/build-host-cli.sh --bin run_matrix
./scripts/build-host-cli.sh --bin run_femtovg_e2e --features femtovg-e2e
./scripts/build-host-cli.sh --bin rusage_exec
./scripts/build-cm-tools.sh
./scripts/build-lib.sh ios && (cd apps && xcodebuild -project WasmBenchmark.xcodeproj \
  -scheme WasmBenchmarkIOS -configuration Release -destination "generic/platform=iOS" \
  -derivedDataPath build/DerivedData-xs-ios -allowProvisioningUpdates build)
# install apps/build/DerivedData-xs-ios/.../WasmBenchmarkIOS.app with devicectl, then:
./scripts/feature-matrix.sh out/feature-matrix
./scripts/run-m4-pass.sh out/r/m4
./scripts/run-device-pass.sh out/r/iphonexs
E2E=0,1 N=5 ./scripts/run-device-pass.sh out/r/iphonexs-e2e
RUNTIMES_LIST=tinywasm ./scripts/run-m4-pmu-pass.sh out/r/pmu
./scripts/summarize-pass.py out/r docs/<data-dir>     # derived tables go to out/r/report-tables
./scripts/build-report.py docs/<report>.md out/r/report-tables
```
