//! graphql-js validation-shape benchmark runner: an AssemblyScript port
//! that validates a fixed schema + query AST through 5 graphql-js-shape
//! rules. 61 KB, 0 imports, 13 `call_indirect` (most dispatch was lowered
//! to direct calls because AS knows receiver types statically). Exports
//! `validate_once(arg: i32) -> i32`, returning the error count.
//!
//! See `workloads/graphql-validation/ASSEMBLYSCRIPT-NOTES.md` for the
//! background.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use wasmtime::{Engine, Linker, Module, Store};

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
    // Default `memory_reservation` is 4 GiB (the wasm32 address-space cap),
    // which `mmap` rejects on memory-constrained mobile (iPhone XS / Apple
    // Watch SE2). Drop to 64 MiB — comfortably above what the workload
    // actually allocates, and still big enough that growth is rare.
    config.memory_reservation(64 * 1024 * 1024);
    config.memory_reservation_for_growth(0);
    into_anyhow(Engine::new(&config)).context("Engine::new failed")
}
