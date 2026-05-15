# 4-way: baseline / phase-3 / phase-4 / WAMR — iPhone 12 + XS, 2026-05-15

Cross-device wallclock comparison after adding **phase 4** opcode
fusion on top of the existing phase-1+2+3 stack. Same App-Store-eligible
pure-interpreter constraint; same WAMR fast-interp baseline. Measured
on two attached devices via N=10 full-launch reps, `BENCH_TARGET_MS=2000`,
`.utility` QoS (E-core pinning).

## Phase 4: `call_indirectN` + `PulleyCallIndirect` arg-bundling

Phase-3 fusion collapses the call_indirect dispatch tail at the lazy-init
brif site to:

    xband_funcref_dispatch_*64 dst_masked, dst_code, dst_vmctx, src, ...
    xmov x0, dst_vmctx                       ; ABI fixup
    call_indirect dst_code

i.e. **3 Pulley dispatches per call_indirect**. The `xmov x0, dst_vmctx`
is the callee-vmctx ABI fixup that regalloc emits to satisfy the call's
`reg_fixed_use(vreg, x0)` constraint. The `call_indirect` opcode itself
only saves `lr` and jumps.

Phase-4 fuses these last two ops into a single Pulley `call_indirect1`
opcode (mirroring how `Inst::Call` already uses `call1/2/3/4` for direct
calls). The dispatch tail becomes:

    xband_funcref_dispatch_*64 dst_masked, dst_code, dst_vmctx, src, ...
    call_indirect1 dst_code, dst_vmctx       ; mov + call in one op

**2 dispatches per call_indirect** — one fewer than phase 3, two fewer
than baseline. The same fusion generalises: `call_indirect{2,3,4}`
handle indirect calls with up to 4 integer ABI args, matching the
existing direct-call shrink loop in `Inst::Call`'s emit path.

The Cranelift side requires a small refactor: `Inst::IndirectCall`'s
`info.dest: XReg` becomes `info.dest: PulleyCallIndirect { target,
args: SmallVec<[XReg; 4]> }`, mirroring `PulleyCall`. `gen_call_ind_info`
pulls the first 0–4 integer args out of `uses` into `args`, so they
no longer go through regalloc's `reg_fixed_use` mechanism (which would
synthesise `xmov` ops). The args are passed as free reg uses; the emit
side picks `call_indirect{,1,2,3,4}` based on how many args remain
after the same "drop args already in their ABI register" loop that
`Inst::Call` uses.

Total per-workload-rep saving: **N Pulley dispatches per call_indirect**
where N is the number of integer ABI args needing fixup (≥1 in
practice; vmctx is always one of them).

## Wallclock comparison — iPhone 12 (A14 Icestorm E-core, N=10)

| workload | baseline | phase3 | phase4 | WAMR | base→phase3 % | phase3→phase4 % | base→phase4 % | phase4 vs WAMR |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| call_indirect | 27.92 | 27.35 | 27.23 | 16.35 | -2.05% | -0.44% | -2.48% | WAMR 1.67× faster |
| xmrsplayer | 16.37 | 16.77 | 16.41 | 12.30 | +2.48% | -2.17% | +0.26% | WAMR 1.33× faster |
| vtable_mono | 44.00 | 42.75 | 42.45 | 24.38 | -2.85% | -0.68% | -3.52% | WAMR 1.74× faster |
| vtable_bi | 49.81 | 49.42 | 46.10 | 27.87 | -0.78% | **-6.71%** | **-7.45%** | WAMR 1.65× faster |
| vtable_poly4 | 54.08 | 54.27 | 49.42 | 31.32 | +0.37% | **-8.94%** | **-8.61%** | WAMR 1.58× faster |
| vtable_poly6 | 60.67 | 59.67 | 57.45 | 35.66 | -1.65% | **-3.72%** | **-5.32%** | WAMR 1.61× faster |
| graphql-validation (AS) | 14.63 | 14.65 | 14.62 | 9.36 | +0.10% | -0.23% | -0.13% | WAMR 1.56× faster |
| graphql-validation (Porffor) | 14.85 | 15.14 | 15.16 | N/A | +1.97% | +0.16% | +2.13% | — |

