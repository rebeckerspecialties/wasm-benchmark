//! Focused CLI runner for the call_indirect-shaped workloads that the
//! table-mutability-tracking optimization stack targets:
//!
//!   - call_indirect.wasm        (synthetic dispatch microbenchmark)
//!   - graphql-validation-as     (AssemblyScript port; 13 sites, slot-0 null)
//!   - graphql-validation-porf   (Porffor port; 131 sites, megamorphic)
//!   - xmrsplayer.wasm           (real-world Rust soundtracker player;
//!                                15 s of unreal.s3m through 31
//!                                call_indirect sites at 44.1 kHz)
//!   - sqlite3.wasm              (single-shot; exported table → opt is correctly off)
//!
//! Use `BENCH_TARGET_MS=2000` to bump iteration budget 10x for tighter
//! noise floor. Optionally `WORKLOADS=call_indirect,graphql-porf` to
//! restrict to a subset (matches by case-insensitive prefix).
//!
//! Designed to be wrapped by `taskpolicy -b` for E-core scheduling on
//! Apple Silicon, and by `xctrace --template "CPU Counters"` /
//! `xctrace --template "Time Profiler"` for low-level profiling.

use std::time::Duration;

use anyhow::Result;
use benchmark_core::{
    graphql_validation, run_call_indirect, run_vtable_bi, run_vtable_mono, run_vtable_poly4,
    run_vtable_poly6, run_xmrsplayer, sqlite3, GRAPHQL_VALIDATION_AS_WASM,
    GRAPHQL_VALIDATION_PORF_WASM, RunReport, SQLITE3_WASM,
};

struct Case {
    name: &'static str,
    run: fn() -> Result<RunReport>,
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> Result<()> {
    let cases: &[Case] = &[
        Case { name: "call_indirect",            run: || run_call_indirect(0xC1AA) },
        Case { name: "vtable_mono",              run: || run_vtable_mono(0xC1AA) },
        Case { name: "vtable_bi",                run: || run_vtable_bi(0xC1AA) },
        Case { name: "vtable_poly4",             run: || run_vtable_poly4(0xC1AA) },
        Case { name: "vtable_poly6",             run: || run_vtable_poly6(0xC1AA) },
        Case { name: "graphql-validation-as",    run: || graphql_validation::run_graphql_validation_as(GRAPHQL_VALIDATION_AS_WASM) },
        Case { name: "graphql-validation-porf",  run: || graphql_validation::run_graphql_validation_porf(GRAPHQL_VALIDATION_PORF_WASM) },
        Case { name: "xmrsplayer",               run: || run_xmrsplayer(0) },
        Case { name: "sqlite3",                  run: || sqlite3::run_sqlite3(SQLITE3_WASM) },
    ];

    // Optional filter via `WORKLOADS=call_indirect,graphql-porf` etc.
    let filter: Option<Vec<String>> = std::env::var("WORKLOADS")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.split(',').map(|p| p.trim().to_ascii_lowercase()).collect());

    let target_ms = std::env::var("BENCH_TARGET_MS").unwrap_or_else(|_| "200".to_string());
    eprintln!(
        "BENCH_TARGET_MS={} ({} run window per workload, except sqlite3 which is single-shot)",
        target_ms,
        match target_ms.parse::<u64>() {
            Ok(v) => format!("{} ms", v),
            _ => "default".to_string(),
        },
    );

    println!(
        "{:<28}  {:>5}  {:>11}  {:>11} {:>11} {:>11}  {:>10} {:>10}  {:>10}",
        "case", "iter", "load(ms)", "min", "median", "p99", "cpu_u(ms)", "cpu_s(ms)", "rss(KB)"
    );
    println!("{}", "-".repeat(118));

    for c in cases {
        if let Some(ref allowlist) = filter {
            let lc = c.name.to_ascii_lowercase();
            if !allowlist.iter().any(|f| lc.contains(f)) {
                continue;
            }
        }
        match (c.run)() {
            Ok(r) => println!(
                "{:<28}  {:>5}  {:>11.3}  {:>11.3} {:>11.3} {:>11.3}  {:>10.2} {:>10.2}  {:>10}",
                c.name,
                r.iterations,
                ms(r.load_time),
                ms(r.run_min),
                ms(r.run_median),
                ms(r.run_p99),
                r.cpu_user_ns as f64 / 1e6,
                r.cpu_system_ns as f64 / 1e6,
                r.rss_peak_bytes / 1024,
            ),
            Err(e) => println!("{:<28}  ERROR: {:#}", c.name, e),
        }
    }
    Ok(())
}
