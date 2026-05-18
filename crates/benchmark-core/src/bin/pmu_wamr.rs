//! Long-running PMU driver — runs the porf-accurate workload through
//! WAMR for ~12 s of pure interpretation time so `xctrace record
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
//! Selects one workload via env var `PMU_WORKLOAD` (default
//! `porf-accurate`; also `porf` and `as` for the matching variants).
//! Iteration count via `PMU_ITERS` (default 600 — enough to fill a
//! 12 s window at porf-accurate's ~17 ms/iter median).

use anyhow::Result;
use benchmark_core::{
    wamr, GRAPHQL_VALIDATION_AS_WASM, GRAPHQL_VALIDATION_PORF_ACCURATE_WASM,
    GRAPHQL_VALIDATION_PORF_WASM,
};
use std::time::Instant;

fn main() -> Result<()> {
    let workload =
        std::env::var("PMU_WORKLOAD").unwrap_or_else(|_| "porf-accurate".to_string());
    let iters: usize = std::env::var("PMU_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);

    wamr::init()?;
    eprintln!(
        "pmu_wamr: workload={} iters={} (PMU_WORKLOAD / PMU_ITERS env)",
        workload, iters
    );

    let wasm: &[u8] = match workload.as_str() {
        "porf-accurate" => GRAPHQL_VALIDATION_PORF_ACCURATE_WASM,
        "porf" => GRAPHQL_VALIDATION_PORF_WASM,
        "as" => GRAPHQL_VALIDATION_AS_WASM,
        other => {
            return Err(anyhow::anyhow!(
                "unknown PMU_WORKLOAD={other}; want one of porf-accurate / porf / as"
            ))
        }
    };

    // Single warmup + the iteration loop. The benchmark module's
    // per-iteration shape already does instance teardown + reload to
    // match Porffor's no-GC semantics, so this loop's wallclock
    // includes that path on every iteration — exactly the same shape
    // the per-workload PMU runner uses on-device.
    let start = Instant::now();
    for _ in 0..iters {
        match workload.as_str() {
            "porf-accurate" | "porf" => {
                wamr::run_graphql_validation_porf_wamr(wasm)?;
            }
            _ => {
                wamr::run_workload_wamr(wasm, "validate_once", 0)?;
            }
        }
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
