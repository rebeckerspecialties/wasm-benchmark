// Shared SwiftUI view used by all three app targets (watchOS, iOS, macOS).
// Each target's project.yml entry includes this file via the
// `apps/Shared` source path.
//
// The view runs every workload defined by `WORKLOADS` once on appear,
// streams a per-workload report to stderr (visible via simctl /
// xcodebuild console), and renders a list summary on screen.

import SwiftUI

/// Static catalog of workloads exposed by benchmark-core's C ABI.
/// Each entry is `(human label, FFI runner, default input)`.
struct Workload: Identifiable, Sendable {
    let id: Int
    let label: String
    /// Closure that invokes the C entry point and returns a `BenchReport`.
    /// Marked `@Sendable` so the catalog itself is `Sendable` — the
    /// runner closure is invoked from a background queue.
    let run: @Sendable () -> BenchReport
}

// Each workload appears twice — once for each runtime — so the device
// run produces a side-by-side comparison.
let WORKLOADS: [Workload] = [
    Workload(id:  0, label: "[Pulley] fib(30)",                        run: { bench_run_fib(30) }),
    Workload(id:  1, label: "[ WAMR ] fib(30)",                        run: { bench_run_fib_wamr(30) }),
    Workload(id:  2, label: "[Pulley] fib_tail(100000) [return_call]", run: { bench_run_fib_tail(100000) }),
    Workload(id:  3, label: "[ WAMR ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wamr(100000) }),
    Workload(id:  4, label: "[Pulley] factorial(20)",                  run: { bench_run_factorial(20) }),
    Workload(id:  5, label: "[ WAMR ] factorial(20)",                  run: { bench_run_factorial_wamr(20) }),
    Workload(id:  6, label: "[Pulley] sieve(10000)",                   run: { bench_run_sieve(10000) }),
    Workload(id:  7, label: "[ WAMR ] sieve(10000)",                   run: { bench_run_sieve_wamr(10000) }),
    Workload(id:  8, label: "[Pulley] crc32(64KB)",                    run: { bench_run_crc32() }),
    Workload(id:  9, label: "[ WAMR ] crc32(64KB)",                    run: { bench_run_crc32_wamr() }),
    Workload(id: 10, label: "[Pulley] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd() }),
    Workload(id: 11, label: "[ WAMR ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wamr() }),
    Workload(id: 12, label: "[Pulley] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma() }),
    Workload(id: 13, label: "[ WAMR ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_wamr() }),
    Workload(id: 14, label: "[Pulley] convolution 256×256",            run: { bench_run_convolution() }),
    Workload(id: 15, label: "[ WAMR ] convolution 256×256",            run: { bench_run_convolution_wamr() }),
    Workload(id: 16, label: "[Pulley] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp() }),
    Workload(id: 17, label: "[ WAMR ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wamr() }),
    Workload(id: 18, label: "[Pulley] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory() }),
    Workload(id: 19, label: "[ WAMR ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wamr() }),
    Workload(id: 20, label: "[Pulley] call_indirect (200K dispatches)", run: { bench_run_call_indirect() }),
    Workload(id: 21, label: "[ WAMR ] call_indirect (200K dispatches)", run: { bench_run_call_indirect_wamr() }),
    // sqlite3 speedtest1 — single-shot, no WAMR comparison until
    // libiwasm.a is rebuilt with WAMR_BUILD_LIBC_WASI=1.
    Workload(id: 22, label: "[Pulley] sqlite3 speedtest1 (in-mem)", run: { bench_run_sqlite3() }),
    // Hand-written graphql-js validation-shape benchmarks. Two compilers,
    // same workload, very different `call_indirect` density:
    //   AS port (61 KB, 13 call_indirect): optimizer-friendly baseline
    //   Porffor port (121 KB, 98 call_indirect): preserves megamorphic
    //                                            dispatch shape — primary
    //                                            target for our optimization
    //                                            work.
    Workload(id: 23, label: "[Pulley] graphql-validation (AS)",      run: { bench_run_graphql_validation_as() }),
    Workload(id: 24, label: "[Pulley] graphql-validation (Porffor)", run: { bench_run_graphql_validation_porf() }),
    // xmrsplayer rendering 15 s of unreal.s3m (Scream Tracker 3 module)
    // through 31 call_indirect sites. Real-world dispatch-shaped workload
    // alongside the synthetic call_indirect.wasm and the JS-on-wasm
    // graphql-validation cases. Per-call wallclock is dominated by 15 s
    // of synthesis, so the harness usually settles on 1 iteration on
    // weak cores and a handful on M-class.
    Workload(id: 25, label: "[Pulley] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer() }),
    Workload(id: 26, label: "[ WAMR ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wamr() }),
    // C++-style vtable dispatch (StarlingMonkey-shaped pure-virtual
    // hierarchy). Four entry points sweep the IC's polymorphism
    // dimension — mono is the best case for a 1-way IC, bi/poly4/
    // poly6 test progressively worse polymorphism. Pulley-only
    // since the IC question is Pulley-specific.
    Workload(id: 27, label: "[Pulley] vtable_mono (200K)",  run: { bench_run_vtable_mono() }),
    Workload(id: 28, label: "[Pulley] vtable_bi (200K)",    run: { bench_run_vtable_bi() }),
    Workload(id: 29, label: "[Pulley] vtable_poly4 (200K)", run: { bench_run_vtable_poly4() }),
    Workload(id: 30, label: "[Pulley] vtable_poly6 (200K)", run: { bench_run_vtable_poly6() }),
    // WAMR variants for everything the wasm side can support. graphql-
    // validation Porffor on WAMR may fail at load (Porffor uses wasm
    // exceptions; our WAMR build has WAMR_BUILD_EXCE_HANDLING=0); the
    // harness reports the error string from wasm_runtime_get_exception
    // and the row is shown as ERROR. Treat that as data: "WAMR
    // can't run this shape with this build" is the cross-runtime
    // signal we want.
    Workload(id: 31, label: "[ WAMR ] graphql-validation (AS)",      run: { bench_run_graphql_validation_as_wamr() }),
    Workload(id: 32, label: "[ WAMR ] graphql-validation (Porffor)", run: { bench_run_graphql_validation_porf_wamr() }),
    Workload(id: 33, label: "[ WAMR ] vtable_mono (200K)",  run: { bench_run_vtable_mono_wamr() }),
    Workload(id: 34, label: "[ WAMR ] vtable_bi (200K)",    run: { bench_run_vtable_bi_wamr() }),
    Workload(id: 35, label: "[ WAMR ] vtable_poly4 (200K)", run: { bench_run_vtable_poly4_wamr() }),
    Workload(id: 36, label: "[ WAMR ] vtable_poly6 (200K)", run: { bench_run_vtable_poly6_wamr() }),
    // wasm3 (pure C interpreter) variants. wasm3 doesn't implement
    // SIMD, wasm exceptions, or WASI, so matmul_simd / matmul_fma /
    // graphql-validation Porffor will fail at load — the row reports
    // ERROR with wasm3's error string. xmrsplayer uses `return_call`,
    // which wasm3 *does* implement, so it should run (subject to the
    // 256 KiB wasm3 stack budget; see crates/benchmark-core/src/wasm3.rs).
    Workload(id: 37, label: "[wasm3 ] fib(30)",                        run: { bench_run_fib_wasm3(30) }),
    Workload(id: 38, label: "[wasm3 ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wasm3(100000) }),
    Workload(id: 39, label: "[wasm3 ] factorial(20)",                  run: { bench_run_factorial_wasm3(20) }),
    Workload(id: 40, label: "[wasm3 ] sieve(10000)",                   run: { bench_run_sieve_wasm3(10000) }),
    Workload(id: 41, label: "[wasm3 ] crc32(64KB)",                    run: { bench_run_crc32_wasm3() }),
    Workload(id: 42, label: "[wasm3 ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wasm3() }),
    Workload(id: 43, label: "[wasm3 ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_wasm3() }),
    Workload(id: 44, label: "[wasm3 ] convolution 256×256",            run: { bench_run_convolution_wasm3() }),
    Workload(id: 45, label: "[wasm3 ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wasm3() }),
    Workload(id: 46, label: "[wasm3 ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wasm3() }),
    Workload(id: 47, label: "[wasm3 ] call_indirect (200K dispatches)",run: { bench_run_call_indirect_wasm3() }),
    Workload(id: 48, label: "[wasm3 ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wasm3() }),
    Workload(id: 49, label: "[wasm3 ] graphql-validation (AS)",        run: { bench_run_graphql_validation_as_wasm3() }),
    Workload(id: 50, label: "[wasm3 ] graphql-validation (Porffor)",   run: { bench_run_graphql_validation_porf_wasm3() }),
    Workload(id: 51, label: "[wasm3 ] vtable_mono (200K)",             run: { bench_run_vtable_mono_wasm3() }),
    Workload(id: 52, label: "[wasm3 ] vtable_bi (200K)",               run: { bench_run_vtable_bi_wasm3() }),
    Workload(id: 53, label: "[wasm3 ] vtable_poly4 (200K)",            run: { bench_run_vtable_poly4_wasm3() }),
    Workload(id: 54, label: "[wasm3 ] vtable_poly6 (200K)",            run: { bench_run_vtable_poly6_wasm3() }),
    // WasmEdge variants. WasmEdge is the incumbent production runtime
    // (the WatchOS audio app ships it). Built with
    // WASMEDGE_USE_LLVM=OFF + the 27-patch Apple-mobile enablement
    // stack — pure-interpreter, App-Store-eligible. SIMD + wasm-
    // exceptions are both enabled in the same build (WAMR can't do
    // this), so graphql-validation Porffor loads successfully on this
    // path (it traps at run-time on the missing host import — same
    // shape as Pulley would without the host stub).
    Workload(id: 55, label: "[WE    ] fib(30)",                        run: { bench_run_fib_wasmedge(30) }),
    Workload(id: 56, label: "[WE    ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wasmedge(100000) }),
    Workload(id: 57, label: "[WE    ] factorial(20)",                  run: { bench_run_factorial_wasmedge(20) }),
    Workload(id: 58, label: "[WE    ] sieve(10000)",                   run: { bench_run_sieve_wasmedge(10000) }),
    Workload(id: 59, label: "[WE    ] crc32(64KB)",                    run: { bench_run_crc32_wasmedge() }),
    Workload(id: 60, label: "[WE    ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wasmedge() }),
    Workload(id: 61, label: "[WE    ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_wasmedge() }),
    Workload(id: 62, label: "[WE    ] convolution 256×256",            run: { bench_run_convolution_wasmedge() }),
    Workload(id: 63, label: "[WE    ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wasmedge() }),
    Workload(id: 64, label: "[WE    ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wasmedge() }),
    Workload(id: 65, label: "[WE    ] call_indirect (200K dispatches)",run: { bench_run_call_indirect_wasmedge() }),
    Workload(id: 66, label: "[WE    ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wasmedge() }),
    Workload(id: 67, label: "[WE    ] graphql-validation (AS)",        run: { bench_run_graphql_validation_as_wasmedge() }),
    Workload(id: 68, label: "[WE    ] graphql-validation (Porffor)",   run: { bench_run_graphql_validation_porf_wasmedge() }),
    Workload(id: 69, label: "[WE    ] vtable_mono (200K)",             run: { bench_run_vtable_mono_wasmedge() }),
    Workload(id: 70, label: "[WE    ] vtable_bi (200K)",               run: { bench_run_vtable_bi_wasmedge() }),
    Workload(id: 71, label: "[WE    ] vtable_poly4 (200K)",            run: { bench_run_vtable_poly4_wasmedge() }),
    Workload(id: 72, label: "[WE    ] vtable_poly6 (200K)",            run: { bench_run_vtable_poly6_wasmedge() }),
    // zwasm (clojurewasm/zwasm, Zig) variants. Built `-Djit=false`
    // so it's pure-interpreter / App-Store-eligible. Zig 0.16 has no
    // arm64_32 target → device-watch rows return ERROR ("zwasm not
    // linked into this build") — treat as data, not a regression.
    Workload(id: 73, label: "[zwasm ] fib(30)",                        run: { bench_run_fib_zwasm(30) }),
    Workload(id: 74, label: "[zwasm ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_zwasm(100000) }),
    Workload(id: 75, label: "[zwasm ] factorial(20)",                  run: { bench_run_factorial_zwasm(20) }),
    Workload(id: 76, label: "[zwasm ] sieve(10000)",                   run: { bench_run_sieve_zwasm(10000) }),
    Workload(id: 77, label: "[zwasm ] crc32(64KB)",                    run: { bench_run_crc32_zwasm() }),
    Workload(id: 78, label: "[zwasm ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_zwasm() }),
    Workload(id: 79, label: "[zwasm ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_zwasm() }),
    Workload(id: 80, label: "[zwasm ] convolution 256×256",            run: { bench_run_convolution_zwasm() }),
    Workload(id: 81, label: "[zwasm ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_zwasm() }),
    Workload(id: 82, label: "[zwasm ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_zwasm() }),
    Workload(id: 83, label: "[zwasm ] call_indirect (200K dispatches)",run: { bench_run_call_indirect_zwasm() }),
    Workload(id: 84, label: "[zwasm ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_zwasm() }),
    Workload(id: 85, label: "[zwasm ] graphql-validation (AS)",        run: { bench_run_graphql_validation_as_zwasm() }),
    Workload(id: 86, label: "[zwasm ] graphql-validation (Porffor)",   run: { bench_run_graphql_validation_porf_zwasm() }),
    Workload(id: 87, label: "[zwasm ] vtable_mono (200K)",             run: { bench_run_vtable_mono_zwasm() }),
    Workload(id: 88, label: "[zwasm ] vtable_bi (200K)",               run: { bench_run_vtable_bi_zwasm() }),
    Workload(id: 89, label: "[zwasm ] vtable_poly4 (200K)",            run: { bench_run_vtable_poly4_zwasm() }),
    Workload(id: 90, label: "[zwasm ] vtable_poly6 (200K)",            run: { bench_run_vtable_poly6_zwasm() }),
    // wasmz (Ray-D-Song/wasmz, Zig) variants. Pure interpreter, no
    // JIT — same App-Store-eligibility profile as zwasm. Ported to
    // Zig 0.16 via patches/wasmz/ because the upstream-pinned 0.15.2
    // build runner segfaults on macOS 26 Tahoe. Same arm64_32-watchOS
    // caveat as zwasm — device-watch rows return ERROR ("wasmz not
    // linked into this build") and that's signal, not noise.
    Workload(id: 91,  label: "[wasmz ] fib(30)",                        run: { bench_run_fib_wasmz(30) }),
    Workload(id: 92,  label: "[wasmz ] fib_tail(100000) [return_call]", run: { bench_run_fib_tail_wasmz(100000) }),
    Workload(id: 93,  label: "[wasmz ] factorial(20)",                  run: { bench_run_factorial_wasmz(20) }),
    Workload(id: 94,  label: "[wasmz ] sieve(10000)",                   run: { bench_run_sieve_wasmz(10000) }),
    Workload(id: 95,  label: "[wasmz ] crc32(64KB)",                    run: { bench_run_crc32_wasmz() }),
    Workload(id: 96,  label: "[wasmz ] matmul simd128 (64×64 f32)",     run: { bench_run_matmul_simd_wasmz() }),
    Workload(id: 97,  label: "[wasmz ] matmul relaxed-simd FMA",        run: { bench_run_matmul_fma_wasmz() }),
    Workload(id: 98,  label: "[wasmz ] convolution 256×256",            run: { bench_run_convolution_wasmz() }),
    Workload(id: 99,  label: "[wasmz ] audio DSP (1000 frames × 512)",  run: { bench_run_audio_dsp_wasmz() }),
    Workload(id: 100, label: "[wasmz ] bulk_memory (memory.copy/fill)", run: { bench_run_bulk_memory_wasmz() }),
    Workload(id: 101, label: "[wasmz ] call_indirect (200K dispatches)",run: { bench_run_call_indirect_wasmz() }),
    Workload(id: 102, label: "[wasmz ] xmrsplayer (1024-frame buffer)", run: { bench_run_xmrsplayer_wasmz() }),
    Workload(id: 103, label: "[wasmz ] graphql-validation (AS)",        run: { bench_run_graphql_validation_as_wasmz() }),
    Workload(id: 104, label: "[wasmz ] graphql-validation (Porffor)",   run: { bench_run_graphql_validation_porf_wasmz() }),
    Workload(id: 105, label: "[wasmz ] vtable_mono (200K)",             run: { bench_run_vtable_mono_wasmz() }),
    Workload(id: 106, label: "[wasmz ] vtable_bi (200K)",               run: { bench_run_vtable_bi_wasmz() }),
    Workload(id: 107, label: "[wasmz ] vtable_poly4 (200K)",            run: { bench_run_vtable_poly4_wasmz() }),
    Workload(id: 108, label: "[wasmz ] vtable_poly6 (200K)",            run: { bench_run_vtable_poly6_wasmz() }),
    // tinywasm (explodingcamera/tinywasm, pure Rust, no JIT/AOT tier) —
    // every workload, through the generic `bench_run_case` entry point
    // (runtime id 6). sqlite3 reports N/A: the harness only has a WASI
    // import shim for Pulley.
    Workload(id: 109, label: "[tinywm] fib(30)", run: { bench_run_case(6, "fib") }),
    Workload(id: 110, label: "[tinywm] fib_tail(100000) [return_call]", run: { bench_run_case(6, "fib_tail") }),
    Workload(id: 111, label: "[tinywm] factorial(20)", run: { bench_run_case(6, "factorial") }),
    Workload(id: 112, label: "[tinywm] sieve(10000)", run: { bench_run_case(6, "sieve") }),
    Workload(id: 113, label: "[tinywm] crc32(64KB)", run: { bench_run_case(6, "crc32") }),
    Workload(id: 114, label: "[tinywm] matmul simd128 (64×64 f32)", run: { bench_run_case(6, "matmul_simd") }),
    Workload(id: 115, label: "[tinywm] matmul relaxed-simd FMA", run: { bench_run_case(6, "matmul_fma") }),
    Workload(id: 116, label: "[tinywm] convolution 256×256", run: { bench_run_case(6, "convolution") }),
    Workload(id: 117, label: "[tinywm] audio DSP (1000 frames × 512)", run: { bench_run_case(6, "audio_dsp") }),
    Workload(id: 118, label: "[tinywm] bulk_memory (memory.copy/fill)", run: { bench_run_case(6, "bulk_memory") }),
    Workload(id: 119, label: "[tinywm] call_indirect (200K dispatches)", run: { bench_run_case(6, "call_indirect") }),
    Workload(id: 120, label: "[tinywm] xmrsplayer (1024-frame buffer)", run: { bench_run_case(6, "xmrsplayer") }),
    Workload(id: 121, label: "[tinywm] vtable_mono (200K)", run: { bench_run_case(6, "vtable_mono") }),
    Workload(id: 122, label: "[tinywm] vtable_bi (200K)", run: { bench_run_case(6, "vtable_bi") }),
    Workload(id: 123, label: "[tinywm] vtable_poly4 (200K)", run: { bench_run_case(6, "vtable_poly4") }),
    Workload(id: 124, label: "[tinywm] vtable_poly6 (200K)", run: { bench_run_case(6, "vtable_poly6") }),
    Workload(id: 125, label: "[tinywm] graphql-validation (AS)", run: { bench_run_case(6, "graphql_as") }),
    Workload(id: 126, label: "[tinywm] graphql-validation (Porffor)", run: { bench_run_case(6, "graphql_porf") }),
    Workload(id: 127, label: "[tinywm] sqlite3 speedtest1 (in-mem)", run: { bench_run_case(6, "sqlite3") }),
    Workload(id: 128, label: "[tinywm] factorial(20) [scalar build]", run: { bench_run_case(6, "factorial.scalar") }),
    Workload(id: 129, label: "[tinywm] sieve(10000) [scalar build]", run: { bench_run_case(6, "sieve.scalar") }),
    Workload(id: 130, label: "[tinywm] crc32(64KB) [scalar build]", run: { bench_run_case(6, "crc32.scalar") }),
    Workload(id: 131, label: "[tinywm] convolution 256×256 [scalar build]", run: { bench_run_case(6, "convolution.scalar") }),
    Workload(id: 132, label: "[tinywm] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(6, "bulk_memory.scalar") }),
    // Scalar (-simd128) builds of the workloads whose canonical build only
    // has auto-vectorized SIMD — the apples-to-apples interpreter
    // comparison for runtimes without an interpreter SIMD-128 path
    // (wasm3, zwasm). See scripts/build-workloads.sh.
    Workload(id: 133, label: "[Pulley] factorial(20) [scalar build]", run: { bench_run_case(0, "factorial.scalar") }),
    Workload(id: 134, label: "[Pulley] sieve(10000) [scalar build]", run: { bench_run_case(0, "sieve.scalar") }),
    Workload(id: 135, label: "[Pulley] crc32(64KB) [scalar build]", run: { bench_run_case(0, "crc32.scalar") }),
    Workload(id: 136, label: "[Pulley] convolution 256×256 [scalar build]", run: { bench_run_case(0, "convolution.scalar") }),
    Workload(id: 137, label: "[Pulley] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(0, "bulk_memory.scalar") }),
    Workload(id: 138, label: "[ WAMR ] factorial(20) [scalar build]", run: { bench_run_case(1, "factorial.scalar") }),
    Workload(id: 139, label: "[ WAMR ] sieve(10000) [scalar build]", run: { bench_run_case(1, "sieve.scalar") }),
    Workload(id: 140, label: "[ WAMR ] crc32(64KB) [scalar build]", run: { bench_run_case(1, "crc32.scalar") }),
    Workload(id: 141, label: "[ WAMR ] convolution 256×256 [scalar build]", run: { bench_run_case(1, "convolution.scalar") }),
    Workload(id: 142, label: "[ WAMR ] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(1, "bulk_memory.scalar") }),
    Workload(id: 143, label: "[wasm3 ] factorial(20) [scalar build]", run: { bench_run_case(2, "factorial.scalar") }),
    Workload(id: 144, label: "[wasm3 ] sieve(10000) [scalar build]", run: { bench_run_case(2, "sieve.scalar") }),
    Workload(id: 145, label: "[wasm3 ] crc32(64KB) [scalar build]", run: { bench_run_case(2, "crc32.scalar") }),
    Workload(id: 146, label: "[wasm3 ] convolution 256×256 [scalar build]", run: { bench_run_case(2, "convolution.scalar") }),
    Workload(id: 147, label: "[wasm3 ] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(2, "bulk_memory.scalar") }),
    Workload(id: 148, label: "[WE    ] factorial(20) [scalar build]", run: { bench_run_case(3, "factorial.scalar") }),
    Workload(id: 149, label: "[WE    ] sieve(10000) [scalar build]", run: { bench_run_case(3, "sieve.scalar") }),
    Workload(id: 150, label: "[WE    ] crc32(64KB) [scalar build]", run: { bench_run_case(3, "crc32.scalar") }),
    Workload(id: 151, label: "[WE    ] convolution 256×256 [scalar build]", run: { bench_run_case(3, "convolution.scalar") }),
    Workload(id: 152, label: "[WE    ] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(3, "bulk_memory.scalar") }),
    Workload(id: 153, label: "[zwasm ] factorial(20) [scalar build]", run: { bench_run_case(4, "factorial.scalar") }),
    Workload(id: 154, label: "[zwasm ] sieve(10000) [scalar build]", run: { bench_run_case(4, "sieve.scalar") }),
    Workload(id: 155, label: "[zwasm ] crc32(64KB) [scalar build]", run: { bench_run_case(4, "crc32.scalar") }),
    Workload(id: 156, label: "[zwasm ] convolution 256×256 [scalar build]", run: { bench_run_case(4, "convolution.scalar") }),
    Workload(id: 157, label: "[zwasm ] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(4, "bulk_memory.scalar") }),
    Workload(id: 158, label: "[wasmz ] factorial(20) [scalar build]", run: { bench_run_case(5, "factorial.scalar") }),
    Workload(id: 159, label: "[wasmz ] sieve(10000) [scalar build]", run: { bench_run_case(5, "sieve.scalar") }),
    Workload(id: 160, label: "[wasmz ] crc32(64KB) [scalar build]", run: { bench_run_case(5, "crc32.scalar") }),
    Workload(id: 161, label: "[wasmz ] convolution 256×256 [scalar build]", run: { bench_run_case(5, "convolution.scalar") }),
    Workload(id: 162, label: "[wasmz ] bulk_memory (memory.copy/fill) [scalar build]", run: { bench_run_case(5, "bulk_memory.scalar") }),
    // Wasm 3.0 feature benchmarks (cases.rs; one row per runtime, run
    // through `bench_run_case`). Runtimes without the feature report the
    // load / validation error as the row's error text; `.<twin>` rows
    // are the same program without the feature.
    Workload(id: 163, label: "[Pulley] tail-call FSM (65536 return_call)", run: { bench_run_case(0, "tailcall_fsm") }),
    Workload(id: 164, label: "[ WAMR ] tail-call FSM (65536 return_call)", run: { bench_run_case(1, "tailcall_fsm") }),
    Workload(id: 165, label: "[wasm3 ] tail-call FSM (65536 return_call)", run: { bench_run_case(2, "tailcall_fsm") }),
    Workload(id: 166, label: "[WE    ] tail-call FSM (65536 return_call)", run: { bench_run_case(3, "tailcall_fsm") }),
    Workload(id: 167, label: "[zwasm ] tail-call FSM (65536 return_call)", run: { bench_run_case(4, "tailcall_fsm") }),
    Workload(id: 168, label: "[wasmz ] tail-call FSM (65536 return_call)", run: { bench_run_case(5, "tailcall_fsm") }),
    Workload(id: 169, label: "[tinywm] tail-call FSM (65536 return_call)", run: { bench_run_case(6, "tailcall_fsm") }),
    Workload(id: 170, label: "[Pulley] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(0, "eh_parser_exnref") }),
    Workload(id: 171, label: "[ WAMR ] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(1, "eh_parser_exnref") }),
    Workload(id: 172, label: "[wasm3 ] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(2, "eh_parser_exnref") }),
    Workload(id: 173, label: "[WE    ] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(3, "eh_parser_exnref") }),
    Workload(id: 174, label: "[zwasm ] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(4, "eh_parser_exnref") }),
    Workload(id: 175, label: "[wasmz ] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(5, "eh_parser_exnref") }),
    Workload(id: 176, label: "[tinywm] EH parser, exnref (4096 stmts, 25% throw)", run: { bench_run_case(6, "eh_parser_exnref") }),
    Workload(id: 177, label: "[Pulley] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(0, "eh_parser_legacy") }),
    Workload(id: 178, label: "[ WAMR ] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(1, "eh_parser_legacy") }),
    Workload(id: 179, label: "[wasm3 ] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(2, "eh_parser_legacy") }),
    Workload(id: 180, label: "[WE    ] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(3, "eh_parser_legacy") }),
    Workload(id: 181, label: "[zwasm ] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(4, "eh_parser_legacy") }),
    Workload(id: 182, label: "[wasmz ] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(5, "eh_parser_legacy") }),
    Workload(id: 183, label: "[tinywm] EH parser, legacy try/catch (4096 stmts)", run: { bench_run_case(6, "eh_parser_legacy") }),
    Workload(id: 184, label: "[Pulley] GC binary trees (~130K struct.new)", run: { bench_run_case(0, "gc_trees") }),
    Workload(id: 185, label: "[ WAMR ] GC binary trees (~130K struct.new)", run: { bench_run_case(1, "gc_trees") }),
    Workload(id: 186, label: "[wasm3 ] GC binary trees (~130K struct.new)", run: { bench_run_case(2, "gc_trees") }),
    Workload(id: 187, label: "[WE    ] GC binary trees (~130K struct.new)", run: { bench_run_case(3, "gc_trees") }),
    Workload(id: 188, label: "[zwasm ] GC binary trees (~130K struct.new)", run: { bench_run_case(4, "gc_trees") }),
    Workload(id: 189, label: "[wasmz ] GC binary trees (~130K struct.new)", run: { bench_run_case(5, "gc_trees") }),
    Workload(id: 190, label: "[tinywm] GC binary trees (~130K struct.new)", run: { bench_run_case(6, "gc_trees") }),
    Workload(id: 191, label: "[Pulley] call_ref dispatch (200K, typed table)", run: { bench_run_case(0, "callref_dispatch") }),
    Workload(id: 192, label: "[ WAMR ] call_ref dispatch (200K, typed table)", run: { bench_run_case(1, "callref_dispatch") }),
    Workload(id: 193, label: "[wasm3 ] call_ref dispatch (200K, typed table)", run: { bench_run_case(2, "callref_dispatch") }),
    Workload(id: 194, label: "[WE    ] call_ref dispatch (200K, typed table)", run: { bench_run_case(3, "callref_dispatch") }),
    Workload(id: 195, label: "[zwasm ] call_ref dispatch (200K, typed table)", run: { bench_run_case(4, "callref_dispatch") }),
    Workload(id: 196, label: "[wasmz ] call_ref dispatch (200K, typed table)", run: { bench_run_case(5, "callref_dispatch") }),
    Workload(id: 197, label: "[tinywm] call_ref dispatch (200K, typed table)", run: { bench_run_case(6, "callref_dispatch") }),
    Workload(id: 198, label: "[Pulley] call_ref twin: call_indirect (200K)", run: { bench_run_case(0, "callref_dispatch.indirect") }),
    Workload(id: 199, label: "[ WAMR ] call_ref twin: call_indirect (200K)", run: { bench_run_case(1, "callref_dispatch.indirect") }),
    Workload(id: 200, label: "[wasm3 ] call_ref twin: call_indirect (200K)", run: { bench_run_case(2, "callref_dispatch.indirect") }),
    Workload(id: 201, label: "[WE    ] call_ref twin: call_indirect (200K)", run: { bench_run_case(3, "callref_dispatch.indirect") }),
    Workload(id: 202, label: "[zwasm ] call_ref twin: call_indirect (200K)", run: { bench_run_case(4, "callref_dispatch.indirect") }),
    Workload(id: 203, label: "[wasmz ] call_ref twin: call_indirect (200K)", run: { bench_run_case(5, "callref_dispatch.indirect") }),
    Workload(id: 204, label: "[tinywm] call_ref twin: call_indirect (200K)", run: { bench_run_case(6, "callref_dispatch.indirect") }),
    Workload(id: 205, label: "[Pulley] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(0, "relaxed_dot") }),
    Workload(id: 206, label: "[ WAMR ] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(1, "relaxed_dot") }),
    Workload(id: 207, label: "[wasm3 ] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(2, "relaxed_dot") }),
    Workload(id: 208, label: "[WE    ] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(3, "relaxed_dot") }),
    Workload(id: 209, label: "[zwasm ] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(4, "relaxed_dot") }),
    Workload(id: 210, label: "[wasmz ] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(5, "relaxed_dot") }),
    Workload(id: 211, label: "[tinywm] relaxed-SIMD int8 dot (64×64×256)", run: { bench_run_case(6, "relaxed_dot") }),
    Workload(id: 212, label: "[Pulley] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(0, "relaxed_madd") }),
    Workload(id: 213, label: "[ WAMR ] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(1, "relaxed_madd") }),
    Workload(id: 214, label: "[wasm3 ] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(2, "relaxed_madd") }),
    Workload(id: 215, label: "[WE    ] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(3, "relaxed_madd") }),
    Workload(id: 216, label: "[zwasm ] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(4, "relaxed_madd") }),
    Workload(id: 217, label: "[wasmz ] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(5, "relaxed_madd") }),
    Workload(id: 218, label: "[tinywm] relaxed-SIMD FMA Horner (16K pts)", run: { bench_run_case(6, "relaxed_madd") }),
    Workload(id: 219, label: "[Pulley] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(0, "mem64_chase") }),
    Workload(id: 220, label: "[ WAMR ] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(1, "mem64_chase") }),
    Workload(id: 221, label: "[wasm3 ] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(2, "mem64_chase") }),
    Workload(id: 222, label: "[WE    ] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(3, "mem64_chase") }),
    Workload(id: 223, label: "[zwasm ] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(4, "mem64_chase") }),
    Workload(id: 224, label: "[wasmz ] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(5, "mem64_chase") }),
    Workload(id: 225, label: "[tinywm] memory64 pointer chase (64 MiB, 256K hops)", run: { bench_run_case(6, "mem64_chase") }),
    Workload(id: 226, label: "[Pulley] memory64 twin: 32-bit memory", run: { bench_run_case(0, "mem64_chase.mem32") }),
    Workload(id: 227, label: "[ WAMR ] memory64 twin: 32-bit memory", run: { bench_run_case(1, "mem64_chase.mem32") }),
    Workload(id: 228, label: "[wasm3 ] memory64 twin: 32-bit memory", run: { bench_run_case(2, "mem64_chase.mem32") }),
    Workload(id: 229, label: "[WE    ] memory64 twin: 32-bit memory", run: { bench_run_case(3, "mem64_chase.mem32") }),
    Workload(id: 230, label: "[zwasm ] memory64 twin: 32-bit memory", run: { bench_run_case(4, "mem64_chase.mem32") }),
    Workload(id: 231, label: "[wasmz ] memory64 twin: 32-bit memory", run: { bench_run_case(5, "mem64_chase.mem32") }),
    Workload(id: 232, label: "[tinywm] memory64 twin: 32-bit memory", run: { bench_run_case(6, "mem64_chase.mem32") }),
    Workload(id: 233, label: "[Pulley] multi-memory transform (3 memories)", run: { bench_run_case(0, "multimem_transform") }),
    Workload(id: 234, label: "[ WAMR ] multi-memory transform (3 memories)", run: { bench_run_case(1, "multimem_transform") }),
    Workload(id: 235, label: "[wasm3 ] multi-memory transform (3 memories)", run: { bench_run_case(2, "multimem_transform") }),
    Workload(id: 236, label: "[WE    ] multi-memory transform (3 memories)", run: { bench_run_case(3, "multimem_transform") }),
    Workload(id: 237, label: "[zwasm ] multi-memory transform (3 memories)", run: { bench_run_case(4, "multimem_transform") }),
    Workload(id: 238, label: "[wasmz ] multi-memory transform (3 memories)", run: { bench_run_case(5, "multimem_transform") }),
    Workload(id: 239, label: "[tinywm] multi-memory transform (3 memories)", run: { bench_run_case(6, "multimem_transform") }),
    Workload(id: 240, label: "[Pulley] multi-memory twin: one memory", run: { bench_run_case(0, "multimem_transform.single") }),
    Workload(id: 241, label: "[ WAMR ] multi-memory twin: one memory", run: { bench_run_case(1, "multimem_transform.single") }),
    Workload(id: 242, label: "[wasm3 ] multi-memory twin: one memory", run: { bench_run_case(2, "multimem_transform.single") }),
    Workload(id: 243, label: "[WE    ] multi-memory twin: one memory", run: { bench_run_case(3, "multimem_transform.single") }),
    Workload(id: 244, label: "[zwasm ] multi-memory twin: one memory", run: { bench_run_case(4, "multimem_transform.single") }),
    Workload(id: 245, label: "[wasmz ] multi-memory twin: one memory", run: { bench_run_case(5, "multimem_transform.single") }),
    Workload(id: 246, label: "[tinywm] multi-memory twin: one memory", run: { bench_run_case(6, "multimem_transform.single") }),
    Workload(id: 247, label: "[Pulley] extended-const instantiate (2560 globals)", run: { bench_run_case(0, "extconst_init") }),
    Workload(id: 248, label: "[ WAMR ] extended-const instantiate (2560 globals)", run: { bench_run_case(1, "extconst_init") }),
    Workload(id: 249, label: "[wasm3 ] extended-const instantiate (2560 globals)", run: { bench_run_case(2, "extconst_init") }),
    Workload(id: 250, label: "[WE    ] extended-const instantiate (2560 globals)", run: { bench_run_case(3, "extconst_init") }),
    Workload(id: 251, label: "[zwasm ] extended-const instantiate (2560 globals)", run: { bench_run_case(4, "extconst_init") }),
    Workload(id: 252, label: "[wasmz ] extended-const instantiate (2560 globals)", run: { bench_run_case(5, "extconst_init") }),
    Workload(id: 253, label: "[tinywm] extended-const instantiate (2560 globals)", run: { bench_run_case(6, "extconst_init") }),
    Workload(id: 254, label: "[Pulley] extended-const twin: MVP consts", run: { bench_run_case(0, "extconst_init.mvp") }),
    Workload(id: 255, label: "[ WAMR ] extended-const twin: MVP consts", run: { bench_run_case(1, "extconst_init.mvp") }),
    Workload(id: 256, label: "[wasm3 ] extended-const twin: MVP consts", run: { bench_run_case(2, "extconst_init.mvp") }),
    Workload(id: 257, label: "[WE    ] extended-const twin: MVP consts", run: { bench_run_case(3, "extconst_init.mvp") }),
    Workload(id: 258, label: "[zwasm ] extended-const twin: MVP consts", run: { bench_run_case(4, "extconst_init.mvp") }),
    Workload(id: 259, label: "[wasmz ] extended-const twin: MVP consts", run: { bench_run_case(5, "extconst_init.mvp") }),
    Workload(id: 260, label: "[tinywm] extended-const twin: MVP consts", run: { bench_run_case(6, "extconst_init.mvp") }),
]

struct WorkloadResult: Identifiable {
    let id: Int
    let label: String
    let report: BenchReport
    let errorText: String?
}

struct BenchmarkContentView: View {
    @State private var results: [WorkloadResult] = []
    @State private var running: Bool = false
    @State private var currentLabel: String = ""

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text("Pulley vs WAMR vs wasm3 vs WasmEdge vs zwasm vs wasmz vs tinywasm")
                    .font(.title3.bold())
                // Status line — when the run is in progress, shows
                // "running <workload>". When the run completes,
                // flips to a winner summary computed from per-workload
                // medians (see `winnerSummary(_:)` below). This is the
                // at-a-glance answer users tune in for; before this we
                // shipped the data as a ~120-row scrollable dump and
                // expected viewers to import to a spreadsheet to see
                // who actually won.
                Group {
                    if running {
                        HStack(spacing: 6) {
                            ProgressView().controlSize(.small)
                            Text("running \(currentLabel)…")
                        }
                    } else if !results.isEmpty {
                        Text(winnerSummary(results))
                            .font(.caption.bold())
                            .foregroundColor(.green)
                    } else {
                        Text("workload set • \(WORKLOADS.count) cases")
                    }
                }
                .font(.caption)
                Button(running ? "Running…" : "Run all") { runAll() }
                    .disabled(running)
                // tvOS-specific: rows must be intrinsically focusable
                // AND siblings of a properly-sized scroll container so
                // the Siri Remote's 5-way clicks + swipe-up/swipe-down
                // gestures both navigate the list. The canonical
                // pattern is `Button { } label: { ... }` because
                // Buttons are well-tested focus stops with the focus
                // engine's auto-scroll. A bare `.focusable(true)` on
                // a VStack passes the focus check but breaks
                // auto-scroll: after the first move, the focused row
                // ends up off-screen and the engine reports "no
                // focusable target" with a beep on every subsequent
                // direction press. The cardless `.buttonStyle(.plain)`
                // (where it exists) preserves the current VStack
                // layout; otherwise the standard tvOS button
                // highlight takes over (still readable).
                //
                // On non-tvOS targets the bare WorkloadRow is fine —
                // touch / mouse / Digital Crown drives scroll without
                // a focus engine in the way.
                #if os(tvOS)
                LazyVStack(alignment: .leading, spacing: 8) {
                    ForEach(results) { r in
                        Button(action: {}) {
                            WorkloadRow(result: r)
                        }
                        .buttonStyle(.card)
                    }
                }
                #else
                ForEach(results) { r in
                    WorkloadRow(result: r)
                }
                #endif
            }
            .padding()
        }
        .onAppear { runAll() }
    }

    private func runAll() {
        guard !running else { return }
        running = true
        results = []
        // WAMR's stack-guard setup must run on the main thread before
        // any worker thread tries to load a wasm module. The actual
        // runtime call into wasm_runtime_init() returns 0 if the build
        // didn't link in libiwasm.a (e.g. older device libs).
        let wamrOk = bench_init_wamr() == 1
        FileHandle.standardError.write(Data("wamr init: \(wamrOk ? "ok" : "unavailable")\n".utf8))
        // wasm3 has no process-global state, but we call its init
        // symmetrically so all three runtimes' availability is logged
        // up-front in the same line shape.
        let wasm3Ok = bench_init_wasm3() == 1
        FileHandle.standardError.write(Data("wasm3 init: \(wasm3Ok ? "ok" : "unavailable")\n".utf8))
        // WasmEdge — same shape; reports "unavailable" if libwasmedge.a
        // wasn't linked in (e.g. host-only build before
        // scripts/build-wasmedge.sh has run for this target).
        let wasmedgeOk = bench_init_wasmedge() == 1
        FileHandle.standardError.write(Data("wasmedge init: \(wasmedgeOk ? "ok" : "unavailable")\n".utf8))
        let zwasmOk = bench_init_zwasm() == 1
        FileHandle.standardError.write(Data("zwasm init: \(zwasmOk ? "ok" : "unavailable")\n".utf8))
        // wasmz — patched to Zig 0.16; reports "unavailable" if libwasmz.a
        // wasn't linked in (e.g. arm64_32 watchOS or host-only build
        // before scripts/build-wasmz.sh has run for this target).
        let wasmzOk = bench_init_wasmz() == 1
        FileHandle.standardError.write(Data("wasmz init: \(wasmzOk ? "ok" : "unavailable")\n".utf8))
        let tinywasmOk = bench_init_tinywasm() == 1
        FileHandle.standardError.write(Data("tinywasm init: \(tinywasmOk ? "ok" : "unavailable")\n".utf8))
        // One-shot PAC viability probe. Useful as a planning input for
        // the future PAC-signed IC slot scheme; not a benchmark.
        let pac = bench_pac_probe()
        FileHandle.standardError.write(Data(String(
            format: "pac probe: supported=%d code1=0x%016llx code2=0x%016llx code3=0x%016llx | nonzero=%d deterministic=%d input_dep=%d low_zero=%d\n",
            pac.supported, pac.code1, pac.code2, pac.code3,
            pac.nonzero, pac.deterministic, pac.input_dep, pac.low_zero
        ).utf8))
        // Optional `WORKLOADS` env-var filter (comma-separated, case-
        // insensitive substring match against the workload label).
        // Optional `RUNTIMES` env-var filter (comma-separated; valid
        // values are `pulley`, `wamr`, `wasm3`, `wasmedge`, `zwasm`,
        // `wasmz`, `tinywasm`) to keep only the
        // matching runtime — useful for PMU traces where you want to
        // isolate signal from one runtime without the others'
        // identical-across-builds dispatch overhead diluting the trace
        // aggregate. Without filters, every workload runs on every
        // runtime that supports it.
        let workloads: [Workload] = {
            // watchOS doesn't propagate `devicectl --environment-variables`
            // to ProcessInfo (verified empirically — iOS does, watchOS
            // doesn't). For PR-time targeted runs on the watch, edit
            // WATCHOS_WORKLOADS_FILTER below to a comma-separated needle
            // list ("xmrsplayer") or empty string ("") for "all". iOS /
            // macOS continue to read the env vars, so the iPhone /
            // M-series runner is unaffected.
            #if os(watchOS)
            // `matmul` keeps both matmul-simd128 (legacy SIMD) and
            // matmul-relaxed-simd FMA (new — relies on WAMR's
            // relaxed-SIMD support shipped in
            // rebeckerspecialties/wasm-micro-runtime#3); both should
            // now run on WAMR-arm64_32-watchos thanks to the
            // `WAMR_BUILD_RELAXED_SIMD=1` flag in
            // `scripts/build-wamr.sh`. graphql-validation covers the
            // Porffor variant which exercises WAMR's legacy-EH
            // support (rebeckerspecialties/wasm-micro-runtime#2's
            // full-spec lowering).
            let WATCHOS_WORKLOADS_FILTER = "call_indirect,xmrsplayer,vtable_mono,vtable_bi,vtable_poly4,vtable_poly6,graphql-validation,matmul"
            let WATCHOS_RUNTIMES_FILTER = ""
            let env = WATCHOS_WORKLOADS_FILTER
            let runtimesEnv = WATCHOS_RUNTIMES_FILTER
            #else
            let env = ProcessInfo.processInfo.environment["WORKLOADS"] ?? ""
            let runtimesEnv = ProcessInfo.processInfo.environment["RUNTIMES"] ?? ""
            #endif
            let trimmedW = env.trimmingCharacters(in: .whitespaces)
            let trimmedR = runtimesEnv.trimmingCharacters(in: .whitespaces)
            let needles = trimmedW
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
                .filter { !$0.isEmpty }
            let runtimes = trimmedR
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
                .filter { !$0.isEmpty }
            if !needles.isEmpty || !runtimes.isEmpty {
                let joinedW = needles.isEmpty ? "(any)" : needles.joined(separator: ", ")
                let joinedR = runtimes.isEmpty ? "(any)" : runtimes.joined(separator: ", ")
                FileHandle.standardError.write(
                    Data("WORKLOADS filter: \(joinedW); RUNTIMES filter: \(joinedR)\n".utf8)
                )
            }
            return WORKLOADS.filter { w in
                let lc = w.label.lowercased()
                let workloadOk = needles.isEmpty
                    || needles.contains(where: { lc.contains($0) })
                let runtimeOk = runtimes.isEmpty
                    || runtimes.contains(where: { rt in
                        // Labels look like `[Pulley] call_indirect ...`
                        // or `[ WAMR ] call_indirect ...` or
                        // `[wasm3 ] call_indirect ...`. Case-insensitive
                        // substring on the prefix is unambiguous.
                        switch rt {
                        case "pulley":
                            return lc.contains("[pulley]")
                        case "wamr":
                            return lc.contains("[ wamr ]")
                        case "wasm3", "m3":
                            return lc.contains("[wasm3 ]")
                        case "wasmedge", "we":
                            return lc.contains("[we    ]")
                        case "zwasm":
                            return lc.contains("[zwasm ]")
                        case "wasmz":
                            return lc.contains("[wasmz ]")
                        case "tinywasm", "tinywm":
                            return lc.contains("[tinywm]")
                        default:
                            return false
                        }
                    })
                return workloadOk && runtimeOk
            }
        }()
        // .utility QoS pins worker scheduling to efficiency cores on
        // Apple Silicon (P-cores are reserved for .userInitiated+).
        // Matches our M4 E-core measurement methodology (`taskpolicy -b`)
        // so iPhone XS / SE2 numbers are directly comparable to M4 E-core
        // numbers. .background was tried first but iOS may suspend
        // .background work aggressively even with the app foregrounded;
        // .utility is the lowest QoS that keeps the worker running
        // continuously while still preferring E-cores.
        let qosOverride = ProcessInfo.processInfo.environment["BENCH_QOS"]?
            .trimmingCharacters(in: .whitespaces).lowercased() ?? ""
        let chosenQoS: DispatchQoS.QoSClass
        switch qosOverride {
        case "user-initiated", "userinitiated", "p":
            chosenQoS = .userInitiated
        case "user-interactive", "userinteractive":
            chosenQoS = .userInteractive
        default:
            chosenQoS = .utility
        }
        DispatchQueue.global(qos: chosenQoS).async {
            #if os(iOS) || os(macOS)
            // FEMTOVG_E2E=0,1 runs the femtovg E2E (scenes 0 and/or 1) on the
            // runtimes in RUNTIMES instead of the workload list; one runtime
            // per launch keeps each runtime's peak footprint separate.
            if let scenes = ProcessInfo.processInfo.environment["FEMTOVG_E2E"], !scenes.isEmpty {
                runFemtovgE2E(scenes: scenes)
                DispatchQueue.main.async {
                    running = false
                    currentLabel = ""
                }
                return
            }
            #endif
            for w in workloads {
                DispatchQueue.main.async { currentLabel = w.label }
                var report = w.run()
                let rendered = formatReport(&report)
                FileHandle.standardError.write(
                    Data(("[\(w.label)] " + rendered.replacingOccurrences(of: "\n", with: " | ") + "\n").utf8)
                )
                let errText: String? = report.ok != 1
                    ? rendered.replacingOccurrences(of: "ERROR: ", with: "")
                    : nil
                let result = WorkloadResult(id: w.id, label: w.label, report: report, errorText: errText)
                DispatchQueue.main.async { results.append(result) }
            }
            DispatchQueue.main.async {
                running = false
                currentLabel = ""
                // Also emit the winner string to stderr so headless
                // launches via devicectl --console see the verdict
                // even when we can't take a screenshot of the
                // top-of-view status text.
                FileHandle.standardError.write(
                    Data((winnerSummary(results) + "\n").utf8)
                )
            }
        }
    }

}

#if os(iOS) || os(macOS)
/// femtovg E2E (docs/femtovg-e2e-abi.md) for every scene in `scenes` on each
/// runtime named in RUNTIMES (all seven if unset). Frames and passes come
/// from FEMTOVG_FRAMES / FEMTOVG_PASSES (default 121 / 2). Each result is
/// one `FEMTOVG_E2E {json}` line on stderr.
fileprivate func runFemtovgE2E(scenes: String) {
    let env = ProcessInfo.processInfo.environment
    let frames = UInt32(env["FEMTOVG_FRAMES"] ?? "") ?? 121
    let passes = UInt32(env["FEMTOVG_PASSES"] ?? "") ?? 2
    let ids: [String: UInt32] = [
        "pulley": 0, "wamr": 1, "wasm3": 2, "m3": 2, "wasmedge": 3, "we": 3,
        "zwasm": 4, "wasmz": 5, "tinywasm": 6, "tinywm": 6,
    ]
    let requested = (env["RUNTIMES"] ?? "")
        .split(separator: ",")
        .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
        .filter { !$0.isEmpty }
    let runtimes: [UInt32] = requested.isEmpty ? Array(0...6) : requested.compactMap { ids[$0] }
    let sceneIds = scenes.split(separator: ",").compactMap { UInt32($0.trimmingCharacters(in: .whitespaces)) }
    for rt in runtimes {
        for scene in sceneIds {
            guard let cstr = bench_femtovg_e2e(rt, scene, frames, passes) else { continue }
            let line = String(cString: cstr)
            bench_free_cstring(cstr)
            FileHandle.standardError.write(Data(("FEMTOVG_E2E " + line + "\n").utf8))
        }
    }
    FileHandle.standardError.write(Data("FEMTOVG_E2E done\n".utf8))
}
#endif

// Free function (no `self`) — safe to call from a background queue under
// Swift 6 strict concurrency. Frees and nils `error_msg` so the report
// can be stored without a dangling C pointer.
fileprivate func formatReport(_ report: inout BenchReport) -> String {
        if report.ok != 1 {
            let msg: String
            if let cstr = report.error_msg {
                msg = String(cString: cstr)
                bench_free_error_msg(report.error_msg)
                report.error_msg = nil
            } else {
                msg = "(no message)"
            }
            return "ERROR: \(msg)"
        }
        let loadMs = Double(report.load_ns) / 1_000_000.0
        let minMs = Double(report.run_ns_min) / 1_000_000.0
        let medMs = Double(report.run_ns_median) / 1_000_000.0
        let p99Ms = Double(report.run_ns_p99) / 1_000_000.0
        let userMs = Double(report.cpu_user_ns) / 1_000_000.0
        let sysMs = Double(report.cpu_system_ns) / 1_000_000.0
        let rssKB = Double(report.rss_peak_bytes) / 1024.0
        // Measured, not assumed: share of the timed window's CPU time that
        // ran on E-cores (rusage P-core accounting), and IPC from the
        // always-on fixed counters.
        let cpuNs = Double(report.cpu_user_ns + report.cpu_system_ns)
        let eShare = cpuNs > 0 ? 1.0 - min(Double(report.p_cpu_ns) / cpuNs, 1.0) : -1.0
        let ipc = report.cycles > 0 ? Double(report.instructions) / Double(report.cycles) : -1.0
        return String(
            format: "result=%d  iter=%u  load=%.3fms  min=%.3f median=%.3f p99=%.3f ms  cpu(u/s)=%.2f/%.2f ms  rss=%.0fKB  faults=%llu  e_share=%.3f  ipc=%.2f  insns=%llu  cycles=%llu",
            report.result, report.iterations,
            loadMs, minMs, medMs, p99Ms,
            userMs, sysMs, rssKB, report.page_faults,
            eShare, ipc, report.instructions, report.cycles
        )
}

struct WorkloadRow: View {
    let result: WorkloadResult

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(result.label)
                .font(.caption.bold())
            Text(detail)
                .font(.system(.caption2, design: .monospaced))
                .foregroundColor(result.report.ok == 1 ? .primary : .red)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        // Note: focus-engine integration on tvOS is handled at the
        // call site by wrapping each row in `Button { } label: { ... }`
        // with `.buttonStyle(.card)`. That gives auto-scroll behaviour
        // the Siri Remote expects. See `BenchmarkContentView.body`.
    }

    private var detail: String {
        if result.report.ok != 1 {
            return "ERR: \(result.errorText ?? "(no message)")"
        }
        let medMs = Double(result.report.run_ns_median) / 1_000_000.0
        let p99Ms = Double(result.report.run_ns_p99) / 1_000_000.0
        let rssKB = Double(result.report.rss_peak_bytes) / 1024.0
        return String(
            format: "= %d  iter=%u  med %.2f / p99 %.2f ms  rss %.0fKB",
            result.report.result, result.report.iterations,
            medMs, p99Ms, rssKB
        )
    }
}

// =====================================================================
// Winner summary
// =====================================================================
//
// Walks the per-(runtime, workload) result list and tallies how many
// workloads each runtime wins (lowest median time). Returns a one-
// line summary suitable for the status line at the top of the view.
//
// Semantics:
//   * A row is a "candidate" only if it completed with `ok == 1` AND
//     has at least one peer (same workload, different runtime) that
//     also completed — otherwise the workload isn't a comparison.
//   * Within each comparable workload, the winner is the runtime
//     with the smallest `run_ns_median`. Ties (medians within 1 % of
//     each other) count as half-wins for each tied runtime.
//   * The runtime-with-the-most-wins is the "overall winner". If the
//     gap between #1 and #2 is ≤ 1 workload, we call it a tie
//     between the two; the user almost-certainly wants to read the
//     individual rows in that case.
//
// Workload-label format (set by the entries in `WORKLOADS`):
//   "[Pulley] fib(30)"          → runtime="pulley",  workload="fib(30)"
//   "[ WAMR ] fib(30)"          → runtime="wamr",    workload="fib(30)"
//   "[wasm3 ] fib(30)"          → runtime="wasm3",   workload="fib(30)"
//   "[WE    ] fib(30)"          → runtime="wasmedge", workload="fib(30)"
//   "[zwasm ] fib(30)"          → runtime="zwasm",   workload="fib(30)"
//   "[wasmz ] fib(30)"          → runtime="wasmz",   workload="fib(30)"
// =====================================================================

fileprivate func runtimeAndWorkload(from label: String) -> (runtime: String, workload: String)? {
    // Expect leading `[<rt>] <workload>` where `<rt>` is padded.
    guard label.hasPrefix("["), let closeBracket = label.firstIndex(of: "]") else {
        return nil
    }
    let rtRaw = label[label.index(after: label.startIndex)..<closeBracket]
        .trimmingCharacters(in: .whitespaces)
        .lowercased()
    // Normalise the "WE" abbreviation to "wasmedge" so the summary
    // matches the same runtime names we use in the rest of the harness
    // (lib.rs, the bench logs, the cross-runtime table in AGENTS.md).
    let rt = rtRaw == "we" ? "wasmedge" : (rtRaw == "tinywm" ? "tinywasm" : rtRaw)
    // Skip the closing bracket + the space after it.
    let wlStart = label.index(closeBracket, offsetBy: 2, limitedBy: label.endIndex)
        ?? label.endIndex
    let workload = String(label[wlStart...])
    return (rt, workload)
}

fileprivate func winnerSummary(_ results: [WorkloadResult]) -> String {
    // (workload → [runtime: median_ns]) for ok rows only.
    var byWorkload: [String: [String: UInt64]] = [:]
    for r in results where r.report.ok == 1 {
        guard let parsed = runtimeAndWorkload(from: r.label) else { continue }
        byWorkload[parsed.workload, default: [:]][parsed.runtime] = r.report.run_ns_median
    }
    // For each workload with ≥2 ok runtimes, award a win (or half-win
    // on a near-tie) to the runtime with the smallest median.
    var wins: [String: Double] = [:]
    var comparableWorkloads = 0
    for (_, medians) in byWorkload where medians.count >= 2 {
        comparableWorkloads += 1
        // Find the smallest median + any other runtimes within 1 %.
        let minMed = medians.values.min()!
        let tieThreshold = Double(minMed) * 1.01
        let topRuntimes = medians.filter { Double($0.value) <= tieThreshold }
        let share = 1.0 / Double(topRuntimes.count)
        for rt in topRuntimes.keys {
            wins[rt, default: 0.0] += share
        }
    }
    guard comparableWorkloads > 0 else {
        return "No comparable workloads yet (need at least one workload completed on ≥2 runtimes)."
    }
    // Sort by wins descending.
    let ranked = wins.sorted { $0.value > $1.value }
    guard let first = ranked.first else {
        return "No comparable workloads yet."
    }
    let second = ranked.count >= 2 ? ranked[1] : nil
    // Outright winner if the lead over #2 is > 1 workload.
    let isOutright: Bool
    if let s = second {
        isOutright = (first.value - s.value) > 1.0
    } else {
        isOutright = true
    }
    func fmt(_ x: Double) -> String {
        // Drop the fraction when it's an integer (a clean win).
        x == x.rounded() ? String(Int(x)) : String(format: "%.1f", x)
    }
    if isOutright {
        return "🏆 \(first.key) wins \(fmt(first.value)) / \(comparableWorkloads) workloads"
    } else if let s = second {
        return "🤝 tie: \(first.key) & \(s.key) both ≈ \(fmt(max(first.value, s.value))) / \(comparableWorkloads) workloads"
    } else {
        return "🏆 \(first.key) wins \(fmt(first.value)) / \(comparableWorkloads) workloads"
    }
}
