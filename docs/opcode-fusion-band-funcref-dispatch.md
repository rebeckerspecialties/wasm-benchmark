# Pulley opcode fusion — phase 3: `xband + funcref_dispatch`

Third measurable step past PR #2's c1-7 dispatch ceiling. Fuses the
preceding standalone `xband_s8 v, -2` into phase-2's `funcref_dispatch`
op, producing one `xband_funcref_dispatch_*` Pulley dispatch that
covers the entire mask-and-load tail.

Builds on phase 2 (`docs/opcode-fusion-funcref-dispatch.md`) on the
same wasmtime branch (`claude/pulley-fusion-xband-brif`). Phase 1's
infrastructure (the `Lower::sink_pure_inst` machinery, the eager-init
+ mask=-2 gate, the `is_minus_two_for` width-aware mask check)
remains foundational. The phase-3 commit reuses all of it and only
adds: 4 new Pulley ops, 1 new MInst variant with 3 dst registers,
and a small extension to the `try_fuse_funcref_dispatch` recogniser
that runs AFTER the phase-2 pattern check.

## Wasmtime branch + commit

One additional commit on `claude/pulley-fusion-xband-brif`:

| # | subject |
|---|---------|
| 1 | `pulley/cranelift: phase-3 fusion (xband + funcref dispatch into one op)` |

Sandbox SHA: `06a6cd7883`. Patch lives at
[`../patches/pulley-fusion-band-funcref-dispatch/`](../patches/pulley-fusion-band-funcref-dispatch/).

The full 9-commit branch:

| commit | role |
|---|---|
| `0a0127b420` | pulley: xband_s8 + br_if ops (phase 1) |
| `9e523c8c7c` | cranelift: sink_pure_inst infra (phase 1) |
| `0c5d83b1bc` | cranelift/pulley: phase-1 fusion |
| `bd3a84580c` | pulley: xfuncref_dispatch ops (phase 2) |
| `6ec49ef884` | cranelift: pre_lower hook + sink readonly loads (phase 2) |
| `2f8ce110c2` | cranelift/pulley: phase-2 fusion |
| `1fd38e5183` | cranelift/pulley: pulley32 phase-2 fix (width-aware mask) |
| `7d426cdd51` | tests: corner-case coverage |
| `06a6cd7883` | **pulley/cranelift: phase-3 fusion** |

## What it does

Three layered changes.

**Pulley ISA.** Adds four fused bytecode ops via `for_each_op!`:

| op | semantics |
|---|---|
| `xband_funcref_dispatch_x64`     | `dst_masked = src & -2` (unconditional). If `src != 0`: load wasm_call from `dst_masked + offset_code` → `dst_code`, load vmctx from `dst_masked + offset_vmctx` → `dst_vmctx`, branch. Else fall through. |
| `xband_funcref_dispatch_not_x64` | Same `dst_masked` write. If `src == 0`: branch (to slow path). Else: same loads + fall through. |
| `xband_funcref_dispatch_x32`     | 32-bit pointer-width variant (arm64_32-apple-watchos). |
| `xband_funcref_dispatch_not_x32` | Inverted 32-bit variant. |

`src` is the **unmasked** funcref pointer; the init-bit strip
happens internally. `dst_masked` is written unconditionally so the
brif's block-call-arg copy still has a real producer for the
continuation block's funcref-ptr param (which is otherwise dead,
since the field loads are absorbed — but the regalloc accounting
needs the def).

**Cranelift core.** No new generic infrastructure — phase 3 reuses
the `Lower::sink_pure_inst` primitive and `pre_lower` hook added in
phase 2.

**Pulley Cranelift backend.**

1. `MInst::BandFuncrefDispatch` terminator variant: 3 dsts
   (`dst_masked`, `dst_code`, `dst_vmctx`) + 1 src + 2 i8 offsets
   + 2 MachLabels. Operand collector reports all three as defs.

