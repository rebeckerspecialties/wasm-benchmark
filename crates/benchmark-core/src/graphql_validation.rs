//! graphql-js validation-shape benchmark runners.
//!
//! Two parallel implementations of the same workload (validate a fixed
//! schema + query AST through 5 graphql-js-shape rules), compiled from
//! different toolchains:
//!
//! - **AssemblyScript (`-as` variant)**: 61 KB, 0 imports, 13
//!   `call_indirect`. Optimizer-friendly — most dispatch was lowered to
//!   direct calls because AS knows receiver types statically.
//!
//! - **Porffor (`-porf` variant)**: 121 KB, 1 import (host print, easily
//!   stubbed), 98 `call_indirect`. Preserves graphql-js's megamorphic-
//!   dispatch shape much more faithfully — the *primary* workload for our
//!   `call_indirect` optimization work.
//!
//! Both expose:
//! - AS: `validate_once(arg: i32) -> i32` returning the error count
//! - Porffor: `m()` (no args, no return), prints `validate: errors=N` to
//!   stdout via the host-print import we stub
//!
//! See `workloads/graphql-validation/{ASSEMBLYSCRIPT,PORFFOR}-NOTES.md` for
//! the full background.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use wasmtime::{Caller, Engine, Linker, Module, Store};

use crate::{into_anyhow, taskinfo, RunReport};

/// Run the AssemblyScript graphql-validation wasm. Calls the
/// `validate_once(0)` export and reports timing. Single-shot.
pub fn run_graphql_validation_as(wasm_bytes: &[u8]) -> Result<RunReport> {
    let load_start = Instant::now();
    let engine = make_engine()?;
    let module = into_anyhow(Module::from_binary(&engine, wasm_bytes))
        .context("graphql-validation-as: Module::from_binary failed")?;

    // AS module has no imports — empty linker is fine.
    let mut store = Store::new(&engine, ());
    let linker: Linker<()> = Linker::new(&engine);
    let instance = into_anyhow(linker.instantiate(&mut store, &module))
        .context("graphql-validation-as: instantiate failed")?;

    let validate_once = into_anyhow(
        instance.get_typed_func::<i32, i32>(&mut store, "validate_once"),
    )
    .context("graphql-validation-as: export `validate_once` not found")?;
    let load_time = load_start.elapsed();

    // Single-shot run. The fixture is fixed-size and AS validation is
    // ~ms-scale on M4 — wrap a small repeat loop to push the measurement
    // window above noise.
    let warm_start = Instant::now();
    let mut result = into_anyhow(validate_once.call(&mut store, 0))
        .context("graphql-validation-as: validate_once(0) trapped")?;
    let warm = warm_start.elapsed();
    let n = crate::pick_iters(warm, Duration::from_millis(200));

    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();

    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        let r = into_anyhow(validate_once.call(&mut store, 0))
            .context("graphql-validation-as: validate_once trapped")?;
        samples.push(it_start.elapsed().as_nanos() as u64);
        result = r;
    }

    Ok(finalize_report(
        result,
        n,
        load_time,
        samples,
        cpu_before,
        events_before,
    ))
}

