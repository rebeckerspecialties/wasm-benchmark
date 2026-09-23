# tinywasm on iPhone 12 E-cores: profile and first contributions (2026-09-23)

Where tinywasm 0.11 spends its time on the iPhone 12's efficiency cores
(A14 Icestorm, iOS 26.5), why it runs at about twice WAMR fast-interp's
cycles, and what the smallest contributions measurably recover. Data
files: [`tinywasm-iphone12-2026-09-23/`](tinywasm-iphone12-2026-09-23/).

## Summary

- tinywasm is **instruction-bound**, not stall-bound. On the E-core,
  62-90 % of pipeline slots retire useful work (WAMR: 40-66 %), and cache
  and TLB misses are negligible. It needs **2.56× WAMR's retired
  instructions** for the same work (geomean over 14 rows) and runs at
  **1.97× WAMR's cycles**. The lever is fewer instructions per wasm
  operation, not memory-system or branch tuning.
- Where the extra instructions go, per dispatched op:
  - ~12 instructions of dispatch: 4-5 dependent loads to refetch
    `func.instructions` and its length, a bounds check, and the handler
    table load.
  - A redundant re-check of the instruction's own enum discriminant.
  - A frame record.
  - In every handler that pushes (`LocalGet32`, `Const32`, `GlobalGet32`,
    the loads), saves and restores of 4 callee-saved register pairs. They
    are there only because `Vec::push`'s growth call is inlined into the
    handler: 214 of 612 handlers.
  - Memory operations re-resolve the memory on every access (~60
    instructions and ~12 dependent loads per `i32.load8_u`).
  - Calls and returns touch all three value-width stacks.
