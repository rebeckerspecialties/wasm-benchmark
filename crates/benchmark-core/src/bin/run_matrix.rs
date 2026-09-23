//! Cross-runtime CLI: runs every case in `benchmark_core::cases::CASES`
//! on every runtime the build links, the same way the iOS app's rows do,
//! and prints one table row plus one JSON line per (runtime, case).
//!
//! Environment:
//!   RUNTIMES=pulley,wamr,...   runtime tokens (default: all linked)
//!   WORKLOADS=fib,vtable       case-id substrings (default: all)
//!   BENCH_TARGET_MS=2000       timed-window budget per case (default 200)
//!   MATRIX_JSONL=path          also append the JSON lines to this file
//!   MATRIX_REP=3               rep index recorded in each JSON line
//!   MATRIX_THREAD_PER_CASE=1   run each case on its own thread named
//!                              `case:<id>` (caller's QoS). The PMU pass
//!                              uses it: xctrace's per-thread counter
//!                              table then attributes counts to cases.
//!
//! Ad-hoc mode, for smoke modules and one-off checks:
//!   run_matrix --file x.wasm --func f [--arg N] [--expect V]
//!              [--case NAME] [--instantiate-each]
//! runs just that module (`f: i32 -> i32`, no imports) on RUNTIMES.
//! `--case` sets the recorded case id (default: the file stem);
//! `--instantiate-each` times instantiate + call per sample.
//!
//! Wrap with `taskpolicy -b` to schedule on the E-cluster; every JSON line
//! records `e_share`, the measured fraction of the timed window's CPU
//! time that ran on E-cores (rusage P-core accounting), so residency is
//! checked rather than assumed.

use std::io::Write;

use benchmark_core::cases::{run_case, Case, Shape, CASES, RUNTIMES};
use benchmark_core::{self as bc, RunReport};

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn e_share(r: &RunReport) -> f64 {
    let total = r.cpu_user_ns + r.cpu_system_ns;
    if total == 0 {
        return f64::NAN;
    }
    1.0 - (r.p_cpu_ns as f64 / total as f64).min(1.0)
}

fn list_env(name: &str) -> Option<Vec<String>> {
    std::env::var(name)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.split(',').map(|p| p.trim().to_ascii_lowercase()).collect())
}

/// `--file/--func/--arg/--expect` → a one-case table (bytes leaked: CLI).
fn adhoc_case() -> Option<Vec<Case>> {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| {
        args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
    };
    let file = get("--file")?;
    let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("--file {file}: {e}"));
    let func = get("--func").unwrap_or_else(|| "run".into());
    let arg = get("--arg").map(|v| v.parse().expect("--arg")).unwrap_or(0);
    let expected = get("--expect").map(|v| v.parse().expect("--expect"));
    let id: &'static str = Box::leak(
        get("--case")
            .unwrap_or_else(|| {
                std::path::Path::new(&file).file_stem().unwrap().to_string_lossy().into_owned()
            })
            .into_boxed_str(),
    );
    let shape = if args.iter().any(|a| a == "--instantiate-each") {
        Shape::InstantiateEach
    } else {
        Shape::I32ToI32
    };
    Some(vec![Case {
        id,
        label: id,
        wasm: Box::leak(bytes.into_boxed_slice()),
        func: Box::leak(func.into_boxed_str()),
        arg,
        expected,
        shape,
    }])
}