/// Run the Porffor graphql-validation wasm. Calls the `m()` export
/// (Porffor's main).
///
/// **Fresh Store per iteration.** Porffor has no GC — each call to `m()`
/// allocates fresh `Map`s / `Array`s / closures and never frees them, so
/// running N iterations in one Store grows memory monotonically (M4 went
/// from 1.85 ms iter 1 to 100s of ms past iter ~50). Module compile is
/// done once; each iteration spins up a fresh `Store` (cheap relative to
/// compile) so per-iteration memory pressure is bounded by what one
/// validation needs.
pub fn run_graphql_validation_porf(wasm_bytes: &[u8]) -> Result<RunReport> {
    let load_start = Instant::now();
    let engine = make_engine()?;
    let module = into_anyhow(Module::from_binary(&engine, wasm_bytes))
        .context("graphql-validation-porf: Module::from_binary failed")?;

    // Porffor imports `("", "b")` — a host print function called per
    // character. Porffor uses f64 (JS Number) for the char code, not i32.
    let mut linker: Linker<()> = Linker::new(&engine);
    into_anyhow(linker.func_wrap("", "b", |_caller: Caller<'_, ()>, _ch: f64| {}))
        .context("graphql-validation-porf: link host print")?;
    let load_time = load_start.elapsed();

    // Single full instantiation for warmup timing (sets the iteration
    // count budget).
    let warm_start = Instant::now();
    {
        let mut store = Store::new(&engine, ());
        let instance = into_anyhow(linker.instantiate(&mut store, &module))
            .context("graphql-validation-porf: instantiate (warmup) failed")?;
        let m = into_anyhow(
            instance.get_typed_func::<(), (f64, i32)>(&mut store, "m"),
        )
        .context("graphql-validation-porf: export `m` not found")?;
        let _ = into_anyhow(m.call(&mut store, ()))
            .context("graphql-validation-porf: m() trapped")?;
    }
    let warm = warm_start.elapsed();
    let n = crate::pick_iters(warm, Duration::from_millis(200));

    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();

    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        let mut store = Store::new(&engine, ());
        let instance = into_anyhow(linker.instantiate(&mut store, &module))
            .context("graphql-validation-porf: instantiate failed")?;
        let m = into_anyhow(
            instance.get_typed_func::<(), (f64, i32)>(&mut store, "m"),
        )
        .context("graphql-validation-porf: export `m` not found")?;
        let _ = into_anyhow(m.call(&mut store, ()))
            .context("graphql-validation-porf: m() trapped")?;
        samples.push(it_start.elapsed().as_nanos() as u64);
        // Store dropped here; memory released.
    }

    Ok(finalize_report(
        0,
        n,
        load_time,
        samples,
        cpu_before,
        events_before,
    ))
}

fn make_engine() -> Result<Engine> {
    let pulley_target = if cfg!(target_pointer_width = "64") {
        "pulley64"
    } else {
        "pulley32"
    };
    let mut config = wasmtime::Config::new();
    into_anyhow(config.target(pulley_target).map(|_| ()))
        .with_context(|| format!("Config::target({pulley_target}) failed"))?;
    config.wasm_simd(true);
    config.wasm_relaxed_simd(true);
    config.relaxed_simd_deterministic(false);
    config.wasm_tail_call(true);
    config.wasm_bulk_memory(true);
    // Porffor compiles JS try/catch to the LEGACY (phase-3) wasm-eh
    // proposal — `try` / `catch tag` / `throw tag`. The new (phase-4)
    // `try_table` / `throw_ref` proposal isn't used by Porffor, but
    // enable both so the AS workload (which uses neither) keeps
    // loading regardless of future default changes. The legacy flag
    // is marked deprecated upstream ("internal use with spec
    // testsuite") but it's the only way to make Porffor's output
    // load on wasmtime.
    config.wasm_exceptions(true);
    #[allow(deprecated)]
    config.wasm_legacy_exceptions(true);
    // Default `memory_reservation` is 4 GiB (the wasm32 address-space cap),
    // which `mmap` rejects on memory-constrained mobile (iPhone XS / Apple
    // Watch SE2). Drop to 64 MiB — comfortably above what either workload
    // actually allocates, and still big enough that growth is rare.
    config.memory_reservation(64 * 1024 * 1024);
    config.memory_reservation_for_growth(0);
    into_anyhow(Engine::new(&config)).context("Engine::new failed")
}

fn finalize_report(
    result: i32,
    n: u32,
    load_time: Duration,
    mut samples: Vec<u64>,
    cpu_before: Option<taskinfo::ThreadTimes>,
    events_before: Option<taskinfo::TaskEventsInfo>,
) -> RunReport {
    let cpu_after = taskinfo::thread_times();
    let events_after = taskinfo::events_info();
    let basic = taskinfo::basic_info();

    samples.sort_unstable();
    let run_min = Duration::from_nanos(samples[0]);
    let run_median = Duration::from_nanos(samples[samples.len() / 2]);
    let p99_idx = ((samples.len() as f64) * 0.99) as usize;
    let run_p99 = Duration::from_nanos(samples[p99_idx.min(samples.len() - 1)]);

    #[cfg(target_vendor = "apple")]
    let (cpu_user_ns, cpu_system_ns, page_faults) = {
        let to = |t: taskinfo::TimeValue| taskinfo::time_value_to_ns(t);
        match (cpu_before, cpu_after, events_before, events_after) {
            (Some(b), Some(a), Some(eb), Some(ea)) => (
                to(a.user_time).saturating_sub(to(b.user_time)),
                to(a.system_time).saturating_sub(to(b.system_time)),
                (ea.faults as u64).saturating_sub(eb.faults as u64),
            ),
            _ => (0, 0, 0),
        }
    };
    #[cfg(not(target_vendor = "apple"))]
    let (cpu_user_ns, cpu_system_ns, page_faults) = {
        let _ = (cpu_before, cpu_after, events_before, events_after);
        (0u64, 0u64, 0u64)
    };

    let rss_peak_bytes = basic.map(|b| b.resident_size_max).unwrap_or(0);

    RunReport {
        result,
        iterations: n,
        load_time,
        run_min,
        run_median,
        run_p99,
        cpu_user_ns,
        cpu_system_ns,
        rss_peak_bytes,
        page_faults,
    }
}
