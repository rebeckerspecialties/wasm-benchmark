//! Cross-runtime wallclock benchmark for the relaxed-SIMD workload
//! (`matmul_fma.wasm`, which uses `f32x4_relaxed_madd`) and its
//! non-relaxed counterpart (`matmul_simd.wasm`, which uses
//! `f32x4_add(f32x4_mul(a,b), acc)`). Compares Pulley, WAMR
//! fast-interp (with `WAMR_BUILD_RELAXED_SIMD=1`), wasm3,
//! WasmEdge interp, zwasm, and wasmz to demonstrate the impact of
//! native relaxed-SIMD dispatch in the interpreter loop.
//!
//! Runtimes that don't recognize the `f32x4.relaxed_madd` opcode
//! (`0xfd 105`) will produce an `Err`/`ERR` row — that's the
//! cross-runtime signal we want; this binary doesn't try to mask it.

use std::time::Duration;

use anyhow::Result;
use benchmark_core::{
    matmul_fma_reference, matmul_simd_reference, run_workload_with, RunReport, Runtime,
    MATMUL_FMA_WASM, MATMUL_SIMD_WASM,
};

struct Case {
    name: &'static str,
    wasm: &'static [u8],
    fn_name: &'static str,
    arg: i32,
    expect: i32,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn run(rt: Runtime, c: &Case) -> Option<RunReport> {
    let wasm = c.wasm;
    let fn_name = c.fn_name.to_string();
    let arg = c.arg;
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run_workload_with(rt, wasm, &fn_name, arg))
        .ok()?;
    handle.join().ok()?.ok()
}

const RUNTIMES: &[(&str, Runtime)] = &[
    ("Pulley", Runtime::Pulley),
    ("WAMR", Runtime::Wamr),
    ("wasm3", Runtime::Wasm3),
    ("WasmEdge", Runtime::WasmEdge),
    ("zwasm", Runtime::Zwasm),
    ("wasmz", Runtime::Wasmz),
];

fn main() -> Result<()> {
    benchmark_core::wamr::init()?;

    let cases: [Case; 2] = [
        Case {
            name: "matmul simd128 (mul+add)",
            wasm: MATMUL_SIMD_WASM,
            fn_name: "matmul",
            arg: 0xBEEF,
            expect: matmul_simd_reference(0xBEEF),
        },
        Case {
            name: "matmul relaxed-simd (FMA)",
            wasm: MATMUL_FMA_WASM,
            fn_name: "matmul_fma",
            arg: 0xBEEF,
            expect: matmul_fma_reference(0xBEEF),
        },
    ];

    println!(
        "{:<28} {:<10} {:>10} {:>10} {:>10} {:>8}",
        "case", "runtime", "iters", "median_ms", "p99_ms", "correct"
    );
    println!("{}", "-".repeat(82));

    for c in &cases {
        for &(label, rt) in RUNTIMES {
            match run(rt, c) {
                Some(r) => {
                    let ok = r.result == c.expect;
                    println!(
                        "{:<28} {:<10} {:>10} {:>10.3} {:>10.3} {:>8}",
                        c.name,
                        label,
                        r.iterations,
                        ms(r.run_median),
                        ms(r.run_p99),
                        if ok { "✓" } else { "✗" }
                    );
                }
                None => {
                    println!(
                        "{:<28} {:<10} {:>10} {:>10} {:>10} {:>8}",
                        c.name, label, "—", "ERR", "ERR", "—"
                    );
                }
            }
        }
        println!();
    }

    Ok(())
}
