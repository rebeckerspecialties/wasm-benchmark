//! Side-by-side runner: every workload run through wasmtime+Pulley AND
//! through WAMR (fast-interp), printed in one table for direct
//! comparison. Result-correctness is verified against the host Rust
//! reference function and required to match across both runtimes.

use std::time::Duration;

use anyhow::Result;
use benchmark_core::{
    audio_dsp_reference, bulk_memory_reference, call_indirect_reference, convolution_reference,
    crc32_reference, factorial_reference, fib_reference, matmul_fma_reference,
    matmul_simd_reference, run_workload_with, sieve_reference, RunReport, Runtime, AUDIO_DSP_WASM,
    BULK_MEMORY_WASM, CALL_INDIRECT_WASM, CONVOLUTION_WASM, CRC32_WASM, FACTORIAL_WASM, FIB_WASM,
    FIB_TAIL_WASM, MATMUL_FMA_WASM, MATMUL_SIMD_WASM, SIEVE_WASM,
};

struct Case {
    name: &'static str,
    wasm: &'static [u8],
    fn_name: &'static str,
    arg: i32,
    expect: i32,
}

// Spawn on a 64 MiB-stack thread so deep Pulley dispatch on call-heavy
// workloads (e.g. fib(30)'s ~1.6 M wasm calls) doesn't blow the host
// stack on builds where rustc's `become` tail-call elision didn't fire
// at every dispatch site.
fn run(rt: Runtime, c: &Case) -> Result<RunReport> {
    let wasm = c.wasm;
    let fn_name = c.fn_name.to_string();
    let arg = c.arg;
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run_workload_with(rt, wasm, &fn_name, arg))?;
    handle.join().map_err(|_| anyhow::anyhow!("worker thread panicked"))?
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> Result<()> {
    // WAMR's stack-guard setup only succeeds on the main thread; pre-init
    // before spawning the worker thread for each run.
    benchmark_core::wamr::init()?;

    let cases: [Case; 11] = [
        Case { name: "fib(30)",                    wasm: FIB_WASM,           fn_name: "fib",            arg: 30,         expect: fib_reference(30) },
        Case { name: "fib_tail(100000)",           wasm: FIB_TAIL_WASM,      fn_name: "fib_tail",       arg: 100_000,    expect: fib_reference(100_000) },
        Case { name: "factorial(20)",              wasm: FACTORIAL_WASM,     fn_name: "factorial",      arg: 20,         expect: factorial_reference(20) },
        Case { name: "sieve(10000)",               wasm: SIEVE_WASM,         fn_name: "sieve",          arg: 10_000,     expect: sieve_reference(10_000) },
        Case { name: "crc32(64KB)",                wasm: CRC32_WASM,         fn_name: "crc32",          arg: 0xC0FFEE,   expect: crc32_reference(0xC0FFEE) },
        Case { name: "matmul simd128",             wasm: MATMUL_SIMD_WASM,   fn_name: "matmul",         arg: 0xBEEF,     expect: matmul_simd_reference(0xBEEF) },
        Case { name: "matmul relaxed-simd FMA",    wasm: MATMUL_FMA_WASM,    fn_name: "matmul_fma",     arg: 0xBEEF,     expect: matmul_fma_reference(0xBEEF) },
        Case { name: "convolution 256x256",        wasm: CONVOLUTION_WASM,   fn_name: "convolve",       arg: 0xCAFE,     expect: convolution_reference(0xCAFE) },
        Case { name: "audio_dsp",                  wasm: AUDIO_DSP_WASM,     fn_name: "audio_dsp",      arg: 0x5C7,      expect: audio_dsp_reference(0x5C7) },
        Case { name: "bulk_memory",                wasm: BULK_MEMORY_WASM,   fn_name: "bulk_memory",    arg: 0xB0CC,     expect: bulk_memory_reference(0xB0CC) },
        Case { name: "call_indirect",              wasm: CALL_INDIRECT_WASM, fn_name: "call_indirect",  arg: 0xC1AA,     expect: call_indirect_reference(0xC1AA) },
    ];

    println!(
        "{:<28}  {:>4}  {:>11}  {:>11}  {:>11}  {:>4}  {:>11}  {:>11}  {:>11}  {:>4}  {:>10}",
        "case",
        "P_iter", "P_load", "P_med", "P_p99", "P✓",
        "W_iter", "W_load", "W_med", "W_p99", "W✓"
    );
    println!("{}", "-".repeat(140));
    let mut all_ok = true;
    for c in &cases {
        let p = run(Runtime::Pulley, c);
        let w = run(Runtime::Wamr, c);
        let row = |r: &Result<RunReport>, expect: i32, all_ok: &mut bool| -> String {
            match r {
                Ok(r) => {
                    let mark = if r.result == expect { "✓" } else { *all_ok = false; "✗" };
                    format!(
                        "{:>4}  {:>9.3}ms  {:>9.3}ms  {:>9.3}ms  {:>4}",
                        r.iterations, ms(r.load_time), ms(r.run_median), ms(r.run_p99), mark
                    )
                }
                Err(e) => {
                    *all_ok = false;
                    format!("ERR  {}", e)
                }
            }
        };
        let p_row = row(&p, c.expect, &mut all_ok);
        let w_row = row(&w, c.expect, &mut all_ok);
        println!("{:<28}  {}  {}", c.name, p_row, w_row);
    }
    println!("{}", "-".repeat(140));
    if !all_ok {
        std::process::exit(1);
    }
    println!("all {} cases ✓ on both runtimes", cases.len());
    Ok(())
}
