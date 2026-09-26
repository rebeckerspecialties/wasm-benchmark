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

// --- femtovg E2E guest binding -------------------------------------------

#[cfg(feature = "femtovg-e2e")]
mod femtovg_binding {
    use super::*;
    use crate::femtovg_e2e as e2e;
    use tinywasm::FunctionTyped;

    /// Copies `[ptr, ptr + len)` out of the caller's memory (tinywasm's
    /// memory API reads into a buffer; it has no borrowed view).
    fn read(ctx: &FuncContext<'_>, ptr: i32, len: i32) -> tinywasm::Result<Vec<u8>> {
        let mem = ctx.memory("memory")?;
        mem.read_vec(ctx.store(), ptr as u32 as usize, len.max(0) as usize)
    }

    pub(crate) struct TinywasmGuest {
        store: Store,
        init: FunctionTyped<(i32, i32, i32), i32>,
        frame: FunctionTyped<(i32, i32), i32>,
        pages: FunctionTyped<(), i32>,
    }

    impl e2e::Guest for TinywasmGuest {
        fn init(&mut self, scene: i32, width: i32, height: i32) -> Result<i32> {
            tw(self.init.call(&mut self.store, (scene, width, height)))
        }
        fn frame(&mut self, index: i32, count: i32) -> Result<i32> {
            tw(self.frame.call(&mut self.store, (index, count)))
        }
        fn mem_pages(&mut self) -> Result<i32> {
            tw(self.pages.call(&mut self.store, ()))
        }
    }

    pub(crate) fn guest(wasm: &[u8]) -> Result<Box<dyn e2e::Guest>> {
        let module = parse(wasm)?;
        let mut imports = Imports::new();
        imports.define(
            "fvg",
            "set_size",
            HostFunction::from(|_c: FuncContext<'_>, (w, h, d): (i32, i32, i32)| {
                e2e::host_set_size(w, h, d);
                Ok(())
            }),
        );
        imports.define(
            "fvg",
            "image_alloc",
            HostFunction::from(|_c: FuncContext<'_>, (w, h, f, fl): (i32, i32, i32, i32)| {
                Ok(e2e::host_image_alloc(w, h, f, fl))
            }),
        );
        imports.define(
            "fvg",
            "image_update",
            HostFunction::from(
                |c: FuncContext<'_>, (hd, x, y, w, h, f, p, n): (i32, i32, i32, i32, i32, i32, i32, i32)| {
                    let data = read(&c, p, n)?;
                    Ok(e2e::host_image_update(hd, x, y, w, h, f, &data))
                },
            ),
        );
        imports.define(
            "fvg",
            "image_delete",
            HostFunction::from(|_c: FuncContext<'_>, hd: i32| {
                e2e::host_image_delete(hd);
                Ok(())
            }),
        );
        imports.define(
            "fvg",
            "render",
            HostFunction::from(|c: FuncContext<'_>, (vp, vn, cp, cn): (i32, i32, i32, i32)| {
                let verts = read(&c, vp, vn.saturating_mul(16))?;
                let cmds = read(&c, cp, cn.saturating_mul(4))?;
                e2e::host_render(&verts, &cmds);
                Ok(())
            }),
        );
        let mut store = Store::default();
        let instance = tw(ModuleInstance::instantiate(&mut store, &module, Some(&imports)))
            .context("tinywasm instantiate")?;
        let init = tw(instance.func::<(i32, i32, i32), i32>(&store, "fvg_init"))?;
        let frame = tw(instance.func::<(i32, i32), i32>(&store, "fvg_frame"))?;
        let pages = tw(instance.func::<(), i32>(&store, "fvg_mem_pages"))?;
        Ok(Box::new(TinywasmGuest { store, init, frame, pages }))
    }
}

#[cfg(feature = "femtovg-e2e")]
pub(crate) fn femtovg_guest(wasm: &'static [u8]) -> Result<Box<dyn crate::femtovg_e2e::Guest>> {
    femtovg_binding::guest(wasm)
}