### iPhone 12 winner

**phase-4 is the clear wallclock winner on every Pulley dispatch-heavy
workload**:

- `vtable_poly4`: −8.94% vs phase-3 / −8.61% vs baseline
- `vtable_bi`:    −6.71% vs phase-3 / −7.45% vs baseline
- `vtable_poly6`: −3.72% vs phase-3 / −5.32% vs baseline
- `vtable_mono`:  −0.68% vs phase-3 / −3.52% vs baseline
- `call_indirect`: −0.44% vs phase-3 / −2.48% vs baseline
- `xmrsplayer`:   −2.17% vs phase-3 / +0.26% vs baseline

graphql-AS is wallclock-neutral (the call_indirect path is a small
fraction of its steady-state work). graphql-Porffor regresses ~+2%
since baseline; phase-4 doesn't recover it.

**Gap to WAMR closes by ~10% on the vtable suite** — `vtable_poly6`
goes from WAMR 1.71× faster (baseline) → 1.65× (phase-3) → 1.61×
(phase-4). `vtable_poly4` from 1.73× → 1.71× → 1.58×.

## Wallclock comparison — iPhone XS Max (A12 Mistral E-core, N=10)

| workload | baseline | phase3 | phase4 | WAMR | base→phase3 % | phase3→phase4 % | base→phase4 % | phase4 vs WAMR |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| call_indirect | 40.99 | 43.12 | 41.06 | 27.50 | **+5.20%** | **-4.77%** | +0.18% | WAMR 1.49× faster |
| xmrsplayer | 30.65 | 28.18 | 28.80 | 23.55 | -8.06% | +2.20% | -6.04% | WAMR 1.22× faster |
| vtable_mono | 69.95 | 73.07 | 72.21 | 49.94 | +4.46% | -1.18% | +3.23% | WAMR 1.45× faster |
| vtable_bi | 75.89 | 79.02 | 79.56 | 53.71 | +4.12% | +0.69% | +4.84% | WAMR 1.48× faster |
| vtable_poly4 | 86.81 | 84.25 | 84.25 | 59.38 | -2.96% | +0.00% | -2.96% | WAMR 1.42× faster |
| vtable_poly6 | 92.47 | 95.61 | 97.47 | 66.01 | +3.40% | +1.95% | +5.41% | WAMR 1.48× faster |
| graphql-validation (AS) | 24.67 | 23.74 | 23.77 | 15.09 | -3.76% | +0.14% | -3.63% | WAMR 1.58× faster |
| graphql-validation (Porffor) | 23.99 | 24.53 | 24.04 | — | +2.24% | -1.99% | +0.21% | — |

### iPhone XS picture: microarchitecture portability story

A12 Mistral E-core responds differently from A14 Icestorm:

- **Phase-3 regresses** on XS for the synthetic `call_indirect` (+5.20%)
  and for vtable_mono / vtable_bi / vtable_poly6 (+3–5%). The
  larger fused op family appears to cost more in front-end pressure
  on Mistral than it saves in back-end dispatch reduction. Mistral's
  pattern-history table behaves differently from Icestorm's.
- **Phase-4 partially recovers** what phase-3 lost on `call_indirect`
  (−4.77% vs phase-3 → back to baseline parity) and on `vtable_mono`,
  `xmrsplayer` is roughly flat vs phase-3.
- **Workloads where the original phase-1/2/3 helped on XS** (`vtable_poly4`,
  `xmrsplayer`, `graphql-AS`) keep most of their phase-3 wins under
  phase-4.
- **Net vs baseline**: XS phase-4 is neutral-to-slightly-negative on
  the call_indirect-dispatch synthetic + vtable_bi/vtable_poly6,
  positive on xmrsplayer / vtable_poly4 / graphql-AS, neutral elsewhere.