fn main() {
    let adhoc = adhoc_case();
    let cases: &[Case] = adhoc.as_deref().unwrap_or(CASES);
    let runtimes = list_env("RUNTIMES");
    let workloads = list_env("WORKLOADS");
    let rep: i64 = std::env::var("MATRIX_REP").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let thread_per_case = std::env::var_os("MATRIX_THREAD_PER_CASE").is_some();
    let mut jsonl = std::env::var("MATRIX_JSONL").ok().map(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .unwrap_or_else(|e| panic!("MATRIX_JSONL={p}: {e}"))
    });

    // WAMR's stack-guard setup must happen on the main thread before any
    // module loads; the others are no-ops kept for symmetry.
    let _ = bc::wamr::init();
    let _ = bc::wasm3::init();
    let _ = bc::wasmedge::init();
    let _ = bc::zwasm::init();
    let _ = bc::wasmz::init();

    eprintln!(
        "BENCH_TARGET_MS={} rep={rep}",
        std::env::var("BENCH_TARGET_MS").unwrap_or_else(|_| "200 (default)".into())
    );
    println!(
        "{:<9} {:<14} {:>6} {:>10} {:>11} {:>11} {:>11} {:>10} {:>6} {:>5}",
        "runtime", "case", "iter", "load(ms)", "min(ms)", "median(ms)", "p99(ms)", "cpu_u(ms)",
        "e_shr", "ipc"
    );

    for (rt, token, _prefix) in RUNTIMES {
        if let Some(ref allow) = runtimes {
            if !allow.iter().any(|a| a == token) {
                continue;
            }
        }
        for case in cases {
            if let Some(ref allow) = workloads {
                if !allow.iter().any(|a| case.id.contains(a.as_str())) {
                    continue;
                }
            }
            // Process instructions and cycles over the whole case (load,
            // warmups and timed window), the denominators for the PMU
            // pass's per-thread counts in thread-per-case mode.
            let usage_before = benchmark_core::residency::proc_usage();
            let res = if thread_per_case {
                let (rt, case) = (*rt, *case);
                benchmark_core::run_on_thread(&format!("case:{}", case.id), 8 << 20, move || {
                    run_case(rt, &case)
                })
                .and_then(|r| r)
            } else {
                run_case(*rt, case)
            };
            let case_usage = match (benchmark_core::residency::proc_usage(), usage_before) {
                (Some(a), Some(b)) => a.since(&b),
                _ => Default::default(),
            };
            let line = match &res {
                Ok(r) => {
                    let ipc = if r.cycles > 0 { r.instructions as f64 / r.cycles as f64 } else { f64::NAN };
                    println!(
                        "{:<9} {:<14} {:>6} {:>10.3} {:>11.4} {:>11.4} {:>11.4} {:>10.2} {:>6.3} {:>5.2}",
                        token, case.id, r.iterations,
                        r.load_time.as_secs_f64() * 1e3,
                        r.run_min.as_secs_f64() * 1e3,
                        r.run_median.as_secs_f64() * 1e3,
                        r.run_p99.as_secs_f64() * 1e3,
                        r.cpu_user_ns as f64 / 1e6,
                        e_share(r), ipc,
                    );
                    format!(
                        concat!(
                            "{{\"rep\":{},\"runtime\":{},\"case\":{},\"label\":{},\"ok\":true,",
                            "\"result\":{},\"iterations\":{},\"load_ns\":{},\"min_ns\":{},",
                            "\"median_ns\":{},\"p99_ns\":{},\"cpu_user_ns\":{},\"cpu_system_ns\":{},",
                            "\"p_cpu_ns\":{},\"e_share\":{},\"instructions\":{},\"cycles\":{},",
                            "\"rss_peak_bytes\":{},\"page_faults\":{},",
                            "\"case_instructions\":{},\"case_cycles\":{}}}"
                        ),
                        rep, json_str(token), json_str(case.id), json_str(case.label),
                        r.result, r.iterations, r.load_time.as_nanos(), r.run_min.as_nanos(),
                        r.run_median.as_nanos(), r.run_p99.as_nanos(), r.cpu_user_ns,
                        r.cpu_system_ns, r.p_cpu_ns,
                        if e_share(r).is_finite() { format!("{:.4}", e_share(r)) } else { "null".into() },
                        r.instructions, r.cycles, r.rss_peak_bytes, r.page_faults,
                        case_usage.instructions, case_usage.cycles,
                    )
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    println!("{:<9} {:<14} ERROR: {}", token, case.id, msg.lines().next().unwrap_or(""));
                    format!(
                        "{{\"rep\":{},\"runtime\":{},\"case\":{},\"label\":{},\"ok\":false,\"error\":{}}}",
                        rep, json_str(token), json_str(case.id), json_str(case.label), json_str(&msg)
                    )
                }
            };
            if let Some(f) = jsonl.as_mut() {
                let _ = writeln!(f, "{line}");
            }
            let _ = std::io::stdout().flush();
        }
    }
}
