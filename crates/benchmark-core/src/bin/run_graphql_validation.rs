//! Quick M4 runner for the graphql-validation benchmark (AssemblyScript
//! port) on Pulley; prints timings.
use anyhow::Result;
use benchmark_core::{graphql_validation, GRAPHQL_VALIDATION_AS_WASM};

fn main() -> Result<()> {
    println!("AS wasm: {} bytes", GRAPHQL_VALIDATION_AS_WASM.len());

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

    Ok(())
}
