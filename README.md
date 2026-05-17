# wasm-benchmark

[![build](https://github.com/rebeckerspecialties/wasm-benchmark/actions/workflows/build.yml/badge.svg)](https://github.com/rebeckerspecialties/wasm-benchmark/actions/workflows/build.yml)

WebAssembly interpreter benchmark harness targeting Apple Silicon
deployment platforms — Apple Watch (arm64_32-apple-watchos), iPhone
(aarch64-apple-ios), Apple TV (aarch64-apple-tvos), and Mac
(aarch64-apple-darwin). Compares
[wasmtime](https://github.com/bytecodealliance/wasmtime)'s **Pulley**
interpreter against five other pure-interpreter runtimes:
[WAMR](https://github.com/bytecodealliance/wasm-micro-runtime) fast-interp,
[wasm3](https://github.com/wasm3/wasm3), [WasmEdge](https://github.com/WasmEdge/WasmEdge)
(`USE_LLVM=OFF`), [wasmz](https://github.com/Ray-D-Song/wasmz), and
[zwasm](https://github.com/clojurewasm/zwasm) — on dispatch-heavy
workloads (synthetic `call_indirect`, real-world xmrsplayer tracker
player, sqlite3 speedtest1, graphql-validation in two ports — Porffor
and AssemblyScript).

The harness drives the
[per-table-mutability optimization stack on the wasmtime fork](https://github.com/rebeckerspecialties/wasmtime/pull/2)
and the
[opcode-fusion stack PR #4](https://github.com/rebeckerspecialties/wasmtime/pull/4)
that builds on it.

App-Store constraint: **pure-interpreter only**, no JIT / MAP_JIT /
copy-and-patch. Pulley is the only legal wasmtime runtime in that
space. Every comparison runtime is built with JIT/AOT disabled
(`WAMR_BUILD_JIT=0`, `WASMEDGE_USE_LLVM=OFF`, `-Djit=false`, etc.).

## Quickstart

```sh
# Clone with submodules (wasmtime, WAMR, wasm3, WasmEdge, wasmz, zwasm,
# target-lexicon, mach2, porffor, sightglass — all pinned)
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
./scripts/build-wasmz.sh macos      # wasmz libwasmz.a (Zig 0.16 port)
./scripts/build-zwasm.sh macos      # zwasm libzwasm.a

# Build benchmark-core static lib for a platform
./scripts/build-lib.sh macos        # M-series host
./scripts/build-lib.sh ios          # iPhone (aarch64-apple-ios)
./scripts/build-lib.sh watchos      # arm64_32-apple-watchos
./scripts/build-lib.sh tvos         # Apple TV 4K (aarch64-apple-tvos)
./scripts/build-lib.sh all          # everything

# Run the M4 host CLI (E-core pinned)
cargo build --release --bin run_dispatch_workloads
taskpolicy -b ./target/release/run_dispatch_workloads
```

iOS / watchOS / tvOS app builds via `xcodebuild` from `apps/`. See
[AGENTS.md](AGENTS.md) for full build/deploy/measurement procedures
(including the `ARCHS=arm64_32 ONLY_ACTIVE_ARCH=NO` watchOS gotcha and
the xctrace PMU-attach workaround for Xcode 26.5).

CI (`.github/workflows/build.yml`) reproduces the full submodule init +
patch-series application + per-target build on every PR, so a clean
checkout from any branch should succeed end-to-end without local state.

## Repo layout

```
apps/                    SwiftUI app — iOS / watchOS / tvOS / macOS targets
crates/benchmark-core/   Rust library — Pulley + WAMR + wasm3 + WasmEdge +
                         wasmz + zwasm adapters, workload registration,
                         PMU-aware harness
docs/                    project docs (feasibility, fusion phases 1–4,
                         cross-runtime comparisons, IC investigation, ...)
patches/                 upstream-PR-prep patches for each runtime
                         submodule, applied idempotently by build-*.sh
scripts/                 build, setup, PMU analysis
workloads-rs/            standalone Rust sources for *.wasm workloads
workloads-rs-cargo/      cargo-based wasm workload (xmrsplayer-bench)
workloads/               pre-built *.wasm files (checked in)
# --- submodules ---
wasmtime/                rebeckerspecialties/wasmtime, branch
                         `accurate-graphql-needs-legacy-exceptions`
                         (stacks fusion PR #4 → PR #2 → upstream main).
                         Only wasmtime/target/ is gitignored.
wasm-micro-runtime/      rebeckerspecialties/wasm-micro-runtime, with
                         patches/wasm-micro-runtime/0001 applied at
                         build time (throw-only legacy EH).
wasm3/                   pinned upstream; patches/wasm3/0001 applied at
                         build time (v128 opaque slot).
WasmEdge/                pinned at 3ad922d6; patches/wasmedge/0001-0028
                         applied at build time (27-patch Apple-mobile
                         enablement stack).
wasmz/                   Ray-D-Song/wasmz upstream; patches/wasmz/{0001
                         Zig 0.16 stdlib port, 0002 arm64_32 watchOS
                         support} applied at build time.
zwasm/                   clojurewasm/zwasm upstream; patches/zwasm/0001
                         arm64_32 watchOS support applied at build time.
mach2/                   pinned to fork's arm64_32-apple-watchos branch.
target-lexicon/          pinned to fork's arm64_32-apple-watchos branch.
porffor/                 pinned to upstream main (JS→wasm AOT compiler).
sightglass/              pinned to upstream main (sqlite3.wasm source).
```

## Where to read more

- **[AGENTS.md](AGENTS.md)** — deep agent context: toolchain pinning,
  build commands, workload registration pattern, measurement
  methodology, xctrace gotchas, QoS env var, the Pulley dispatch
  loop mode selection, **device-side stabilization status**
  (per-device per-runtime completion matrix), and the **Open
  follow-up — WAMR fast-interp legacy EH (full spec)** section with
  the ~720 LOC scope for the next session.
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

## Current upstream-PR state

Wasmtime fork stack (each branch stacks on the previous):

- **upstream PR [bytecodealliance/wasmtime#13259](https://github.com/bytecodealliance/wasmtime/pull/13259)**:
  unwinder arm64_32 inline-asm format fix. **Merged.**
- **fork PR [#2](https://github.com/rebeckerspecialties/wasmtime/pull/2)
  (`table-mutability-tracking`)**: per-table mutability + four
  `call_indirect` dispatch elisions. 11 commits, 2227 + 16 tests pass.
- **fork PR [#4](https://github.com/rebeckerspecialties/wasmtime/pull/4)
  (`claude/pulley-fusion-xband-brif`)**: opcode-fusion phases 1–4 +
  trap-on-null correctness fix. 12 commits. Watch SE2 vtable_bi
  −7.68 % vs phase 3.
- **`accurate-graphql-needs-legacy-exceptions`**: gates
  `LEGACY_EXCEPTIONS` through `Config::validate`. Required so the
  accurate-Porffor `graphql-validation` workload can LOAD on Pulley
  (Pulley codegen for `try`/`catch` is the integration test for the
  WAMR full-spec EH work).

Runtime-fork PRs (carried in `patches/<runtime>/` until merged):

- **[rebeckerspecialties/wasm-micro-runtime#1](https://github.com/rebeckerspecialties/wasm-micro-runtime/pull/1)** —
  throw-only legacy exception handling for FAST_INTERP. Enables Porffor-
  compiled JS workloads (561 compiler-inserted throws) to run on WAMR.
- **[rebeckerspecialties/wasm3#1](https://github.com/rebeckerspecialties/wasm3/pull/1)** —
  v128 as opaque slot so modules with v128 locals parse on wasm3 (also
  upstream as [wasm3#559](https://github.com/wasm3/wasm3/pull/559)).
- **[Ray-D-Song/wasmz#3](https://github.com/Ray-D-Song/wasmz/pull/3)** —
  Zig 0.16 stdlib port + arm64_32 watchOS device support.
- **[clojurewasm/zwasm#97](https://github.com/clojurewasm/zwasm/pull/97)** —
  arm64_32 watchOS device support.
- **[WasmEdge/WasmEdge#4802](https://github.com/WasmEdge/WasmEdge/pull/4802)** —
  partial upstream of the patches/wasmedge/ Apple-mobile stack.

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
