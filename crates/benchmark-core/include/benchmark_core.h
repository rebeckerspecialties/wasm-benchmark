// C ABI surface of the Rust `benchmark-core` crate.
//
// Imported into Swift via the per-target bridging headers.

#ifndef BENCHMARK_CORE_H
#define BENCHMARK_CORE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

// Stable layout — must match `#[repr(C)] struct BenchReport` in lib.rs.
typedef struct BenchReport {
    int32_t result;
    uint8_t ok;                  // 1 = success, 0 = error
    uint32_t iterations;         // number of run() iterations measured
    uint64_t load_ns;            // parse + lower nanoseconds (one-time)
    uint64_t run_ns_min;         // fastest single-iteration runtime
    uint64_t run_ns_median;
    uint64_t run_ns_p99;
    uint64_t cpu_user_ns;        // task_info user time delta over all runs
    uint64_t cpu_system_ns;      // task_info system time delta
    uint64_t rss_peak_bytes;     // mach_task_basic_info peak resident size
    uint64_t page_faults;        // task_events_info::faults delta
    char *error_msg;             // nul-terminated; NULL on success.
                                 // Caller frees via bench_free_error_msg.
} BenchReport;

// Pulley (wasmtime) path. Each loads its embedded `.wasm` via
// wasmtime + Pulley interpreter, invokes the wasm-side function, and
// returns a populated BenchReport.
BenchReport bench_run_fib(int32_t n);
BenchReport bench_run_fib_tail(int32_t n);
BenchReport bench_run_factorial(int32_t n);
BenchReport bench_run_sieve(int32_t n);
BenchReport bench_run_crc32(void);
BenchReport bench_run_matmul_simd(void);
BenchReport bench_run_matmul_fma(void);
BenchReport bench_run_convolution(void);
BenchReport bench_run_audio_dsp(void);
BenchReport bench_run_bulk_memory(void);
BenchReport bench_run_call_indirect(void);
// SQLite speedtest1 from Sightglass — large realistic workload.
// Single-shot; load_ns + run_ns_min/median/p99 all reflect one
// `_start` invocation. Pulley path only (WAMR side requires
// libc-wasi, not yet enabled in our libiwasm.a builds).
BenchReport bench_run_sqlite3(void);

// Hand-written graphql-js validation-shape benchmarks, two parallel
// implementations of the same workload (validate a fixed schema + query
// AST through 5 graphql-js-shape rules). See
// workloads/graphql-validation/{ASSEMBLYSCRIPT,PORFFOR}-NOTES.md.
//
// AS variant: 61 KB, 0 imports, 13 call_indirect — optimizer-friendly
// (most dispatch lowered to direct calls because AS resolves receiver
// types statically).
//
// Porffor variant: 121 KB, 1 import (host print, stubbed), 98
// call_indirect — preserves graphql-js's megamorphic dispatch shape;
// primary target for call_indirect-focused optimization work.
BenchReport bench_run_graphql_validation_as(void);
BenchReport bench_run_graphql_validation_porf(void);

// xmrsplayer rendering `unreal.s3m` (Scream Tracker 3 module) at
// 44.1 kHz stereo to a null sound driver. Real-world
// `call_indirect`-shaped workload: 27 sites lowered from 12 dyn-trait
// dispatches in xmrsplayer's per-tick effect pipeline. Each call
// renders one audio buffer of 1024 stereo frames (≈ 23 ms of audio,
// matching a typical CoreAudio per-callback budget). Player state
// persists across calls; the song loops indefinitely so any iter
// count works.
BenchReport bench_run_xmrsplayer(void);

// vtable_dispatch — C++-style virtual dispatch through call_indirect.
// Four entry points sweep the IC's polymorphism dimension:
//   - bench_run_vtable_mono   100% monomorphic
//   - bench_run_vtable_bi     alternating bimodal
//   - bench_run_vtable_poly4  4-way rotation
//   - bench_run_vtable_poly6  6-way rotation
BenchReport bench_run_vtable_mono(void);
BenchReport bench_run_vtable_bi(void);
BenchReport bench_run_vtable_poly4(void);
BenchReport bench_run_vtable_poly6(void);

// WAMR comparison variants for the workloads above. graphql-validation
// Porffor on WAMR may fail at instantiation because Porffor compiles
// JS try/catch to wasm exceptions and our WAMR build has
// WAMR_BUILD_EXCE_HANDLING=0 — the runner reports the wasm-level
// error string from wasm_runtime_get_exception in that case.
BenchReport bench_run_vtable_mono_wamr(void);
BenchReport bench_run_vtable_bi_wamr(void);
BenchReport bench_run_vtable_poly4_wamr(void);
BenchReport bench_run_vtable_poly6_wamr(void);
BenchReport bench_run_graphql_validation_as_wamr(void);
BenchReport bench_run_graphql_validation_porf_wamr(void);

