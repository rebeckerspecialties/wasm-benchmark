//! Runner for tinywasm (`explodingcamera/tinywasm`), a pure-Rust
//! WebAssembly interpreter.
//!
//! tinywasm has no native code generation at all (the crate is
//! `#![forbid(unsafe_code)]` outside its opt-in x86 SIMD intrinsics), so
//! there is no JIT or AOT tier to disable. Its `archive` feature
//! serializes tinywasm's own interpreter bytecode, not machine code; the
//! harness leaves it off anyway. With the `nightly-dispatch` feature of
//! this crate, tinywasm's `nightly-tail-calls` feature switches its
//! dispatch to guaranteed tail calls (`become`), the same mechanism as
//! Pulley's `--cfg=pulley_tail_calls`, so both run their best dispatch
//! mode on the pinned nightly.

use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use tinywasm::{FuncContext, HostFunction, Imports, Module, ModuleInstance, Store};

use crate::RunReport;

fn tw<T>(r: tinywasm::Result<T>) -> Result<T> {
    r.map_err(|e| anyhow!("{e}"))
}

pub fn init() -> Result<()> {
    Ok(())
}

fn parse(wasm_bytes: &[u8]) -> Result<Module> {
    tinywasm::parse_bytes(wasm_bytes).map_err(|e| anyhow!("tinywasm::parse_bytes failed: {e}"))
}

pub fn run_workload_tinywasm_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    let load_start = Instant::now();
    let module = parse(wasm_bytes)?;
    let mut store = Store::default();
    let instance = tw(ModuleInstance::instantiate(&mut store, &module, None))
        .context("tinywasm instantiate failed (missing imports?)")?;
    let func = tw(instance.func::<i32, i32>(&store, fn_name))
        .with_context(|| format!("export `{fn_name}` not found or wrong signature"))?;
    let load_time = load_start.elapsed();
    crate::measure_calls(load_time, iters, || {
        tw(func.call(&mut store, arg)).with_context(|| format!("`{fn_name}({arg})` trapped"))
    })
}

pub fn run_workload_tinywasm(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_tinywasm_iters(wasm_bytes, fn_name, arg, 0)
}

/// `Shape::InstantiateEach` on tinywasm: parsed once, then a fresh
/// `Store` + instance per sample, dropped after the clock stops.
pub fn run_instantiate_each_tinywasm(wasm_bytes: &[u8], fn_name: &str, arg: i32)
    -> Result<RunReport>
{
    let load_start = Instant::now();
    let module = parse(wasm_bytes)?;
    let load_time = load_start.elapsed();
    crate::measure_samples(load_time, || {
        let t = Instant::now();
        let mut store = Store::default();
        let instance = tw(ModuleInstance::instantiate(&mut store, &module, None))
            .context("tinywasm instantiate failed")?;
        let func = tw(instance.func::<i32, i32>(&store, fn_name))
            .with_context(|| format!("export `{fn_name}` not found or wrong signature"))?;
        let r = tw(func.call(&mut store, arg)).with_context(|| format!("`{fn_name}({arg})` trapped"))?;
        let elapsed = t.elapsed();
        drop(store);
        Ok((elapsed, r))
    })
}

/// Porffor graphql-validation: `m() -> (f64, i32)` with a
/// `("", "b"): (f64) -> ()` host print. Like the Pulley runner, each
/// iteration instantiates a fresh store, because Porffor never frees and
/// grows linear memory without bound across calls.
pub fn run_graphql_validation_porf_tinywasm(wasm_bytes: &[u8]) -> Result<RunReport> {
    let load_start = Instant::now();
    let module = parse(wasm_bytes)?;
    let load_time = load_start.elapsed();
    // Timed unit: fresh Store + instantiate + m(); the Store is dropped
    // after the clock stops, as on every runtime.
    crate::measure_samples(load_time, || {
        let t = Instant::now();
        let mut store = Store::default();
        let mut imports = Imports::new();
        imports.define("", "b", HostFunction::from(|_ctx: FuncContext<'_>, _ch: f64| Ok(())));
        let instance = tw(ModuleInstance::instantiate(&mut store, &module, Some(&imports)))
            .context("tinywasm porf instantiate failed")?;
        let m = tw(instance.func::<(), (f64, i32)>(&store, "m")).context("export `m`")?;
        let (_, r) = tw(m.call(&mut store, ())).context("tinywasm m() trapped")?;
        let elapsed = t.elapsed();
        drop(store);
        Ok((elapsed, r))
    })
}
