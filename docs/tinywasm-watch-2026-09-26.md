# tinywasm on the Apple Watch Series 10 (2026-09-26)

tinywasm ranks near the bottom of the WasmBench leaderboard on an Apple
Watch Series 10. This report measures why, and what to change.

- **The gap is instruction count, not stalls.** Across 28 rows, tinywasm
  needs 1.90× WAMR's cycles per call on the watch. It retires 2.37× the
  instructions, at a higher IPC (3.22 against 2.58).
- **The watch is not special.** The iPhone 16 Pro Max shows the same gap
  on its efficiency cores (1.95×). Its performance cores hide some of it
  with IPC 6.6, but tinywasm still needs 1.71× WAMR's cycles there.
- **Mispredicts and cache misses are negligible** on tinywasm's worst
  row. On an A18 Pro efficiency core, 80.6% of cycles retire useful
  work, 0.15% are discarded speculation and 0.5% are front-end stalls.
  Each dispatched Wasm op costs about 47 machine instructions, against
  about 14 per WAMR op.
- **A fix is staged in the fork:** memory handlers no longer carry the
  shared-memory lock path inline. On the watch this cuts retired
  instructions by 4–6% and cycles by 5–9% on memory-heavy rows, with no
  change on rows without memory traffic.
- **With the two PRs in review** (#64 and #72), tinywasm needs 8.8% fewer
  cycles on the iPhone 12's efficiency cores. What remains on xmrsplayer is
  fixed cost per op and per call; the [updated plan](#action-plan) ranks
  the next changes.
- **Cheaper calls** (plan item 2, fork #11 and #12) take the total to
  −14 to −17% cycles against `next` on three iPhones, every row faster;
  xmrsplayer −16 to −20%.

![WasmBench on the Apple Watch Series 10: tinywasm, multi-memory twin](tinywasm-watch-2026-09-26/watch-upstream-next.png)

## Where tinywasm trails WAMR on the watch

Harness launches (`BENCH_TARGET_MS=200`, one engine per launch). Cycles
and instructions per call come from the kernel's fixed counters for the
timed window.

| row | tinywasm ÷ WAMR, cycles | ÷ WAMR, instructions | tinywasm IPC | WAMR IPC |
|---|---:|---:|---:|---:|
| relaxed-SIMD FMA Horner (16K pts) | 2.83× | 2.99× | 3.18 | 3.02 |
| **multi-memory twin: one memory** | **2.72×** | **3.97×** | 3.89 | 2.66 |
| matmul relaxed-simd FMA | 2.58× | 2.66× | 3.18 | 3.08 |
| graphql-validation (AS) | 2.58× | 3.91× | 2.86 | 1.89 |
| crc32(64KB) | 2.44× | 3.41× | 3.43 | 2.45 |
| geomean, 28 rows | 1.90× | 2.37× | 3.22 | 2.58 |

The two relaxed-SIMD rows are not a separate cause: LLVM compiles
tinywasm's portable v128 lane code to NEON, so they pay the same per-op
overhead ([below](#the-five-proposed-ideas-against-this-data)). The
analysis below uses the worst scalar row,
"multi-memory twin: one memory", a byte-hash loop in which every op is
cheap.

| device | cycles ÷ WAMR | instructions ÷ WAMR | tinywasm IPC | WAMR IPC | on E-cores |
|---|---:|---:|---:|---:|---:|
| Apple Watch Series 10 | 1.90× | 2.37× | 3.22 | 2.58 | 1.00 |
| iPhone 16 Pro Max, efficiency cores | 1.95× | 2.21× | 3.49 | 3.08 | 0.90 |
| iPhone 16 Pro Max, performance cores | 1.71× | 2.26× | 6.60 | 5.03 | 0.01 |

## CPU counters on the worst row

Instruments cannot read CPU counters on the watch: the guided counter
modes cover t8101–t8160 but not the S10's t8310, and a capture there
records no counter tables. The capture below comes from the iPhone 16 Pro
Max efficiency cores (A18 Pro, t8140), where tinywasm behaves as it does
on the watch. It is tinywasm only, 8 s per mode, inside a 30 s timed
window, so the warmup is a few percent of each capture.

| metric (benchmark thread) | value |
|---|---|
| cycles retiring useful work | 80.6% |
| back-end stalls | 18.8%: execution latency; memory misses are 2% of it |
| front-end stalls (i-cache, i-TLB, fetch bandwidth) | 0.5% |
| discarded speculation (mispredicts) | 0.15% |
| L1D load miss rate | 0.05% of loads |
| loads per instruction | 0.35 |
| indirect branches per call | 1.70 M for 1.25 M dispatched ops |
| machine instructions per dispatched op (watch counters) | ~47; WAMR ~14 per IR op |

tinywasm lowers the pass-1 loop to 13 ops and pass 2 to 6, both well
fused (`tinywasm dump`). WAMR's register IR runs about 11 and 5. The op
counts are close; the gap is the cost per op.

The extra 0.45 M indirect branches come from generic fused ops such as
`BinOpStackConst32(Rotl, 5)`. Each one switches on its operator, a second
indirect jump per op.

In the Time Profiler capture, 51% of the samples fall in value-stack
plumbing: `set` 24%, `capacity` 19%, `pop` 5%, `index` 3%. The stack
lives in the `Store`, so every push or pop loads the stack pointer,
length and capacity from memory and writes the length back.

`LocalSet32` shows the fixed cost per op: 31 instructions for a
3-instruction move.

| part | instructions |
|---|---:|
| frame record (cold paths `bl` to panics) | 3 |
| re-check of the opcode the dispatch table already chose | 3 |
| stack length load, underflow check, store | 5 |
| local index decode and bounds check | 5 |
| the move | 3 |
| next instruction: reload the function `Arc`, slice pointer and length, bounds check, load | 7 |
| dispatch: table address, opcode mask, handler load, frame pop, `br` | 6 |

Memory handlers cost more. Since the threads merge (#60), `with_memory!`
locks a shared memory inline. So every `I32Load`, `I32Store`, … handler
contains the lock and unlock calls and saves five register pairs on every
access, whether or not the memory is shared.

## Fix staged in the fork: shared-memory locking out of line

[rebeckerspecialties/tinywasm#10](https://github.com/rebeckerspecialties/tinywasm/pull/10)
(`perf/cold-shared-memory`, on upstream `next` `693d590c`) runs the
shared-memory body in a cold, out-of-line helper. The ordinary-memory
path stays inline. `I32Store` drops from 188 to 137 instructions of code
and from five saved register pairs to three.

The table below is the A/B on the watch: three interleaved installs of
each build, with tinywasm on the worst row, the five scalar-build rows,
xmrsplayer, graphql-validation and two controls. Values are medians of
the three runs, per call.

| row | instructions, before → after | instructions | cycles |
|---|---|---:|---:|
| **multi-memory twin: one memory** | 59.24 → 56.49 M | **−4.6%** | **−8.6%** |
| convolution 256×256 [scalar build] | 125.03 → 117.75 M | −5.8% | −4.8% |
| sieve(10000) [scalar build] | 6.27 → 6.01 M | −4.2% | −8.3% |
| crc32(64KB) [scalar build] | 45.85 → 44.04 M | −4.0% | −6.2% |
| call_indirect (200K dispatches) | 173.14 → 167.24 M | −3.4% | −5.9% |
| graphql-validation (AS) | 83.73 → 80.94 M | −3.3% | −2.8% |
| xmrsplayer (1024-frame buffer) | 117.30 → 113.87 M | −2.9% | −3.6% |
| bulk_memory [scalar build] | 97.02 → 95.92 M | −1.1% | −3.1% |
| fib(30) (no memory ops) | 887.95 → 886.28 M | −0.2% | −0.0% |
| call_ref twin (no memory ops in the loop) | 134.55 → 134.52 M | −0.0% | −0.3% |
| geomean, 10 rows | | −3.0% | −4.4% |

`factorial(20)` is left out because each call takes about 1 µs, too short
for the counters. A single one-second run's wall time on the watch varies
more than these deltas: in a matched pair of runs the baseline measured
11.8 ms and the change 12.4 ms. The counters are the comparison.

### What it costs shared memory

The change moves code but does not change the locking: the same mutex
guards the same body, so contention is unaffected. It adds a call before
the lock on the shared path. The version of #10 that also moved atomics
out of line cost them 1–2% more cycles. A second commit keeps the lock
inline for atomic operations (`with_memory!(@lock_inline …)`), because
their memory is usually shared.

Measured with `tinywasm run` on the M4 (loops in
[`shared-memory.wat`](tinywasm-watch-2026-09-26/shared-memory.wat)).
Values are per access; runs of 1 M and 11 M iterations are subtracted to
cancel startup, and each is the median of 5 interleaved reps on each core
type
([raw](tinywasm-watch-2026-09-26/shared-memory-reps.json)):

| loop, per access | instructions | P-core cycles | E-core cycles |
|---|---:|---:|---:|
| ordinary memory, load + store | 171.6 → 161.5 (−5.8%) | −7.3% | −5.9% |
| shared memory, plain load + store | 301.1 → 291.6 (−3.2%) | **+1.6%** | −5.9% |
| shared memory, atomic RMW | 428.1 → 404.1 (−5.6%) | −2.0% | −6.3% |

The remaining cost is about 0.6 cycles on P-cores out of about 36, on a
path that already calls `pthread_mutex_lock` and `pthread_mutex_unlock`
for every access.

How other runtimes handle the same access:

| runtime | lock on a plain load/store to shared memory? | shared memory on a separate path? |
|---|---|---|
| wasmtime (Cranelift) | no; an `RwLock` guards only grow and size | yes: [wasmtime#4187](https://github.com/bytecodealliance/wasmtime/pull/4187) kept owned memories off the shared-memory indirection, which cost shared memories 1–3% |
| Pulley | threads disabled: Rust has no UB-free racy load/store ([wasmtime#9818](https://github.com/bytecodealliance/wasmtime/pull/9818), [#11747](https://github.com/bytecodealliance/wasmtime/issues/11747)) | — |
| WAMR | no; atomics, grow and wait/notify take a global mutex only for shared memories | only atomics and grow |
| WasmEdge, zwasm, wasmz | no | no |
| wasmi, wasm3 | no threads ([wasmi#777](https://github.com/wasmi-labs/wasmi/issues/777)) | — |
| DLR-FT wasm-interpreter (safe Rust) | yes, a spin lock | yes: ordinary memory is a plain `Vec<u8>` ([#353](https://github.com/DLR-FT/wasm-interpreter/pull/353), [#408](https://github.com/DLR-FT/wasm-interpreter/pull/408)) |

The threads proposal's
[relaxed memory model](https://webassembly.github.io/threads/core/exec/relaxed.html)
makes racing non-atomic accesses non-deterministic rather than undefined
behavior. [Weakening WebAssembly](https://dl.acm.org/doi/10.1145/3360559)
(OOPSLA 2019) compiles them to plain loads and stores. A per-access lock
is therefore a safe-Rust implementation choice, not a spec requirement:
tinywasm's `shared.rs` notes that a lock-free design would need different
storage. #60 already marked the shared branch `core::hint::cold_path()`,
but that hint does not promise any codegen effect, and the calls stayed
in the handlers. Moving the slow path out of line is the pattern of
tinywasm#57, [wasmi#2014](https://github.com/wasmi-labs/wasmi/pull/2014)
and Pulley's trap helpers.

A zero-cost version is possible later. Whether a memory is shared is
known from the module's memory types when the function is lowered, so
dedicated shared-memory opcodes would remove the runtime branch
altogether. The instruction encoding is the maintainer's area.

### iPhone 12 (A14) efficiency cores

The M4 is a cross-check, not a stand-in for A-series silicon, so the
final version of #10 (with atomics inline) was also measured on an
iPhone 12. The two apps were built by `scripts/tinywasm-ab-build-ios.sh`
(upstream `next` against #10), with the shared-memory loops added as
temporary rows. They ran in five interleaved launches each (`.utility`,
2 s windows) via `scripts/tinywasm-ab-iphone.sh`. Every counted row ran
at 97% or more E-core residency; `factorial(20)` is again left out
([raw](tinywasm-watch-2026-09-26/iphone12-ab.csv)).

| row | instructions | cycles |
|---|---:|---:|
| multi-memory twin: one memory | −3.6% | −4.2% |
| crc32(64KB) [scalar build] | −4.0% | −6.3% |
| sieve(10000) [scalar build] | −4.2% | −7.8% |
| convolution 256×256 [scalar build] | −5.7% | −3.3% |
| bulk_memory [scalar build] | −1.1% | −2.2% |
| graphql-validation (AS) | −3.4% | −2.7% |
| xmrsplayer (1024-frame buffer) | −2.7% | −1.1% |
| shared memory, plain load + store | −4.2% | −4.1% |
| shared memory, atomic RMW | −6.3% | −10.7% |
| ordinary memory, plain load + store | −5.8% | −4.4% |
| call_indirect / call_ref twin / fib(30) (controls) | −1.2 / −0.0 / −0.1% | −0.6 / −0.3 / −0.0% |
| geomean, 13 rows | −3.3% | −3.7% |

On the A14 the plain shared-memory path is faster too; only the M4's
performance cores showed the +1.6%.

## With the submitted PRs in place

Upstream `next` (`693d590c`) plus the two performance PRs in review:
[#64](https://github.com/explodingcamera/tinywasm/pull/64) borrows the
instruction stream across tail dispatch, and
[#72](https://github.com/explodingcamera/tinywasm/pull/72) is the
shared-memory change above. Raw data:
[`with-submitted-prs/`](tinywasm-watch-2026-09-26/with-submitted-prs/).

### #64 against current `next`

Five interleaved launches per build on the efficiency cores of three
phones (`scripts/tinywasm-ab-iphone.sh`, 2 s windows). Each cell is the
change in cycles per call, median over the launches. The PR carries the
same table.

| row | A14 (iPhone 12) | A12 (iPhone XS Max) | A13 (iPhone SE) |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −6.0% | −11.4% | −9.0% |
| audio DSP (1000 frames × 512) | −8.9% | −15.1% | −13.7% |
| graphql-validation (AS) | −7.3% | −9.9% | −8.0% |
| multi-memory twin: one memory | −7.0% | −11.4% | −11.3% |
| crc32 (64 KB) [scalar] | −13.5% | −20.9% | −15.6% |
| convolution 256×256 [scalar] | −12.6% | −14.0% | −12.3% |
| sieve (10000) [scalar] | −11.9% | −14.2% | −12.8% |
| bulk_memory [scalar] | −8.6% | −13.5% | −13.4% |
| matmul relaxed-simd FMA | −5.9% | −13.1% | −11.6% |
| GC binary trees | −2.5% | −1.8% | −2.8% |
| fib(30) | −6.1% | −12.1% | −11.6% |
| tail-call FSM | −3.2% | −1.4% | −5.6% |
| call_indirect (200K) | −2.5% | +3.8% | −1.3% |
| call_ref (200K) | +0.4% | +6.2% | −1.4% |
| vtable_poly4 (200K) | +1.1% | +5.2% | −1.0% |
| EH parser, exnref | +4.4% | +12.5% | +2.9% |
| **geomean, cycles** | **−5.7%** | **−7.4%** | **−8.2%** |
| geomean, instructions | −1.9% | −2.2% | −2.0% |
| geomean, wall time | −5.9% | −9.9% | −8.3% |

The rows that regress call a different function on every call, or unwind
across frames. With #64, every switch to another function leaves the
borrowed dispatch chain for the outer loop, which clones the function
`Rc` again.

Both PRs together on the iPhone 12: −8.8% cycles and −4.7% instructions
(geomean of 12 rows, 11 faster). xmrsplayer −8.9%, graphql-validation
−10.7%, audio DSP −7.6%.

### Where xmrsplayer's time goes now

xmrsplayer is the benchmark closest to the production audio guest. It
imports nothing and uses no v128 ops, and neither does any other scored
row: only sqlite3, which the app does not run, has imports.

| xmrsplayer, `next` + #64 + #72 | value |
|---|---|
| iPhone 12 E-core pipeline slots: useful / discarded / front-end / back-end | 73.2% / 13.7% / 7.3% / 5.8% |
| indirect branches per dispatched op (A14) | 1.1 |
| dispatched ops per buffer | 2.50 M |
| instructions per dispatched op (M4 E-core) | 41.7 |
| WAMR's instructions for the same buffer, per tinywasm op | 19.5 (48.7 M on the watch) |

Unlike the byte-hash row (0.15% on the A18), xmrsplayer loses 14% of the
A14's slots to mispredicted branches. Fewer dispatches help there as well.

Time Profiler, iPhone 12 E-cores (7,869 samples):

| group | samples |
|---|---:|
| loads through a local address (`LoadLocal*` and the out-of-line `exec_load_local` helper each one calls) | 19.5% |
| pure stack moves (`LocalGet32`, `SetLocalConst32`, `Const32`, `LocalTee32`, `LocalSet32`) | 18.5% |
| calls and returns (`exec_call_direct`, returns, the outer dispatch loop, zeroing locals) | 14.0% |
| branches | 10.3% |

About 15 of the 42 instructions per op are fixed cost (M4 build,
disassembly weighted by the dispatch histogram, 93% of dispatches
covered). The frame record that cold panic paths force costs 4.1,
re-checking the opcode the table already chose 2.2, fetching the next
instruction with its bounds check 4, and the table dispatch 5.

A call costs far more than an op. On the M4 E-cores
([`hostcall.wat`](tinywasm-watch-2026-09-26/with-submitted-prs/hostcall.wat)),
one loop iteration costs:

| loop body | instructions | cycles |
|---|---:|---:|
| add 1 inline | 67 | 17 |
| call a wasm function that adds 1 | 427 | 135 |
| call a typed host function that adds 1 | 602 | 160 |

With the loops' other ops subtracted (per the dispatch histogram), a
wasm call and return cost about 300 instructions and a host call about
500.

### The five proposed ideas against this data

| idea | finding | verdict |
|---|---|---|
| 1. per-signature host-call trampolines | tinywasm already monomorphizes typed host functions per signature: `HostFunction::from` builds a `call_stack` that reads the arguments straight off the value stack. A host call costs about 500 instructions | helps only guests that call the host often (per sample); no scored row calls the host |
| 2. intrinsic opcodes for known imports | puts embedder-specific meaning into an IR that is parsed before imports are known, and archived | no WasmBench effect; grows the IR |
| 3. parse-time `FastCall(NativeFnPtr)` for imports | imports bind at instantiation, and one parsed or archived module serves many instances, so a native pointer cannot live in the IR. Host calls already push no call frame | not possible as proposed. The section-order idea does apply to wasm-to-wasm calls (plan item 2) |
| 4. native SIMD for v128 | already NEON: LLVM vectorizes the portable lane code. `f32x4.relaxed_madd` is an `fmul.4s` and an `fadd.4s` in a 29-instruction handler. Only `i8x16.shuffle` and `swizzle` stay scalar (150–200 instructions; NEON `tbl` is one) | lane ops: nothing to gain. Shuffle and swizzle: SIMD rows only |
| 5. no-yield contract | the default (unbudgeted) dispatch has no per-op yield or fuel check; a call tests one flag | already the case |

### Upstream's `exp/acc`

The maintainer's accumulator branch (`c8cf1ff`, 2026-09-11, "a bunch of
experiments") is the structural fix in principle: it lowers stack
traffic into accumulator ops at parse time. At that commit it is slower
than its own base `c67ce64` on the M4 E-cores (median of three runs of
`scripts/tinywasm-runner`):

| row | instructions | cycles |
|---|---:|---:|
| xmrsplayer | +23.7% | +6.0% |
| graphql-validation (AS) | +25.8% | +9.8% |
| audio DSP | +24.0% | +6.0% |
| multi-memory twin | +23.7% | +10.9% |
| crc32 [scalar] | +38.8% | +25.9% |
| fib(30) | +1.1% | −8.7% |
| call_indirect | +17.5% | +1.8% |
| vtable_poly4 | +8.8% | +4.2% |

## Cheaper calls and returns (plan item 2)

Staged in the fork as
[rebeckerspecialties/tinywasm#11](https://github.com/rebeckerspecialties/tinywasm/pull/11)
(on `next`) and
[#12](https://github.com/rebeckerspecialties/tinywasm/pull/12) (on `next`
with #64 and #72).

An instruction trace of one iteration of a loop that calls a one-line
function showed where a call went on `next`. The `Call` handler took 153
instructions and `Return32` 95:

- 16 to save and restore 12 registers;
- about 38 to reserve and zero three value-stack lanes, two of which the
  callee never used;
- about 18 to clone the callee's `Shared<WasmFunction>` and drop the
  caller's, again on return: two atomic refcount updates each way;
- two table lookups with bounds checks, plus the host and owner checks.

With #64 each call and each return also went back to #64's run loop,
which cloned the handle again: 423 instructions per iteration and eight
refcount updates.

#11 has the executor borrow the executing function and module from the
instance, which `InterpreterRuntime` holds for the whole run. A call or
return inside the instance switches a reference. A call through an import,
table or reference into another instance, or a return or unwinding
exception into one, ends the run, and `InterpreterRuntime` resumes that
frame with an executor for the other instance. Fuel and time budgets carry
over, so budgeted runs suspend at the same points; the suspension counts
of a cross-instance loop match `next`'s for every fuel size tried. Entering
a function also skips unused lanes, and a single-result return moves its
result once. #12 lets #64's chain re-borrow the new function's
instructions instead of returning to its loop.

| one call and return, M4 build | instructions per iteration | refcount updates |
|---|---:|---:|
| `next` | 391 | 4 |
| `next` + #11 | 310 | 0 |
| `next` + #64 + #72 | 423 | 8 |
| `next` + #64 + #72 + #12 | 311 | 0 |

Both branches pass tinywasm's test suite (unit, doc and spec tests) with
the tail-call dispatch, with the default dispatch, and without default
features, and a new `tests/cross_instance_calls.rs` covers calls, tail
calls, table calls, callbacks and exceptions across an instance boundary
in both directions, with and without budgets.

### #11 against `next`

Efficiency cores, change in cycles per call, median of five interleaved
launches per build
([raw](tinywasm-watch-2026-09-26/cheaper-calls/)):

| benchmark | A14 | A12 | A13 |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −3.6% | −5.7% | −3.3% |
| audio DSP (1000 frames × 512) | −0.2% | +0.3% | +0.2% |
| graphql-validation (AS) | −3.3% | −4.9% | −2.9% |
| multi-memory twin: one memory | −0.3% | −0.1% | +0.3% |
| crc32 (64 KB) | +0.1% | −4.9% | −0.2% |
| convolution 256×256 | −0.3% | +0.6% | +0.3% |
| sieve (10000) | −0.9% | −0.1% | −0.1% |
| bulk_memory (memory.copy/fill) | +0.2% | +0.8% | +0.0% |
| matmul relaxed-simd FMA | −0.1% | +0.0% | −0.3% |
| GC binary trees (~130K struct.new) | −1.4% | −0.5% | −1.4% |
| fib(30) | −6.0% | −4.5% | −5.7% |
| tail-call FSM (65536 return_call) | −12.6% | −9.7% | −10.4% |
| call_indirect (200K) | −10.9% | −9.7% | −10.9% |
| call_ref (200K) | −12.2% | −14.9% | −9.0% |
| vtable_poly4 (200K) | −12.2% | −7.3% | −8.6% |
| EH parser, exnref (4096 stmts, 25% throw) | −18.2% | −20.4% | −17.0% |
| **geomean, cycles** | **−5.3%** | **−5.3%** | **−4.5%** |
| geomean, instructions | −4.2% | −4.2% | −4.1% |
| geomean, wall time | −5.3% | −5.3% | −4.4% |
| rows faster (cycles) | 14/16 | 12/16 | 12/16 |

### #12 against its base (`next` + #64 + #72)

Two separate interleaved A/Bs (the second only the base and #12) agree within a percent on every geomean; this is the second:

| benchmark | A14 | A12 | A13 |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −5.9% | −4.7% | −4.9% |
| audio DSP (1000 frames × 512) | +0.2% | +1.8% | −0.1% |
| graphql-validation (AS) | −5.1% | −7.6% | −6.4% |
| multi-memory twin: one memory | +1.1% | +0.5% | +0.2% |
| crc32 (64 KB) | +3.9% | +1.0% | +2.1% |
| convolution 256×256 | +1.4% | +0.1% | +0.6% |
| sieve (10000) | −0.0% | +0.6% | −0.1% |
| bulk_memory (memory.copy/fill) | +1.6% | +0.4% | +0.9% |
| matmul relaxed-simd FMA | +2.9% | −4.7% | −0.9% |
| GC binary trees (~130K struct.new) | −0.7% | −1.4% | −1.1% |
| fib(30) | −7.1% | −4.4% | −6.2% |
| tail-call FSM (65536 return_call) | −14.6% | −18.4% | −13.5% |
| call_indirect (200K) | −15.6% | −20.6% | −16.5% |
| call_ref (200K) | −17.9% | −27.1% | −15.4% |
| vtable_poly4 (200K) | −13.5% | −21.1% | −13.4% |
| EH parser, exnref (4096 stmts, 25% throw) | −26.4% | −32.9% | −23.1% |
| **geomean, cycles** | **−6.4%** | **−9.4%** | **−6.4%** |
| geomean, instructions | −5.4% | −5.9% | −5.6% |
| geomean, wall time | −6.5% | −9.2% | −6.3% |
| rows faster (cycles) | 10/16 | 10/16 | 12/16 |

The loop rows (crc32, convolution, bulk_memory, the multi-memory twin) retire the same instructions on both builds and their handlers are unchanged; only their code addresses differ.

### Everything submitted against `next`

`next` + #64 + #72 + #12, from the three-way run (`next`, the base and the
stack in one interleaved A/B):

| benchmark | A14 | A12 | A13 |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −15.8% | −20.3% | −17.2% |
| audio DSP (1000 frames × 512) | −7.6% | −15.5% | −15.7% |
| graphql-validation (AS) | −15.1% | −18.1% | −16.8% |
| multi-memory twin: one memory | −11.1% | −15.4% | −14.8% |
| crc32 (64 KB) | −13.5% | −21.6% | −16.8% |
| convolution 256×256 | −14.7% | −15.0% | −15.4% |
| sieve (10000) | −18.5% | −16.8% | −18.1% |
| bulk_memory (memory.copy/fill) | −9.2% | −13.1% | −14.1% |
| matmul relaxed-simd FMA | −8.8% | −6.4% | −13.2% |
| GC binary trees (~130K struct.new) | −2.1% | −2.9% | −3.8% |
| fib(30) | −12.4% | −15.9% | −17.0% |
| tail-call FSM (65536 return_call) | −19.2% | −20.0% | −20.2% |
| call_indirect (200K) | −16.5% | −17.8% | −18.1% |
| call_ref (200K) | −17.5% | −22.2% | −16.7% |
| vtable_poly4 (200K) | −13.3% | −16.6% | −15.1% |
| EH parser, exnref (4096 stmts, 25% throw) | −22.8% | −23.8% | −21.5% |
| **geomean, cycles** | **−13.8%** | **−16.5%** | **−16.0%** |
| geomean, instructions | −9.6% | −9.0% | −9.5% |
| geomean, wall time | −13.8% | −16.8% | −16.0% |
| rows faster (cycles) | 16/16 | 16/16 | 16/16 |

On the M4's efficiency cores the same builds give −7.0% cycles for #11
and −16.9% for the stack
([runner data](tinywasm-watch-2026-09-26/cheaper-calls/m4-e-core-runs.csv)).

## Frameless handlers (plan item 3)

Two commits on `next` (`d1165c2`), branch `perf/frameless-handlers`.

On `next`, 1 of the 615 tail-call handlers runs without a stack frame. A
handler that contains a call saves and restores its frame record on every
instruction it executes, even when the call is on a path that validated
code never takes, and almost every handler had one:

- `instruction_handler_mismatch()`, behind the `let` that re-checks the
  opcode the table already chose;
- `stack_underflow()` from a pop, and the bounds-check panics of value-stack
  and global indexing;
- the conversion of the `Trap::ValueStackOverflow` a push could return into
  an `ExecError`, which boxes the error.

The first commit keeps these out of the handlers. The mismatch and
instruction-pointer panics become cold functions that the handlers
`become`: a branch, not a call. Value-stack and global accesses that
validation rules out stop through `invariant_violated`, which with
`nightly-tail-calls` in a release build is `core::intrinsics::abort`, a
`brk` in place; debug builds and the loop dispatch still panic. A push
inside a function body no longer returns a `Result`, because `enter_locals`
reserves the function's whole operand stack
([#59](https://github.com/explodingcamera/tinywasm/pull/59)) or traps
before the body runs. Only a module that skipped validation can break these
invariants, and tinywasm's README already requires archives, which skip it,
to come from a trusted source. The second commit inlines the five memory
helpers that were not `#[inline(always)]` (`exec_load_local` and its
siblings), so a load or store through a local address no longer calls out.

444 of the 615 handlers are now frameless, and `i32.add` is 26 instructions
instead of 33. The memory, call and return handlers still save a frame:
their traps, the shared-memory path and the calls themselves remain calls.
They take 19% of xmrsplayer's dispatches (on `next`, framed handlers take
93%). Removing those frames needs a cold handler they `become` with the
trap or slow path, which changes the handler macro and the error type more
than these commits do.

Both commits pass tinywasm's test suite with the tail-call dispatch, with
the default dispatch and without default features. On top of
[#74](https://github.com/explodingcamera/tinywasm/pull/74) they merge with
one trivial conflict and pass as well.

### Against `next`

Both commits, efficiency cores, change in cycles per call, median of five
interleaved launches per build, from a three-way run of `next`, the first
commit and both
([raw](tinywasm-watch-2026-09-26/frameless-handlers/)):

| benchmark | A14 | A12 | A13 |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −6.8% | −4.1% | −9.7% |
| audio DSP (1000 frames × 512) | −4.1% | −5.5% | −8.1% |
| graphql-validation (AS) | −3.5% | −3.1% | −7.5% |
| multi-memory twin: one memory | −6.2% | −8.2% | −9.9% |
| crc32 (64 KB) | −4.8% | −6.8% | −9.4% |
| convolution 256×256 | −4.0% | −7.1% | −6.8% |
| sieve (10000) | −4.8% | −5.6% | −9.4% |
| bulk_memory (memory.copy/fill) | −5.1% | −5.8% | −8.2% |
| matmul relaxed-simd FMA | −5.0% | −9.8% | −8.2% |
| GC binary trees (~130K struct.new) | −1.5% | −1.9% | −2.2% |
| fib(30) | −5.8% | −6.2% | −8.5% |
| tail-call FSM (65536 return_call) | −6.2% | −4.9% | −8.2% |
| call_indirect (200K) | −2.5% | −3.6% | −4.0% |
| call_ref (200K) | −3.0% | −3.4% | −4.9% |
| vtable_poly4 (200K) | −4.9% | −5.6% | −6.2% |
| EH parser, exnref (4096 stmts, 25% throw) | −2.9% | −3.4% | −4.2% |
| **geomean, cycles** | **−4.5%** | **−5.3%** | **−7.2%** |
| geomean, instructions | −6.9% | −7.3% | −6.8% |
| geomean, wall time | −4.5% | −5.0% | −7.3% |
| rows faster (cycles) | 16/16 | 16/16 | 16/16 |

Per commit, geomean change in cycles:

| step | A14 | A12 | A13 |
|---|---:|---:|---:|
| first commit against `next` | −3.5% | −4.9% | −6.8% |
| second commit against the first | −1.0% | −0.4% | −0.5% |
| first commit against `next`, separate two-way run | −3.3% | −4.6% | −6.2% |

The second commit cuts xmrsplayer's instructions by 4.9% on all three
phones but its cycles by only 0.4–2.4%: the call, return and prologue it
removes are cheap, well-predicted instructions. fib and sieve moved by up
to 4.7% between the two commits with the same instruction counts, from
code layout alone, and matmul's instruction count varies between launches
of the same build.

### M4 efficiency cores

Geomean of the seven benchmark rows of the runner, median of three runs
([runner data](tinywasm-watch-2026-09-26/frameless-handlers/m4-e-core-runs.csv)):

| against `next` | instructions | cycles |
|---|---:|---:|
| first commit | −5.8% | −5.2% |
| both commits | −8.2% | −5.7% |
| both commits, fuel-budgeted dispatch | −8.0% | −5.9% |
| both commits, loop dispatch (no `nightly-tail-calls`) | −1.8% | −1.7% |
| #74 | −5.1% | −6.6% |
| #74 and both commits | −13.3% | −10.8% |

The first commit alone costs the loop dispatch 0.3% instructions: its
handlers share one frame anyway, and the single large loop function
compiles differently. The second commit more than makes up for it.

## After upstream #75

The maintainer closed #64 and merged his own version of it as
[#75](https://github.com/explodingcamera/tinywasm/pull/75) (`next`
`a0ea681`, 2026-09-27). Both pass the executing function's instruction
slice through the tail-call handlers as an argument, so the fetch of the
next instruction no longer goes through the executor. They differ in how a
handler learns that the function changed:

- #64 also passed the function's address through every handler, and every
  control-flow handler (branches included) loaded `cf.func_addr` and
  compared it with that argument to decide whether to leave the chain;
- #75 passes only the slice. The code that changes functions (a call, a
  return into another function, an exception caught in one) says so by
  returning `ExecFlow::Switch` instead of `Next`, and the chain returns to
  its run loop, which clones the new function's handle and borrows its
  instructions. Branches never pay for it.

### #75 against #64

Both on `d1165c2`, efficiency cores, change in cycles per call, median of
five interleaved launches per build (a seven-build run: `next`, #64, #75
and our stack on #75)
([raw](tinywasm-watch-2026-09-26/after-75/)):

| benchmark | A14 | A12 | A13 |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −2.4% | −2.9% | −1.9% |
| audio DSP (1000 frames × 512) | −1.1% | −2.6% | −2.3% |
| graphql-validation (AS) | +0.3% | −1.6% | −2.0% |
| multi-memory twin: one memory | −2.3% | −2.6% | −2.3% |
| crc32 (64 KB) | −2.1% | −2.5% | −2.7% |
| convolution 256×256 | −2.4% | −3.4% | −3.4% |
| sieve (10000) | +4.2% | −1.1% | +1.0% |
| bulk_memory (memory.copy/fill) | −0.8% | −1.3% | −1.3% |
| matmul relaxed-simd FMA | −3.5% | −9.0% | −5.8% |
| GC binary trees (~130K struct.new) | −0.6% | +0.1% | −0.1% |
| fib(30) | −0.0% | +0.4% | −0.7% |
| tail-call FSM (65536 return_call) | −0.0% | −1.2% | −1.2% |
| call_indirect (200K) | −1.1% | +2.5% | −1.6% |
| call_ref (200K) | +0.0% | +0.2% | −0.1% |
| vtable_poly4 (200K) | −0.8% | −0.6% | −0.0% |
| EH parser, exnref (4096 stmts, 25% throw) | −1.6% | −2.5% | +0.3% |
| **geomean, cycles** | **−0.9%** | **−1.8%** | **−1.5%** |
| geomean, instructions | −0.9% | −1.5% | −1.1% |
| geomean, wall time | −0.8% | −1.4% | −1.2% |
| rows faster (cycles) | 13/16 | 12/16 | 14/16 |

#75 retires about 1% fewer instructions than #64 and is 0.9–1.8% faster,
as the maintainer measured. Against `d1165c2`, #64 gives −5.8 / −6.0 /
−8.2% cycles and #75 −6.6 / −7.6 / −9.6%, but only −1.6 to −3.1%
instructions. The cycles come from the dispatch's dependency chain, which
instruction counts do not show. On `d1165c2` every handler reached the
next handler through four dependent loads before its indirect branch:

```
ldr x9, [x0, #0x20]        ; executor.func
ldr x8, [x9, #0x10]        ; func.instructions.ptr
ldr x2, [x8, x1, lsl #3]   ; the next instruction
ldr x3, [x8, x9, lsl #3]   ; its handler (x8 = the table, x9 = its opcode)
br  x3
```

With the slice in argument registers it is two: the instruction, then its
handler. On the M4's efficiency cores #75 is −2.0% instructions and
−10.2% cycles against `d1165c2`.

### Our changes on #75

Every step, geomean change over the 16 rows, from the same seven-build run:

| step | cycles A14 | A12 | A13 | instructions A14 | A12 | A13 |
|---|---:|---:|---:|---:|---:|---:|
| #64 on `d1165c2` | −5.8% | −6.0% | −8.2% | −1.8% | −1.6% | −1.9% |
| #75 (`a0ea681`) | −6.6% | −7.6% | −9.6% | −2.7% | −3.1% | −3.0% |
| #75 against #64 | −0.9% | −1.8% | −1.5% | −0.9% | −1.5% | −1.1% |
| #74, rebased | −6.0% | −8.3% | −5.2% | −4.4% | −4.6% | −4.2% |
| chain, on #74 | −0.2% | −0.2% | −0.6% | −0.8% | −0.7% | −0.7% |
| frameless (#13), rebased | −5.8% | −6.0% | −7.6% | −7.2% | −7.2% | −7.2% |
| frameless, on #74 + chain | −6.8% | −7.1% | −8.6% | −7.5% | −7.6% | −7.5% |
| #74 + chain + frameless | −12.6% | −15.0% | −13.8% | −12.4% | −12.4% | −12.0% |
| the same against `d1165c2` | −18.4% | −21.5% | −22.1% | −14.7% | −15.1% | −14.7% |

All of it against #75, per row:

| benchmark | A14 | A12 | A13 |
|---|---:|---:|---:|
| xmrsplayer (1024-frame buffer) | −13.4% | −14.0% | −17.8% |
| audio DSP (1000 frames × 512) | −4.3% | −6.8% | −9.4% |
| graphql-validation (AS) | −9.1% | −12.1% | −14.0% |
| multi-memory twin: one memory | −11.0% | −8.6% | −11.2% |
| crc32 (64 KB) | −9.8% | −8.9% | −10.5% |
| convolution 256×256 | −3.7% | −6.0% | −6.9% |
| sieve (10000) | −14.3% | −8.6% | −14.1% |
| bulk_memory (memory.copy/fill) | −8.7% | −7.7% | −8.9% |
| matmul relaxed-simd FMA | −5.0% | −8.3% | −6.0% |
| GC binary trees (~130K struct.new) | −1.8% | −4.5% | −3.3% |
| fib(30) | −12.3% | −12.9% | −12.5% |
| tail-call FSM (65536 return_call) | −21.5% | −21.5% | −20.0% |
| call_indirect (200K) | −18.1% | −24.7% | −20.2% |
| call_ref (200K) | −20.8% | −29.9% | −19.4% |
| vtable_poly4 (200K) | −19.0% | −24.0% | −19.5% |
| EH parser, exnref (4096 stmts, 25% throw) | −23.9% | −32.9% | −24.4% |
| **geomean, cycles** | **−12.6%** | **−15.0%** | **−13.8%** |
| geomean, instructions | −12.4% | −12.4% | −12.0% |
| geomean, wall time | −12.6% | −15.0% | −14.2% |
| rows faster (cycles) | 16/16 | 16/16 | 16/16 |

#74 needed a real rebase: it and #75 both change how the dispatch learns that
the function changed. On #75, a call or return within the instance returns
`Switch` and goes back to #75's loop, which now borrows the new function's
instructions from the instance instead of cloning its handle; leaving the
instance returns `Complete` with #74's `left` set. #75 made each call and
return about 16 instructions dearer (a loop calling a one-line function: 397
instructions per iteration on `d1165c2`, 413 on #75); #74 takes it to 323.
Continuing the chain across calls instead of going back to the loop (#12's
idea) is worth only 0.2–0.6% once the loop no longer clones the handle, so
it was dropped.

### iPhone 12 PMU

The guided CPU Counters modes on the iPhone 12 (A14) efficiency cores,
one 6 s capture per build, workload and mode: pipeline slots, front-end
delivery, all and conditional branch mispredicts with memory-order flushes,
indirect / call / return mispredicts, L1D, and L1I with the iTLB. A
capture's rate per cycle times the row's median cycles per call (the
timing run above, same builds, same phone) gives a count per call. Counts
from different modes of the same build come from different captures and
can differ by about 10%
([raw](tinywasm-watch-2026-09-26/after-75/iphone12-pmu/)).

Cycles per call (millions) by where they went, and counts per call. “#75 +
ours” is #75 with #74, the chain commit and both frameless commits:

| workload | build | cycles | useful | back end | front end | discarded | mispredicts | memory-order flushes |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| xmrsplayer | `d1165c2` | 43.4 | 30.3 | 3.5 | 3.0 | 6.5 | 322 k | 1.6 k |
|  | #75 | 39.5 | 29.3 | 2.2 | 3.2 | 4.8 | 273 k | 7.7 k |
|  | #75 + ours | 34.2 | 23.4 | 2.2 | 3.4 | 5.2 | 253 k | 13.4 k |
| graphql (AS) | `d1165c2` | 35.1 | 21.1 | 2.6 | 3.3 | 8.1 | 353 k | 0.8 k |
|  | #75 | 32.3 | 20.6 | 2.0 | 3.0 | 6.6 | 373 k | 3.1 k |
|  | #75 + ours | 29.3 | 17.6 | 1.6 | 3.1 | 7.0 | 361 k | 7.0 k |
| call_indirect | `d1165c2` | 61.8 | 40.6 | 9.7 | 1.7 | 9.9 | 389 k | 0.1 k |
|  | #75 | 59.9 | 41.6 | 8.2 | 2.3 | 7.8 | 386 k | 1.3 k |
|  | #75 + ours | 49.0 | 32.5 | 6.4 | 1.7 | 8.5 | 322 k | 1.8 k |
| fib(30) | `d1165c2` | 280.6 | 237.9 | 12.5 | 9.4 | 20.9 | 744 k | 0.5 k |
|  | #75 | 262.3 | 228.6 | 7.3 | 8.0 | 18.4 | 744 k | 0.5 k |
|  | #75 + ours | 230.0 | 196.1 | 8.3 | 7.5 | 18.1 | 663 k | 0.4 k |
| audio DSP | `d1165c2` | 2,857 | 2,437 | 345.9 | 52.5 | 22.1 | 740 k | 460 k |
|  | #75 | 2,627 | 2,290 | 256.6 | 45.7 | 34.7 | 705 k | 1,084 k |
|  | #75 + ours | 2,513 | 2,042 | 345.9 | 67.5 | 58.1 | 691 k | 1,891 k |

- #75 is a latency change. Back-end stalls drop on every workload (by 36%
  on xmrsplayer, 42% on fib), and mispredicts per call drop on xmrsplayer
  (−15%), for about 3% fewer instructions overall.
- Our changes on #75 are mostly instructions: useful-work cycles drop
  11–22% per call on every workload, and mispredicts per call drop on all
  five (by 17% on call_indirect, 11% on fib). The front end is flat except
  on audio DSP (+22 M cycles per call).
- One front goes the wrong way on every dispatch speedup: memory-order
  flushes. They grow 2.4–4.8× per call with #75 and another 1.7–2.3× with
  our changes on xmrsplayer, graphql and audio DSP.
  On audio DSP, the loop closest to the production audio app, they are
  2.7× as frequent as branch mispredicts (1.9 M against 0.7 M per call),
  and its back-end stalls return to `d1165c2`'s level.

Per change, on the three workloads that have every build:

| change (per call) | cycles | instructions | mispredicts | memory-order flushes |
|---|---|---|---|---|
| #75 against #64 | −2.4 / +0.3 / −1.1% | −1.1 / −1.0 / −0.1% | −7.8 / +1.5 / −1.4% | +148 / +35 / −1% |
| #74 on #75 | −3.6 / −3.5 / −12.7% | −3.7 / −4.1 / −10.9% | −3.7 / −1.4 / −16.4% | −1 / +26 / +202% |
| frameless on #75 | −5.5 / −4.6 / −2.9% | −11.9 / −9.0 / −5.0% | +4.7 / −1.1 / −0.7% | +72 / +47 / −84% |

(xmrsplayer / graphql / call_indirect; call_indirect's flush counts are in
the hundreds per call.)

Where the flushes come from: the M4's efficiency cores show the same
ratio on audio DSP (837 flush samples against 305 branch-mispredict samples
at the same sampling period, all on E cores), and their samples carry
backtraces. 51% of the flushes are in `BinOpStackConst32`, 17% in
`I32Add`, 10% in `AddConst32` and 5% in `Const32`; by source line, 62% at
the in-place write of the stack top (`Stack::set`), 28% in `Vec::push` and
9% in `Vec::pop`. Each handler stores the value-stack length and top, and
the next handler loads them again. The likely reason this flushes so often:
every handler inlines its own copy of that code, so the store and the load
that alias are a different pair of PCs for every pair of opcodes, which
leaves the memory-dependence predictor little to learn from, and the faster
the dispatch, the earlier the next handler's load issues.
On the same capture, 77% of the branch mispredicts are one conditional
branch, `IncLocalJump32`'s loop test: the guest's loop exits, which every
loop that uses that superinstruction shares.

### Memory-order flushes: the dependence predictor runs out of room

The theory: every handler inlines its own copy of the value-stack code, so
the stores and loads that alias across a handler boundary are a different
pair of instructions for every pair of handlers, and the CPU's
memory-dependence predictor cannot track them all.

A model first ([`scripts/memdep-bench`](../scripts/memdep-bench/)): K
copies of one handler, built like tinywasm's tail-call dispatch (`become`
through a table, a `Vec` value stack reached through an executor and a
store), each updating the stack top in place like `BinOpStackConst32`, and
each with its own constant so the copies cannot be merged. The copies
dispatch in a cycle, so K handler pairs are in play. M4 efficiency cores;
1, 8, 64 and 512 copies from one sweep, 16 to 32 from a finer second one
([raw](tinywasm-watch-2026-09-26/after-75/memdep/)):

| handler copies | 1 | 8 | 16 | 20 | 24 | 28 | 32 | 64 | 512 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| flushes per 1k dispatches | 0.00 | 0.00 | 0.10 | 0.22 | 0.54 | 3.66 | 4.26 | 3.88 | 2.67 |
| cycles per dispatch | 5.40 | 5.43 | 5.45 | 5.72 | 5.84 | 6.53 | 6.64 | 7.14 | 7.92 |

Every copy runs the same 25 instructions per dispatch. Two controls:

- Without aliasing (each copy updates a slot of its own), 64 copies run at
  5.28 cycles per dispatch with no flushes, so code size and dispatch
  prediction are not the cause.
- The same 64 copies in blocks of 64 dispatches, so that only a couple of
  pairs are in play at a time, flush 0.12 times per 1k dispatches: what
  counts is how many pairs alternate, not how many exist.

At 64 copies the extra cycles are back-end stalls (25.3% of the slots
against 5.3% at 8 copies), in execution latency (processing mode: 2,438
against 139), while the flushes take 1.8% of the slots. Once the predictor
is out of room, most loads wait for older stores, and the flushes are the
smaller part of the cost.

In tinywasm itself: a loop whose 84 statements cycle through D distinct
stack binops (`i32.add`, `i32.xor`, `i32.rotl`, ...), so 3 + 2D distinct
handler pairs are in play, with the same instructions per dispatch:

| distinct handler pairs | 5 | 7 | 11 | 19 | 27 | 35 | 45 |
|---|---:|---:|---:|---:|---:|---:|---:|
| #75: flushes per 1k dispatches | 0.00 | 0.01 | 0.02 | 0.22 | 0.12 | 0.25 | 0.08 |
| #75: cycles per dispatch | 6.95 | 6.95 | 7.02 | 7.17 | 7.25 | 7.24 | 7.31 |
| #75 + ours: flushes per 1k dispatches | 0.00 | 0.01 | 0.01 | 0.93 | 1.08 | 1.05 | 0.97 |
| #75 + ours: cycles per dispatch | 6.14 | 6.13 | 6.17 | 6.54 | 6.65 | 6.70 | 6.72 |

The knee is between 11 and 19 pairs (tinywasm's handlers store and load
several slots each: the stack length, values and locals). From 5 to 45 pairs
back-end stalls with our changes go from 13.2% to 19.7% of the slots, with
three times the execution latency, and the faster handlers pay more: +9.4%
cycles against #75's +5.2%. The real loops are far past the knee: audio DSP
needs 40 distinct handler pairs for 90% of its dispatches, xmrsplayer 136
and graphql 226 (opcode-pair histograms).

So the theory holds, with a correction: the flushes are only the visible
part, and most of the cost is loads that wait, which is what the iPhone 12
showed on audio DSP (+89 M back-end cycles per call with our changes). A
stack pointer passed through the handlers the way #75 passes the
instruction slice would make every stack address computable without a
load, so nothing is left to predict; a top-of-stack register would also
remove the top value's round trip.

### What #75 means for what comes next

- The maintainer takes ideas and re-implements them in his idiom, small and
  typed: the function switch became an `ExecFlow` variant rather than a
  check in every handler, and he credited the original. Proposals that come
  with the measurements and a sketch fit that better than large branches.
- Count latency, not only instructions. #75 cut about 3% of the
  instructions and 7–10% of the cycles by removing loads from the dispatch's dependency
  chain; the inlined memory helpers (frameless, second commit) removed 4.9%
  of xmrsplayer's instructions for 0.4–2.4% of its cycles.
- The next latency win of #75's kind is the value stack. The simple
  handlers reach it through the executor and the store (two dependent loads)
  and store its length back on every op: about 7 of the 23 instructions of
  `i32.add` on our stack, and the source of the memory-order flushes that
  grow with every dispatch speedup. Passing the stack's length (or a top
  pointer) through the handlers the way #75 passes the instruction slice
  would take both out (see the previous section). The maintainer has since
  called `acc` a failed experiment; [Registers for the value
  stack](#registers-for-the-value-stack) shows why it gained nothing and what does.
- Mispredicts stay the largest loss on the dispatch-heavy workloads
  (xmrsplayer, graphql, call_indirect: 15–24% of the slots). Faster handlers
  do not touch them; fewer dispatches do (fusion and specialization, plan
  item 4, and register operands, item 5).
- Measurement: audio DSP gets one sample per launch (one ~1.8 s call per
  2 s window), so its "median of five" is five single calls; it needs the
  one-buffer-per-call shape xmrsplayer already has. matmul's instruction
  counts move up to 12.6% between launches of the same build (the process
  counters include other threads), so it should stay out of geomeans until
  the harness counts only the benchmark thread. The small kernels (crc32,
  convolution, sieve) have 47–2,600 samples per launch and mostly under 2%
  spread within a build, but move 1–5% between builds with the same
  instruction counts (code layout), which more data cannot fix.

## Registers for the value stack

On [discussion #78](https://github.com/explodingcamera/tinywasm/discussions/78) the maintainer
said he would like to try passing the stack pointer through the handlers.
He had two worries:

- another argument takes a register he wants to keep for accumulators later;
- a top-of-stack cache he tried before lost its gain to "the extra branch in the common path".

He also called `exp/acc` a failed experiment. This section measures both worries.

### The argument-register budget

Between two handlers, only what the calling convention passes in registers stays out of memory.
A probe crate ([`scripts/tos-bench`](../scripts/tos-bench/) has the method) with 24 integer and 24
float arguments per function, compiled for each target with `nightly-2026-07-05`, counts how many
arrive in registers (integer / float):

| target (lowest-end hardware) | Rust ABI | `extern "sysv64"` | `extern "rust-preserve-none"` |
|---|---|---|---|
| arm64 iOS and tvOS (A10X, A12), arm64_32 watchOS (S4), Android (Snapdragon 835) | 8 / 8 | — | 24+ / 8 |
| arm64 Windows (Snapdragon 835) | 8 / 8 | — | 23 / 8 |
| x86-64 macOS and Linux (Core m3, i3) | 6 / 8 | 6 / 8 | 12 / 8 |
| x86-64 Windows | 4 / 4 | 6 / 8 | 12 / 8 |

tinywasm's `Unbudgeted` handlers take five integer arguments: the executor, the instruction slice's
pointer and length, the instruction pointer and the next instruction. The `Bounded` ones take four:
the executor, instruction pointer, instruction and budget. So:

- **arm64 (every Apple target and the Snapdragon):** three integer registers to spare (four in
  `Bounded`), and all eight FP/SIMD argument registers are unused. A frameless handler also has 18
  caller-saved general registers (x0–x17) for its own work.
- **x86-64 System V:** one to spare, and only four caller-saved registers left for the body. In a
  release build without LTO, 302 of the 615 handlers push callee-saved registers (against 222 of 629
  on arm64 that keep a frame).
- **x86-64 Windows:** already one short. Every handler that dispatches writes the next `Instruction`
  to the stack before its `jmp` (614 of 615), and with three scratch registers left, 551 of 615 push
  callee-saved registers. `i32.add` pushes and pops `rsi` and `rdi` on every dispatch (24 instructions
  on its hot path); on System V it is 19 with no push.
- **`extern "rust-preserve-none"`** (nightly, feature `rust_preserve_none_cc`, like `become`): 12
  integer arguments on every x86-64 OS and 23–24 on arm64. No register is callee-saved, so no
  handler pushes anything. `extern "sysv64"` is stable and gives Windows the System V six.

### A model of where the stack state lives

[`scripts/tos-bench`](../scripts/tos-bench/) holds everything but the stack state fixed:
- **Handlers:** eight handler kinds (const, local.get, local.set, local.tee, binop, in-place update,
  load, store), plus the local.set and store that empty the stack, in up to 64 copies each.
- **Dispatch:** like tinywasm's, `become` through a table, with the executor, instruction slice,
  instruction pointer and next instruction as arguments.
- **Program:** 64 random statements (277 dispatches) over 16 locals and a 1024-word heap.
- **Scaling:** every handler picks one of *m* copies of its kind, so 9 to 171 distinct handlers and
  19 to 276 handler pairs are in play.

Every variant computes the same checksum:

| variant | the stack's height | its top |
|---|---|---|
| `vec` | `Vec` length, in memory (tinywasm today) | in memory |
| `sp` | handler argument | in memory |
| `sp_wt` | handler argument | handler argument, written through to its slot; a pop reloads it from the slot below |
| `sp_wt_nr` | handler argument | as `sp_wt`, but the pops that empty the stack (chosen at parse time, like a fused variant) skip the reload |
| `sp_wb` / `sp_wb_nr` | handler argument | handler argument, stored only when a push covers it |
| `tos_flag` | handler argument | cached behind a validity flag: a branch on every push and pop |
| `vec_tos` | `Vec` length, in memory | handler argument (the shape of `exp/acc`) |
| `sp_spill` | the ninth integer argument, so passed on the stack | in memory |
| `sp_wt_pn` | `sp_wt` with `extern "rust-preserve-none"` handlers | |
| `sp_wt_nr_ni` | `sp_wt_nr` without the Instruction argument: handlers reload their instruction | |
| `sp_wt_nr_sl` | `sp_wt_nr` with the stack's slice as two more arguments (`rust-preserve-none`) | |
| `sp_wt_nr_sl_ni` | `sp_wt_nr_sl` without the Instruction argument, under the Rust ABI | |

M4 efficiency cores, cycles per dispatch, median of three interleaved runs:

| handlers (pairs) | 9 (19) | 17 (64) | 32 (140) | 53 (224) | 87 (265) | 130 (273) | 171 (276) |
|---|---:|---:|---:|---:|---:|---:|---:|
| `vec` (today) | 8.38 | 8.12 | 8.52 | 8.50 | 8.49 | 8.57 | 8.65 |
| `sp` | 5.00 | 5.18 | 5.41 | 5.52 | 5.64 | 5.62 | 5.68 |
| `sp_wt` | 5.25 | 5.11 | 4.91 | 5.02 | 4.99 | 4.97 | 4.99 |
| `sp_wt_nr` | 4.52 | 4.48 | 4.77 | 5.03 | 4.84 | 4.63 | 4.66 |
| `sp_wb` | 4.65 | 4.75 | 4.91 | 4.98 | 4.90 | 4.80 | 4.78 |
| `sp_wb_nr` | 4.36 | 4.46 | 4.69 | 5.02 | 4.80 | 4.58 | 4.62 |
| `tos_flag` | 4.66 | 4.81 | 5.27 | 5.67 | 5.60 | 5.00 | 4.93 |
| `vec_tos` | 8.40 | 8.28 | 8.56 | 8.73 | 8.74 | 8.82 | 9.09 |
| `sp_spill` | 7.97 | 8.59 | 8.88 | 9.15 | 9.27 | 9.40 | 9.22 |
| `sp_wt_pn` | 4.76 | 4.87 | 4.94 | 5.02 | 4.94 | 4.95 | 5.10 |

Instructions per dispatch stay within 21.6–24.8 for every variant but `sp_spill` (29), so the
differences are memory round trips, not work. The numbers move with code layout:
- In an earlier build, before `S` and `W` were separate kinds, `sp` ran at 5.45–6.81.
- The top-of-stack variants moved less than 0.4.

CPU Counters on the same cores, one 3 s capture per cell, events per 1,000 dispatches and
shares of the pipeline slots, at 171 handlers:

| at 171 handlers | `vec` | `sp` | `sp_wt` | `sp_wt_nr` | `sp_wb` | `sp_wb_nr` | `tos_flag` | `vec_tos` | `sp_spill` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| memory-order flushes | 9.8 | 39.1 | 5.6 | **2.2** | 25.8 | 22.0 | 21.7 | 2.0 | 22.7 |
| branch mispredicts | 2.2 | 1.8 | 1.6 | **1.3** | 1.9 | 1.3 | 1.3 | 2.1 | 4.1 |
| useful slots | 50.6% | 61.8% | 75.9% | **82.1%** | 71.1% | 75.7% | 75.4% | 49.6% | 58.8% |
| back-end (processing) | 35.8% | 11.9% | 10.9% | 7.4% | 6.4% | 3.6% | 2.4% | 39.9% | 25.8% |
| discarded | 4.2% | 16.0% | 4.0% | 2.4% | 12.8% | 12.1% | 10.1% | 1.7% | 5.4% |
| delivery | 9.4% | 10.3% | 9.2% | 8.1% | 9.7% | 8.6% | 12.1% | 8.8% | 10.0% |
| execution latency, share of the processing mode | 18.3% | 11.5% | 6.2% | 4.8% | 6.6% | 5.8% | 7.5% | 18.5% | 16.1% |

At 53 handlers the flushes are 9.0 (`vec`), 38.6 (`sp`), 8.3 (`sp_wt`), 2.3 (`sp_wt_nr`), 24.5
(`sp_wb`), 16.0 (`sp_wb_nr`), 5.2 (`tos_flag`), 2.6 (`vec_tos`) and 21.9 (`sp_spill`) per 1,000
dispatches.

A second build adds the variants that trade registers. At 171 handlers, with cycles as the median
of five to ten runs:

| at 171 handlers | `vec` | `sp_wt_nr` | `sp_wt_nr_ni` | `sp_wt_nr_sl` | `sp_wt_nr_sl_ni` |
|---|---:|---:|---:|---:|---:|
| integer arguments | 5 | 7 | 6 | 9 | 8 |
| cycles per dispatch | 8.36 | 4.45 | 4.92 | 4.08 | 4.42 |
| against `vec` | | −47% | −41% | −51% | −47% |
| memory-order flushes per 1,000 | 9.8 | 2.1 | 1.4 | 0.05 | 0.01 |
| branch mispredicts per 1,000 | 1.2 | 0.8 | 1.1 | 0.7 | 0.9 |

The same handlers compiled for x86-64 (release, no LTO). Each cell is the number of stack-slot
accesses in one binop handler, i.e. arguments passed through memory on every dispatch; arm64 has
none in any variant but `sp_spill`:

| binop handler | integer arguments | x86-64 System V | x86-64 Windows |
|---|---:|---:|---:|
| `vec` (today) | 5 | 0 | 1 (the next `Instruction`) |
| `sp` | 6 | 0 | 3 |
| `sp_wt_nr` | 7 | 2 (the top) | 5 |
| `sp_wt_nr_ni` | 6 | 0 | 4 |
| `sp_wt_nr_sl_ni` | 8 | 4 | 8 |
| `sp_wt_pn`, `sp_wt_nr_sl` (`rust-preserve-none`) | 7, 9 | 0 | 0 |

On Windows the Rust-ABI variants also push and pop two to four callee-saved registers per dispatch;
the `rust-preserve-none` ones push none and are 26 and 25 instructions against 36.

What this says:

1. **The height is the critical path.** With the length in the `Vec`, every stack op makes a
   store-to-load round trip through memory, and the next handler waits for it.
   - `vec` runs at 8.1–8.7 cycles per dispatch at every handler count, with 36% of the slots stalled
     in the back end.
   - A top register beside that `Vec` changes nothing: `vec_tos` runs at 8.3–9.1, with 40% back end.
   - That is the shape of `exp/acc`, whose accumulators sat beside the `Vec` stack. It gained nothing
     either (+6% cycles on xmrsplayer, [above](#upstreams-expacc)).
2. **The height alone gains 34% but quadruples the flushes.** With the length chain gone, a
   value-slot load can run ahead of a value-slot store whose address is still being computed: 39
   flushes per 1,000 dispatches, and 16% of the slots are discarded.
3. **With the top in a register too, the flushes fall below today's.**
   - Written through (`sp_wt`): −42%, 5.6 flushes per 1,000 dispatches.
   - With pops that empty the stack skipping the reload (`sp_wt_nr`): −46%, 2.2 flushes per 1,000
     (under a quarter of today's) and the fewest mispredicts. 82% of the slots do useful work, against 51%
     today.
   - The reload after a statement's last pop is wasted. It reads the slot below the operands (in the
     model, the last local), which the `local.set` stores race. Removing it cuts the flushes from 5.6 to
     2.2 per 1,000 at 171 handlers, and from 8.3 to 2.3 at 53.
4. **Write-back stores less but flushes more** (16–26 per 1,000). It spills the old top in the
   handler just before the one that reads it back. Write-through stored the value when it was
   pushed, a dispatch earlier.
5. **The flag is not the cost it was.**
   - With the height in a register, `tos_flag` matches write-through at both ends of the range, but
     is 13% slower at 53 handlers.
   - Its branches predict well in this model (1.3 mispredicts per 1,000), but it takes a third
     register and flushes 22 times per 1,000.
   - The branch-free version needs neither the register nor the branch.
6. **Over budget is worse than today.** The height as a stack-passed argument (`sp_spill`) runs 7%
   slower than `vec`: the round trip moves from the `Vec` to the argument slot.
7. **`rust-preserve-none` costs nothing on arm64** (`sp_wt_pn` 4.79 against `sp_wt` 4.81).
8. **Dropping the Instruction argument costs 10%.** Handlers that reload their instruction from the
   slice (`sp_wt_nr_ni`) are still 41% faster than today. That is how x86-64 System V fits the height
   and the top into its six registers under the Rust ABI.
9. **The slice in registers ends the flushes.** Two more arguments hold the stack's slice itself, so
   no slot address waits for the two loads of the base pointer.
   - `sp_wt_nr_sl`: 0.05 flushes per 1,000 and −51%. The loads never run ahead of a store whose
     address is unknown, so the dependence predictor has nothing left to learn.
   - It needs nine arguments. Without the Instruction argument it fits the Rust ABI's eight on arm64:
     `sp_wt_nr_sl_ni`, −47%.
   - In tinywasm it would also need the lane moved out of the store while the chain runs, since it
     cannot be borrowed twice.

### What it means for tinywasm

- **Which lane.** In the opcode histograms of the three real loops, every op listed is either a
  32-bit-lane op or control flow. The listed ops cover 92.6% of xmrsplayer's dispatches, 80.4% of
  audio DSP's and 61.6% of graphql's. So the 32-bit lane's height and top are the two registers to add. The 64- and
  128-bit lanes can stay in memory at first.
- **Registers.** `Unbudgeted` would use 7 of arm64's 8 integer argument registers and `Bounded` 6.
  x86-64 System V has 6: with `extern "rust-preserve-none"` (12) nothing spills; with the Rust ABI the
  handlers would have to stop receiving the next `Instruction` and reload it, which is 10% slower than
  passing it and 41% faster than today (finding 8). Windows x64 needs `rust-preserve-none` or
  `sysv64` either way, and it already spills today.
- **No branch and no new IR.** The top register is valid whenever a handler reads it, because
  validation already guarantees a non-empty stack there.
  - Pushes write through, so everything outside the handlers keeps working on the in-memory stack
    unchanged: calls, returns, host calls, traps, suspension and exceptions. Only the height has to
    be written back when the chain returns to its run loop (`Switch`, `Complete` and errors).
  - The one parse-time change: pops that leave the lane empty get variants without the reload, like
    the existing fused variants. These are the statement-ending `local.set`, `br_if`, stores and
    `drop`.
- **Fused ops keep working.** The ones that read locals directly do not touch the stack. The ones
  on the stack top read and write the register (`BinOpStackConst32`, `AddConst32`, the loads).
- **The model overstates the gain.** tinywasm's handlers do more per dispatch than the model's ~23
  instructions, and the fused ops already skip the stack on part of the dispatches, so the real gain
  will be well below the model's −46%. The test is a prototype on the phones: the height alone
  first, as the maintainer asked, then the top.

## Action plan

Ranked by expected effect on xmrsplayer-like guests. The estimates are
from the shares above, not measurements.

| # | change | evidence | estimate | owner | status |
|---|---|---|---|---|---|
| 1 | Land #64 and #72 | −8.8% cycles together on A14 E-cores; #64 alone −5.7 / −7.4 / −8.2% on A14 / A12 / A13 | measured | us | #72 merged; #64 closed, replaced by the maintainer's [#75](https://github.com/explodingcamera/tinywasm/pull/75) (merged 2026-09-27; −6.6 / −7.6 / −9.6% cycles, 0.9–1.8% faster than #64) |
| 2 | Cheaper wasm calls and returns: the executor borrows the executing function from its instance (no refcount updates), a same-module direct-call path, unused value-stack lanes skipped, and #64's chain kept across calls within an instance | 14% of xmrsplayer samples; 391 → 310 instructions per call and return | measured: −4.5 to −5.3% cycles alone; −6.4 to −9.4% on top of #64 + #72 (xmrsplayer −4.7 to −5.9%, call-heavy rows −13 to −33%) | us: safe, no IR change | upstream [#74](https://github.com/explodingcamera/tinywasm/pull/74), rebased onto #75 on 2026-09-27: −6.0 / −8.3 / −5.2%. The chain across calls (fork #12) is closed: worth 0.2–0.6% on #75 |
| 3 | Frameless handlers: cold paths `become` a shared cold handler instead of calling panics or boxing an error; inline the `exec_load_local` helpers | 4.1 frame instructions per op; 19.5% of samples in local-address loads that each call a helper | measured: −4.5 / −5.3 / −7.2% cycles and −6.8 to −7.3% instructions on A14 / A12 / A13, every row faster; memory, call and return handlers keep their frames | us: safe, no IR change | fork [#13](https://github.com/rebeckerspecialties/tinywasm/pull/13), rebased onto #75: −5.8 / −6.0 / −7.6%, every row faster |
| 4 | Specialize the hottest generic ops (`BinOpStackConst32` by operator; `LocalGet32` → `LocalGet32`) | 1.7–15.8% of dispatches take a second indirect branch; 13.7% of A14 slots discarded | −3 to −6% on the rows that use them | maintainer's call (IR size) | discussion |
| 5 | The value stack's height and top as handler arguments: the 32-bit lane's height and a written-through top, with pops that empty the lane chosen at parse time to skip the reload | stack moves are 35% of dispatches and 18.5% of samples; the length and top go through memory between handlers, which causes the memory-order flushes that grow with every dispatch speedup (audio DSP: 2.7× as frequent as branch mispredicts). Model on the M4 E-cores: −46% cycles per dispatch and a quarter of the flushes; with the stack's slice in registers too, −51% and no flushes | well below the model's; the only item that can close the gap to WAMR and wasm3 | maintainer's call (discussion #78: he wants the height measured on its own first) | prototype the height, then the top |
| 6 | Memory operand offsets in the instruction (side-pool `resolve`) | ~2.3% of samples on the byte-hash row | small | maintainer (planned u16 memory index; our #63 was closed) | wait |
| 7 | NEON `tbl` for `i8x16.shuffle` and `swizzle` | 150–200 scalar instructions each | SIMD rows and femtovg only | opt-in like `simd-x86` | later |
| 8 | Host-call fast path (ideas 1 and 3) | ~500 instructions per host call | only for guests that call the host per sample | us | if production needs it |

Matching WAMR on xmrsplayer takes about 55% fewer instructions than
`next`. Items 1 and 2 remove 9% of them and 16–20% of the cycles; on the
iPhone XS Max a buffer drops from 51.2 to 40.8 ms (WAMR took 24.8 ms of
CPU per buffer on the same phone in the 2026-09-22 pass). Items 3 and 4
reach perhaps a third of the rest; the remainder needs register operands
(item 5).

The discussion draft and the reduced example
([`reduced.wat`](tinywasm-watch-2026-09-26/reduced.wat)) are for Matt to
post: tinywasm's CONTRIBUTING asks for text written by the contributor.

## Method

- **Builds:** WasmBench Release builds. tinywasm `next` `2e469af5` for the
  suite runs and the counter capture; `693d590c` (parser limits only
  since) for the A/B. WAMR `b70d708d` with the relaxed-SIMD and PROT_NONE
  patches.
- **Counters:** instructions and cycles are the process's fixed counters
  (`task_info`) over the timed window. They include the app's other
  threads, which is noise of about 1% on short rows.
- **Data:** raw console lines are under
  [`tinywasm-watch-2026-09-26/`](tinywasm-watch-2026-09-26/).
  - `watch-suite/`, `iphone16-e-cores/` and `iphone16-p-cores/`: per-row
    result lines.
  - `ab/`: the A/B reps.
  - `pmu/`: the guided-mode counter summaries (`pmu.jsonl`) and the Time
    Profiler histogram.
  - `shared-memory.wat` and `shared-memory-reps.json`: the shared-memory
    loops and their per-rep counters.
  - `iphone12-ab.csv`: the iPhone 12 A/B, every sample (`tmp` rows are the
    shared-memory loops).
  - `with-submitted-prs/`: the #64 A/B on three phones (`pr64-*.csv`) and
    the #64 + #72 A/B on the iPhone 12 (`submitted-a14.csv`), every
    sample; xmrsplayer's iPhone 12 counter summaries and Time Profiler
    histogram; dispatch histograms and the M4 runs of
    [`scripts/tinywasm-runner`](../scripts/tinywasm-runner/) (a standalone
    runner; `op-histogram.patch` adds the histogram to a tinywasm
    checkout); `hostcall.wat`, the call-cost loops.
  - `cheaper-calls/`: the A/Bs of plan item 2 on three phones, every
    sample: `isolation-*.csv` (`next` against #11), `stack-*.csv` (`next`,
    `next` + #64 + #72 and #12 in one run), `stack-rerun-*.csv` (the base
    against #12 again), `upstream-74-*.csv` (#11 rebased onto `next`
    `d1165c2`, as posted upstream; on the A12 and A13 one launch of the
    PR build lost its console stream, so its medians there are of four
    launches), `upstream-74-rerun-a12.csv` (a complete rerun on the A12:
    −4.8% cycles geomean against the posted −5.2%), and the M4 runner
    runs.
  - `registers/`: the value-stack register study ([`scripts/tos-bench`](../scripts/tos-bench/)):
    - `abi-probe.md`: the argument registers per target and ABI.
    - `tinywasm-x86-handlers.txt`: pushes and stack arguments of tinywasm's handlers on x86-64.
    - `sweep.txt`, `sweep-slice.txt` and `sweep-before-final-kinds.txt`: every timing run.
    - `pmu.jsonl` and `pmu-slice.jsonl`: the counter captures.
    - `asm/`: where each variant's state lives in machine code per target.
