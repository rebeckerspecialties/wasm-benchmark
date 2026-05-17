//! Quick local runner for graphql-validation (Porffor + AS) against the
//! WAMR fast-interpreter, with our new throw-only legacy-EH support.
//! Used to validate the fast-interp throw lowering end-to-end without
//! having to bounce through the watchOS / iOS bundle.

use anyhow::Result;
use benchmark_core::{
    wamr, GRAPHQL_VALIDATION_AS_WASM, GRAPHQL_VALIDATION_PORF_WASM,
};

fn main() -> Result<()> {
    wamr::init()?;
    println!("wamr init: ok");

    println!("\n=== AS variant on WAMR ===");
    match wamr::run_workload_wamr(GRAPHQL_VALIDATION_AS_WASM, "validate_once", 0) {
        Ok(r) => println!(
            "result={} iter={} load={:.3}ms median={:.3}ms",
            r.result,
            r.iterations,
            r.load_time.as_secs_f64() * 1000.0,
            r.run_median.as_secs_f64() * 1000.0,
        ),
        Err(e) => println!("ERROR: {e:#}"),
    }

    println!("\n=== Porffor variant on WAMR (was 'invalid section id' before this PR) ===");
    match wamr::run_graphql_validation_porf_wamr(GRAPHQL_VALIDATION_PORF_WASM) {
        Ok(r) => println!(
            "result={} iter={} load={:.3}ms median={:.3}ms",
            r.result,
            r.iterations,
            r.load_time.as_secs_f64() * 1000.0,
            r.run_median.as_secs_f64() * 1000.0,
        ),
        Err(e) => println!("ERROR: {e:#}"),
    }
    Ok(())
}