- Three changes, now a stack of PRs on `next` in the fork
  ([#1](https://github.com/rebeckerspecialties/tinywasm/pull/1),
  [#2](https://github.com/rebeckerspecialties/tinywasm/pull/2),
  [#3](https://github.com/rebeckerspecialties/tinywasm/pull/3); patches in
  [`patches/`](tinywasm-iphone12-2026-09-23/patches/)). Measured together on
  the iPhone 12 E-cores: every build launched 5 times, interleaved, cycles
  per call, geomean of 16 rows:

  | change | size | cycles vs `next` | instructions |
  |---|---|---:|---:|
  | #1 grow the value stack out of line | +15 / −8 lines, 1 file | **−4.1 %** | −3.5 % |
  | #1 + #2 inline the fused binop / compare helpers | +4 lines | **−5.3 %** | −5.5 % |
  | #1 + #2 + #3 reserve each function's operand stack on entry | +173 / −61, 11 files | **−8.0 %** | −7.2 % |
  | upper bound: #1 + #2 and a `push` with no growth path | experiment | −7.9 % | −7.7 % |

  #1 is the **simplest, most direct first contribution**: +15 / −8
  lines in one file, fixed and dynamic stacks behave exactly as before, and it
  addresses a spot the maintainer already marked ("Revisit when
  Vec::push_within_capacity is stable"). #3 keeps the whole upper-bound
  gain in a mergeable form, but it adds a public `WasmFunction` field and
  bumps the archive version, so upstream it starts as an issue.

## Setup

- iPhone 12 (A14, iOS 26.5), benchmark app at `.utility` QoS. Every
  profiled thread ran 99.9-100 % of its samples on E-cores (the Time
  Profiler's per-sample core column).
- tinywasm `next` at `b45a98a` (v0.11.0 plus the C-API commit; the
  interpreter is identical to the crates.io 0.11.0 the harness ships),
  built with `nightly-tail-calls` (the `become` dispatch), fat LTO,
  `-C target-cpu=apple-a12`.
- Profiles: `scripts/run-device-pmu.sh` launches the app through
  `xctrace --launch` with one row selected, records 12 s per capture, and
  reduces the busiest thread. With Xcode 27 on iOS 26.5, `--attach` no
  longer finds the app process while `--launch` now records counters,
  the reverse of Xcode 26.5.
- Captures: CPU Counters guided modes (bottlenecks, indirect-branch
  mispredicts, L1D, instruction TLB, branch mix) and the Time Profiler.
  Rates are per 1000 cycles of the benchmark thread.
- A/B: each tinywasm build is installed in turn, the same 16 rows run 5
  times, and the baseline runs again at the end. Its cycles per call came
  back within 0.2 %, and rep-to-rep spread was 1 % (median).

## Where tinywasm stands on this device

Cycles per call ÷ WAMR fast-interp's (same phone, same rows, E-cores):

| case | tinywasm | tinywasm + 0001 + 0002 | upper bound | wasm3 | Pulley | tinywasm instructions ÷ WAMR |
|---|---:|---:|---:|---:|---:|---:|
| fib(30) | 1.48 | 1.38 | 1.34 | 0.73 | 0.73 | 1.87 |
| matmul relaxed-simd FMA | 2.54 | 2.40 | 2.23 | — | 1.29 | 2.68 |
| audio DSP | 2.15 | 1.99 | 1.92 | 0.56 | 1.37 | 3.91 |
| call_indirect | 2.17 | 2.11 | 2.07 | 0.70 | 1.72 | 2.43 |
| xmrsplayer | 1.95 | 1.83 | 1.82 | 0.70 | 1.26 | 2.47 |
| vtable_poly4 | 2.11 | 2.00 | 1.95 | 0.74 | 1.42 | 2.46 |
| graphql-validation (AS) | 2.09 | 2.00 | 1.97 | 0.86 | 1.39 | 3.17 |
| graphql-validation (Porffor) | 1.55 | 1.50 | 1.49 | — | 1.39 | 2.01 |
| sieve (scalar) | 2.42 | 2.19 | 2.15 | 0.52 | 1.32 | 3.39 |
| crc32 (scalar) | 2.48 | 2.24 | 2.15 | 0.58 | 1.30 | 3.53 |
| convolution (scalar) | 2.37 | 2.22 | 2.10 | 0.73 | 1.20 | 3.61 |
| bulk_memory (scalar) | 1.89 | 1.70 | 1.65 | 0.68 | 2.02 | 2.69 |
| tail-call FSM | 1.05 | 1.01 | 0.98 | 0.73 | 0.49 | 1.11 |
| call_ref twin (call_indirect) | 1.99 | 1.93 | 1.90 | 0.69 | 1.31 | 2.22 |
| **geomean** | **1.97** | **1.85** | **1.80** | **0.68** | **1.24** | **2.56** |

## PMU profile

A14 Icestorm E-core, benchmark thread. Bottleneck buckets are % of
pipeline slots.

| workload | runtime | useful | processing | delivery | discarded | indirect branches /1k cycles | indirect mispredicts /1k cycles | indirect mispredict rate | L1D load misses /1k cycles | iTLB misses /1k cycles |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| fib(30) | tinywasm | 85.7 | 5.2 | 2.1 | 7.0 | 82.1 | 0.01 | 0.0 % | 0.06 | 0.03 |
| fib(30) | WAMR | 65.6 | 13.9 | 12.2 | 8.3 | 95.5 | | | | |
| call_indirect | tinywasm | 67.2 | 14.8 | 2.9 | 15.0 | 44.1 | 5.94 | 13.5 % | 0.06 | 0.05 |
| call_indirect | WAMR | 55.6 | 15.7 | 12.8 | 15.9 | 90.2 | | | | |
| xmrsplayer | tinywasm | 69.2 | 6.1 | 6.5 | 18.3 | 62.8 | 6.37 | 10.1 % | 0.35 | 0.94 |
| xmrsplayer | WAMR | 50.7 | 12.2 | 18.0 | 19.1 | 102.8 | | | | |
| graphql-validation (AS) | tinywasm | 62.2 | 5.8 | 8.7 | 23.3 | 54.1 | 10.19 | 18.8 % | 1.18 | 0.20 |
| graphql-validation (AS) | WAMR | 39.9 | 9.9 | 20.8 | 29.4 | 94.6 | | | | |
| crc32 (scalar) | tinywasm | 86.0 | 8.8 | 1.2 | 4.0 | 84.8 | 1.42 | 1.7 % | 0.17 | 0.05 |
| crc32 (scalar) | WAMR | 57.1 | 40.5 | 0.8 | 1.6 | 193.8 | | | | |
| convolution (scalar) | tinywasm | 81.6 | 10.7 | 0.8 | 6.8 | 67.0 | 2.31 | 3.4 % | 0.15 | 0.03 |
| convolution (scalar) | WAMR | 50.1 | 48.1 | 0.6 | 1.2 | 168.0 | | | | |
| audio DSP | tinywasm | 89.9 | 7.6 | 1.8 | 0.7 | 91.4 | 0.02 | 0.0 % | 0.06 | 0.03 |
| audio DSP | WAMR | 46.3 | 50.6 | 0.9 | 2.2 | 153.4 | | | | |

Reading it:

- **Dispatch rate.** WAMR dispatches 90-194 indirect branches per 1000
  cycles where tinywasm manages 44-91, so each WAMR operation costs
  about half as many cycles. WAMR's loop kernels are backend-bound
  instead (processing 40-51 %, dependency chains through frame slots).
- **tinywasm's pipeline** is already efficient. Near-zero L1D, L1I and
  TLB miss rates and high useful shares mean there is little stall time
  to remove; the instruction count is the cost.
- **Misprediction** matters only on branchy real code: 10-19 % of
  tinywasm's indirect branches mispredict on xmrsplayer, call_indirect
  and graphql. It is roughly the same bucket share as WAMR, so it is not
  tinywasm-specific.

## Where the time goes

Top handlers by self time (Time Profiler, 11.7-11.8K samples each; full
lists in [`profiles/`](tinywasm-iphone12-2026-09-23/profiles/)):

| workload | top handlers (self %) |
|---|---|
| fib(30) | JumpCmpLocalConst32 11.3, exec_return_32 9.8, exec_call_self 9.0, LocalSet32 8.3, CallSelf 7.6, BinOpLocalConstTee32 7.5, AddLocalConst32 7.2, exec_binop_32 4.3, memset 2.0 |
| call_indirect | LocalGet32 13.0, (exec_call_wasm) 10.8, exec_return_32 9.5, CallIndirect 7.2, I32Load 6.6, BinOpLocalLocal32 6.6, exec_binop_32 3.3 |
| xmrsplayer | LocalGet32 12.2, SetLocalConst32 4.5, Const32 4.4, exec_call_direct 4.2, I32Store16 4.2, JumpIfZero32 3.4 |
| graphql-validation (AS) | LocalGet32 11.3, I32Store 8.1, GlobalGet32 6.9, exec_call_direct 5.6, BinOpGlobalConst32 4.1, Const32 3.9 |
| crc32 (scalar) | I32Load8U 10.5, AddLocalConst32 9.8, I32Load 9.3, I32Store8 8.6, I32Xor 7.0, IncLocalJump32 6.5 |
| convolution (scalar) | I32Load8U 34.8, AddLocalConst32 20.2, I32Add 14.8, I32Store8 5.5, AndConst32 5.4 |
| audio DSP | I32Load16S 9.8, BinOpStackConst32 9.5, I32Load 9.5, exec_binop_32 6.7, BinOpLocalConstTee32 6.6, LocalGet32 5.8 |

Inclusive call and return time (the `Call*` handler plus its return) is
31 % of fib and 39 % of call_indirect.

### What a handler executes

From the disassembly of the same build, the dispatch tail of every
handler:

```
add  x1, x1, #1              ; next instruction index
ldr  x9, [x0, #0x20]         ; executor.func
ldr  x8, [x9, #0x18]         ; func.instructions.len
cmp  x1, x8 ; b.hs panic     ; bounds check
ldr  x8, [x9, #0x10]         ; func.instructions.ptr
ldr  x2, [x8, x1, lsl #3]    ; the 8-byte instruction
adrp/add x8, HANDLERS ; and x9, x2, #0xffff ; ldr x3, [x8, x9, lsl #3]
br   x3                      ; become handler(...)
```

and at its head `and/cmp/b.ne` re-checks the discriminant the table
dispatch already selected on, plus a frame record.

Any handler that pushes to a value stack also inlines `Vec::push`'s
growth call. The allocator call forces 4 `stp`/`ldp` pairs of
callee-saved registers on every execution, even though the default
configuration's stacks are fixed-size and never grow. In the iOS build,
`LocalGet32`'s hot path is 57 instructions before 0001 and 37 after.

A memory load re-resolves everything on each access:
- the function's operand side table for the offset and memory index;
- the module's memory address and the store's memory instance;
- the address width, the address pop and its underflow check;
- the offset add with overflow check, the bounds check, and the push.

`i32.load8_u` is about 60 instructions with ~12 dependent loads. WAMR
keeps the linear-memory base and size at hand and needs a handful.

## The measured changes

Patches against `next` `b45a98a`:

- **0001 `perf: grow the value stack out of line`.** `Stack::push`
  checks capacity and hands the at-capacity case to a
  `#[cold] #[inline(never)] push_grow`, which keeps the existing
  overflow check and growth. After the check, `Vec::push` cannot reach
  its own growth path, so the allocator call leaves the handlers it was
  inlined into (152 in the macOS build).
- **0002 `perf: inline the fused binop and compare helpers`.**
  `#[inline(always)]` on `exec_binop_32/64` and `exec_cmp_32/64`. The
  fused `BinOp*` handlers then no longer call out (and spill) for the
  operator switch. Binary +16 KB.
- **Upper bound (`experiment-no-growth-push.diff`, not mergeable
  as-is).** `push` without any growth path: identical for fixed stacks
  (the default), but dynamic stacks would trap instead of growing. The
  mergeable form reserves each function's maximum operand-stack height
  per lane at call time (`enter_locals` already grows there), which
  needs the parser to record that height (it already tracks the operand
  stack per lane in `visit.rs`). That is a public `WasmFunction` field,
  so it needs an issue first.

iPhone 12 E-cores, cycles per call ÷ baseline (N=5 launches each; the
baseline re-run at the end came back at 0.998):

| case | 0001 | 0001+0002 | upper bound |
|---|---:|---:|---:|
| fib(30) | 0.964 | 0.928 | 0.905 |
| matmul relaxed-simd FMA | 0.955 | 0.945 | 0.878 |
| audio DSP | 0.961 | 0.928 | 0.895 |
| call_indirect | 0.972 | 0.974 | 0.954 |
| xmrsplayer | 0.961 | 0.937 | 0.935 |
| vtable_poly4 | 0.977 | 0.947 | 0.923 |
| graphql-validation (AS) | 0.981 | 0.955 | 0.939 |
| graphql-validation (Porffor) | 0.975 | 0.966 | 0.961 |
| sieve (scalar) | 0.910 | 0.907 | 0.889 |
| crc32 (scalar) | 0.913 | 0.901 | 0.867 |
| convolution (scalar) | 0.939 | 0.938 | 0.886 |
| bulk_memory (scalar) | 0.930 | 0.901 | 0.876 |
| tail-call FSM | 0.974 | 0.962 | 0.940 |
| EH parser (exnref) | 0.973 | 0.971 | 0.958 |
| GC binary trees | 0.985 | 0.986 | 0.984 |
| call_ref twin | 0.985 | 0.970 | 0.957 |
| **geomean** | **0.959** | **0.944** | **0.921** |

On the M4 Max E-cores, cycles per call, geomean over 15 workloads:

| change | cycles |
|---|---:|
| 0001 | −3.6 % |
| 0001 + 0002 | −4.6 % |
| upper bound | −7.9 % |

## The PR stack on `next`, and the reservation measured

The two changes above and the reservation (theory 1 below) are now a
stack of three PRs in the fork
[rebeckerspecialties/tinywasm](https://github.com/rebeckerspecialties/tinywasm),
on `next` `b45a98a` (the branch the maintainer works on; `main` is 44
commits behind it):

1. [#1 perf: grow the value stack out of line](https://github.com/rebeckerspecialties/tinywasm/pull/1)
   (`perf/value-stack-cold-growth`, on `next`)
2. [#2 perf: inline the fused binop and compare helpers](https://github.com/rebeckerspecialties/tinywasm/pull/2)
   (`perf/inline-fused-binop-helpers`, on #1)
3. [#3 perf: reserve each function's operand stack on entry](https://github.com/rebeckerspecialties/tinywasm/pull/3)
   (`perf/reserve-operand-stack`, on #2)

**PR 3, reserve each function's operand stack on entry.**
- The parser already tracks the operand stack per lane (`lane_counts` in
  `visit.rs`) and now records its highest point per function as
  `WasmFunction::max_stack`.
- `enter_locals` reserves `locals + max_stack` in each lane when a
  function is entered, growing a dynamic stack there; it already grew
  for the locals at that point.
- The handlers' `push` then traps at capacity instead of calling a
  growth path.
- Pushes from outside a function body (host arguments and results,
  `push_dyn`) keep a growing `push_or_grow`, since no reservation covers
  them.
- Costs: a public field on `tinywasm_types::WasmFunction`, archive
  version `06` (with `examples/rust/src/print.twasm` regenerated), and a
  stack overflow now detected on function entry rather than at the
  push that crosses the limit.

**A/B on the iPhone 12 E-cores.** The same 16 rows as above. Five
builds:
- `next`;
- PR 1;
- PR 1 + 2;
- the whole stack;
- an upper bound: PR 1 + 2 with a `push` that has no growth path and no
  reservation, which behaves the same on fixed stacks (the default) but
  would trap on dynamic ones.

This time every rep installs and launches each build once, in an order
rotated per rep (5 reps), so drift hits all builds alike. (The #3 build
is `b16188b`, which differs from the PR's `bc8a0a1` only in a unit
test's expected archive header.) The binaries
were checked for the intended shape: 155 handler call sites of the cold
growth path in PR 1 and PR 1 + 2, none with the whole stack (only the
3 lanes' `push_or_grow`), and none in the upper bound.

Per step, geomean over the 16 rows (per-rep data:
[`ab-stack-iphone12.csv`](tinywasm-iphone12-2026-09-23/ab-stack-iphone12.csv);
the rep-to-rep spread of each row's cycles is 0.4-0.9 % in the median):

| step | cycles | instructions | rows faster |
|---|---:|---:|---:|
| #1 vs `next` | **−4.1 %** | −3.5 % | 16/16 |
| #2 vs #1 | **−1.3 %** | −2.0 % | 12/16 |
| #3 vs #2 | **−2.8 %** | −1.9 % | 15/16 |
| the whole stack vs `next` | **−8.0 %** | −7.2 % | 16/16 |
| upper bound vs #2 | −2.7 % | −2.4 % | 16/16 |
| #3 vs the upper bound | −0.1 % | +0.5 % | 8/16 |

- **#1 and #2** reproduce the first A/B above: −4.1 % both times, and
  −5.3 % against −5.6 %. The four rows #2 does not speed up move by
  0.5 % or less.
- **#3 keeps the whole upper-bound gain** on the geomean. Its extra work
  at function entry shows on the most call-heavy rows. The EH parser
  (exnref) is the one row slower than with #1 + #2 (+1.3 %, consistent
  over all 5 reps; +2.3 % against the upper bound), and the tail-call FSM
  is +1.2 % against the upper bound. Sieve and xmrsplayer come out about
  3 % faster than the upper bound, which is most likely code layout.

Cycles per call on `next`, and each build as a ratio to it (median of 5
launches):

| row | `next` Mcycles | #1 | #1 + #2 | whole stack | upper bound |
|---|---:|---:|---:|---:|---:|
| fib(30) | 303.8 | 0.964 | 0.931 | 0.913 | 0.905 |
| matmul relaxed-simd FMA | 8.17 | 0.960 | 0.951 | 0.889 | 0.879 |
| audio DSP | 3089 | 0.960 | 0.926 | 0.881 | 0.888 |
| call_indirect | 63.36 | 0.970 | 0.975 | 0.952 | 0.953 |
| xmrsplayer | 45.52 | 0.943 | 0.946 | 0.909 | 0.938 |
| vtable_poly4 | 128.5 | 0.984 | 0.961 | 0.937 | 0.934 |
| graphql-validation (AS) | 36.61 | 0.975 | 0.945 | 0.936 | 0.926 |
| graphql-validation (Porffor) | 31.03 | 0.970 | 0.958 | 0.957 | 0.952 |
| sieve (scalar) | 2.04 | 0.909 | 0.908 | 0.857 | 0.889 |
| crc32 (scalar) | 14.75 | 0.915 | 0.904 | 0.866 | 0.869 |
| convolution (scalar) | 40.93 | 0.935 | 0.933 | 0.882 | 0.887 |
| bulk_memory (scalar) | 30.59 | 0.936 | 0.907 | 0.869 | 0.877 |
| tail-call FSM | 14.59 | 0.978 | 0.967 | 0.953 | 0.942 |
| EH parser (exnref) | 42.01 | 0.980 | 0.981 | 0.994 | 0.972 |
| GC binary trees | 284.4 | 0.989 | 0.990 | 0.984 | 0.984 |
| call_ref twin (call_indirect) | 55.03 | 0.983 | 0.970 | 0.962 | 0.954 |
| **geomean** | | **0.959** | **0.947** | **0.920** | **0.921** |

The first attempt at this A/B stalled: the phone auto-locked in the
middle of the second launch, and iOS suspended the app. The benchmark
app now keeps the screen on while it runs.

**Checks** at each PR's own commit: tinywasm's CI matrix
(`.github/workflows/test.yaml`) on this Mac. That is `cargo test
--workspace` and `--examples` on 1.98 and on nightly-2026-07-05 with
`nightly-tail-calls`, each with and without default features, plus
clippy on 1.98 and `cargo fmt --check` with the nightly rustfmt.
- All pass, and the 12 spec suites have 0 failures in every
  configuration.
- The examples' wasm was built with the nightly and linked with 1.93.1's
  `rust-lld`, because this host's 1.98+ `rust-lld` cannot load its
  libLLVM.
- There is no Binaryen here, so the `.opt.wasm` inputs are the
  unoptimized builds.
- `resume_execution` now runs.
- For PR 3, the whole suite also passes with the default value stacks
  switched to dynamic stacks that start empty and grow to exactly each
  reservation (a local stress configuration, not committed). A function
  whose recorded maximum undercounted its pushes would trap there.

## Theories for closing more of the gap

Roughly in order of effort, with the evidence above:

1. **Reserve operand-stack capacity per function.** Done as #3 above:
   −2.8 % on top of #1 + #2, the whole upper-bound gain, and no handler
   calls into growth any more.
2. **Memory-access fast path.** Cache memory 0's store address (or
   width, base and length) in the executor at function entry and module
   switch, and give memory-0 loads and stores with a small offset an
   inline-offset instruction form, so they skip the operand side table.
   `I32Load8U` alone is 35 % of convolution's time; the target is the
   ~12 dependent loads per access.
3. **Carry the instruction slice through the `become` handlers** (pass
   the pointer and length as handler arguments, reloaded only by call
   and return handlers). This removes 3 dependent loads from every
   dispatch.
4. **Lighter calls.** `enter_locals` and every return touch all three
   value lanes, and zero the locals through `memset` calls (2 % of fib
   alone). Calls and returns are 31-39 % of call-heavy code.
5. **Structural:** WAMR fast-interp is a register-style IR (operands
   are frame-slot offsets, no push/pop), which is where most of the
   2.56× instruction gap comes from. tinywasm's fused `*LocalLocal*`
   forms move in that direction; a full register IR would be a redesign,
   not a contribution.

## Contributing upstream

tinywasm's [CONTRIBUTING.md](https://github.com/explodingcamera/tinywasm/blob/next/CONTRIBUTING.md)
sets the process:
- PRs target `next`.
- Larger changes and public API changes start with an issue.
- PRs are squash-merged with Conventional Commit titles.
- AI-assisted code is allowed, but the issue and pull-request text must
  be written by the contributor, who must understand and review all
  submitted code.

The maintainer answers quickly: PR #55 (an external dispatch-table
change) got same-day review with local benchmarks. It was closed with
a preference for no new unstable features or `unsafe`, and the idea was
reimplemented in safe Rust (`437a77c`, "Similar to #55 but with only
safe code"). #56 (an external parser fix)
merged in a week.
#1 and #2 fit that bar: safe Rust, no API change, focused. #3 changes a
public type and the archive format, so upstream it starts as an issue.

The maintainer is working on interpreter performance too. Their
`exp/acc` branch (2026-09-11, one commit on an older `next`) experiments
with accumulator and register lowering. Its commit message says the
accumulator work has not paid off so far, and that the parts to bring
into `next` are a single-pass parser rework and some dispatch tweaks.
Those touch the same files as #3's parser change (a few lines in
`push_sizes`) and possibly #1's `push`, so the stack may need a small
rebase when they land.

The PRs' checks are listed in [The PR stack on `next`](#the-pr-stack-on-next-and-the-reservation-measured);
the spec suites they run have these sizes:

  | suite | tests |
  |---|---:|
  | `test-wasm-1` | 19 245 |
  | `test-wasm-2` | 28 012 |
  | `test-wasm-3` | 21 228 |
  | `test-wasm-latest` | 21 233 |
  | `test-wasm-simd` | 25 990 |
  | `test-wasm-relaxed-simd` | 77 |
  | `test-wasm-gc` | 784 |
  | `test-wasm-memory64` | 1 606 |
  | `test-wasm-multi-memory` | 912 |
  | `test-wasm-custom-page-sizes` | 207 |
  | `test-wasm-wide-arithmetic` | 109 |
  | `test-wasm-custom` | 113 |

- `resume_execution` and the examples' tests need the example wasm from
  `examples/rust/build.sh`. On this host that script cannot run as is,
  because the nightly's `rust-lld` cannot load its libLLVM and there is
  no Binaryen. The wasm is built with the same `cargo build` lines and
  `-C linker=` pointing at 1.93.1's `rust-lld`, and the `.opt.wasm`
  files are copies of the unoptimized builds.

## Reproducing

```sh
# profiles (device attached; app built and installed as in AGENTS.md)
RUNTIMES_LIST="tinywasm wamr" MODES_wamr="bottleneck:bottlenecks characteristics:call_branch_instructions timeprofile" \
  ./scripts/run-device-pmu.sh out/iphone12-pmu

# A/B of the PR stack: a tinywasm worktree for the builds, one app per build,
# then interleaved launches on the phone and the summary
git -C ~/src/tinywasm worktree add --detach ~/src/tinywasm-worktrees/ios next
W=~/src/tinywasm-worktrees/ios
./scripts/tinywasm-ab-build-ios.sh base "$W" b45a98a
./scripts/tinywasm-ab-build-ios.sh pr1 "$W" 68df3d4
./scripts/tinywasm-ab-build-ios.sh pr2 "$W" 5103db1
./scripts/tinywasm-ab-build-ios.sh pr3 "$W" bc8a0a1
./scripts/tinywasm-ab-build-ios.sh ub "$W" 5103db1 docs/tinywasm-iphone12-2026-09-23/patches/experiment-upper-bound-on-0002.diff
./scripts/tinywasm-ab-iphone.sh out/tw-stack-ab base pr1 pr2 pr3 ub
./scripts/tinywasm_ab_summary.py out/tw-stack-ab base pr1 pr2 pr3 ub \
  --steps pr1:base,pr2:pr1,pr3:pr2,pr3:base,ub:pr2,pr3:ub --csv out/tw-stack-ab.csv
```
