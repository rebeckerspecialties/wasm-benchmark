# AGENTS.md — wasm-benchmark project guide

This is a WebAssembly-interpreter benchmark harness targeting Apple Silicon
deployment platforms (App-Store-eligible: no JIT, no MAP_JIT, no
copy-and-patch) — primarily arm64_32-apple-watchos, aarch64-apple-ios,
aarch64-apple-tvos, and aarch64-apple-darwin. The harness compares
**Pulley** (wasmtime's interpreter), **WAMR** (WebAssembly Micro
Runtime fast-interp), **wasm3** (the m3 pure C interpreter),
**WasmEdge** (`WASMEDGE_USE_LLVM=OFF` with the 27-patch Apple-mobile
enablement stack), and **zwasm** (clojurewasm's Zig runtime built
`-Djit=false`), and was set up to drive a per-table-mutability
optimization stack upstream (see
[PR #2](https://github.com/rebeckerspecialties/wasmtime/pull/2)).

## Next session starting points

Pick this up cold without re-deriving state:

- **Working branch**: `claude/wasm-benchmark-continue-wuuPd` on
  `rebeckerspecialties/wasm-benchmark`. CI workflow `build.yml`
  reproduces a clean checkout end-to-end (last green sha: `2fc46a6`).
- **Latest wasmtime fork branch**: `accurate-graphql-needs-legacy-
  exceptions` (one commit on top of PR #4 → PR #2 → upstream main).
  Submodule `wasmtime/` pins this branch.
- **WAMR fork branches**:
  - `feat/legacy-eh-fast-interp-throw` — landed throw-only patch,
    extracted as `patches/wasm-micro-runtime/0001-feat-interpreter-
    legacy-exception-handling-throw-only-for-fast-interp.patch`,
    filed as fork [PR #1](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/1).
  - `feat/legacy-eh-fast-interp-full` — successor branch (created
    2026-05-17 from upstream `cd390ea0`, throw-only patch in working
    tree as the baseline). Adds same-function `try`/`catch`/
    `catch_all`/`rethrow`/`delegate` dispatch. Replaces the
    throw-only patch in `patches/wasm-micro-runtime/` with a
    4-commit series when complete. **Locked design lives in the
    `### Open follow-up — WAMR fast-interp legacy exception
    handling (full spec)` section below.**
- **Integration test wasm for the WAMR full-EH PR**:
  [`workloads/graphql-validation-porf-accurate.wasm`](workloads/graphql-validation-porf-accurate.wasm)
  (150 KB, **1 `try` + 1 `catch 0` + 2 `throw 0`** — the throws are
  in a callee, the catch is in the caller's try body). Porffor-
  compiled JS that mirrors real graphql-js's `GraphQLError extends
  Error` hierarchy with `try { visit(...) } catch (e) { if
  (e !== abortObj) throw e; }`. With the throw-only patch in place,
  this currently fails at runtime with
  `Exception: unsupported opcode` — the loader auto-emits
  `WASM_OP_CATCH` into the fast IR ([`wasm_loader.c:11974`](wasm-micro-runtime/core/iwasm/interpreter/wasm_loader.c#L11974)
  emit_label runs for every opcode; only TRY skip_labels it at
  line 12278-12281), and our existing diff routes CATCH to the
  "unsupported opcode" handler in [`wasm_interp_fast.c:1869-1888`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_fast.c#L1869).
  The catch handler is **not a no-op** — it does tag-pattern
  dispatch, calls `call 104`, compares against global 53, and may
  re-throw, so skipping it would silently corrupt results.
- **Two adjacent gaps** also surfaced 2026-05-17, both
  **explicitly NOT** part of the WAMR full-EH PR:
  - `workloads/sqlite3.wasm` doesn't run on WAMR because the
    benchmark harness never wired up its 10 host imports (8 WASI
    `fd_*`/`environ_*` + 2 Sightglass `bench.*`). The wasm is
    plain MVP (no SIMD/EH/atomics/bulk/threads); fast-interp
    loads it fine, instantiate fails with `failed to call
    unlinked import function (wasi_snapshot_preview1,
    environ_sizes_get)`. **Local-only follow-up**: add
    `wasm_runtime_register_natives` stubs in
    `crates/benchmark-core/src/wamr.rs` + new `run_sqlite3_wamr`
    bin. Decided 2026-05-17 to hand-roll standalone WAMR stubs
    rather than share with the wasmtime path.
  - `workloads/matmul_fma.wasm` uses `f32x4.relaxed_madd` (twice).
    WAMR's `HANDLE_OP(WASM_OP_SIMD_PREFIX)` switch in
    [`wasm_interp_fast.c:5929-7478`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_fast.c#L5929)
    enumerates standard SIMD sub-opcodes 0x00-0xff explicitly;
    relaxed sub-opcodes (0x100..) hit the default
    `"unsupported SIMD opcode"` arm at line 7474. No
    `WAMR_BUILD_RELAXED_SIMD` flag exists upstream; only a dormant
    `WASM_FEATURE_RELAXED_SIMD` bit at `aot_runtime.h:32`.
    **Future WAMR-fork PR-2**: 3-commit series — enum extension,
    runtime cases in the SIMD switch, default-off cmake flag.
- **Open fork PRs** awaiting upstream review:
  - [`rebeckerspecialties/wasm3#1`](https://github.com/rebeckerspecialties/wasm3/pull/1) — v128 opaque slot
  - [`rebeckerspecialties/wasm-micro-runtime#1`](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/1) — throw-only legacy EH
  - [`rebeckerspecialties/wasmtime#2`](https://github.com/rebeckerspecialties/wasmtime/pull/2) — table-mutability tracking (11 commits)
  - [`rebeckerspecialties/wasmtime#4`](https://github.com/rebeckerspecialties/wasmtime/pull/4) — Pulley fusion stack (12 commits)
- **Hot tools to remember**:
  - `./scripts/run_fusion_n10.sh` (N=10 launcher with EXPECTED_LINES)
  - `./scripts/aggregate_4way.py <n10dir>` (cross-device wallclock table)
  - `./scripts/run_per_workload_pmu.sh` / `run_m4_per_workload_pmu.sh`

## Project goal & current state

**Motivation**: A WatchOS audio app (incumbent: WasmEdge with custom
patches) runs at ~25 % CPU on iPhone XS for a WASI-audio music
player; S8 Apple Watch is 2–3× slower per core. Dispatch density on
narrow E-cores is the binding performance constraint. wasmtime's
Pulley interpreter is the only App-Store-legal runtime in that space.
Goal: identify and ship dispatch optimizations that move the needle
on these targets.

**Status** (2026-05-15):
- PR #2 (`table-mutability-tracking` on the wasmtime fork) is the
  predicate-factory branch — 11 commits, 2227 + 16 tests pass.
- **PR #4** ([`claude/pulley-fusion-xband-brif`](https://github.com/rebeckerspecialties/wasmtime/pull/4))
  stacks 11 fusion commits (phases 1–4) on top of PR #2's branch tip.
  Full cross-device measurements: iPhone 12 (A14 Icestorm) + iPhone XS
  (A12 Tempest) + Apple Watch SE2 (S8) + M4 host E-core.
- An IC investigation in `pulley-call-indirect-ic*` branches was
  abandoned after PMU evidence on `vtable_dispatch.wasm` showed the
  IC's back-end savings cancel against new front-end / mispredict
  pressure on Apple silicon E-cores. See
  `out/exp-c-device/ic/ARCHIVED-BRANCH-SHAS.md` for recovery info.
- Opcode fusion in Pulley landed across four layered phases on the
  `claude/pulley-fusion-xband-brif` wasmtime branch (12 commits —
  11 fusion + 1 correctness fix from review):
  - **Phase 1** (`BandBrIf`, `xband_s8 + br_if`): measured 2026-05-14
    — wallclock flat in isolation, Discarded +7.87 %. Superseded by
    phase 2 at the same call site; stays as fallback when phase 2's
    continuation-load pattern doesn't match.
  - **Phase 2** (`FuncrefDispatch`, `brif + 2 xloads`): measured
    2026-05-14 — call_indirect wallclock **−5.0 %** vs baseline (the
    first measurable wallclock win past PR #2's c1-7 ceiling), PMU
    Discarded −1.74 % vs baseline / −8.91 % vs phase 1.
  - **Phase 3** (`BandFuncrefDispatch`, `band + brif + 2 xloads`):
    measured 2026-05-15 — PMU total cycles **−4.31 %** vs phase 2 /
    −0.96 % vs baseline; Discarded **−7.33 %** vs phase 2 /
    −8.95 % vs baseline; wallclock within noise of phase 2 at N=10
    but per-rep range tightens by ~half (call_indirect 1.34 → 0.74
    ms). Dispatch tail at the call_indirect lazy-init site shrinks
    from baseline's 5 Pulley dispatches to **2**.
  - **Phase 4** (`PulleyCallIndirect` + `call_indirect{1,2,3,4}`):
    measured 2026-05-15 on iPhone 12 + iPhone XS Max + Watch SE2 + M4
    host. Mirrors `Inst::Call`'s direct-call arg-bundling for
    `Inst::IndirectCall`: new `PulleyCallIndirect { target, args }`
    payload + four new Pulley ops that combine `xmov xN, argN` ABI
    fixups with the indirect call into a single dispatch. iPhone 12
    A14 Icestorm wallclock vs phase 3 (N=10, vtable suite):
    **vtable_poly4 −8.94 %, vtable_bi −6.71 %, vtable_poly6 −3.72 %**.
    iPhone XS A12 Tempest recovers phase-3's call_indirect regression
    (−4.77 % vs phase 3, back to baseline parity). Watch SE2 S8
    matches A14 on vtable suite (**vtable_bi −7.68 %, poly4 −4.62 %,
    poly6 −4.86 %** vs phase 3) — the actual deployment target.
    Dispatch tail shrinks to **1 fused op + 1 call_indirectN op per
    call_indirect lazy-init site** (from 5 in baseline / 2 in phase 3).
    See `docs/four-way-baseline-phase3-phase4-wamr.md` for the full
    cross-device wallclock matrix + per-microarch analysis + phase-5
    candidates.

  Discarded improves at every phase transition despite adding 4 new
  opcodes each time — the "larger fused ops consolidate predictor
  entries" hypothesis from phase 2's writeup continued to hold
  through phase 3. The per-new-opcode-family predictor cost is NOT
  linear; iPhone 12 Icestorm's pattern-history table actually
  benefits from fewer, larger op handlers. Phase 4's wins come
  from a different mechanism: pure dispatch-count reduction in the
  callee-vmctx ABI move.

  Test totals: 2237 / 2237 disas + 16 / 16 environ + 7 / 7 pulley
  fusion integration. Differential fuzz (`cargo fuzz run
  differential --no-default-features`, `ALLOWED_ENGINES=
  pulley,wasmtime`) ran ~21 min with 0 crashes / 0 Pulley-vs-native
  divergences.

  See `docs/opcode-fusion-band-brif.md`,
  `docs/opcode-fusion-funcref-dispatch.md`,
  `docs/opcode-fusion-band-funcref-dispatch.md`, and
  `docs/four-way-baseline-phase3-phase4-wamr.md` for the four
  measurement closeouts. Cross-runtime context vs WAMR is in
  `docs/three-way-baseline-phase3-wamr.md` (iPhone 12 PMU) and
  `docs/cross-runtime-pulley-vs-wamr.md`.

## Toolchain pinning

Two pinned toolchains, both LLVM-bitcode compatible with Xcode 26
for cross-language whole-program LTO:

| Toolchain | Use for | rustc |
|---|---|---|
| **`1.94.1`** (stable, current default) | analysis (`cargo tree`, etc.); host triples (`aarch64-apple-darwin`); wasm32 workloads | `1.94.1 (29ea6fb6a 2026-03-24)` |
| **`nightly-2026-01-25`** | actually compiling for `arm64_32-apple-watchos` (Tier-3) via `-Z build-std`; also used for `aarch64-apple-ios` so we get `--cfg=pulley_tail_calls` (the `become`-based dispatch — the stable LLVM-TCO variant stack-overflows on `convolution`) | `1.95.0-nightly (f134bbc78 2026-01-24)` |

`scripts/build-workloads.sh` has `TOOLCHAIN=1.93.1` baked in but
falls through to the default cargo when that toolchain isn't
installed — wasm32-unknown-unknown is Tier-1, so 1.94.1 works fine.

**Rustup targets needed**:
- `aarch64-apple-darwin` (default on M-series Macs)
- `aarch64-apple-ios` (Tier 2, shipped std)
- `wasm32-unknown-unknown` (for `workloads-rs/*.rs` → `workloads/*.wasm`)
- `wasm32-wasip1` (for wasmtime's own `cargo test --test disas` — used
  by `crates/test-programs/artifacts/build.rs`; install with
  `rustup target add wasm32-wasip1`)
- `aarch64-apple-tvos` and `arm64_32-apple-watchos` are Tier-3 — no
  rustup target install; the nightly build-std path provides std for
  both (see `scripts/build-lib.sh`'s `tvos` / `watchos` arms).

**Footgun**: `rustup run nightly` resolves to a newer dated nightly
(e.g. `1.97.0-nightly`) that is **not** LLVM-bitcode-compatible with
Xcode 26. Always invoke nightly explicitly as `rustup run
nightly-2026-01-25`.

## Pulley dispatch loop selection (critical for perf)

Pulley ships three dispatch loop implementations in
`wasmtime/pulley/src/interp/`:

| mode | flag | safety | use |
|---|---|---|---|
| 1. Default `match` in `loop {}` | none | safe (stable Rust) | baseline; ~30-50 % slower than (3) on narrow E-cores |
| 2. LLVM-best-effort TCO | `--cfg=pulley_assume_llvm_makes_tail_calls` | **UNSAFE** — stack-overflows on `convolution` workload | do not use |
| 3. nightly `become` guaranteed TCO | `--cfg=pulley_tail_calls` | safe (requires nightly) | **standard** |

`scripts/build-lib.sh` sets `--cfg=pulley_tail_calls` for every
target. Measured wins vs default-loop dispatch: M4 22-34 %, iPhone XS
A12 33-46 %, Apple Watch SE2 S8 30-50 % across the workload set.
This is the largest single perf decision in the project — far
larger than any of PR #2's elisions. Don't change it without
re-measuring across all three E-core platforms.

Build constraint: the `nightly-2026-01-25 + linker-plugin-lto +
embed-bitcode=yes` combo is required for cross-language LTO with
Xcode 26's clang on the iOS/watchOS device .a libs. The macOS host
CLI binary (`run_dispatch_workloads`) is built without
linker-plugin-lto since Apple's macOS `ld` doesn't accept the
`-plugin-opt=...` flags rustc emits with that flag.

## Repo layout

```
apps/                    iOS / watchOS / tvOS / macOS SwiftUI app
crates/benchmark-core/   Rust library — Pulley + WAMR + wasm3 +
                         WasmEdge + zwasm adapters, workload
                         registration, PMU-aware harness
crates/test-programs/    (vendored from wasmtime)
workloads-rs/            one .rs per workload (cdylib, no_std)
workloads-rs-cargo/      xmrsplayer-bench (uses cargo for crates.io deps)
workloads/               pre-built *.wasm (checked in — apps don't
                         need a wasm toolchain at build time)
scripts/                 build-workloads.sh, build-lib.sh, build-wamr.sh,
                         build-wasm3.sh, build-wasmedge.sh,
                         build-zwasm.sh, apply_patch_series.sh,
                         analyze_pmu.py, aggregate_3way.py,
                         aggregate_4way.py, parse_n10.py,
                         run_fusion_n10.sh, run_fusion_pmu.sh,
                         run_per_workload_pmu.sh (iPhone PMU),
                         run_m4_per_workload_pmu.sh (M4 host PMU),
                         m4_phase4_bucket_shares.py
wasmtime/                git submodule pointing at rebeckerspecialties/
                         wasmtime, branch `accurate-graphql-needs-
                         legacy-exceptions` (stacks fusion PR #4 → table-
                         mutability PR #2 → upstream main). Only
                         wasmtime/target/ is gitignored (build artifacts).
wasm3/                   wasm3 submodule (m3 pure-C interp). Built into
                         libm3.a via scripts/build-wasm3.sh; per-target
                         output dirs (build / build-aarch64-apple-ios / ...)
                         mirror the WAMR layout.
WasmEdge/                WasmEdge submodule pinned at 3ad922d6 (the
                         same pin webgpu-caps ships against). Built via
                         scripts/build-wasmedge.sh after applying the
                         27 patches in patches/wasmedge/. Per-target
                         output dirs same convention as wasm3 / WAMR.
zwasm/                   clojurewasm/zwasm submodule. Built via
                         scripts/build-zwasm.sh with `-Djit=false`.
                         arm64_32-apple-watchos device support carried
                         in patches/zwasm/0001-arm64_32-apple-watchos-
                         support.patch (single_threaded static lib +
                         ILP32 narrowing fixes + self-contained panic).
wasmz/                   Ray-D-Song/wasmz submodule. Carried as a
                         Zig-0.16 port (patches/wasmz/0001-zig-0.16-
                         stdlib-port.patch) since wasmz pins Zig 0.15.2
                         and Zig 0.15 segfaults on macOS 26 Tahoe.
                         arm64_32-apple-watchos device support added in
                         patches/wasmz/0002-arm64_32-apple-watchos-
                         support.patch (single_threaded + self-
                         contained panic / logFn).
patches/                 Out-of-tree patch series. Currently
                         patches/wasmedge/0001-0027 — Apple-mobile
                         memory-guard fallbacks + interpreter
                         super-instruction fast-paths + arm64_32 size_t
                         + NSInteger fixes for watchOS device. Applied
                         in series by apply_patch_series.sh; idempotent.
out/                     experiment outputs, PMU traces, summaries
docs/                    project docs
```

## Building

```sh
# wasm workloads (rustc, wasm32-unknown-unknown)
./scripts/build-workloads.sh

# benchmark-core static lib for a platform
./scripts/build-lib.sh macos        # M-series host
./scripts/build-lib.sh ios          # iPhone (aarch64-apple-ios)
./scripts/build-lib.sh watchos      # arm64_32-apple-watchos
./scripts/build-lib.sh watchos-sim  # aarch64-apple-watchos-sim
./scripts/build-lib.sh tvos         # Apple TV 4K (aarch64-apple-tvos)
./scripts/build-lib.sh tvos-sim     # aarch64-apple-tvos-sim
./scripts/build-lib.sh all          # build everything

# Cross-runtime libs. Each script writes a per-target output dir under
# its submodule (build/, build-aarch64-apple-ios, ...). build-lib.sh
# picks them up via the cargo `have_wamr` / `have_wasm3` /
# `have_wasmedge` / `have_zwasm` cfgs.
./scripts/build-wamr.sh all         # WAMR libiwasm.a per platform
./scripts/build-wasm3.sh all        # wasm3 libm3.a per platform
./scripts/build-wasmedge.sh all     # WasmEdge libwasmedge.a per platform
                                    # (applies patches/wasmedge/*.patch in
                                    #  series via scripts/apply_patch_series.sh)
./scripts/build-zwasm.sh all        # zwasm libzwasm.a per platform
                                    # (no arm64_32 device-watch — Zig 0.16
                                    #  has no arm64_32 target)

# M4 host runner (used for E-core PMU + taskpolicy -b)
cargo build --release --bin run_dispatch_workloads

# iOS app
cd apps && xcodebuild -project WasmBenchmark.xcodeproj \
  -scheme WasmBenchmarkIOS -configuration Release \
  -destination "generic/platform=iOS" \
  -derivedDataPath build/DerivedData-c12-ios \
  -allowProvisioningUpdates build

# watchOS app (Watch SE2 S8). MUST pass ARCHS=arm64_32 + ONLY_ACTIVE_ARCH=NO
# since Xcode 26 defaults to arm64 (S9+) but the Rust lib is arm64_32-only.
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

## Workload registration pattern

Each `workloads-rs/<name>.rs` is a standalone `#![no_std] #![no_main]`
cdylib compiled to `workloads/<name>.wasm`. A single .wasm can have
multiple `pub extern "C"` entry points; the harness picks via
`run_workload(WASM_BYTES, "fn_name", arg)`.

To add a workload:
1. Write `workloads-rs/<name>.rs` (see `call_indirect.rs` for the
   `panic_handler` + entry-point pattern).
2. Run `./scripts/build-workloads.sh` — emits `workloads/<name>.wasm`.
3. Verify expected wasm ops survive LTO:
   `wasm-tools print workloads/<name>.wasm | grep call_indirect`
4. In `crates/benchmark-core/src/lib.rs`:
   - Add `pub const <NAME>_WASM: &[u8] = include_bytes!(...)`
   - Add `pub fn run_<name>(arg: i32) -> Result<RunReport>` wrapper
   - Add `#[unsafe(no_mangle)] pub extern "C" fn bench_run_<name>() -> BenchReport`
5. In `crates/benchmark-core/include/benchmark_core.h`:
   - Add `BenchReport bench_run_<name>(void);`
6. In `apps/Shared/BenchmarkContentView.swift`:
   - Add `Workload(id: NN, label: "[Pulley] <name> ...", run: { bench_run_<name>() })`
   - For each cross-runtime peer (WAMR / wasm3) the new workload runs
     on, add a matching `[ WAMR ]` / `[wasm3 ]` row using the same
     human label. Keep the bracketed prefix exact — `BenchmarkContentView`'s
     RUNTIMES env-var filter substring-matches on `[pulley]`, `[ wamr ]`,
     `[wasm3 ]` (note the trailing space inside the bracket).
7. In `crates/benchmark-core/src/bin/run_dispatch_workloads.rs`:
   - Add a `Case { name: "<name>", run: || run_<name>(seed) }` entry

## Measurement methodology

### Wallclock — N=10 cross-platform

- **M4 E-core**: `taskpolicy -b ./target/release/run_dispatch_workloads`
  with `BENCH_TARGET_MS=2000`. **Caveat**: M4 E-cores are heavily
  contended by macOS system services. Per-iter wallclock can have
  30× outliers from OS preemption. Use **CPU time** (`cpu_user_ns` /
  iter from the report) rather than wallclock for variance-sensitive
  comparisons, or run on iPhone 12 instead. We've seen iPhone 12
  N=10 ranges 5× tighter than M4 E-core N=10.
- **iPhone 12 (A14 Icestorm) / iPhone XS (A12 Tempest)**: launch via
  `devicectl device process launch --console --terminate-existing
  --environment-variables ...` with the workload + iter-budget filter.
  `.utility` QoS pins to E-cores (set in `BenchmarkContentView.swift`).
- **Apple TV 4K (A12 / A15)**: same `devicectl` launch flow, bundle ID
  `com.rebeckerspecialties.wasmbench.tv`. tvOS reads
  `devicectl --environment-variables` into Swift `ProcessInfo` the same
  way iOS does (unlike watchOS), so the `WORKLOADS` / `RUNTIMES` env
  filters work normally. tvOS 26+ deployment target.
- **Apple Watch SE2 (S8)**: same `devicectl` launch flow, bundle ID
  `com.rebeckerspecialties.wasmbench.watch`. **xcodebuild requires
  `ARCHS=arm64_32 ONLY_ACTIVE_ARCH=NO`** since Xcode 26 defaults to
  arm64 (S9+) but we only build the Rust lib for arm64_32. The watch
  app's workload filter is the hardcoded `WATCHOS_WORKLOADS_FILTER`
  constant in `apps/Shared/BenchmarkContentView.swift` —
  `devicectl --environment-variables` does **not** propagate to Swift
  `ProcessInfo` on watchOS (verified empirically), though it does
  propagate to Rust `std::env::var` (so `BENCH_TARGET_MS=2000` works
  for the iteration-budget side). Watch BLE tunnel drops mid-session
  are common; wrap installs in a 5-attempt retry loop with `sleep 3`
  between attempts. If a measurement run stalls on `Network.NWError`
  60, wake the watch (touch / charger / side button) and retry.
- **iOS scheduler stickiness**: at every QoS tier we've tested
  (`.utility`, `.userInitiated`, `.userInteractive`), iPhone 12
  keeps sustained dispatch loops on E-cores. P-core PMU on iPhone 12
  is structurally unobtainable from outside the app — even at
  `.userInteractive` we see <30 P-core samples per 15 s window.
- **N=10 batching**: with `BENCH_TARGET_MS=2000`, one rep is one
  full iOS app launch (~30 s wall including spin-up). 20 reps
  (10 IC OFF + 10 IC ON) is ~10–15 minutes. Use
  `scripts/run_fusion_n10.sh` as the launcher; it handles the
  per-rep wait-and-terminate cycle with `EXPECTED_LINES` matching the
  workload-set × runtime count (default 12 for the 6-workload set;
  override to 16 for the 8-workload set incl. graphql-validation).

### PMU / xctrace gotchas (Xcode 26.5)

- **`--launch` mode is broken in Xcode 26.5 for iPhone 12 / iOS 26.3+**:
  xctrace exits with code 0 but the trace contains only
  `RunIssues.storedata` (~52 KB) — no actual counter data. This is
  a regression vs Xcode 26.4 which captured 90+ MB.
- **`--attach <pid>` works** as a workaround:
  ```sh
  # 1. Launch the app via devicectl with a long target so it's still
  #    running when xctrace attaches:
  xcrun devicectl device process launch --device <UDID> \
    --terminate-existing \
    --environment-variables '{"WORKLOADS":"vtable","BENCH_TARGET_MS":"15000"}' \
    com.rebeckerspecialties.wasmbench.ios &
  sleep 3
  # 2. Get the on-device PID:
  PID=$(xcrun devicectl device info processes --device <UDID> \
        | grep -i wasmbench | awk '{print $1}' | head -1)
  # 3. Attach xctrace:
  xcrun xctrace record --device <DEV-ID> --template "CPU Counters" \
    --output capture.trace --attach $PID --time-limit 15000ms
  ```
- **Two different device IDs**: `devicectl` uses the UDID
  (`B5D4CA48-8949-525C-8E5D-4F661161BD9D`); `xctrace` uses Apple's
  device ID (`00008101-000A044A3C28801E`). Map with `xcrun xctrace
  list devices`.
- **Template name change in 26.5**: "CPU Bottlenecks" → "CPU
  Counters" (same internal resource `rsrc://templateCPUBottlenecks`,
  same `analysisMode: bottleneck` config). Use `"CPU Counters"` on
  26.5; templates from 26.4-captured traces (`form.template`) can
  be passed via `--template <path>` for explicit-config recapture.
- **Per-platform counter availability**:
  - **A12 Tempest (iPhone XS)**: does NOT expose
    `CounterMetricByThread` schema → PMU bucket analysis
    unavailable on this platform. Wallclock only.
  - **A14 Icestorm (iPhone 12)**: exposes counters; attach mode
    works on iOS 26.5. Use `scripts/run_per_workload_pmu.sh` for
    one xctrace capture per (workload, runtime) combo at
    `BENCH_TARGET_MS=12000` inside a 20 s attach window.
  - **S8 (Apple Watch SE2)**: does NOT expose `CounterMetricByThread`.
    Wallclock only. (Same status as A12 Tempest.)
  - **M4 Sawtooth E-core**: host-side, `xctrace record --launch
    -- /usr/sbin/taskpolicy -b ./target/release/run_dispatch_workloads`
    (launch mode works on macOS, unlike iOS attach-only workaround).
    Use `scripts/run_m4_per_workload_pmu.sh`. Caveat: single-shot
    absolute cycle counts are noisy due to macOS scheduler
    contention; **bucket shares are stable** and are the trustworthy
    PMU signal on M4.
- **Export quirk**: `xctrace export --xpath '...' > file.xml` may
  silently produce 0-byte output. Use the `--output` flag instead:
  `xctrace export --xpath '...' --output file.xml`.
- **XPath**: simpler is better. `//trace-toc/run/data/table[@schema=
  "CounterMetricByThread"]` works across trace variants; the more
  explicit `[1]/run[1]/data[1]/table[7]` form depends on table index
  which varies between traces.

### Bucket analysis tools

- `scripts/analyze_pmu.py LABEL_A a.xml LABEL_B b.xml` — pairwise
  diff for two captures. Aggregates `CounterMetricByThread` rows by
  core type (P / E) and sums the four buckets (Useful / Processing /
  Delivery / Discarded). Bucket-share shift and absolute-cycle delta.
- `scripts/aggregate_3way.py <root>` — emits the markdown 3-way table
  (baseline / phase3 / WAMR) used in `docs/three-way-baseline-phase3-
  wamr.md`. Wallclock from `<root>/n10/iphone12-*-r{1..10}.log`, PMU
  bucket totals from `<root>/pmu-{baseline,phase3,wamr}/*.xml`.
- `scripts/aggregate_4way.py <n10dir>` — wallclock-only 4-way table
  (baseline / phase3 / phase4 / WAMR) per device. Same log-parser
  signature; PMU is wallclock-only so no XML inputs.
- `scripts/m4_phase4_bucket_shares.py` — emits the
  A14-Icestorm-vs-M4-Sawtooth bucket-shares cross-microarch table.
- `scripts/parse_n10.py <n10dir>` — per-condition median + range
  summary for a single n10 dir.

### Bash launcher gotcha — line buffering

`echo` from inside a `nohup`'d bash script is block-buffered to the
output file → monitors watching the file see no progress until the
buffer fills (minutes). Fix: launch with `stdbuf -oL`:
```sh
nohup stdbuf -oL /tmp/run_n10.sh ... > /tmp/n10.log 2>&1 &
```
Or watch the rep-file count directly instead of the launcher log.

## QoS env-var override

The iOS app reads `BENCH_QOS` to override the default `.utility`
dispatch QoS:
- `BENCH_QOS=user-initiated` → `DispatchQoS.userInitiated` (intended
  to bias toward P-cores; iOS doesn't actually deliver on this for
  sustained loops on iPhone 12)
- `BENCH_QOS=user-interactive` → `DispatchQoS.userInteractive`
- unset / anything else → `.utility` (E-core-preferred)

Set via `devicectl --environment-variables '{"BENCH_QOS":"user-
initiated","WORKLOADS":"vtable","BENCH_TARGET_MS":"2000"}' ...`.

## wasmtime submodule

`./wasmtime/` is a proper git submodule pointing at
`rebeckerspecialties/wasmtime` at the
`accurate-graphql-needs-legacy-exceptions` branch. The pinned SHA is
maintained via standard `git submodule update --init --recursive` and
`scripts/setup.sh`. Only `wasmtime/target/` is gitignored (~70 GB
build artifacts on a populated workspace; the source tree itself is
~169 MB).

Active branches on the fork (pinned-by-SHA at the bottom of this
stack):

- **`unwinder-arm64_32-asm-format`** — upstream PR #13259, merged.
- **`table-mutability-tracking`** — PR #2's branch. 11 commits ahead
  of upstream `origin/main`. 2227 disas + 16 integration tests pass.
- **`claude/pulley-fusion-xband-brif`** — Phases 1–4 opcode fusion
  stack at the call_indirect lazy-init dispatch tail. **12 commits**
  on top of `table-mutability-tracking` (11 fusion + 1 trap-on-null
  correctness fix). Per-phase docs: `docs/opcode-fusion-band-brif.md`
  (phase 1), `docs/opcode-fusion-funcref-dispatch.md` (phase 2),
  `docs/opcode-fusion-band-funcref-dispatch.md` (phase 3),
  `docs/four-way-baseline-phase3-phase4-wamr.md` (phase 4).
- **`accurate-graphql-needs-legacy-exceptions`** — one commit on top
  of fusion stack. Adds `LEGACY_EXCEPTIONS` to wasmtime's
  `features_known_to_wasmtime` mask so the public
  `Config::wasm_legacy_exceptions(true)` API actually works through
  `Config::validate` instead of bailing with "feature not supported
  on this compiler configuration." Required so the accurate-graphql
  Porffor benchmark CAN LOAD on Pulley — though Pulley still can't
  RUN it (codegen-side: "Unsupported feature: operator Try"); that's
  the integration test for the next-session WAMR fast-interp EH PR.

### Patch-stack discipline across all runtime submodules

Every runtime submodule pins an UPSTREAM SHA in `.gitmodules`
(target-lexicon + mach2 + wasmtime are the only ones pointing at our
forks, and those are intentional because the patches in question are
either already-merged upstream or not yet upstreamed). Local fixes
we want to carry without bumping the submodule pin live as
`.patch` files in `patches/<runtime>/`, applied at build time by
each `scripts/build-<runtime>.sh` via
`scripts/apply_patch_series.sh`. The apply step is idempotent —
`git reset --hard HEAD` first, then forward-apply each patch in the
series, skipping any that are already applied (reverse-apply check).

Current patch series under `patches/`:

  * `wasm-micro-runtime/0001-feat-interpreter-legacy-exception-
    handling-throw-only-for-fast-interp.patch` — applied by
    `scripts/build-wamr.sh`. Carries the throw-only legacy-EH
    enablement (PR #1 in our WAMR fork).
  * `wasm3/0001-wasm3-accept-v128-as-opaque-slot.patch` — applied
    by `scripts/build-wasm3.sh`. Lets wasm3 parse Rust-vectorized
    modules with v128 LOCAL declarations. Open as both
    [wasm3#559](https://github.com/wasm3/wasm3/pull/559) (upstream)
    and [rebeckerspecialties/wasm3#1](https://github.com/rebeckerspecialties/wasm3/pull/1)
    (fork — for downstream pinning while upstream review proceeds).
  * `wasmedge/0001-0028-*.patch` (27 patches) — applied by
    `scripts/build-wasmedge.sh`. Apple-mobile guarded-memory
    fallbacks + interpreter super-instructions + arm64_32 size_t
    fixes. Partially upstreamed (#4802 in flight).
  * `wasmz/0001-zig-0.16-stdlib-port.patch` + `0002-arm64_32-apple-
    watchos-support.patch` — applied by `scripts/build-wasmz.sh`.
    Zig 0.15.2 → 0.16 port + arm64_32 device support (upstreamed
    as wasmz#3).
  * `zwasm/0001-arm64_32-apple-watchos-support.patch` — applied by
    `scripts/build-zwasm.sh`. arm64_32 device support (upstreamed
    as zwasm#97).

CI (`.github/workflows/build.yml`) reproduces a clean checkout +
submodule init + every patch series + cross-target builds for
macOS / iOS / iOS-sim / arm64_32-apple-watchos on every PR.

## Cross-runtime comparison

The harness builds against **five** comparison runtimes alongside Pulley:

1. **WAMR** (`wasm-micro-runtime/` submodule, `libiwasm.a`) — fast
   preprocessed-bytecode interpreter; SIMD + bulk-memory + tail-call +
   ref-types all enabled. Wasm exceptions are not (FAST_INTERP +
   EXCE_HANDLING is a forbidden combination in WAMR's CMake).
2. **wasm3** (`wasm3/` submodule, `libm3.a`) — pure C in-place
   interpreter, 11 sources, no external deps when WASI is disabled.
   Supports `return_call` / `return_call_indirect` but **no SIMD** and
   **no wasm exceptions** — `matmul_simd`, `matmul_fma`,
   `graphql-validation (Porffor)`, and `sqlite3` rows are expected to
   fail at load and surface as ERROR rows (treat as data, not a
   regression). xmrsplayer uses `return_call`, which wasm3 implements,
   so it should run subject to the 256 KiB wasm3 stack budget.
3. **WasmEdge** (`WasmEdge/` submodule, `libwasmedge.a`) — the incumbent
   production runtime. Pure interpreter via `WASMEDGE_USE_LLVM=OFF` plus
   the 28-patch series in `patches/wasmedge/` (24 ported from
   `webgpu-caps`, plus 0026 for arm64_32 size_t narrowing in
   `FuncTypeKeyHash` + memory span, 0027 for arm64_32-watchOS
   `NSInteger` sign-comparison narrowing in `lib/host/wasi/macos.mm`,
   and 0028 to skip the Apple-mobile 4 GiB-guarded allocator on
   arm64_32 — 4 GiB of guard reservations don't fit in the 4 GiB
   total ILP32 address space; gate on `defined(__LP64__)`). SIMD +
   wasm-exceptions are both enabled, so the Porffor variant of
   graphql-validation actually *loads* (vs WAMR refusing it).
   On `arm64_32-apple-watchos`, `WasmEdge_VMInstantiate` itself still
   SIGTRAPs at workload time (signal 5 / BRK) — traced to an
   `assuming(x)` predicate in `lib/executor/instantiate/*` evaluating
   false on the 32-bit ABI; `assuming()` in NDEBUG is
   `x ? : __builtin_unreachable()` which clang/arm64_32 compiles to a
   `brk #1`. The Rust adapter (`crates/benchmark-core/src/wasmedge.rs`)
   short-circuits with a clean `Err` on `target_os = "watchos" &&
   not(target_pointer_width = "64")` so the rest of the watch suite
   completes; a follow-up debug-build investigation will identify the
   specific predicate.
4. **wasmz** (`wasmz/` submodule, `libwasmz.a`) — Zig WebAssembly
   runtime by `Ray-D-Song/wasmz`. wasmz's source pins
   `minimum_zig_version = "0.15.2"`, but Zig 0.15 segfaults on macOS
   26 Tahoe. Carried as a Zig-0.16 port via
   `patches/wasmz/0001-zig-0.16-stdlib-port.patch` (≈25 stdlib edits
   covering `std.meta.intToEnum` → `std.enums.fromInt`, `posix.PROT`
   packed-struct migration, `std.Thread.Mutex/Condition` API churn,
   `Target.Os.Tag.solaris` → `.illumos`, and a static-lib build step).
   arm64_32-apple-watchos is enabled via
   `patches/wasmz/0002-arm64_32-apple-watchos-support.patch`
   (`single_threaded = true` for the static lib + a self-contained
   `panic` / `logFn` in src/capi.zig to avoid pulling
   `std.Io.Threaded`, which doesn't compile under ILP32). Same iOS
   dyld-stub + 8 MiB-stack workarounds.
5. **zwasm** (`zwasm/` submodule, `libzwasm.a`) — clojurewasm's Zig
   runtime built `-Djit=false`. Zig 0.16 cross-compiles cleanly to
   `aarch64-ios` / `aarch64-tvos` / `aarch64-watchos-simulator` /
   `aarch64-macos`. arm64_32-apple-watchos is enabled via
   `patches/zwasm/0001-arm64_32-apple-watchos-support.patch`
   (`single_threaded = true` + self-contained `panic` / `logFn` in
   src/c_api.zig + ILP32 narrowing fixes: 4 / 8 GiB guard constants
   gated on `@sizeOf(usize) >= 8`, `@intCast(u64 → usize)` for
   `Memory.read` / `write` and the v128 narrow-load loop, skipped
   auto-init of `std.Io.Threaded` in `types.zig.loadCore` —
   non-WASI workloads never deref the io vtable). Zig 0.16 spells
   the triple `aarch64-watchos-ilp32` (legacy `arm64_32-` arch was
   removed in ziglang/zig PR #20820). Needs a dedicated 8 MiB-stack
   thread (`std::thread::Builder::stack_size`) because Zig's load
   path overflows the 272 KiB Swift dispatch worker stack; also
   needs a tiny weak `_dyld_get_image_header_containing_address`
   stub on iOS/tvOS/watchOS (Zig's panic-stackwalk references a
   dyld symbol that's in dyld at runtime but missing from Apple's
   mobile TBDs).

### Device-side stabilization status (2026-05-16)

End-to-end full-workload-set runs on attached devices, after the
build-workloads.sh SIMD-policy fix + WasmEdge patch 0028 +
adapter-level arm64_32-WE skip:

| device | hardware | runtimes that complete full set |
|---|---|---|
| iPhone 12 | A14 Icestorm, aarch64-apple-ios | **all 6** (Pulley, WAMR, wasm3, WasmEdge, zwasm, wasmz) |
| iPhone XS Max | A12 Tempest, aarch64-apple-ios | **all 6** |
| iPhone 16 Pro Max | A18 Pro, aarch64-apple-ios | **all 6** |
| Watch SE2 | S8, arm64_32-apple-watchos | Pulley, WAMR, wasm3 run all 7 watch-filter workloads cleanly; **wasmz + zwasm init: ok** on device (verified 2026-05-16 via WatchKit app + new patches) and contribute their adapter rows; WasmEdge row still returns clean ERROR (`brk #1` inside `WasmEdge_VMInstantiate` on arm64_32 — adapter short-circuits; debug-build follow-up pending) |
| Apple TV 4K | A12, aarch64-apple-tvos | **all 6 init: ok** on tvOS 26 (verified 2026-05-17); Pulley + WAMR + WasmEdge + zwasm complete the full workload set; wasmz crashes mid-run on the deterministic `factorial(20)` miscompile (same upstream bug as iPhone; tracked at [Ray-D-Song/wasmz#1+#2](https://github.com/Ray-D-Song/wasmz/issues/1)). Pulley A12 is ~1.8× faster on tvOS than the same A12 in iPhone XS Max (sustained-clock difference: TV stays plugged in / no thermal envelope). graphql-validation (Porffor) row works for Pulley + WAMR (via the throw-only EH PR) + WasmEdge + zwasm. |
| iPhone 16 Pro Max + Apple TV further runs | — | skipped per user direction; iPhone 16 was validated once at fib-only filter and showed all 6 runtimes returning fib(30)=832040 |

The two crashes the iPhone 12 + Watch were hitting before
stabilization:

1. **iPhone 12 mid-run SIGBUS** — root cause: wasm3 + wasmz both
   choke on `v128` LOCAL slots that Rust's auto-vectorizer emitted
   into non-SIMD workloads when `+simd128` was unconditionally set in
   `scripts/build-workloads.sh`. wasm3 reports a clean parse-time
   "unknown value_type" error; wasmz silently accepts the module but
   then either returns the caller-supplied default result or leaves
   the runtime in a state that SIGBUSes the next call. Fixed by
   gating `+simd128 +relaxed-simd` to the matmul workloads only (they
   genuinely use v128 intrinsics) and compiling everything else with
   `-simd128 -relaxed-simd`. wasm3 + wasmz now run every previously-
   broken workload.
2. **Watch SE2 mid-run SIGTRAP** — root cause: WasmEdge's
   Apple-mobile guarded allocator reserves ≈2.2 MB per memory
   instance with a 1 MB guard page on `__aarch64__ &&
   WASMEDGE_APPLE_MOBILE_VM`; that fits in iOS's 64-bit address space
   but not in arm64_32-apple-watchos's 4 GiB total ILP32 space. Patch
   0028 gates the guarded paths on `defined(__LP64__)` so arm64_32
   falls through to the malloc-based allocator. The allocator Warning
   line is gone — but `WasmEdge_VMInstantiate` itself still BRKs on
   arm64_32 (`assuming(x)` UB in lib/executor/instantiate/*); the
   wasmedge adapter short-circuits with a clean error on arm64_32 so
   the rest of the watch suite still completes. A follow-up debug-
   build investigation would identify the exact failing predicate
   inside Instantiate.

The Pulley-vs-WAMR gap is **structural, not IC-related** —
WAMR's preprocessed register-IR has fewer match_loop-equivalent
dispatches per source-level wasm op. The PR-#4 phases-1–4 fusion stack
closes ~10 % of that gap on the iPhone 12 vtable suite (vtable_poly4
1.73× → 1.58×; vtable_bi 1.78× → 1.65×) without changing the
structural disadvantage.

### Open follow-up — WAMR fast-interp legacy exception handling (full spec)

**Status (2026-05-17 late-EOD)**: throw-only legacy EH landed in
[rebeckerspecialties/wasm-micro-runtime#1](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/1).
Branch `feat/legacy-eh-fast-interp-full` now carries **commits 1
through 8** of the full-spec successor — loader EH metadata table,
runtime EH-frame stack push/pop, WASM_OP_THROW catch-walk with the
return_func exception hook, WASM_OP_RETHROW re-raise via per-entry
caught-tag storage, WASM_OP_DELEGATE forward-to-outer dispatch
(loader counts try/catch/catch_all blocks between the delegate's
try and the target block → `delegate_target_depth = delta`; runtime
walker reads `delta` off the eh-table entry and does
`i -= delta; continue` so the next eh-stack entry examined is the
first one strictly outside the target block), and tag-with-params
payload routing for same-function dispatch (loader emits cell-wise
src offsets after THROW + records per-catch cell-wise dst offsets;
runtime walker copies `frame_lp[dst[c]] = frame_lp[src[c]]` on
match). `workloads/graphql-validation-porf-accurate.wasm` runs
end-to-end at ~17.3 ms median (no regression on AS / porf-fast
either). The eight committed patches now live in
`patches/wasm-micro-runtime/` as `0001-…` through `0008-…`,
applied on top of the upstream pin `cd390ea0`.

The runtime eh-stack entry is `EH_ENTRY_CELLS = 2` cells wide as of
commit 5. Cell 0 packs `eh_idx | EH_TRY_CATCH_STATE_BIT`; cell 1
holds the wasm tag index of the exception currently being handled
on that entry — undefined while the entry is in TRY state, written
by the throw walker on catch dispatch, read by RETHROW. Frame
allocation grows by `exception_handler_count * 2` cells per call;
functions without try blocks still pay zero cells.

**Throw-firing correctness verified** via
`crates/benchmark-core/src/bin/probe_eh_void.rs` driving
`/tmp/eh_void.wasm` (compile from `/tmp/eh_void.wat` via
`wat2wasm --enable-exceptions`). Five void-result try-region shapes
all PASS:

| case | what | want |
|---|---|---|
| `test_local_throw` | typed catch handles same-function throw | 99 |
| `test_catch_all` | catch_all matches any throw | 77 |
| `test_inter_fn` | callee throws; caller's catch fires via return_func hook | 55 |
| `test_nested` | inner catch wins; outer never fires | 33 |
| `test_no_throw` | normal-flow CATCH-skip on empty try | 11 |

**Pending — non-void result-type try-regions** (`try (result T)`).
The runtime walker and return_func hook are correct for any
blocktype; what's missing is loader-side: fast-interp's
`reserve_block_ret` at END emits a COPY from the *current
frame_offset top* to `block->dynamic_offset`, and that "current
top" is fixed at load time. For a try-region with two bodies
(try + catch), the COPY's source slot ends up hard-coded to the
catch body's last value's slot. Try-bodies that complete normally
then take the CATCH-fall-through path and run the COPY with the
*wrong* source slot — returning the catch body's would-be value
instead of the try body's actual value.

**Fix for the result-type follow-up**: at CATCH processing in the
loader (before `RESET_STACK()` and the catch-body's PUSH_TYPE
sequence), emit a COPY for the try body's last value into
`block->dynamic_offset` — same shape as `case WASM_OP_ELSE`'s
`reserve_block_ret(loader_ctx, WASM_OP_ELSE, …)` aligns the if-
body's result. Once both bodies deposit to the same slot,
end_of_region_pc can point at the post-COPY position and both
paths return the right value. graphql-validation-porf-accurate is
not blocked by this — its single try is `06 40` (void).

**Land-mines documented during the deep-dive** (kept here so the
next session doesn't relearn them):

  1. **Loader-side pass-1 / pass-2 size accounting must match.**
     Any `emit_uint32`/`emit_label`/etc you add must run in BOTH
     traverses or pass 2 will overrun the `code_compiled` buffer
     allocated based on pass 1's measurement. Commit 3's pass-1/
     pass-2 mismatch on `emit_uint32(eh_idx)` for CATCH / CATCH_ALL
     was a 4-byte overrun per catch that corrupted the very next
     loader allocation in the heap — typically
     `func->exception_handlers` itself (catch_count gets zeroed).
     Bug signature: loader populates correctly, but runtime sees
     `entry->catch_count == 0` and the throw escapes as "wasm
     exception thrown (tag N)". Gate the *populate* on
     `p_code_compiled != NULL`, never the *emit*.
  2. **IR encoding under `WASM_ENABLE_LABELS_AS_VALUES`** (default
     on macOS / Linux): each "opcode" in the rewritten IR is an
     8-byte pointer (on 64-bit + unaligned access) into the dispatch
     handle table, NOT a 1-byte opcode value. `emit_label(opcode)`
     emits 8 bytes; `skip_label()` rewinds 8 bytes. `i32.const`,
     `f32.const`, etc. ARE stripped from the IR — the value goes
     into the per-function const pool and downstream ops reference
     a slot offset via `frame_offset`. Don't reason about IR layout
     by counting source bytes.
  3. **The build script's `git reset --hard HEAD`** in
     `scripts/build-wamr.sh` wipes uncommitted WAMR changes every
     time. During iterative dev, either commit on the submodule's
     `feat/legacy-eh-fast-interp-full` branch before building, or
     run `cmake/make` directly in
     `wasm-micro-runtime/product-mini/platforms/darwin/build/`.
     The recorded submodule pin in the outer repo should stay at
     `cd390ea0` (upstream) so the patches/ stack applies cleanly;
     when actively editing WAMR, `git checkout
     feat/legacy-eh-fast-interp-full` in the submodule to switch
     to the dev branch, then back to `cd390ea0` before committing
     outer-repo changes.
  4. **`frame->exception_raised` is NOT zero-initialized by
     `ALLOC_FRAME`** in fast-interp. The return_func hook reads it
     on every wasm-to-wasm return; without an explicit `frame->
     exception_raised = false` next to the existing `frame->
     eh_count = 0` line in `call_func_from_entry`, the hook fires
     on every call return with stale memory and turns every
     program into "wasm exception thrown (tag N)" for random N.
  5. **WAMR's `wasm_runtime_load` does NOT copy the input wasm
     bytes** — it stores a pointer into the caller-owned buffer for
     the lifetime of the module. Drop the buffer too early and every
     export lookup silently returns NULL (no exception set; the
     module pointer remains valid but its internal section indexes
     point at freed memory). Surfaces only when wasm is built with
     a custom name section (e.g. `wat::parse_str`'s default emit)
     because that section was at the location overwritten by
     allocator reuse first. Workaround in test harness: keep the
     wasm `Vec<u8>` alive alongside the module in the owning struct
     — see the `_bytes` field on
     `crates/benchmark-core/tests/eh_correctness.rs::Module`.

**Test infrastructure**: 47 integration-test cases (45 active + 2
ignored placeholders for known gaps) in
[`crates/benchmark-core/tests/eh_correctness.rs`](crates/benchmark-core/tests/eh_correctness.rs).
The active suite covers same-function dispatch (typed catch /
catch_all / no-throw fall-through), inter-function unwind (3+
frame chains, deep recursion to 50, 101-frame stress with throw +
rethrow at every level), nested try-regions (2 + 3 levels),
throw-inside-catch outward propagation, multiple catches with tag
matching, catch_all-as-fallback, uncaught throws, try-inside-loop/
if/catch-body, sequential try-regions in one function (10 and 32
deep), repeated invocation of a try-bearing function, a 6-tag stress
check, three `rethrow` cases (depth 0 in-frame, depth 1 across
nested catches, tag-preservation across rethrow), eight
`delegate` cases (basic forward, normal-flow eh-stack pop,
forwarding through a non-try block, skipping a middle try-with-
catches, forwarding to function-block-as-escape, callee-side
delegate caught by caller's try, 3-level nested delegates, and
catch-body-internal delegate that must escape rather than
re-match an already-consumed outer catch), and eight tag-with-params
cases (single i32 / i64 / mixed i32+i64, two i32s, multiple catches
selected by signature, nested catches inheriting param values,
rethrow-preserves-payload via the still-alive dst slots, catch_all-
drops-payload, and repeated-throw-with-fresh-payload). Two
`#[ignore]` cases document the remaining gaps as runnable tests
that should pass once each follow-up lands:
`cross_function_tag_with_params` (callee's source frame is freed
before caller's walker runs — needs a cross-frame payload buffer)
and `br_out_of_try_pops_eh_stack` (br across try-region boundary).
Each test compiles inline wat via `wat::parse_str` and runs against
the same WAMR build the benchmarks use. Run with `cargo test -p
benchmark-core --test eh_correctness`. The probe binary
`crates/benchmark-core/src/bin/probe_eh_void.rs` is retained as a
faster smoke check.

**Benchmark perf baseline**: the early "~11 ms median" porf-fast /
porf-accurate numbers in the commit-3 / commit-5 commit messages
were single-run outliers — multi-run characterization (10 runs at
each of three commit points: 378cc6d / e7f527a6 / 334f642c) shows
the stable steady state is `iter≈11 median≈17.5-18.5 ms` for both
porf-fast and porf-accurate. The number is the same at every
commit from 3 onward, including with commit-5's
`EH_ENTRY_CELLS = 2` allocation bump. Hot-op invariants verified:
basic non-EH workloads (fib, sieve, crc32, matmul, convolution)
match Pulley within ±50 % each direction with WAMR generally
faster on dispatch-heavy patterns, unchanged by commits 1-5.

**Wat parser caveats** worth remembering when extending the suite:

  * Rust's `wast` parser only accepts the LINEAR `try / instr* /
    catch $tag / instr* / catch_all / instr* / end` form for
    legacy-EH. The wabt-style folded `(try (do ...) (catch ...))`
    syntax does NOT parse.
  * Inside a linear try/catch body, push operands BEFORE the
    consuming op (`i32.const 99` then `global.set $g`), not the
    folded `(global.set $g (i32.const 99))` — the folded form
    parses as two separate sequential ops in linear context and
    trips a stack-mismatch validation error.

**Failure mode (precise)**: with the throw-only patch applied,
`workloads/graphql-validation-porf-accurate.wasm` (1 `try`, 1
`catch 0`, 2 `throw 0`) traps at runtime with
`Exception: unsupported opcode`. Cause: WAMR's loader
auto-emits every opcode via `emit_label(opcode)` at
[`wasm_loader.c:11974`](wasm-micro-runtime/core/iwasm/interpreter/wasm_loader.c#L11974);
the fast-interp path explicitly `skip_label()`s for `WASM_OP_BLOCK`,
`WASM_OP_LOOP`, `WASM_OP_NOP`, and `WASM_OP_TRY`, but **not** for
`WASM_OP_CATCH`/`CATCH_ALL`/`RETHROW`/`DELEGATE` — so those four
opcodes pass straight through into the rewritten IR and the runtime
handler routes them to `"unsupported opcode"`
([`wasm_interp_fast.c:1869-1888`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_fast.c#L1869)).

The catch handler in porf-accurate is **not a no-op** — it does
tag-pattern dispatch, calls `call 104`, compares against `global 53`,
and may re-throw. Skipping CATCH silently corrupts results.

**Cost-model rule (the maintainer pushback the fix must clear)**:
EH must not tax `HANDLE_OP(WASM_OP_CALL)` /
`HANDLE_OP(WASM_OP_*_LOAD_*)` / `HANDLE_OP(WASM_OP_*_STORE_*)` on
the success path. Verified the same rule in classic-interp under
`WASM_ENABLE_EXCE_HANDLING=1`: zero `#if WASM_ENABLE_EXCE_HANDLING`
inside hot-op handlers; the only per-program cost is one
`SET_LABEL_TYPE` byte store per `PUSH_CSP`
([`wasm_interp_classic.c:518-522`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_classic.c#L518))
and `eh_size` cells added to `max_stack_cell_num` per frame
([`wasm_interp_classic.c:6786-6787`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_classic.c#L6786)).
EH state lives in a separate per-frame eh-stack array sized by
`func->exception_handler_count` — already populated by the loader
([`wasm_loader.c:12018`](wasm-micro-runtime/core/iwasm/interpreter/wasm_loader.c#L12018)).

**4-commit shape** (each commit independently mergeable upstream):

| # | scope | key files |
|---|---|---|
| 1 | Loader: extend the existing `#if WASM_ENABLE_EXCE_HANDLING != 0` block in `wasm_loader.c` (after line 12277) — `skip_label()` for `WASM_OP_CATCH` / `CATCH_ALL` / `RETHROW` / `DELEGATE`. Add `WASMFastEHEntry` struct on `WASMFunction` with `{catch_count, catches[]={tag_index, handler_pc, frame_offset_cells}, catch_all_pc, delegate_target_depth, end_of_region_pc}`. Populate during the existing validation pass — no second walk over the bytecode. The TRY case already records `func->exception_handler_count++`; extend it to record table-index immediates for the new fast-IR ops. | `core/iwasm/interpreter/wasm.h`, `core/iwasm/interpreter/wasm_loader.c` |
| 2 | Runtime: allocate `frame->eh_stack[exception_handler_count]` next to `frame_lp`. New fast-IR op `EXT_OP_FAST_TRY <uint32 eh_idx>` pushes one entry; `EXT_OP_FAST_END_TRY` pops. `HANDLE_OP(WASM_OP_CATCH)` / `CATCH_ALL` become "pop eh_stack + branch to pre-patched end-of-region ptr" — same shape as `WASM_OP_BR` (uses `RECOVER_BR_INFO`-style target). Hot ops (CALL/LOAD/STORE) untouched. | `core/iwasm/interpreter/wasm_interp_fast.c` |
| 3 | THROW dispatch: extend the existing throw-only handler at line 1839 to walk `frame->eh_stack` top-down. On match → restore frame_lp to saved height, copy tag params from throw site, set frame_ip to catch handler pc, dispatch. On miss in current function → existing `got_exception` bailout, BUT extended: hook `return_func` (line 7840) so when caller resumes with `wasm_get_exception(module) != NULL`, it re-enters a new `find_a_catch_handler:` label inside the dispatch loop. Mirrors classic-interp lines 6877-6883 + 1933-1958 exactly. | `core/iwasm/interpreter/wasm_interp_fast.c` |
| 4 | RETHROW + DELEGATE: re-raise saved tag/payload (RETHROW) or pop N eh-frames before resuming walk (DELEGATE). Porffor doesn't emit these but spec_testsuite/legacy/{rethrow,try_delegate}.wast does. **Status as of commit 6:** RETHROW lands as commit 5, DELEGATE as commit 6 — both share the eh-stack walker's `EH_TRY_CATCH_STATE_BIT` machinery and pay zero cost in CALL / LOAD / STORE. DELEGATE additionally skips the shared `check_branch_block_for_delegate` helper (its `emit_br_info` call would write 12 bytes of dead branch metadata in the rewritten IR and shift the depth immediate past where the runtime reads it — same gotcha that bit RETHROW). | `core/iwasm/interpreter/wasm_interp_fast.c`, `core/iwasm/interpreter/wasm_loader.c` |

Final cmake patch: rewrite the
[`unsupported_combination.cmake:67-77`](wasm-micro-runtime/build-scripts/unsupported_combination.cmake#L67)
comment block to say "FAST_INTERP + EXCE_HANDLING is fully supported
for the legacy proposal; `try_table`/`throw_ref` (Phase 4 EH) is the
next gap" — and drop the throw-only restriction language.

**Out of scope** for this PR: `try_table` / `throw_ref` (the
post-Phase-3 EH proposal). Confirmed via Explore 2026-05-17 —
classic-interp has zero matches for `try_table`/`throw_ref`/
`WASM_OP_TRY_TABLE`/`WASM_OP_THROW_REF` in the entire WAMR tree, so
that's a separate proposal effort, not part of fast-interp parity.

**Validation gates** (must pass before opening the PR):
- `target/release/run_graphql_validation_wamr` — the existing bin
  already runs AS + porf-fast and (per 2026-05-17 edit) also probes
  `GRAPHQL_VALIDATION_PORF_ACCURATE_WASM`. Expected: third block
  switches from `ERROR: ... unsupported opcode` to a numeric
  `result=...` matching the wasmtime/Pulley side.
- Existing AS + porf-fast results unchanged (latency parity, same
  iteration counts to ±5 %).
- Apple device build: `scripts/build-wamr.sh watchos` still produces
  a working `libiwasm.a` for arm64_32 — no new symbol that the
  arm64_32 ILP32 ABI rejects.

**Cited design references**:
- Classic-interp `find_a_catch_handler` walk:
  [`wasm_interp_classic.c:1753-1976`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_classic.c#L1753).
- Classic-interp inter-function unwind via `return_func`:
  [`wasm_interp_classic.c:6877-6883`](wasm-micro-runtime/core/iwasm/interpreter/wasm_interp_classic.c#L6877).
- Fast-interp branch-target pre-patching (the template for
  pre-resolving catch handler PCs at load time):
  `emit_br_info()` + `RECOVER_BR_INFO()` in
  `core/iwasm/interpreter/wasm_loader.c` (BR/BR_IF/BR_TABLE handlers)
  and `core/iwasm/interpreter/wasm_interp_fast.c:971-1038`.

**Realistic effort**: 1-3 focused days. The slot-allocator
interaction is the design risk — fast-interp doesn't have a runtime
control-stack-pointer to walk, so the "transfer to catch handler"
step has to be entirely IP-based with pre-computed slot offsets.
Classic-interp's `find_a_catch_handler` is the porting template; the
asymmetry is that fast-interp pre-resolves block boundaries at load
time, so `UNWIND_CSP` becomes a static IR transfer.

**Why this PR is worth doing** beyond our benchmark: WAMR's
`unsupported_combination.cmake:67` (`EXCE_HANDLING + FAST_INTERP`)
has been a known limitation since the original
[EH PR #3096](https://github.com/bytecodealliance/wasm-micro-runtime/pull/3096)
(April 2024). Anyone running Porffor / AssemblyScript-with-
exceptions / Emscripten C++-exceptions on WAMR fast-interp today
hits this wall.

### Skipped runtimes (App Store / Apple-platform feasibility)

| runtime | reason skipped |
|---|---|
| **wasmer** | All backends (Singlepass, Cranelift, LLVM) are JIT — emit native code at runtime and require MAP_JIT. No pure-interpreter backend. Cannot ship on iOS / watchOS / tvOS. |
| **Silverfir-nano** (`mbbill/Silverfir-nano`) | Self-describes as a "compact optimizing WebAssembly 3.0 **JIT**"; JIT is mandatory, no interpreter mode. Disqualified. |
| **wasmz** (`Ray-D-Song/wasmz`) | Source pins `minimum_zig_version = "0.15.2"` (its build.zig uses `std.meta.intToEnum`, `Target.Os.Tag.solaris`, and other Zig-0.15-only stdlib APIs). On macOS 26 Tahoe, Zig 0.15.1 + 0.15.2 segfault even when building a trivial `zig init` — their build runner has unresolved libSystem symbols (`_realpath$DARWIN_EXTSN`, `_sigaction`, ...) at link time. Carried as the Zig 0.16 port in `patches/wasmz/0001-zig-0.16-stdlib-port.patch` (≈25 mechanical stdlib edits); arm64_32-apple-watchos device support added via `patches/wasmz/0002-arm64_32-apple-watchos-support.patch`. **No longer skipped** — wasmz is one of the 5 on-device runtimes on all targets including Watch SE2. |

**Current phase-4 Pulley/WAMR wallclock ratios (lower = closer)**:

| workload | iPhone 12 A14 | iPhone XS A12 | Watch SE2 S8 | Apple TV 4K A12 |
|---|---:|---:|---:|---:|
| xmrsplayer | 1.33× | 1.22× | **1.16×** | 1.28× |
| graphql-validation (AS) | 1.56× | 1.58× | 1.24× | 1.91× |
| call_indirect | 1.67× | 1.49× | 1.46× | 1.78× |
| vtable_bi | 1.65× | 1.48× | 1.48× | 1.77× |
| vtable_poly4 | 1.58× | 1.42× | 1.36× | 1.54× |
| vtable_poly6 | 1.61× | 1.48× | 1.42× | 1.65× |
| vtable_mono | 1.74× | 1.45× | 1.54× | 1.81× |

Apple TV 4K runs the SAME A12 chip as iPhone XS Max but sustains higher
clocks (no thermal envelope; plugged-in power). On `fib(30)`, the TV
finishes Pulley in 71 ms vs the iPhone XS's 128 ms — a 1.8× sustained-
clock advantage on identical silicon. The Pulley/WAMR RATIO is closer
to phone-A14 than phone-A12, indicating Pulley scales linearly with
clock on this microarchitecture while WAMR fast-interp's overhead
relative to Pulley is consistent across both.

Watch SE2 has the tightest ratios for the production-shaped workloads
(xmrsplayer 1.16×, graphql-AS 1.24×) — meaningful because the watch
is the actual deployment target. See
`docs/four-way-baseline-phase3-phase4-wamr.md` and the phase-5
candidates section there for what's next.

**WAMR can't run Porffor** (graphql-validation Porffor variant): WAMR's
interp build forbids `EXCE_HANDLING + FAST_INTERP` AND `SIMD +
CLASSIC_INTERP` simultaneously; Porffor's WAT needs both SIMD (v128
string compare) and wasm-exceptions (JS try/catch lowering).
Structural N/A, documented inline in `scripts/build-wamr.sh`.

WAMR build: `./scripts/build-wamr.sh` (configures + builds
`libiwasm.a` for each target). PMU-only traces should filter to a
single runtime via `RUNTIMES=pulley` env var so WAMR's dispatch
overhead doesn't dilute the Pulley signal.