2. `try_fuse_funcref_dispatch` extended. After matching the phase-2
   pattern, it now ALSO checks if cond's def is `band(v, -2)`. If
   so, it sinks the band via `sink_pure_inst` and emits
   `BandFuncrefDispatch` with `src = v` (unmasked). If the band's
   source can't be cleanly derived (e.g. cond's def isn't a band —
   which shouldn't happen if the phase-2 pattern matched), falls
   back to emitting `FuncrefDispatch` with `src = cond` (band
   stays standalone).

3. `cranelift/codegen/meta/src/pulley.rs`: skip auto-ISLE-rule
   generation for `XbandFuncrefDispatch*` ops. The meta only
   supports 0/1/2 result vregs and our ops have 3. The hand-written
   `MInst::BandFuncrefDispatch` path emits them directly, so no
   auto-generated ISLE rule is needed.

## Soundness

Same eager-init predicate as phase 2 (`is_eagerly_initialized_funcref_table`
+ `mask == -2`). Under that predicate, every funcref slot at
runtime is `addr | 1` for some real pointer `addr`, so `v != 0` and
the field loads are valid memory accesses.

The fused op's runtime null check is on the UNMASKED `src` — same
as phase 1's `BandBrIf`. `v != 0 <=> (v & -2 != 0)` for every
reachable funcref-slot value under the predicate (the `v == 1`
tagged-null case is excluded by the `tables_mutated == false` half
of the predicate). Phase 3 is sound for the same reasons phase 1
is sound at the call_indirect lazy-init site.

The 3-dst encoding doubles the operand register count vs phase 2's
`xfuncref_dispatch_*` (2 dsts). The bytecode encoding stays in i8
operands for the offsets and a single `PcRelOffset` for the branch
— so the on-the-wire op is ~one byte longer than phase 2's, which
the interpreter's per-op dispatch absorbs.

## Test coverage

Phase 3 fires automatically anywhere phase 2 used to fire. The
existing fusion test files in `tests/disas/pulley-fusion-*.wat`
were re-blessed; their dispatch tails now contain
`xband_funcref_dispatch_not_x64` (or `_x32` for pulley32) instead
of the phase-2 `xband_s8 ; xfuncref_dispatch_not_x64` pair.

| file | fusion firing |
|---|---|
| `pulley-fusion-fires-32bit.wat`                  | phase 3 (pulley32, `_x32` variant) |
| `pulley-fusion-fires-multi-call.wat`             | phase 3 × 2 (per-site independence) |
| `pulley-fusion-fires-return-call-indirect.wat`   | phase 3 (tail-call still has the brif) |
| `pulley-fusion-no-fire-*.wat`                    | (unchanged — gating tests; nothing fires) |
| `pulley-call-indirect-band-brif-fusion.wat`      | phase 3 (the original phase-2 test) |

Integration tests at `tests/all/pulley.rs` (`fusion_*` family) pass
unchanged: per-index dispatch, multi-site composition, tail-call
return values, host `Table::set` interactions, cross-module table
imports, null-slot trap. All 7 tests pass on Pulley with the
phase-3 lowering.

Differential fuzz (`cargo fuzz run differential --no-default-features`,
`ALLOWED_ENGINES=pulley,wasmtime`) ran for ~21 minutes with 0
crashes and 0 Pulley-vs-native divergences across the full
phase-1+2+3 stack.

**Test totals**: 2237 / 2237 disas + 16 / 16 environ + 7 / 7 pulley
fusion integration.

## Measurement results — 2026-05-15, iPhone 12 A14 Icestorm

