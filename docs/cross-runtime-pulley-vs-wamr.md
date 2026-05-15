# Pulley (phase 3 fusion) vs WAMR fast-interp — iPhone 12, 2026-05-15

Cross-runtime wallclock comparison after the 3-phase opcode-fusion
stack landed on the `claude/pulley-fusion-xband-brif` wasmtime branch.
Same iPhone 12 A14 Icestorm, `.utility` QoS, N=10 medians,
`BENCH_TARGET_MS=2000`, one full app launch per rep. Runtimes:

- **Pulley** — wasmtime 45.0.0 with the 9-commit phase-1+2+3 fusion
  stack (commit `06a6cd7883`), `--cfg=pulley_tail_calls`,
  linker-plugin-lto + embed-bitcode for cross-language LTO with
  Xcode 26's clang.
- **WAMR** — wasm-micro-runtime, `WAMR_BUILD_INTERP=1
  WAMR_BUILD_FAST_INTERP=1 WAMR_BUILD_AOT=0 WAMR_BUILD_JIT=0
  WAMR_BUILD_FAST_JIT=0 WAMR_BUILD_EXCE_HANDLING=0`. The
  preprocessed-bytecode (fast) interpreter — apples-to-apples
  comparison vs Pulley's `become`-based tail-call interpreter.

Both runtimes are pure interpreters (no native codegen) — the
App-Store-eligible space we care about.

**Wallclock methodology**: per-iteration `median` from the harness's
`BenchReport`. This is the **steady-state iteration time**. Module
load (which is where Cranelift's phase-1/2/3 fusion lowering runs
for Pulley, and where WAMR's bytecode preprocessing happens) is
reported separately as `load_ns` and **excluded** from the median.
Module-load latency is a separate optimisation track and is not
covered here.

## Results

| workload | Pulley | WAMR | Pulley/WAMR | per-rep range Pulley/WAMR (ms) |
|---|---:|---:|---:|---:|
| `call_indirect` (200 K dispatches)   | 27.317 ms | 16.337 ms | **1.67×** | 1.39 / 0.34 |
| `xmrsplayer` (1024-frame buffer)     | 16.954 ms | 12.397 ms | **1.37×** | 0.99 / 0.45 |
| `graphql-validation (AS)`            | 14.660 ms |  9.347 ms | **1.57×** | 0.25 / 0.21 |
| `graphql-validation (Porffor)`       | 15.224 ms | — (trap)  | — | 0.27 / —  |
| `vtable_mono` (200 K)                | 43.118 ms | 24.196 ms | **1.78×** | 3.91 / 0.60 |
| `vtable_bi`   (200 K)                | 51.767 ms | 27.834 ms | **1.86×** | 5.03 / 0.19 |
| `vtable_poly4` (200 K)               | 54.035 ms | 31.170 ms | **1.73×** | 0.61 / 1.86 |
| `vtable_poly6` (200 K)               | 58.446 ms | 35.430 ms | **1.65×** | 3.07 / 2.58 |

**Across the apples-to-apples workloads, WAMR is 1.37–1.86× faster
than our optimised Pulley.** The gap is largest on the vtable
suite (1.65–1.86×) and the synthetic `call_indirect` (1.67×) —
exactly the dispatch-heavy workloads our fusion targeted. The
graphql-AS gap (1.57×) is similar; the only workload where Pulley
beats WAMR-by-default is `graphql-validation (Porffor)`, which
WAMR can't run at all without rebuilding with
`WAMR_BUILD_EXCE_HANDLING=1`.

## WAMR-side load failures

Two workloads where WAMR fails to load or run, even with the bumped
8 MiB initial heap added to the runner:

- **graphql-validation (Porffor)** —
  `wasm_runtime_load failed: WASM module load failed: invalid section id`
  Porffor compiles JS `try`/`catch` to the wasm-exceptions proposal.
  Our WAMR build has `WAMR_BUILD_EXCE_HANDLING=0`, so the
  `wasm_exceptions` section is rejected at load time. Pulley
  (wasmtime, `config.wasm_exceptions(true)`) handles it fine. **A
  WAMR rebuild with exception support enabled would let this run**,
  but is currently out of scope.

- **graphql-validation (AS)** — when WAMR was instantiated with the
  generic runner's previous 64 KiB heap, AS's `~start` TLSF
  allocator would `memory.grow` to its initial 1 MiB slab, fail,
  and trap `unreachable`. Bumping the WAMR generic runner to a
  32 KiB stack + 8 MiB heap (`crates/benchmark-core/src/wamr.rs`)
  fixes this; the workload now runs at 9.35 ms median (above).

## Reading the numbers — what they tell us

1. **WAMR's fast-interp is structurally faster than Pulley** for
   dispatch-bound code on iPhone 12 Icestorm. The fusion stack
   narrowed the gap from PR #2's c1-7 baseline (Pulley call_indirect
   28.23 ms) to phase 3 (27.32 ms), a 3 % reduction. WAMR sits at
   16.34 ms — 41 % below Pulley. **The remaining gap (1.67× on
   call_indirect) is *not* closable with more Cranelift-emit-time
   fusion.** WAMR's win is its load-time register-IR rewrite — it
   compiles wasm to a fundamentally different bytecode shape with
   fewer dispatches per source-level wasm op. Pulley's bytecode IS
   essentially the wasm op stream with light fusion; structurally
   it can never match WAMR's dispatch count.

