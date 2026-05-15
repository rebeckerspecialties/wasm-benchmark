# Pulley opcode fusion — `xband + brif` at the call_indirect lazy-init site

First measurable step past PR #2's c1-7 dispatch ceiling. Fuses the
`band -2 + brif` pair emitted by `get_or_init_func_ref_table_elem`
into a single Pulley dispatch when the table is eagerly initialized.

Tracks the `xband_brif_eq_zero` proposal from PR #2's "Next branch —
opcode fusion" section.

## Wasmtime branch + commits

Branched from `table-mutability-tracking` (PR #2). The three commits
are checked in as patches under
[`../patches/pulley-fusion-xband-brif/`](../patches/pulley-fusion-xband-brif/)
because this branch was first prepared in a cloud sandbox without
push creds for `rebeckerspecialties/wasmtime`. Apply locally:

```sh
cd wasmtime
git checkout -b claude/pulley-fusion-xband-brif table-mutability-tracking
git am ../patches/pulley-fusion-xband-brif/*.patch
git push -u origin claude/pulley-fusion-xband-brif
```

| # | subject |
|---|---------|
| 1 | `pulley: add xband_s8 + br_if fused dispatch ops` |
| 2 | `cranelift: Lower::sink_pure_inst — absorb pure ALU ops into terminators` |
| 3 | `cranelift/pulley: fuse band+brif at call_indirect lazy-init site` |

Sandbox SHAs were `450400a`, `5f057da`, `7df73dc` — after `git am`
locally the SHAs will differ because the user's signing key
re-signs the commits. The patch content is identical.

## What it does

Three layered changes.

**Pulley ISA (`450400a`).** Adds four fused bytecode ops via
`for_each_op!`:

| op | semantics |
|---|---|
| `xband32_s8_br_if_x32`      | `low32(dst) = low32(src) & sext(mask)`; if `low32(src) != 0` jump |
| `xband32_s8_br_if_not_x32`  | same compute; if `low32(src) == 0` jump (inverted form) |
| `xband64_s8_br_if_x64`      | `dst = src & sext(mask)`; if `src != 0` jump |
| `xband64_s8_br_if_not_x64`  | same compute; if `src == 0` jump (inverted form) |

Forward and inverted pairs let MachBuffer flip branch direction for
fallthrough optimization without losing the fusion. The 32-/64-bit
variants cover the pointer-width split across aarch64-apple-darwin/-ios
and arm64_32-apple-watchos.

**Cranelift core (`5f057da`).** Adds `Lower::sink_pure_inst`. Distinct
from the existing `sink_inst`:

- `sink_inst` is for side-effecting CLIF insts (loads, stores) being
  merged into another inst. Asserts `has_lowering_side_effect` and that
  no results have lowered uses.
- `sink_pure_inst` is for non-side-effecting ALU ops whose value flows
  into a fused terminator's output operand. Lowered uses are *expected*
  (the absorbing MachInst writes to the value's vreg directly), so the
  block-call-arg machinery downstream observes the right value with no
  SSA violation.

This was the missing piece. Without it, a "compute + branch" MachInst
that wants to share a vreg with a pre-existing pure CLIF inst causes a
double-write (regalloc SSA violation), because the pure inst's standard
lowering still runs and writes the same vreg.

**Pulley Cranelift backend (`7df73dc`).** Three coupled changes:

1. `func_environ::get_or_init_func_ref_table_elem`: when
   `is_eagerly_initialized_funcref_table(table_index)`, the brif tests
   `value_masked` (the band's result) instead of `value` (the raw
   loaded funcref). Semantically equivalent on every slot value
   reachable in eagerly-initialized tables; the only diverging case
   (`v == 1`, explicit tagged-null from `table.fill(null)`) is excluded
   by the `tables_mutated == false` half of the predicate.

2. New `MInst::BandBrIf` terminator variant in `inst.isle` with
   `dst / src / mask / size / taken / not_taken`. `is_term` →
   `Branch`. Operand collector reports dst as def, src as use, so
   regalloc allocates the band-result vreg normally.

3. Rust-side `try_fuse_band_brif` in `pulley_shared::lower`, called
   from `lower_branch` before ISLE dispatch. Recognises `brif(c)` where
   `c = band(v, -2)` — restricted to exactly the funcref init-bit-strip
   mask — and emits `BandBrIf { dst: vreg(c), src: vreg(v), mask: -2, size, taken, not_taken }`,
   and `sink_pure_inst(band_inst)`. The band's standalone lowering is
   then skipped in `lower_clif_block`, leaving a single MachInst
   producing the masked-funcref vreg.

   The `mask == -2` gate is load-bearing for soundness: the fused op
   tests the UNMASKED `src` for non-zero, not the masked `dst`. That
   equivalence (`(v & mask != 0) ⟺ (v != 0)`) holds for mask = -2
   on every reachable funcref-slot value under the eager-init
   predicate, but fails for general user-code masks (e.g. `band(v,
   127)` or `band(v, 60)`). Without the gate, real-world workloads
   like xmrsplayer would silently flip branch direction on unrelated
   user-code `band+brif` sites. Restricting to mask = -2 keeps the
   lowering sound by construction and confines the fusion to the
   call_indirect lazy-init shape it was designed for.

`MInst::BandBrIf::emit` follows `Inst::BrIf`'s scaffolding: forward
encoding goes into the buffer, inverted encoding goes to
`add_cond_branch` for MachBuffer's fallthrough flip, plus an
unconditional jump to `not_taken`.

## Soundness

The fusion is gated on `Module::is_eagerly_initialized_funcref_table`,
the predicate already established by PR #2. Under that predicate, every
funcref slot at runtime is `addr | 1` for some real pointer `addr`,
because:

- The table is immutable (`tables_mutated[idx] == false`), so no
  `table.fill / table.set / table.copy / table.grow / table.init` can
  produce a tagged-null `Some(1)` value.
- The precomputed elem image covers the full minimum-size range with no
  `FuncIndex::reserved_value()` sentinels, so the eager-init pass in
  `Instance::initialize_tables` writes a real tagged pointer into
  every slot at instance creation.

In that regime, `(v & -2) != 0 ⟺ v != 0` holds — the fused op's "test
masked value" semantics agree with the original brif's "test loaded
value" semantics, and the IR rewrite (which is what unlocks the ISLE
pattern match) is semantically transparent.

The `b8646ee` (constant-index direct call) and `13cafc7` (sig-check
elision) commits earlier in PR #2's stack already short-circuit
call_indirect through eagerly-initialized tables for the cases they
catch. The fusion here targets the residual path that still emits
`band + brif + call_indirect` — primarily the variable-index calls
through immutable tables, the IC-style dispatch case in
`typed-funcrefs-eager-init.wat`.

## Test coverage

- **Cranelift CLIF diff** (`tests/disas/call-indirect-immutable-elide-null.wat`):
  one-line update pinning the new IR shape (`brif v13` in place of
  `brif v12`).
- **Pulley bytecode disasm** (`tests/disas/pulley-call-indirect-band-brif-fusion.wat`):
  new test, pins the fused `xband64_s8_br_if_not_x64` in the
  call_indirect dispatch tail (inverted form, since MachBuffer flipped
  branch direction so the hot path is the fall-through).
- **wasmtime-environ table_mutability suite**: 16 / 16 pass, unchanged.
- **Cranelift disas suite**: 2228 / 2228 pass (2227 pre-existing + the
  new fusion test).

## Predicted measurement (TBD)

From PR #2's description of the fusion proposal:

- `~−7 %` Useful cycles
- `~−5 %` Discarded cycles

Predictor anchor preserved: the fused op is still a conditional branch
on the same condition value, so the c1-8 mispredict regression
(`Discarded +6.5 %`) should not reappear. The br_if encoding is the
anchor; only the preceding ALU op is folded in.

## Measurement results — 2026-05-14, iPhone 12 A14 Icestorm

**Baseline**: `wasmtime @ table-mutability-tracking` (PR #2 c1-7 tip).
**Fusion**: `wasmtime @ claude/pulley-fusion-xband-brif` (mask = -2 gate;
see "Soundness" above for why the gate was tightened from i8-fittable to
exactly -2 during this run).

**Configuration**: iPhone 12, iOS 26.5, `.utility` QoS, six workloads
per rep (Pulley `call_indirect`, `xmrsplayer`, `vtable_{mono,bi,poly4,poly6}`
+ WAMR `call_indirect`, `xmrsplayer` as a side-channel). Wallclock:
`BENCH_TARGET_MS=2000`, N=10 reps, each rep one full app launch. PMU:
`BENCH_TARGET_MS=15000`, attach-mode xctrace per AGENTS.md → "PMU /
xctrace gotchas" (the 26.5 `--launch` regression workaround), filtered
to `RUNTIMES=pulley`. Raw per-rep logs in
`out/exp-fusion-xband-brif/{n10,pmu}/`.

### Wallclock — N=10 medians (ms)

| runtime | workload | baseline | fusion | Δ ms | Δ % | per-rep range |
|---|---|---:|---:|---:|---:|---:|
| Pulley | call_indirect      | 28.233 | 28.157 | −0.075 | **−0.27 %** | base 1.43, fused 1.96 |
| Pulley | xmrsplayer         | 16.581 | 16.604 | +0.023 | +0.14 % | base 0.89, fused 0.63 |
| Pulley | vtable_mono        | 44.819 | 44.346 | −0.472 | −1.05 % | base 0.46, fused 7.47 |
| Pulley | vtable_bi          | 49.549 | 50.007 | +0.457 | +0.92 % | base 2.97, fused 3.71 |
| Pulley | vtable_poly4       | 55.212 | 55.963 | +0.752 | +1.36 % | base 2.59, fused 5.28 |
| Pulley | vtable_poly6       | 61.628 | 59.910 | −1.718 | −2.79 % | base 3.65, fused 3.62 |
| WAMR   | call_indirect      | 16.227 | 16.239 | +0.011 | +0.07 % | base 0.33, fused 0.63 |
| WAMR   | xmrsplayer         | 13.402 | 13.470 | +0.068 | +0.51 % | base 0.47, fused 0.49 |

Every signed delta is within or smaller than the per-rep range
(1–7 ms vs 28–62 ms medians). **Wallclock is flat at N=10 — no
measurable improvement on either fusion target, no measurable damage
on the side-channel workloads.**

### PMU — E-core aggregate, 86 s combined window per condition

| bucket | baseline | fusion | Δ abs | Δ % | predicted |
|---|---:|---:|---:|---:|---:|
| Useful     | 509,396,192 | 502,346,090 |  −7,050,102 | **−1.38 %** | −7 % |
| Processing | 278,098,185 | 263,844,023 | −14,254,162 | −5.13 %     | — |
| Delivery   |  53,471,449 |  65,882,235 | +12,410,786 | **+23.21 %** | — |
| Discarded  | 153,615,765 | 165,706,620 | +12,090,855 | **+7.87 %** | −5 % |
| TOTAL      | 994,581,591 | 997,778,968 |  +3,197,377 | +0.32 %     | — |

Share-of-cycles shift (E-core):

| bucket | baseline share | fusion share |
|---|---:|---:|
| Useful     | 51.2 % | 50.3 % |
| Processing | 28.0 % | 26.4 % |
| Delivery   |  5.4 % |  6.6 % |
| Discarded  | 15.4 % | 16.6 % |

### Hypothesis verdict — **falsified**

The hypothesis was: Useful drops ~−7 %, Discarded stays
flat-to-negative, the c1-8 brif-anchor regression (`Discarded +6.5 %`)
does not reappear because the fused op is still a conditional branch
on the same value. The measurement contradicts every part of this:

- **Useful: −1.38 %** (about 1/5 of predicted, but the sign is right).
- **Discarded: +7.87 %** (worse than the c1-8 brif-elision
  regression we tried to avoid).
- **Total cycles: +0.32 %** — net neutral, matching the flat wallclock.
- The shift moved cycles from Processing (−5.13 %) into Discarded
  (+7.87 %) and Delivery (+23.21 %), with only a small Useful credit.

### Root cause hypothesis — predictor anchor was NOT preserved

The fused op IS a conditional branch on `src`, but it is a new opcode
at a fresh PC. Pulley's nightly `pulley_tail_calls` dispatch
(`become`-based) puts the indirect-branch / tail-call **inside each
handler**, so the CPU's indirect-branch predictor learns per-handler
target distributions. Replacing the original `xband_s8 → br_if`
two-handler chain with one `xband_s8_br_if` handler:

1. Removes one indirect branch per call_indirect site (the saving the
   patch was designed to capture — visible as the Processing
   −5.13 %).
2. Inserts a new opcode whose handler's tail-call has its own
   predictor history, learned from scratch. The previous brif handler
   already had a warmed predictor entry for "branch to next dispatch
   step after a funcref null-check"; the new BandBrIf handler does
   not. At 15 s sustained run (~500 call_indirect site evaluations per
   iter × ~530 iters = ~260 K BandBrIf executions per workload) the
   entry has clearly not converged on the Icestorm pattern table to
   match the brif's accuracy.
3. The +23.21 % Delivery and +7.87 % Discarded together account for
   ~24.5 M cycles of new front-end stalling — within an order of
   magnitude of the savings on Useful + Processing, hence the wash.

The closeout question from the task brief — "verify on PMU before
claiming the win; if PMU looks better but wall clock doesn't,
understand specifically why" — is answered: PMU does NOT look
better. The cycles moved into the wrong bucket. The predictor anchor
argument was wrong, in the same way (and same direction) as the c1-8
brif-elision regression that PR #2 commit `8fbd7271fb` already
documented.

### Implication for next step

**Phase 1 and Phase 2 are alternatives at the runtime, not layered.**
Both target the same call_indirect lazy-init tail in the eager-init
path. Phase 2 has now been measured — see
[`opcode-fusion-funcref-dispatch.md`](./opcode-fusion-funcref-dispatch.md)
— and the shapes work out as:

```
band -2 ; brif ; xload code ; xload vmctx ; call_indirect    (baseline: 5 ops)
BandBrIf ; xload code ; xload vmctx ; call_indirect           (phase 1: 4 ops)
xband64_s8 ; FuncrefDispatch ; call_indirect                  (phase 2: 3 ops)
```

(Phase 2 keeps the `xband64_s8` as a separate op since `src` is
consumed as the band's result — see the phase-2 doc's "Soundness"
section for why this trades 1 fewer saved dispatch for a cleaner
predictor anchor.)

At the call_indirect site phase 2 dominates phase 1: more dispatches
saved (2 vs 1), same number of new op families added (1 — phase 1's
4 op variants vs phase 2's 4 op variants both count as one "family"
for predictor purposes), measurable wallclock win (call_indirect
−5 % vs ~flat), better PMU buckets (Discarded reclaimed). In a final
upstream PR, phase 2 *replaces* phase 1's runtime contribution at this
site; phase 1's `BandBrIf` op stays as a fallback only when phase 2's
continuation-block load pattern doesn't match.

**Phase 1's codegen infrastructure does layer, though.** Of the three
patches:

| patch | what it adds | reused by phase 2 |
|---|---|---|
| 0001 | `xband_s8_br_if[_not]_{x32,x64}` ops in Pulley ISA | obsoleted at call_indirect site |
| 0002 | `Lower::sink_pure_inst` — absorb pure CLIF inst into a fused MachInst | **yes** — phase 2 fuses 4 pure CLIF insts, needs the same primitive |
| 0003 | `func_environ` IR rewrite + `is_eagerly_initialized_funcref_table` gate + Cranelift recognizer | IR rewrite + predicate gate **yes**; recognizer obsoleted |

So patch 0002 is genuinely foundational and stays. Patches 0001 and
0003's recognizer become unused when phase 2 ships at the same site,
but their shape was the cheap-test-first design for the harder
phase-2 recognizer.

**Phase 1's PMU measurement is calibration for phase 2.** The bucket
diff above gives two per-unit numbers that did not exist before:

- per-dispatch saving: **Processing −5.1 %** from removing one
  match_loop dispatch per call_indirect site (band's handler).
- per-new-opcode cost: **Discarded +7.9 %** from adding one new
  handler whose tail-call indirect-branch predictor entry is cold.

Phase 2 removes ~3 dispatches (band, brif, two xloads → one fused op)
and adds 1 new opcode. The back-of-envelope prediction is:

| bucket | scaled prediction | predicted by PR #2 |
|---|---:|---:|
| Processing | ~−15 % (3× phase 1's per-dispatch saving) | — |
| Discarded  | ~+8 % (same single-new-opcode cost) | — |
| Total cycles | net negative (savings dominate) | −15 % to −25 % |
| Useful     | improvement once the Discarded tax is overcome | predicted by PR #2 |

If phase 2's Discarded uptick stays near +8 % while Processing drops
~3× phase 1's amount, the per-opcode predictor cost is amortizing
linearly across the fused dispatches and we have a real win. If
Discarded grows faster than +8 % per new opcode, the cost is
super-linear and we need to rethink the dispatch model (proposal 3
AOT peephole, or a redesign that doesn't add new handler-PC entries)
rather than keep adding bigger fused ops.

**Recommendation**: **Build phase 2 on top of phase 1's branch**,
keeping patches 0001-0003 as draft (don't push to fork-as-PR), and
measure against this baseline. Use the calibration numbers above as
the falsification threshold. A positive phase-2 result retroactively
makes phase 1's infrastructure (patch 0002 in particular) part of the
final upstream PR; a negative phase-2 result kills both and pivots to
proposal 3 or a different architectural direction.

### Outliers / caveats

- The wallclock per-iter for xmrsplayer in the PMU runs (~19 ms) is
  consistently higher than in the N=10 runs (~16.6 ms). Same code,
  same device, only difference is `BENCH_TARGET_MS` (15000 vs 2000).
  At 15 s sustained run the workload is hitting some periodic event
  (likely thermal throttle or background scheduler interaction). This
  does not affect the cycle-bucket DIFF since both conditions use the
  same target; it just means the per-iter ms in the PMU log shouldn't
  be compared to the N=10 ms.

- `vtable_*` deltas range −2.79 % to +1.36 %. These workloads use
  mutable tables (the IC-experiment shape) and should not hit the
  `is_eagerly_initialized_funcref_table` predicate at all — the
  fusion path's IR rewrite simply doesn't fire for them. Deltas this
  size are noise; this is what the side-channel was meant to confirm.
  If the deltas had been signed and consistent, it would have flagged
  an unintended fusion path firing on non-target workloads (which we
  fixed via the mask = -2 gate before this measurement run).

- The mask = -2 gate was added during this measurement work after the
  initial run crashed regalloc on xmrsplayer's user-code `band(v,
  127)` and `band(v, 60)` sites (the unconstrained `i8::try_from`
  recognizer was firing too broadly). See the amended `0003-cranelift-pulley-fuse-band-brif-at-call_indirect-lazy-init-site.patch`
  in `patches/pulley-fusion-xband-brif/` for the fix and "Soundness"
  above for the reasoning. The measurement above is post-fix; the
  pre-fix build never reached steady state on xmrsplayer.

## Measurement plan

Per AGENTS.md → "Measurement methodology":

1. **iPhone 12 A14 Icestorm**, `.utility` QoS, `BENCH_TARGET_MS=2000`,
   N=10 medians.
2. Build the device .a with `./scripts/build-lib.sh ios` (the
   `--cfg=pulley_tail_calls` nightly + linker-plugin-lto path remains
   unchanged).
3. Workloads: `call_indirect.wasm` (synthetic) and `xmrsplayer.wasm`
   (real-world tracker player) — the two main dispatch-heavy
   workloads with measurable fusion benefit.
4. PMU buckets via `xctrace record --template "CPU Counters"` in
   attach mode (xctrace 26.5 `--launch` regression workaround from
   AGENTS.md → "PMU / xctrace gotchas"). Compare fused-on vs fused-off
   buckets with `scripts/analyze_pmu.py`.

Hypothesis check on PMU first, *then* wallclock:

- If `Useful` drops by ~−7 % and `Discarded` stays flat-to-negative,
  the fusion is winning at the dispatch loop. Look for the
  corresponding wallclock improvement; if absent, the bottleneck has
  moved elsewhere (likely L1d on the funcref/code/vmctx loads — see
  proposal (2) `funcref_load_dispatch`).
- If `Discarded` rises, the predictor anchor was disturbed. Inspect
  the inverted-vs-forward encoding split in the MachBuffer output.

## Known follow-ups

- **arm64_32 (32-bit Pulley) variants**: the `xband32_s8_br_if_x32`
  ops are added but only the 64-bit fusion path is exercised by
  `try_fuse_band_brif`'s size dispatch (currently both I32 and I64
  inputs are accepted). On 32-bit Pulley targets, `pointer_type ==
  I32`, so the same call site falls through to the 32-bit fused op.
  Worth confirming with an arm64_32 disas test before declaring the
  Apple Watch variant covered.

- **Proposal (2) `funcref_load_dispatch`**: the larger fusion — fuse
  `band + brif + xload code + xload vmctx` into one op. Predicted
  −15-25 % cycles. Should be tackled only after (1)'s PMU + wallclock
  measurements are in.

- **Proposal (3) AOT peephole pass**: a recogniser over Cranelift-
  emitted Pulley bytecode that rewrites the canonical sequence to the
  fused forms. Composable with (1) and (2). Soundness: must match
  exactly to avoid regressions on shapes not covered by the
  per-call-site predicate.

- **Lifting the `mask == -2` gate**: the fusion peephole currently
  fires only for `band(v, -2)` because the fused op tests the unmasked
  `src` for non-zero (see Soundness above). Generalising would require
  either (a) restricting to masks where `(v & mask != 0) ⟺ (v != 0)`
  holds across the value space the brif sees (i.e. proving it
  per-call-site — practical only at the call_indirect lazy-init shape
  the IR rewrite produces), or (b) a new family of fused ops whose
  branch tests `dst != 0` instead of `src != 0`. (b) would make the
  fusion sound for any mask but adds 4 more bytecodes; not worth it
  until a measured workload benefits from band+brif fusion outside
  the call_indirect site.

- **Lifting the IR-rewrite gate**: the IR rewrite (replacing
  `brif(value)` with `brif(value_masked)` at the call_indirect
  lazy-init site) is gated on `is_eagerly_initialized_funcref_table`
  for safety. A future change could rewrite the IR more broadly
  (e.g. all immutable tables) once the runtime guarantees `v != 1` by
  construction across a wider set of call_indirect shapes.
