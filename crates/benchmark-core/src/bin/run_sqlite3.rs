use benchmark_core::{sqlite3, SQLITE3_WASM};
use std::time::Instant;

fn main() {
    println!("sqlite3.wasm size: {} bytes", SQLITE3_WASM.len());
    let t0 = Instant::now();
    let report = sqlite3::run_sqlite3(SQLITE3_WASM).unwrap_or_else(|e| {
        eprintln!("sqlite3 FAILED: {:#}", e);
        std::process::exit(1);
    });
    let total = t0.elapsed();
    println!(
        "sqlite3: load={:.3}ms run={:.3}ms total={:.3}ms cpu(u/s)={:.2}/{:.2}ms rss={}KB faults={}",
        report.load_time.as_secs_f64() * 1000.0,
        report.run_min.as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1000.0,
        report.cpu_user_ns as f64 / 1e6,
        report.cpu_system_ns as f64 / 1e6,
        report.rss_peak_bytes / 1024,
        report.page_faults,
    );
}
