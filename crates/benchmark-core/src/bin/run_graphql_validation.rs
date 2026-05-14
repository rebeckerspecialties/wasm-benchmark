//! Quick M4 runner for the graphql-validation benchmarks. Exercises both
//! the AS and Porffor variants and prints timings.
use anyhow::Result;
use benchmark_core::{
    graphql_validation, GRAPHQL_VALIDATION_AS_WASM, GRAPHQL_VALIDATION_PORF_WASM,
};

fn main() -> Result<()> {
    println!(
        "AS  wasm: {} bytes, Porffor wasm: {} bytes",
        GRAPHQL_VALIDATION_AS_WASM.len(),
        GRAPHQL_VALIDATION_PORF_WASM.len()
    );

    println!("\n=== AS variant ===");
    let as_report = graphql_validation::run_graphql_validation_as(GRAPHQL_VALIDATION_AS_WASM)?;
    println!(
        "result={} iter={}  load={:.3}ms  min={:.3} median={:.3} p99={:.3} ms  cpu(u/s)={:.2}/{:.2} ms  rss={}KB faults={}",
        as_report.result,
        as_report.iterations,
        as_report.load_time.as_secs_f64() * 1000.0,
        as_report.run_min.as_secs_f64() * 1000.0,
        as_report.run_median.as_secs_f64() * 1000.0,
        as_report.run_p99.as_secs_f64() * 1000.0,
        as_report.cpu_user_ns as f64 / 1e6,
        as_report.cpu_system_ns as f64 / 1e6,
        as_report.rss_peak_bytes / 1024,
        as_report.page_faults,
    );

    println!("\n=== Porffor variant ===");
    let porf_report = graphql_validation::run_graphql_validation_porf(GRAPHQL_VALIDATION_PORF_WASM)?;
    println!(
        "result={} iter={}  load={:.3}ms  min={:.3} median={:.3} p99={:.3} ms  cpu(u/s)={:.2}/{:.2} ms  rss={}KB faults={}",
        porf_report.result,
        porf_report.iterations,
        porf_report.load_time.as_secs_f64() * 1000.0,
        porf_report.run_min.as_secs_f64() * 1000.0,
        porf_report.run_median.as_secs_f64() * 1000.0,
        porf_report.run_p99.as_secs_f64() * 1000.0,
        porf_report.cpu_user_ns as f64 / 1e6,
        porf_report.cpu_system_ns as f64 / 1e6,
        porf_report.rss_peak_bytes / 1024,
        porf_report.page_faults,
    );
    Ok(())
}