2. **The IC investigation closure was correct.** Earlier
   `out/exp-c-device/ic/PATH-A-RESULTS.md` showed the IC's
   xmrsplayer win on iPhone 12 was −3.78 % vs the c1-7 baseline.
   The phase-1+2+3 fusion delivered −2.5 % on call_indirect (the
   highest-dispatch-density workload) — in the same range. **The
   fusion track and the IC track both hit a similar ceiling because
   they're optimising the same dimension: bytecode dispatches per
   wasm op.** WAMR's gap (~40 %) is the order-of-magnitude
   structural difference between an interpreter that dispatches on
   wasm ops vs. one that pre-compiles to a register IR.

3. **Vtable polymorphism is roughly linear in Pulley but
   roughly linear-and-shallower in WAMR.** Per-dispatch dispatch
   counts on these workloads:

   | shape | Pulley | WAMR | ratio |
   |---|---:|---:|---:|
   | mono     | 43.1 ms | 24.2 ms | 1.78× |
   | bi       | 51.8 ms | 27.8 ms | 1.86× |
   | poly4    | 54.0 ms | 31.2 ms | 1.73× |
   | poly6    | 58.4 ms | 35.4 ms | 1.65× |

   The ratio narrows slightly as polymorphism increases. Both
   runtimes scale linearly in the IC's "miss rate" sense, but
   Pulley's per-dispatch overhead is higher to start with, so the
   absolute milliseconds-per-extra-target are also higher
   (`vtable_poly6 - vtable_mono` = 15.3 ms on Pulley vs 11.2 ms on
   WAMR).

4. **For the WatchOS audio app**: xmrsplayer at 16.95 ms (Pulley)
   vs 12.40 ms (WAMR) per 1024-frame buffer. The 44.1 kHz × 1024
   = 23.2 ms frame budget. Pulley uses **73 %** of frame budget;
   WAMR uses **53 %**. Apple Watch SE2 (S8) is 2–3× slower per
   core than iPhone 12 — so the same workload on S8 would be
   ~35–50 ms (Pulley) or ~25–37 ms (WAMR) per buffer, vs the same
   23.2 ms budget. **Pulley is over-budget on S8 even with the
   3-phase fusion**; WAMR is on-budget. App-Store eligibility
   forces Pulley over WAMR for this product, but the gap is a
   real product-quality risk that the fusion track alone can't
   close.

## Methodology notes

- The Pulley **`load_time`** for the workloads above (median across
  the rep, from the harness `load=N ms` field) is approximately:

  | workload | Pulley load |
  |---|---:|
  | call_indirect | ~11 ms |
  | xmrsplayer | ~213 ms |
  | graphql-AS | ~137 ms |
  | graphql-Porffor | ~308 ms |
  | vtable_* | ~10 ms |

  This includes the entire Cranelift lowering pipeline (including
  the phase-1/2/3 fusion peephole work at lowering time) plus
  Pulley bytecode emission. It is **not** reflected in the median
  iter time in the comparison table above. Module-load latency
  optimisation is a separate track.

- WAMR's load time for these is dramatically smaller (0.16 ms for
  vtable, 5.95 ms for graphql-AS, ~6.9 ms for xmrsplayer) because
  WAMR's bytecode preprocessing is much cheaper than Cranelift's
  full lowering. That difference doesn't appear in the steady-state
  comparison but would matter for cold-start scenarios — also out of
  scope for this measurement round.

- The harness uses `pick_iters(warm_call_time, BENCH_TARGET_MS=2000)`
  to choose iteration count per workload; medians are over those
  iterations within a single rep, and the cross-rep median over 10
  reps gives the per-workload number. Per-rep ranges in the table
  are `(max(rep_median) − min(rep_median))` across the 10 reps.

- The phase-3 Pulley results above are within noise of the
  prior single-runtime phase-3 measurement (see
  `docs/opcode-fusion-band-funcref-dispatch.md` → "Measurement
  results — 2026-05-15"). No re-baseline of Pulley was performed;
  the wasmtime branch is the same `06a6cd7883` tip.

## Raw data

- N=10 per-rep logs: `out/exp-cross-runtime/n10/iphone12-phase3-r{1..10}.log`
- Parse script: `scripts/parse_n10.py out/exp-cross-runtime/n10`