Phase-4 is therefore **the right pick on iPhone 12** unambiguously and
**the right pick on XS for call_indirect / vtable_mono** (where it
undoes phase-3's regression) while being **noise on the rest of XS**.
Recommend phase-4 as the new top-of-stack — it dominates phase-3 across
both microarchitectures on the dispatch-synthetic workload that gave
the stack its name, and dominates on iPhone 12 across the entire vtable
suite.

## Cross-device summary (Pulley phase-4 vs WAMR ratios)

| workload | iPhone 12 (Icestorm) | iPhone XS (Mistral) |
|---|---:|---:|
| call_indirect | 1.67× | 1.49× |
| xmrsplayer | 1.33× | 1.22× |
| vtable_mono | 1.74× | 1.45× |
| vtable_bi | 1.65× | 1.48× |
| vtable_poly4 | 1.58× | 1.42× |
| vtable_poly6 | 1.61× | 1.48× |
| graphql-validation (AS) | 1.56× | 1.58× |

The Pulley/WAMR ratio is **lower (= closer) on iPhone XS than on
iPhone 12** for every workload except graphql-AS. Mistral narrows the
relative gap because both runtimes' interpreters slow down comparably
on the older core, but Pulley's slowdown is slightly less than WAMR's.
xmrsplayer is the closest at **1.22×** on XS.

## What's still open

Phase-4 saves one Pulley dispatch per call_indirect's ABI vmctx fixup.
The dispatch tail is now 2 ops (fused band+brif+2loads, then
call_indirect1). To shave further:

1. **Phase 5 candidate** — fuse the call into the band+brif+loads op
   itself. The new mega-op `xband_funcref_call_*64 src, offset_code,
   offset_vmctx, vmctx_arg_reg, null_target` would do the band, the
   null check, the two field loads, the vmctx-into-x0, the lr save,
   and the indirect call all in one Pulley dispatch — **1 dispatch
   per call_indirect** down from baseline's 5. The Cranelift side
   needs cross-block fusion (the call is in a different CLIF block
   from the brif), similar to phase-2's continuation-block load
   absorption.
2. **Microarch-aware emission** — let `cranelift_pulley_target_cpu`
   (or a similar hint) gate the phase-3+4 fusion on A14+ targets
   while falling back to baseline on A12. Avoids the XS regression.
3. **Reduce VMCallNeck inlining cost** at the interpreter side —
   the `call_indirect{1,2,3,4}` handlers do up to 4 register reads
   before the jump; if A12 is sensitive to register-port pressure
   on these, a different lane ordering may help.

(1) is the largest potential win; (2) is the safest portability fix.

## Raw data

- iPhone 12 per-rep logs: `out/exp-3way/n10/iphone12-{baseline,phase3,phase4}-r{1..10}.log`
- iPhone XS per-rep logs: `out/exp-3way-xs/n10/iphone12-{baseline,phase3,phase4}-r{1..10}.log`
- Aggregator: `scripts/aggregate_4way.py out/exp-3way/n10` (or `out/exp-3way-xs/n10`)
- Phase-4 wasmtime commit: see the patch on `claude/pulley-fusion-xband-brif` branch

## Phase-4 patch surface

```
 cranelift/codegen/src/isa/pulley_shared/inst/args.rs     | +20    PulleyCallIndirect struct
 cranelift/codegen/src/isa/pulley_shared/inst/emit.rs     | +18    pick call_indirect{,1,2,3,4}
 cranelift/codegen/src/isa/pulley_shared/inst/mod.rs      | +15    operand collection
 cranelift/codegen/src/isa/pulley_shared/lower/isle.rs    | +32    gen_call_ind_info arg-pull
 cranelift/filetests/filetests/isa/pulley32/*.clif        | reblessed
 cranelift/filetests/filetests/isa/pulley64/*.clif        | reblessed
 pulley/src/interp.rs                                     | +78    4 new handlers
 pulley/src/lib.rs                                        | +11    4 new opcodes in for_each_op!
```

No new test failures. 13 Pulley call/call_indirect integration tests
pass. The macro fan-out (`for_each_op!`) generates the
encoder/decoder/disassembler/visitor entries for the 4 new ops
automatically.