// Initialize the WAMR runtime. MUST be called from the main thread before
// any bench_run_*_wamr call. Returns 1 on success, 0 on failure (e.g. if
// WAMR is not linked in for this build target).
uint8_t bench_init_wamr(void);

// WAMR fast-interpreter path. Same workloads, same arguments, same
// expected i32 result as the Pulley path — for direct comparison.
BenchReport bench_run_fib_wamr(int32_t n);
BenchReport bench_run_fib_tail_wamr(int32_t n);
BenchReport bench_run_factorial_wamr(int32_t n);
BenchReport bench_run_sieve_wamr(int32_t n);
BenchReport bench_run_crc32_wamr(void);
BenchReport bench_run_matmul_simd_wamr(void);
BenchReport bench_run_matmul_fma_wamr(void);
BenchReport bench_run_convolution_wamr(void);
BenchReport bench_run_audio_dsp_wamr(void);
BenchReport bench_run_bulk_memory_wamr(void);
BenchReport bench_run_call_indirect_wamr(void);
BenchReport bench_run_xmrsplayer_wamr(void);

// wasm3 (m3 pure C interpreter) path. No SIMD support; matmul_simd,
// matmul_fma, and graphql-validation Porffor will load-fail and the
// row reports ERROR (treat as data: "wasm3's interp can't run this
// shape"). bench_init_wasm3 is a no-op kept symmetrical to bench_init_wamr.
uint8_t bench_init_wasm3(void);

BenchReport bench_run_fib_wasm3(int32_t n);
BenchReport bench_run_fib_tail_wasm3(int32_t n);
BenchReport bench_run_factorial_wasm3(int32_t n);
BenchReport bench_run_sieve_wasm3(int32_t n);
BenchReport bench_run_crc32_wasm3(void);
BenchReport bench_run_matmul_simd_wasm3(void);
BenchReport bench_run_matmul_fma_wasm3(void);
BenchReport bench_run_convolution_wasm3(void);
BenchReport bench_run_audio_dsp_wasm3(void);
BenchReport bench_run_bulk_memory_wasm3(void);
BenchReport bench_run_call_indirect_wasm3(void);
BenchReport bench_run_xmrsplayer_wasm3(void);
BenchReport bench_run_vtable_mono_wasm3(void);
BenchReport bench_run_vtable_bi_wasm3(void);
BenchReport bench_run_vtable_poly4_wasm3(void);
BenchReport bench_run_vtable_poly6_wasm3(void);
BenchReport bench_run_graphql_validation_as_wasm3(void);
BenchReport bench_run_graphql_validation_porf_wasm3(void);

// WasmEdge — pure interpreter (WASMEDGE_USE_LLVM=OFF + 27-patch
// Apple-mobile enablement stack). Incumbent runtime for the user's
// WatchOS audio app; canonical comparison target. Unlike WAMR,
// WasmEdge's interpreter has SIMD + exceptions enabled together, so
// graphql-validation Porffor loads successfully (host-import trap is
// the next blocker, handled the same way Pulley/WAMR handle it).
uint8_t bench_init_wasmedge(void);

BenchReport bench_run_fib_wasmedge(int32_t n);
BenchReport bench_run_fib_tail_wasmedge(int32_t n);
BenchReport bench_run_factorial_wasmedge(int32_t n);
BenchReport bench_run_sieve_wasmedge(int32_t n);
BenchReport bench_run_crc32_wasmedge(void);
BenchReport bench_run_matmul_simd_wasmedge(void);
BenchReport bench_run_matmul_fma_wasmedge(void);
BenchReport bench_run_convolution_wasmedge(void);
BenchReport bench_run_audio_dsp_wasmedge(void);
BenchReport bench_run_bulk_memory_wasmedge(void);
BenchReport bench_run_call_indirect_wasmedge(void);
BenchReport bench_run_xmrsplayer_wasmedge(void);
BenchReport bench_run_vtable_mono_wasmedge(void);
BenchReport bench_run_vtable_bi_wasmedge(void);
BenchReport bench_run_vtable_poly4_wasmedge(void);
BenchReport bench_run_vtable_poly6_wasmedge(void);
BenchReport bench_run_graphql_validation_as_wasmedge(void);
BenchReport bench_run_graphql_validation_porf_wasmedge(void);

