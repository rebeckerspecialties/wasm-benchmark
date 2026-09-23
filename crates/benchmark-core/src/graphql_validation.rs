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

use crate::{into_anyhow, RunReport};

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

    let window = crate::Window::start();

    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        let r = into_anyhow(validate_once.call(&mut store, 0))
            .context("graphql-validation-as: validate_once trapped")?;
        samples.push(it_start.elapsed().as_nanos() as u64);
        result = r;
    }

    Ok(window.finish(result, n, load_time, samples))
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

    // Timed unit: fresh Store + instantiate + m(); the Store is dropped
    // (memory released) after the clock stops, as on every runtime.
    crate::measure_samples(load_time, || {
        let t = Instant::now();
        let mut store = Store::new(&engine, ());
        let instance = into_anyhow(linker.instantiate(&mut store, &module))
            .context("graphql-validation-porf: instantiate failed")?;
        let m = into_anyhow(instance.get_typed_func::<(), (f64, i32)>(&mut store, "m"))
            .context("graphql-validation-porf: export `m` not found")?;
        let (_, r) = into_anyhow(m.call(&mut store, ()))
            .context("graphql-validation-porf: m() trapped")?;
        let elapsed = t.elapsed();
        drop(store);
        Ok((elapsed, r))
    })
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
