//! Long-running PMU driver — runs the graphql-validation (AS) workload
//! through WAMR for ~12 s of pure interpretation time so `xctrace record
//! --template "CPU Counters"` can collect a statistically stable
//! sample of the 4-bucket bottleneck breakdown (Useful / Processing /
//! Delivery / Discarded).
//!
//! "Delivery" is the closest single-counter proxy for L1-I cache +
//! frontend stalls on Apple Silicon; "Discarded" is the proxy for
//! branch-mispredict pipeline flushes (bad-speculation cycles). The
//! 4-bucket model is a coarser view than raw `L1D_CACHE_MISS_LD` /
//! `BRANCH_MISPRED_NONSPEC` counters, but it's what the bundled
//! `CPU Counters` template emits without a custom .tracetemplate —
//! and the bucket-share metric is the one the per-workload PMU
//! script already speaks (`scripts/run_per_workload_pmu.sh` +
//! `scripts/analyze_pmu.py`), so this driver lines up with the
//! existing cross-runtime comparison pipeline.
//!
//! Usage (local macOS):
//!   xcrun xctrace record --template "CPU Counters" \
//!     --output out.trace --time-limit 12s \
//!     --launch -- target/release/pmu_wamr
//!
//! Iteration count via `PMU_ITERS` (default 600): each iteration is one
//! runner invocation (load, auto-sized measurement window, unload).

use anyhow::Result;
use benchmark_core::{wamr, GRAPHQL_VALIDATION_AS_WASM};
use std::time::Instant;

fn main() -> Result<()> {
    let iters: usize = std::env::var("PMU_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);

    wamr::init()?;
    eprintln!("pmu_wamr: workload=graphql-validation (AS) iters={iters} (PMU_ITERS env)");

    let start = Instant::now();
    for _ in 0..iters {
        wamr::run_workload_wamr(GRAPHQL_VALIDATION_AS_WASM, "validate_once", 0)?;
    }
    let elapsed = start.elapsed();
    eprintln!(
        "pmu_wamr: done in {:.3}s ({:.2} ms/iter avg over {} runner invocations)",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1000.0 / iters as f64,
        iters,
    );
    Ok(())
}
