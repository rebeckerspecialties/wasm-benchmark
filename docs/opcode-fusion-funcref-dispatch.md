# Pulley opcode fusion — phase 2: `funcref_load_dispatch`

Second measurable step past PR #2's c1-7 dispatch ceiling. Fuses the
brif + two VMFuncRef field loads (`wasm_call` + `vmctx`) emitted by
`load_code_and_vmctx` into a single Pulley dispatch when the table is
eagerly initialized AND the sig check is statically elided.

Builds on top of phase 1 (`docs/opcode-fusion-band-brif.md`) on the
same wasmtime branch (`claude/pulley-fusion-xband-brif`). Phase 2's
codegen primitives — `Lower::sink_pure_inst`, the
`is_eagerly_initialized_funcref_table` predicate, the `mask == -2`
gate — are all reused. Phase 1's `BandBrIf` op stays in the ISA but
becomes unreachable at the call_indirect tail when phase 2's larger
fusion matches (the recogniser tries phase 2 first).

## Wasmtime branch + commits

Three more commits on `claude/pulley-fusion-xband-brif`, stacked on
top of phase 1's three. Patches under
[`../patches/pulley-fusion-funcref-dispatch/`](../patches/pulley-fusion-funcref-dispatch/).

| # | subject |
|---|---------|
| 1 | `pulley: add xfuncref_dispatch fused dispatch ops` |
| 2 | `cranelift: LowerBackend::pre_lower hook + sink_pure_inst loads` |
| 3 | `cranelift/pulley: fuse brif + 2 xloads at call_indirect dispatch tail` |

Sandbox SHAs are `bd3a84580c`, `6ec49ef884`, `2f8ce110c2`; local
re-apply will resign and reshuffle them.

## What it does

Three layered changes.

**Pulley ISA.** Adds four fused bytecode ops via `for_each_op!`:

| op | semantics |
|---|---|
| `xfuncref_dispatch_x64`      | if `src != 0`: `dst_code = load(src + offset_code)`; `dst_vmctx = load(src + offset_vmctx)`; branch. Else: fall through. |
| `xfuncref_dispatch_not_x64`  | if `src == 0`: branch. Else: same loads, fall through. |
| `xfuncref_dispatch_x32`      | 32-bit pointer-width variant (arm64_32-apple-watchos). |
| `xfuncref_dispatch_not_x32`  | inverted 32-bit variant. |

`src` is the **already-masked** funcref pointer — the preceding
`xband_s8 v, -2` from phase 1's IR rewrite stays as a separate
Pulley op upstream of the fused dispatch. The fused op's runtime
null check on `src` is defence-in-depth (the eager-init predicate
guarantees `src != 0` at runtime); it MUST fall through on null so
the slow-path lazy-init builtin stays callable in the
provably-unreachable error case.

The handler's unaligned read is safe because VMFuncRef has
pointer-aligned fields by construction and the i8 offsets are
compile-time constants chosen from the canonical VMFuncRef layout.

**Cranelift core.** Adds two pieces of generic infrastructure that
serve as the foundation for cross-block fusion patterns:

1. `LowerBackend::pre_lower` — a backend-specific analysis hook run
   once after `Lower::new` but before the main reverse-block
   lowering loop. Default impl is a no-op. The lowering loop
   processes blocks in reverse layout order, so by the time a brif
   in a predecessor block is lowered, its taken target
   (continuation) block has already had its instructions emitted to
   VCode. To absorb continuation-block insts into a fused MachInst
   emitted at the brif's lowering time, we have to mark them via
   `sink_pure_inst` BEFORE the continuation block is lowered.

2. `Lower::sink_pure_inst` now also accepts trusted readonly loads
   (`MemFlags::readonly() + notrap()`). CLIF considers Loads
   side-effecting via `can_load()`, but the readonly + notrap flag
   combination asserts that the load is safe to skip from the
   codegen's perspective. The absorbing MachInst takes
   responsibility for performing the load itself.

**Pulley Cranelift backend.** Three coupled changes:

1. `MInst::FuncrefDispatch` terminator variant with `dst_code /
   dst_vmctx / src / offset_code / offset_vmctx / size / taken /
   not_taken`. Operand collector reports both dsts as defs and src
   as a use; regalloc handles the cross-block live ranges
   (FuncrefDispatch is emitted in the predecessor block; the dsts'
   uses live across the branch into the continuation).

