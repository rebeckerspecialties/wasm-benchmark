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
   `c = band(v, i8)`, emits `BandBrIf { dst: vreg(c), src: vreg(v), mask, size, taken, not_taken }`,
   and `sink_pure_inst(band_inst)`. The band's standalone lowering is
   then skipped in `lower_clif_block`, leaving a single MachInst
   producing the masked-funcref vreg.

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

- **Lifting the IR-rewrite gate**: the fusion ISLE pattern works
  unconditionally once the IR shape is `brif(band(v, c)) ...`. We gate
  the IR rewrite on the predicate for safety, but the fusion itself
  is general. A future change could rewrite the IR more broadly
  (e.g. all immutable tables) once the runtime guarantees `v != 1` by
  construction across a wider set of call_indirect shapes.
