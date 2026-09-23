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
- The two smallest changes, both safe Rust with no behavior change
  (patches in [`patches/`](tinywasm-iphone12-2026-09-23/patches/)), measured
  on the iPhone 12 E-cores (N=5 launches per build, cycles per call,
  geomean of 16 rows):

  | change | size | cycles | instructions |
  |---|---|---:|---:|
  | 0001 grow the value stack out of line | +15 / −8 lines | **−4.1 %** | −3.5 % |
  | 0001 + 0002 inline the fused binop / compare helpers | +4 lines more | **−5.6 %** | −5.5 % |
  | upper bound: `push` without any growth path | experiment | −7.9 % | −7.7 % |

  0001 is the **simplest, most direct first contribution**: +15 / −8
  lines in one file, fixed and dynamic stacks behave exactly as before, and it
  addresses a spot the maintainer already marked ("Revisit when
  Vec::push_within_capacity is stable").

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

## Theories for closing more of the gap

Roughly in order of effort, with the evidence above:

1. **Reserve operand-stack capacity per function** (the upper bound
   above, mergeable form). −7.9 % on the A14, and it makes most pushing
   handlers frameless.
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
0001 and 0002 fit that bar: safe Rust, no API change, focused. The
reservation change (item 1) needs an issue first.

**Checks run on the branch** (`perf/value-stack-cold-growth` in the local
tinywasm checkout, both commits on `next` `b45a98a`, rustc 1.98.0 and
nightly-2026-07-05):
- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace` reports no code warnings. The only warnings
  are the pre-existing `lints.cargo` manifest notices.
- The spec suites have 0 failures, both with the default dispatch and
  with `--features tinywasm/nightly-tail-calls`:

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

- Unit tests and the integration tests all pass, except
  `resume_execution`, which was not run. It needs
  `examples/rust/out/fibonacci.wasm` from `examples/rust/build.sh`, which
  needs Binaryen's `wasm-opt` (not installed here); run it before
  sending.

## Reproducing

```sh
# profiles (device attached; app built and installed as in AGENTS.md)
RUNTIMES_LIST="tinywasm wamr" MODES_wamr="bottleneck:bottlenecks characteristics:call_branch_instructions timeprofile" \
  ./scripts/run-device-pmu.sh out/iphone12-pmu

# A/B: build the app against a local tinywasm checkout, e.g.
cargo build --release -p benchmark-core --lib --features nightly-dispatch --features femtovg-e2e \
  --target aarch64-apple-ios --target-dir target/tw-exp \
  --config 'patch.crates-io.tinywasm.path="../tinywasm/crates/tinywasm"'
# copy its libbenchmark_core.a over target/aarch64-apple-ios/release/, xcodebuild into a
# separate -derivedDataPath, install, then
N=5 RUNTIMES_LIST=tinywasm WORKLOADS='fib(30),call_indirect (200k,...' \
  UDID=00008101-000A044A3C28801E ./scripts/run-device-pass.sh out/tw-ab/<build>
```
