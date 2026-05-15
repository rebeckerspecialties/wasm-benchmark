# AGENTS.md — wasm-benchmark project guide

This is a WebAssembly-interpreter benchmark harness targeting Apple Silicon
deployment platforms (App-Store-eligible: no JIT, no MAP_JIT, no
copy-and-patch) — primarily arm64_32-apple-watchos, aarch64-apple-ios,
and aarch64-apple-darwin. The harness compares Pulley (wasmtime's
interpreter) against WAMR (WebAssembly Micro Runtime) and was set up
to drive a per-table-mutability optimization stack upstream (see
[PR #2](https://github.com/rebeckerspecialties/wasmtime/pull/2)).

## Project goal & current state

**Motivation**: A WatchOS audio app (incumbent: WasmEdge with custom
patches) runs at ~25 % CPU on iPhone XS for a WASI-audio music
player; S8 Apple Watch is 2–3× slower per core. Dispatch density on
narrow E-cores is the binding performance constraint. wasmtime's
Pulley interpreter is the only App-Store-legal runtime in that space.
Goal: identify and ship dispatch optimizations that move the needle
on these targets.

**Status** (2026-05-14):
- PR #2 (`table-mutability-tracking` on the wasmtime fork) is the
  predicate-factory branch — 11 commits, 2227 + 16 tests pass.
- An IC investigation in `pulley-call-indirect-ic*` branches was
  abandoned after PMU evidence on `vtable_dispatch.wasm` showed the
  IC's back-end savings cancel against new front-end / mispredict
  pressure on Apple silicon E-cores. See
  `out/exp-c-device/ic/ARCHIVED-BRANCH-SHAS.md` for recovery info.
- Next branch: opcode fusion in Pulley
  (`xband_brif_eq_zero`, `funcref_load_dispatch`, AOT peephole) per
  the fusion section of PR #2's description. **Phase 1
  (`xband_brif_eq_zero`) measured 2026-05-14 on iPhone 12 — hypothesis
  falsified in isolation (wallclock flat, Discarded +7.87 %). Phase 2
  (`funcref_load_dispatch`) measured same day on top of phase 1's
  branch: call_indirect wallclock **−5.0 %** vs baseline (the first
  measurable wallclock win past PR #2's c1-7 ceiling), PMU Discarded
  **−1.74 %** vs baseline / **−8.91 %** vs phase 1 — phase 1's
  predictor-anchor regression is reclaimed. The
  per-new-opcode-family predictor cost is NOT linear; the larger
  funcref-dispatch op consolidates the predictor's view of the
  dispatch tail better than the narrower BandBrIf. Phase 2
  supersedes phase 1 at the same call site; phase 1's `BandBrIf` op
  stays in the ISA as a fallback when the continuation-block load
  pattern doesn't match. See `docs/opcode-fusion-band-brif.md` and
  `docs/opcode-fusion-funcref-dispatch.md` for the two measurement
  closeouts.**

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
apps/                    iOS / watchOS / macOS SwiftUI app
crates/benchmark-core/   Rust library — Pulley + WAMR adapters,
                         workload registration, PMU-aware harness
crates/test-programs/    (vendored from wasmtime)
workloads-rs/            one .rs per workload (cdylib, no_std)
workloads-rs-cargo/      xmrsplayer-bench (uses cargo for crates.io deps)
workloads/               pre-built *.wasm (checked in — apps don't
                         need a wasm toolchain at build time)
scripts/                 build-workloads.sh, build-lib.sh, analyze_pmu.py
wasmtime/                working clone of bytecodealliance/wasmtime
                         (gitignored; see PR #2's table-mutability-tracking branch)
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
./scripts/build-lib.sh all          # build everything

# M4 host runner (used for E-core PMU + taskpolicy -b)
cargo build --release --bin run_dispatch_workloads

# iOS app
cd apps && xcodebuild -project WasmBenchmark.xcodeproj \
  -scheme WasmBenchmarkIOS -configuration Release \
  -destination "generic/platform=iOS" \
  -derivedDataPath build/DerivedData-c12-ios \
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
- **iOS scheduler stickiness**: at every QoS tier we've tested
  (`.utility`, `.userInitiated`, `.userInteractive`), iPhone 12
  keeps sustained dispatch loops on E-cores. P-core PMU on iPhone 12
  is structurally unobtainable from outside the app — even at
  `.userInteractive` we see <30 P-core samples per 15 s window.
- **N=10 batching**: with `BENCH_TARGET_MS=2000`, one rep is one
  full iOS app launch (~30 s wall including spin-up). 20 reps
  (10 IC OFF + 10 IC ON) is ~10–15 minutes. Use `/tmp/run_n10.sh`
  as the launcher template (in this session's transcripts).

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
    unavailable on this platform.
  - **A14 Icestorm (iPhone 12)**: exposes counters; attach mode
    works on iOS 26.5.
  - **M4 Sawtooth E-core**: via `taskpolicy -b`, default device.
- **Export quirk**: `xctrace export --xpath '...' > file.xml` may
  silently produce 0-byte output. Use the `--output` flag instead:
  `xctrace export --xpath '...' --output file.xml`.
- **XPath**: simpler is better. `//trace-toc/run/data/table[@schema=
  "CounterMetricByThread"]` works across trace variants; the more
  explicit `[1]/run[1]/data[1]/table[7]` form depends on table index
  which varies between traces.

### Bucket analysis tool

`scripts/analyze_pmu.py LABEL_A path/a.xml LABEL_B path/b.xml`
aggregates `CounterMetricByThread` rows by core type (P / E) and
sums the four buckets (Useful / Processing / Delivery / Discarded)
from the `uint64-array` column. Diff is printed with bucket-share
shift and absolute-cycle delta.

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

## wasmtime working clone

`./wasmtime/` is gitignored (it's a 47 GB working clone, not a
submodule). Active branches:

- **`table-mutability-tracking`** — PR #2's branch. 11 commits ahead
  of upstream `origin/main` (excluding mach2 bumps + unwinder dep
  underneath). 2227 disas + 16 integration tests pass.
- **`unwinder-arm64_32-asm-format`** — upstream PR #13259 dependency.
- **(deleted)** `pulley-call-indirect-ic`,
  `pulley-call-indirect-ic-noseqlock` — IC investigation, closed
  out. SHAs in `out/exp-c-device/ic/ARCHIVED-BRANCH-SHAS.md`.
- **`claude/pulley-fusion-xband-brif`** — Phase 1 opcode fusion
  (`xband_s8 + br_if`). Three commits on top of
  `table-mutability-tracking`. See
  `docs/opcode-fusion-band-brif.md`.

## Cross-runtime comparison

The harness builds against **WAMR** (`wasm-micro-runtime/` submodule)
as a comparison runtime. WAMR's fast-interp consistently beats
Pulley by 25–40 % on dispatch-heavy workloads
(`call_indirect.wasm`, `xmrsplayer.wasm`). The gap is **structural,
not IC-related** — WAMR's register-style fused-op IR has fewer
match_loop-equivalent dispatches per source-level wasm op. Closing
this gap is what the next branch's opcode-fusion work targets;
see PR #2 description's "Next branch — opcode fusion" section.

WAMR build: `./scripts/build-wamr.sh` (configures + builds
`libiwasm.a` for each target). PMU-only traces should filter to a
single runtime via `RUNTIMES=pulley` env var so WAMR's dispatch
overhead doesn't dilute the Pulley signal.
