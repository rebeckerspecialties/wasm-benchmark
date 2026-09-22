//! PMU driver for the matmul_fma (relaxed-SIMD) workload. Runs the
//! workload N times under a single runtime so xctrace's `CPU Counters`
//! template can collect a stable bucket-share sample (Useful /
//! Processing / Delivery / Discarded).
//!
//! Usage:
//!   PMU_RUNTIME=wamr   PMU_ITERS=10000 \
//!     xcrun xctrace record --template "CPU Counters" \
//!       --output out-wamr.trace --time-limit 14s \
//!       --launch -- /usr/sbin/taskpolicy -b target/release/pmu_matmul_fma
//!
//!   PMU_RUNTIME=pulley PMU_ITERS=10000 \
//!     xcrun xctrace record … --launch -- … target/release/pmu_matmul_fma
//!
//! The `taskpolicy -b` wrapper pins to E-cores so the bucket numbers
//! correspond to the iPhone XS / SE2 E-core deployment target.
//!
//! Default 10_000 iterations × ~1.2 ms/iter ≈ 12 s of pure interpret
//! time, comfortably inside a 14 s xctrace attach window after the
//! per-process startup overhead. Adjust `PMU_ITERS` if you need a
//! longer or shorter run.

use anyhow::{Context, Result};
use benchmark_core::{wamr, MATMUL_FMA_WASM};
use std::time::Instant;

fn main() -> Result<()> {
    let rt = std::env::var("PMU_RUNTIME").unwrap_or_else(|_| "wamr".to_string());
    let iters: usize = std::env::var("PMU_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000);
    let arg: i32 = std::env::var("PMU_ARG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0xBEEF);

    eprintln!(
        "pmu_matmul_fma: runtime={} iters={} arg={:#x} \
         (PMU_RUNTIME / PMU_ITERS / PMU_ARG env)",
        rt, iters, arg
    );

    match rt.as_str() {
        "wamr" => run_wamr(iters, arg)?,
        "pulley" => run_pulley(iters, arg)?,
        other => return Err(anyhow::anyhow!("unknown PMU_RUNTIME={other}; want wamr|pulley")),
    }

    Ok(())
}

fn run_wamr(iters: usize, arg: i32) -> Result<()> {
    wamr::init()?;
    // Single load+instantiate; the iter loop sits inside `run_workload_
    // wamr_iters` so per-call we only pay (call_wasm + i32 marshal).
    // That's the same shape the per-workload PMU runner uses on-device.
    let start = Instant::now();
    let r = wamr::run_workload_wamr_iters(MATMUL_FMA_WASM, "matmul_fma", arg, iters as u32)
        .context("WAMR matmul_fma trapped")?;
    let dur = start.elapsed();
    eprintln!(
        "pmu_matmul_fma[wamr]: done in {:.3}s ({:.3} ms/iter median over {} iters, p99={:.3} ms, result={:#x})",
        dur.as_secs_f64(),
        r.run_median.as_secs_f64() * 1000.0,
        r.iterations,
        r.run_p99.as_secs_f64() * 1000.0,
        r.result,
    );
    Ok(())
}

fn run_pulley(iters: usize, arg: i32) -> Result<()> {
    let start = Instant::now();
    let r = benchmark_core::run_workload_iters(MATMUL_FMA_WASM, "matmul_fma", arg, iters as u32)
        .context("Pulley matmul_fma trapped")?;
    let dur = start.elapsed();
    eprintln!(
        "pmu_matmul_fma[pulley]: done in {:.3}s ({:.3} ms/iter median over {} iters, p99={:.3} ms, result={:#x})",
        dur.as_secs_f64(),
        r.run_median.as_secs_f64() * 1000.0,
        r.iterations,
        r.run_p99.as_secs_f64() * 1000.0,
        r.result,
    );
    Ok(())
}