// zwasm (clojurewasm/zwasm) — Zig pure-interpreter built with
// `-Djit=false`. arm64_32-apple-watchos is structurally unsupported
// (Zig 0.16 has no arm64_32 target + zwasm assumes 64-bit pointers);
// every workload row on that platform returns ERROR with "zwasm not
// linked into this build."
uint8_t bench_init_zwasm(void);

BenchReport bench_run_fib_zwasm(int32_t n);
BenchReport bench_run_fib_tail_zwasm(int32_t n);
BenchReport bench_run_factorial_zwasm(int32_t n);
BenchReport bench_run_sieve_zwasm(int32_t n);
BenchReport bench_run_crc32_zwasm(void);
BenchReport bench_run_matmul_simd_zwasm(void);
BenchReport bench_run_matmul_fma_zwasm(void);
BenchReport bench_run_convolution_zwasm(void);
BenchReport bench_run_audio_dsp_zwasm(void);
BenchReport bench_run_bulk_memory_zwasm(void);
BenchReport bench_run_call_indirect_zwasm(void);
BenchReport bench_run_xmrsplayer_zwasm(void);
BenchReport bench_run_vtable_mono_zwasm(void);
BenchReport bench_run_vtable_bi_zwasm(void);
BenchReport bench_run_vtable_poly4_zwasm(void);
BenchReport bench_run_vtable_poly6_zwasm(void);
BenchReport bench_run_graphql_validation_as_zwasm(void);
BenchReport bench_run_graphql_validation_porf_zwasm(void);

// wasmz (Ray-D-Song/wasmz) — Zig pure-interpreter, ported to Zig 0.16
// (the upstream sources pin Zig 0.15.2 but Zig 0.15's build runner
// segfaults on macOS 26 Tahoe). Same arm64_32-apple-watchos caveat as
// zwasm — wasmz assumes 64-bit pointers and Zig 0.16 has no arm64_32
// target, so every workload row on that platform returns ERROR with
// "wasmz not linked into this build."
uint8_t bench_init_wasmz(void);

BenchReport bench_run_fib_wasmz(int32_t n);
BenchReport bench_run_fib_tail_wasmz(int32_t n);
BenchReport bench_run_factorial_wasmz(int32_t n);
BenchReport bench_run_sieve_wasmz(int32_t n);
BenchReport bench_run_crc32_wasmz(void);
BenchReport bench_run_matmul_simd_wasmz(void);
BenchReport bench_run_matmul_fma_wasmz(void);
BenchReport bench_run_convolution_wasmz(void);
BenchReport bench_run_audio_dsp_wasmz(void);
BenchReport bench_run_bulk_memory_wasmz(void);
BenchReport bench_run_call_indirect_wasmz(void);
BenchReport bench_run_xmrsplayer_wasmz(void);
BenchReport bench_run_vtable_mono_wasmz(void);
BenchReport bench_run_vtable_bi_wasmz(void);
BenchReport bench_run_vtable_poly4_wasmz(void);
BenchReport bench_run_vtable_poly6_wasmz(void);
BenchReport bench_run_graphql_validation_as_wasmz(void);
BenchReport bench_run_graphql_validation_porf_wasmz(void);

// Free a `BenchReport.error_msg` previously returned by bench_run_*.
// Calling with NULL is a no-op.
void bench_free_error_msg(char *ptr);

// Diagnostics.
size_t bench_fib_wasm_size(void);

// PAC (Pointer Authentication, ARMv8.3-A `PACGA` instruction) viability
// probe. Tests whether `PACGA` is exposed in user mode on the running
// hardware. Prerequisite for any future PAC-signed IC slot scheme on
// aarch64 / arm64_32. See `pac_probe.rs` for protocol.
typedef struct PacProbeResult {
    uint64_t code1;        // pacga(addr, mod)
    uint64_t code2;        // pacga(addr, mod) — should equal code1
    uint64_t code3;        // pacga(addr^1, mod) — should differ from code1
    uint8_t  nonzero;      // 1 if code1 != 0
    uint8_t  deterministic;// 1 if code1 == code2
    uint8_t  input_dep;    // 1 if code1 != code3
    uint8_t  low_zero;     // 1 if (code1 & 0xFFFFFFFF) == 0 (PACGA contract)
    uint8_t  supported;    // 1 if all of the above
} PacProbeResult;

PacProbeResult bench_pac_probe(void);

#ifdef __cplusplus
}
#endif

#endif // BENCHMARK_CORE_H
