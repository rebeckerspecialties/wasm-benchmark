//! Local-iteration CLI: run every workload in benchmark-core, print
//! per-workload (load, run, result) and assert against the host reference.

use std::time::Duration;

use anyhow::Result;
use benchmark_core::{
    audio_dsp_reference, bulk_memory_reference, call_indirect_reference, convolution_reference,
    crc32_reference, factorial_reference, fib_reference, matmul_fma_reference,
    matmul_simd_reference, run_audio_dsp, run_bulk_memory, run_call_indirect, run_convolution,
    run_crc32, run_factorial, run_fib, run_fib_tail, run_matmul_fma, run_matmul_simd, run_sieve,
    sieve_reference, RunReport,
};

struct Case {
    name: &'static str,
    run: fn() -> Result<RunReport>,
    expect: i32,
}

fn main() -> Result<()> {
    let cases: [Case; 11] = [
        Case { name: "fib(30)",          run: || run_fib(30),                expect: fib_reference(30) },
        Case { name: "fib_tail(100000) [return_call]", run: || run_fib_tail(100000), expect: fib_reference(100000) },
        Case { name: "factorial(20)",    run: || run_factorial(20),          expect: factorial_reference(20) },
        Case { name: "sieve(10000)",     run: || run_sieve(10000),           expect: sieve_reference(10000) },
        Case { name: "crc32(64KB)",      run: || run_crc32(0xC0FFEE),        expect: crc32_reference(0xC0FFEE) },
        Case { name: "matmul 64x64 simd128", run: || run_matmul_simd(0xBEEF),   expect: matmul_simd_reference(0xBEEF) },
        Case { name: "matmul 64x64 relaxed-simd FMA", run: || run_matmul_fma(0xBEEF), expect: matmul_fma_reference(0xBEEF) },
        Case { name: "convolution 256x256", run: || run_convolution(0xCAFE), expect: convolution_reference(0xCAFE) },
        Case { name: "audio_dsp",        run: || run_audio_dsp(0x5C7),       expect: audio_dsp_reference(0x5C7) },
        Case { name: "bulk_memory [memory.copy/fill]", run: || run_bulk_memory(0xB0CC), expect: bulk_memory_reference(0xB0CC) },
        Case { name: "call_indirect (200K dispatches)", run: || run_call_indirect(0xC1AA), expect: call_indirect_reference(0xC1AA) },
    ];

    let mut all_ok = true;
    println!(
        "{:<32}  {:>4}  {:>10}  {:>10} {:>10} {:>10}  {:>9} {:>9}  {:>10}  {:>4}",
        "case", "iter", "load", "min", "median", "p99", "cpu_u", "cpu_s", "rss(KB)", "ok"
    );
    println!("{}", "-".repeat(120));
    for case in &cases {
        match (case.run)() {
            Ok(r) => {
                let mark = if r.result == case.expect { "✓" } else { all_ok = false; "✗" };
                let to_ms = |d: Duration| d.as_secs_f64() * 1000.0;
                println!(
                    "{:<32}  {:>4}  {:>9.3}ms  {:>8.3}ms{:>8.3}ms{:>8.3}ms  {:>7.2}ms{:>7.2}ms  {:>10}  {:>4}",
                    case.name,
                    r.iterations,
                    to_ms(r.load_time),
                    to_ms(r.run_min),
                    to_ms(r.run_median),
                    to_ms(r.run_p99),
                    r.cpu_user_ns as f64 / 1_000_000.0,
                    r.cpu_system_ns as f64 / 1_000_000.0,
                    r.rss_peak_bytes / 1024,
                    mark,
                );
                if r.result != case.expect {
                    println!("    expected {} got {}", case.expect, r.result);
                }
            }
            Err(e) => {
                all_ok = false;
                println!("{:<32}  ERROR: {e:#}", case.name);
            }
        }
    }
    println!("{}", "-".repeat(80));
    if !all_ok {
        std::process::exit(1);
    }
    println!("all {} cases ✓", cases.len());
    Ok(())
}