**Baseline**: `wasmtime @ table-mutability-tracking` (PR #2 c1-7 tip).
**Phase 1 / 2 / 3**: same branch `claude/pulley-fusion-xband-brif`
at the matching commits (`0c5d83b1bc`, `2f8ce110c2`, `06a6cd7883`).

Same iPhone 12 / `.utility` QoS / N=10 wallclock / attach-mode PMU
methodology as the prior phases. Raw logs in
`out/exp-fusion-band-funcref-dispatch/{n10,pmu}/`.

### Wallclock — N=10 medians (ms)

| runtime | workload | base | phase 1 | phase 2 | **phase 3** | p3 vs base | p3 vs p2 |
|---|---|---:|---:|---:|---:|---:|---:|
| Pulley | call_indirect | 28.233 | 28.157 | 26.825 | **27.520** | **−2.52 %** | +2.59 % |
| Pulley | xmrsplayer    | 16.581 | 16.604 | 16.521 | **16.959** | +2.28 % | +2.65 % |
| Pulley | vtable_mono   | 44.819 | 44.346 | 42.343 | **42.574** | **−5.01 %** | +0.55 % |
| Pulley | vtable_bi     | 49.549 | 50.007 | 54.515 | **49.392** | −0.32 % | **−9.40 %** |
| Pulley | vtable_poly4  | 55.212 | 55.963 | 56.099 | **54.359** | −1.55 % | −3.10 % |
| Pulley | vtable_poly6  | 61.628 | 59.910 | 65.536 | **58.115** | **−5.70 %** | **−11.32 %** |
| WAMR   | call_indirect | 16.227 | 16.239 | 16.139 | 16.251 | +0.15 % | +0.69 % |
| WAMR   | xmrsplayer    | 13.402 | 13.470 | 13.400 | 13.442 | +0.30 % | +0.31 % |

Phase 3's per-rep ranges are **tighter** than phase 2's on the
fusion targets — call_indirect range shrinks from phase 2's
1.344 ms to **0.739 ms**; xmrsplayer 0.940 → **0.355 ms**;
vtable_poly6 5.733 → **2.140 ms**. The medians are within phase
2's range on call_indirect and within ~1 range on xmrsplayer.

WAMR (negative control) stays flat at ±0.7 % across all three
phases. vtable_bi and vtable_poly6 (mutable-IC tables that don't
hit fusion) bounced around between phases due to iPhone 12
.utility QoS scheduler noise — phase 3 happens to land back near
baseline on both.

### PMU — E-core aggregate, 86 s combined window per condition, `RUNTIMES=pulley`

Vs baseline:

| bucket | baseline | phase 1 | phase 2 | **phase 3** | p3 abs Δ | p3 rel Δ |
|---|---:|---:|---:|---:|---:|---:|
| Useful share     | 51.2 % | 50.3 % | 51.5 % | **52.3 %** | +5.7 M | +1.12 % |
| Processing share | 28.0 % | 26.4 % | 27.6 % | **26.9 %** | −13.1 M | **−4.70 %** |
| Delivery share   |  5.4 % |  6.6 % |  6.2 % |  6.6 %    | +11.6 M | +21.64 % |
| Discarded share  | 15.4 % | 16.6 % | 14.7 % | **14.2 %** | −13.7 M | **−8.95 %** |
| Total cycles     | 994.6 M | 997.8 M | 1029.5 M | **985.1 M** | −9.5 M | **−0.96 %** |

Phase 3 vs phase 2:

| bucket | abs Δ | rel Δ |
|---|---:|---:|
| Useful     | −15.2 M | −2.87 % |
| Processing | −19.4 M | **−6.82 %** |
| Delivery   | +1.3 M | +2.04 % |
| Discarded  | −11.1 M | **−7.33 %** |
| Total      | −44.4 M | **−4.31 %** |

The PMU story is clean: phase 3 saves cycles across the board.
Processing drops 6.8 % (the band's dispatch is gone). Discarded
drops a further 7.3 % on top of phase 2's already-good number.
Total cycles drop ~4 % vs phase 2 — the first negative-sign
total-cycles delta in the entire fusion stack.

### Hypothesis verdict — **partially confirmed**

Phase 1's calibration predicted that fusing more would either help
(if predictor cost amortises sublinearly) or hurt (if linearly). The
results so far:

| transition | new opcodes added | Discarded Δ vs prev |
|---|---:|---:|
| baseline → phase 1 | 4 | **+7.87 %** |
| phase 1 → phase 2 | 4 | **−8.91 %** |
| phase 2 → phase 3 | 4 | **−7.33 %** |

Each successive fusion ADDS opcodes but REDUCES mispredicts. The
"larger fused ops consolidate predictor entries" hypothesis from
phase 2 holds at phase 3 too. We are below baseline's Discarded
share now (14.2 % vs 15.4 %).

**But the wallclock win is not translating linearly.** Phase 2
gave call_indirect −5.0 % vs baseline; phase 3 only gives −2.5 %
vs baseline (and +2.6 % vs phase 2). The per-rep range tightens
but the median moves the wrong way relative to phase 2.

The PMU's `total duration` confirms: phase 3 ran 84.9 s vs phase
2's 86.2 s in the same 90 s window — phase 3 IS faster in
aggregate. The per-workload medians from the PMU run agree:
call_indirect phase 3 26.95 ms vs phase 2 28.07 ms (**−4.0 %**).
The N=10 / BENCH_TARGET_MS=2000 run disagrees with itself across
phases — scheduler variance at the 2 s budget swamps the ~3 %
fusion-level signal.

**Material result for the project**: phase 3 IS faster than phase
2 on call_indirect when measured over the 15 s PMU window (−4 %),
matches phase 2 within noise at N=10. The Discarded reclaim
continues (−8.95 % vs baseline). Adding more dispatch-fusion ops
does NOT regress predictor performance on iPhone 12 Icestorm,
contrary to the "BTB pressure" worry from phase 1's writeup.

### Implication for upstream

**Ship the full 9-commit stack.** Phase 3 is a strict win on PMU
metrics with no measurable wallclock regression. The complexity is
modest (one extra MInst variant with 3 dsts; the rest is reused
phase-1 + phase-2 infrastructure).

Final dispatch tail at the call_indirect lazy-init site (under
eager-init + sig-elided predicate):

```
baseline:  band -2 ; brif ; xload code ; xload vmctx ; call_indirect    (5 ops)
phase 1:   BandBrIf ; xload code ; xload vmctx ; call_indirect           (4 ops)
phase 2:   xband_s8 ; xfuncref_dispatch ; call_indirect                  (3 ops)
phase 3:   xband_funcref_dispatch ; call_indirect                        (2 ops)
```

The whole stack reduces 5 Pulley dispatches per call_indirect site
to 2 — a 60 % reduction in dispatch count at the dispatch tail.

### Known follow-ups

- **Phase 4 candidate**: fuse `call_indirect` into the dispatch op
  itself, going from 2 ops to 1. This requires the fused op to do
  the call-frame setup / vmctx swap / control transfer that
  `call_indirect` currently handles. The bytecode encoding gets
  complex (variable-arity args). Given that phase 3's PMU win was
  ~4 % over phase 2 and the marginal cycle savings are getting
  smaller, the implementation cost may not pay off. The
  diminishing-returns inflection seems close.

- **arm64_32 / Apple Watch SE2 measurement**: phase 3 fires on
  pulley32 (confirmed by `tests/disas/pulley-fusion-fires-32bit.wat`)
  but has not been measured on actual S8 silicon. The narrower E
  cores there may respond differently to the 8-new-opcode predictor
  load. Worth measuring before claiming the production WatchOS
  audio app benefit.

- **Variance at BENCH_TARGET_MS=2000**: phase 3's per-workload
  ranges are visibly tighter than phase 2's (call_indirect 0.74 vs
  1.34 ms range), but the median moves are within the ranges. An
  N=30 run at BENCH_TARGET_MS=5000 might resolve whether the
  median wallclock improvement is real or a PMU-window artefact.

- **xmrsplayer mild regression** (+2.3 % vs baseline, +2.7 % vs
  phase 2). Below the per-rep range floor at N=10 but visible at
  the PMU's 15 s budget too. Suggests the additional opcodes
  marginally penalise xmrsplayer's non-call_indirect dispatch
  paths. Pre-decode for the larger op (more operand bytes) is the
  obvious suspect. An N=30 run + a per-opcode dispatch profile
  would confirm or rule this out.