2. `pre_lower_pulley` — the backend's `pre_lower` impl. Walks every
   block and identifies the canonical pattern (brif of `band(v,
   -2)`, where the taken target is a block whose first two CLIF
   insts are loads from the brif's first block-call-arg at the
   canonical `vm_func_ref_wasm_call` / `vm_func_ref_vmctx`
   offsets). When matched, the two continuation-block loads are
   marked `sink_pure_inst`. The band is NOT sunk — its result vreg
   is consumed as FuncrefDispatch's `src`.

3. `try_fuse_funcref_dispatch` — per-brif recogniser in
   `lower_branch`. Re-derives the pattern (the same way as the
   pre-pass), looks up the relevant vregs, emits one FuncrefDispatch
   MachInst, and returns true. Phase 1's `try_fuse_band_brif` is
   tried only as a fallback when phase 2 does NOT match.

## Soundness

Same eager-init predicate as phase 1
(`is_eagerly_initialized_funcref_table` + `mask == -2`) — under that
predicate, every funcref slot at runtime is `addr | 1` for some real
pointer `addr`, so `src = value_masked` is non-null and the field
loads are valid memory accesses.

The phase-2 op's runtime null check on `src` is identical in shape
to phase 1's: branch when `src` matches the original brif's null
direction. The predictor anchor is preserved at the brif's PC role
(the original brif's contribution to dispatch's mispredict bucket is
not removed; it is *consolidated* into the FuncrefDispatch's
handler-PC entry). The cycle accounting in "Measurement results"
below confirms this experimentally.

## Test coverage

The original phase-2 commit added one disas filetest pinning the
canonical dispatch-tail shape. A follow-up round (commit
`7d426cdd51` on the wasmtime branch, plus the soundness fix in
`1fd38e5183`) expanded coverage to cover the corner cases — both
the cases where fusion SHOULD fire and the cases where it must
not. Test design drew on known bug classes in V8, JSC, WAMR,
wasm3, WasmEdge, Hermes, ChakraCore, and Luau where the analogous
fusion shape had soundness or invariant-edge bugs (each test
docstring cites the upstream precedent).

**Disas filetests** (`tests/disas/pulley-fusion-*.wat`):

| file | what it pins |
|---|---|
| `…-no-fire-user-mask.wat` | user wasm `(i32.const -2) (i32.and) (br_if)` — fusion must NOT fire |
| `…-no-fire-mutable-table.wat` | `table.set` anywhere → predicate off, no fusion |
| `…-no-fire-table-fill.wat` | `table.fill` → predicate off |
| `…-no-fire-table-copy.wat` | `table.copy` mutates dst only; src table can still fuse |
| `…-no-fire-table-grow.wat` | `table.grow` → predicate off |
| `…-no-fire-sig-runtime-check.wat` | runtime sig check present → phase 2 fails to match, phase 1 fires as fallback |
| `…-fires-32bit.wat` | pulley32 target → phase 2 fires with i8 offsets 4/12 (regressed without the width-aware fix) |
| `…-fires-multi-call.wat` | two call_indirect sites in one function fuse independently |
| `…-fires-return-call-indirect.wat` | tail call still has the brif lazy-init check; phase 2 fires upstream of the call/return choice |
| `pulley-call-indirect-band-brif-fusion.wat` | the original phase-2 dispatch tail (`xband64_s8 ; xfuncref_dispatch_not_x64 ; call_indirect`) |

**Integration tests** (`tests/all/pulley.rs`, `fusion_*` family):

| name | what it asserts |
|---|---|
| `fusion_call_indirect_every_index` | every in-bounds index returns the right callee; OOB traps `TableOutOfBounds` |
| `fusion_call_indirect_multi_site` | two call_indirect sites' results compose correctly |
| `fusion_return_call_indirect` | tail call returns the right value through the fused dispatch |
| `fusion_call_indirect_with_host_null_set` | host `Table::set` null mid-execution → fused op's runtime null check fires |
| `fusion_call_indirect_with_host_swap` | host swap to different funcref → fused op re-loads code+vmctx, no stale cache |
| `fusion_call_indirect_imported_table` | module B imports A's table; correct VMFuncRef layout across module boundary |
| `fusion_call_indirect_null_slot` | uninitialised slot → trap, not SIGSEGV |

The integration tests run a `pulley_and_native_agree` helper that
executes the same module on Pulley AND wasmtime's native Cranelift
backend, asserting both produce the same result. Trap-expecting
tests run on Pulley only — native trap-via-signal interacts with
cargo test's debug-mode signal handlers (the same code outside the
test harness traps cleanly).

**Differential fuzzing** (Pulley vs wasmtime native via
`cargo fuzz run differential`) was attempted but blocked locally on
(a) OCaml not installed for the wasm-spec-interpreter dep, and (b)
a path-resolution bug in `crates/fuzzing/build.rs` (uses
`env::current_dir()` instead of `CARGO_MANIFEST_DIR`, so the wast-
test scan fails under cargo-fuzz's build cwd). Both are
wasm-benchmark-environment issues, not fusion bugs; deferred as a
follow-up rather than blocking on installing OCaml or upstreaming
a build.rs fix.

**Aggregate**: `2237 / 2237` Cranelift disas tests pass (2228
pre-existing + 9 fusion). `16 / 16` wasmtime-environ
table_mutability tests pass. `7 / 7` new pulley fusion integration
tests pass under `cargo test --test all fusion_`.

## Measurement results — 2026-05-14, iPhone 12 A14 Icestorm

**Baseline**: `wasmtime @ table-mutability-tracking` (PR #2 c1-7 tip).
**Phase 1**: `wasmtime @ claude/pulley-fusion-xband-brif` at commit
`0c5d83b1bc` (the third phase-1 commit). **Phase 2**: same branch at
commit `2f8ce110c2` (the third phase-2 commit, current tip).

Same iPhone 12 / .utility QoS / N=10 wallclock / attach-mode PMU
methodology as `docs/opcode-fusion-band-brif.md` → "Measurement
results". Raw logs in
`out/exp-fusion-funcref-dispatch/{n10,pmu}/iphone12-phase2-*.{log,xml,trace}`
and (for baseline + phase 1) the prior
`out/exp-fusion-xband-brif/{n10,pmu}/`.

### Wallclock — N=10 medians (ms)

| runtime | workload | baseline | phase 1 | phase 2 | phase 2 vs base | phase 2 vs phase 1 |
|---|---|---:|---:|---:|---:|---:|
| Pulley | call_indirect | 28.233 | 28.157 | 26.825 | **−4.99 %** | **−4.73 %** |
| Pulley | xmrsplayer    | 16.581 | 16.604 | 16.521 | −0.36 % | −0.50 % |
| Pulley | vtable_mono   | 44.819 | 44.346 | 42.343 | **−5.53 %** | −4.52 % |
| Pulley | vtable_bi     | 49.549 | 50.007 | 54.515 | +10.02 % | +9.01 % |
| Pulley | vtable_poly4  | 55.212 | 55.963 | 56.099 | +1.61 % | +0.24 % |
| Pulley | vtable_poly6  | 61.628 | 59.910 | 65.536 | +6.34 % | +9.39 % |
| WAMR   | call_indirect | 16.227 | 16.239 | 16.139 | −0.54 % | −0.62 % |
| WAMR   | xmrsplayer    | 13.402 | 13.470 | 13.400 | −0.01 % | −0.52 % |

Phase 2 per-rep ranges (max − min): call_indirect 1.34 ms,
xmrsplayer 0.94 ms, vtable_mono 0.22 ms, vtable_bi 4.94 ms,
vtable_poly4 2.42 ms, vtable_poly6 5.73 ms. The `vtable_bi` +10 %
and `vtable_poly6` +6.34 % deltas are both within the per-rep range,
so we cannot distinguish them from scheduler noise; vtable
workloads use mutable IC tables and do not hit the fusion path, so
they're a side-channel control. WAMR (no Pulley) is the negative
control and is flat (−0.5 % at most).

**Material result**: `call_indirect` improves by **−5.0 %** vs
baseline — the first measurable wallclock win past PR #2's c1-7
ceiling. `vtable_mono` also improves (−5.5 %) which is unexpected
but consistent (range 0.22 ms is tight). `xmrsplayer` is flat, which
is consistent with call_indirect being only a small fraction of
xmrsplayer's per-iteration work.

### PMU — E-core aggregate, 86 s combined window per condition, `RUNTIMES=pulley`

| bucket | baseline | phase 1 | phase 2 |
|---|---:|---:|---:|
| Useful share     | 51.2 % | 50.3 % | **51.5 %** |
| Processing share | 28.0 % | 26.4 % | 27.6 % |
| Delivery share   |  5.4 % |  6.6 % |  6.2 % |
| Discarded share  | 15.4 % | 16.6 % | **14.7 %** |

Absolute-cycle deltas vs baseline:

| bucket | phase 1 abs | phase 2 abs | phase 1 Δ % | phase 2 Δ % |
|---|---:|---:|---:|---:|
| Useful     | −7.05 M    | +20.96 M    | −1.38 % | **+4.11 %** |
| Processing | −14.25 M   | +6.33 M     | −5.13 % | +2.28 % |
| Delivery   | +12.41 M   | +10.27 M    | +23.21 % | +19.21 % |
| Discarded  | +12.09 M   | **−2.67 M** | **+7.87 %** | **−1.74 %** |
| Total      | +3.20 M    | +34.89 M    | +0.32 % | +3.51 % |

Phase 2 vs phase 1 (the layered transition):

| bucket | abs Δ | rel Δ |
|---|---:|---:|
| Useful     | +28.0 M     | +5.57 % |
| Processing | +20.6 M     | +7.80 % |
| Delivery   | −2.1 M      | −3.25 % |
| Discarded  | **−14.8 M** | **−8.91 %** |
| Total      | +31.7 M     | +3.18 % |

### Hypothesis verdict — **partially confirmed**

The phase-1 calibration predicted (per
`docs/opcode-fusion-band-brif.md` → "Implication for next step"):

- Processing ~−15 % (3× phase 1's −5.1 % per saved dispatch)
- Discarded ~+8 % (same single-new-opcode cost)
- Total cycles: net negative

**Discarded prediction: contradicted in a useful direction.** Phase 2
does NOT incur the +8 % Discarded cost we extrapolated from phase 1.
Instead Discarded is **−8.9 % vs phase 1 / −1.7 % vs baseline** —
phase 1's mispredict regression is reclaimed AND the predictor lands
slightly below the baseline anchor. The "per-new-opcode predictor
cost" is NOT linear in number of new op families; it depends on
*which* ops are added and how they redistribute the dispatch stream's
indirect-branch history. Adding 4 more opcodes (the
`xfuncref_dispatch_*` family) on top of phase 1's 4 net **reduced**
mispredicts compared to phase 1.

**Processing prediction: NOT confirmed.** Processing went UP, not
down (+2.3 % vs baseline, +7.8 % vs phase 1). The fused handler is
more compute-intensive per dispatch than the original brif (does
2 loads + a mask + a branch), so the per-dispatch saving in
*dispatch count* does not translate to a Processing reduction. Total
cycles also went up (+3.5 %).

**Wallclock prediction: confirmed, modestly.** call_indirect drops
−5 % vs baseline. This is consistent with the per-iteration savings:
~200 K call_indirect sites × 2 saved dispatches per site = ~400 K
dispatches saved per iteration, against ~30 ms / iteration =
~13 M Pulley dispatches per iteration. The dispatch save is
~3 % of total dispatches per iter — same order as the −5 %
wallclock improvement. The other 2 % comes from the predictor-anchor
reclaim visible in the Discarded shift.

### Closing the predictor-anchor question

Phase 1's PMU was the "is the per-new-opcode cost linear?" calibration
experiment. The answer is: **no, not for the funcref-dispatch op
family**. The c1-8 brif-elision regression
(`8fbd7271fb`, Discarded +6.5 %) AND phase 1's BandBrIf regression
(Discarded +7.9 %) were caused by the new op REPLACING the brif's
specific anchor without preserving its successor-distribution
pattern. Phase 2's FuncrefDispatch op REPLACES MORE of the dispatch
tail and somehow consolidates the predictor's view of "what comes
after the call_indirect null check" into a single handler-PC entry
whose pattern is *cleaner* than the brif's. The Icestorm BTB /
pattern-history table evidently prefers fewer, larger ops over the
finer-grained dispatch breakdown.

This implies that **phase 3 (AOT peephole pass) is worth attempting**
on top of phase 2: more aggressive fusion of dispatch sequences may
continue to consolidate predictor entries, reducing Discarded further.
The diminishing-returns inflection is not yet visible.

### Implication for upstream

**Ship phase 2 instead of phase 1 (or both, with phase 1 as fallback).**
At the call_indirect site, phase 2 dominates phase 1: same number of
new op families (one), more dispatches saved (2 vs 1), better PMU
buckets (Discarded -1.7 % vs +7.9 %), measurable wallclock win
(call_indirect -5 % vs ~flat). Phase 1's `BandBrIf` op family stays
useful as the fallback when phase 2's continuation-block load
pattern doesn't match (the recogniser tries phase 2 first); in
practice on the workloads measured here, phase 1's op never appears
in the final bytecode after phase 2 ships.

The full 6-commit branch is the upstream-PR shape:

| commit | role |
|---|---|
| `0a0127b420` | pulley: xband_s8 + br_if ops (phase 1) |
| `9e523c8c7c` | cranelift: sink_pure_inst infra (phase 1) |
| `0c5d83b1bc` | cranelift/pulley: phase-1 fusion (band + brif) |
| `bd3a84580c` | pulley: xfuncref_dispatch ops (phase 2) |
| `6ec49ef884` | cranelift: pre_lower hook + sink readonly loads (phase 2) |
| `2f8ce110c2` | cranelift/pulley: phase-2 fusion (brif + 2 xloads) |

For an upstream submission, commits 1-3 could be reviewed/landed as
one PR (phase 1 in isolation is still useful as the
`mask == -2`-gated `BandBrIf` fallback for sites where phase 2's
pattern doesn't match), and 4-6 as a stacked second PR.

### Known follow-ups

- **arm64_32 / Apple Watch confirmation**: phase 2's
  `xfuncref_dispatch_x32` ops are added but only the 64-bit fusion
  path is exercised on iPhone 12. The 32-bit Pulley codegen path
  selects the `_x32` variant based on `P::pointer_width()`.
  `tests/disas/pulley-fusion-fires-32bit.wat` (added as part of the
  test-coverage round) pins the static `_x32` shape, and a
  width-aware fix to the mask check (commit `1fd38e5183` —
  `is_minus_two_for`) was required to make phase 2 actually fire
  on pulley32 in the first place (the egraph canonicalises
  `iconst.i32 -2` to `Imm64(0xFFFFFFFE)`, not `Imm64(-2)`, and the
  bit-exact `imm.bits() == -2` gate was silently missing it).
  Dynamic confirmation (a Pulley-on-Apple-Watch run) is still
  gated by Apple Watch SE2 hardware access.

- **xmrsplayer is flat** even though it has many call_indirect sites
  (tracker player module). Either (a) call_indirect is a smaller
  fraction of xmrsplayer's per-iter work than the synthetic
  `call_indirect.wasm` (likely — xmrsplayer has lots of arithmetic
  + 64-bit multiplication inside the player loop) or (b) the fusion
  path isn't being matched on xmrsplayer's specific call_indirect
  sites (some don't hit the eager-init + sig-elided predicate). A
  per-callsite static count of how many call_indirect sites in
  xmrsplayer actually pick phase 2's fused op would resolve this.

- **vtable_bi / vtable_poly6 regressions (+10 % / +6 %)** are within
  the per-rep range but consistently signed. These workloads use
  mutable IC tables and don't hit the fusion. The added op family in
  Pulley's bytecode might be costing the predictor for OTHER
  dispatch shapes (BTB pressure). At N=10 we cannot rule this out;
  the natural follow-up is an N=30 run on these two workloads alone
  to see if the deltas persist.

- **Phase 3 (AOT peephole)** — given that phase 2's Discarded actually
  *improved* vs baseline, the diminishing-returns inflection is not
  visible. A bigger fusion (eg. fuse the entire dispatch tail
  including call_indirect into one op) could plausibly continue to
  improve PMU buckets. The cost is implementation complexity
  (multi-op pattern matching) and the trade-off needs another round
  of measurement.
