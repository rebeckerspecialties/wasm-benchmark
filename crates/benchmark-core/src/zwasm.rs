//! Hand-written wasm-c-api bindings + thin runner for zwasm v2
//! (`clojurewasm/zwasm`), a Zig WebAssembly runtime.
//!
//! zwasm v2 replaced its bespoke v1 C API (`zwasm_config_*`,
//! `zwasm_module_invoke`) with the standard wasm-c-api (`wasm.h`) plus a
//! few `zwasm_*` extensions (`zwasm.h`). It also ships three engines
//! (interpreter, JIT, AOT); `scripts/build-zwasm.sh` builds with
//! `-Dengine=interp`, and `patches/zwasm/0001` makes that flag actually
//! compile the JIT out of `libzwasm.a` (upstream v2.7.0 only uses it for
//! the CLI's version string). On top of that, every instance here is
//! created with `zwasm_instance_new_ex(.., ZWASM_ENGINE_INTERP)` and the
//! resolved engine is checked, so a JIT-backed instance is an error, not
//! a silent fast path.
//!
//! arm64_32-apple-watchos is supported upstream since zwasm#98
//! (Zig 0.16 spells the triple `aarch64-watchos-ilp32`).
//!
//! Note: zwasm's interpreter does not execute SIMD-128 (only its JIT
//! does), so the SIMD workloads fail at load on this runtime. That is
//! data, not a harness bug.

use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};

use crate::RunReport;

#[allow(non_camel_case_types)]
mod ffi {
    use std::os::raw::c_void;

    pub type wasm_engine_t = c_void;
    pub type wasm_store_t = c_void;
    pub type wasm_module_t = c_void;
    pub type wasm_instance_t = c_void;
    pub type wasm_extern_t = c_void;
    pub type wasm_func_t = c_void;
    pub type wasm_trap_t = c_void;
    pub type wasm_exporttype_t = c_void;
    pub type wasm_functype_t = c_void;
    pub type wasm_valtype_t = c_void;
    pub type wasm_memory_t = c_void;

    pub const WASM_I32: u8 = 0;
    pub const WASM_F64: u8 = 3;
    pub const ZWASM_ENGINE_INTERP: u8 = 2;

    #[repr(C)]
    pub struct wasm_byte_vec_t {
        pub size: usize,
        pub data: *mut u8,
    }

    /// `wasm_val_t`: a `uint8_t` kind followed by an 8-byte-aligned union,
    /// so the payload sits at offset 8 on both LP64 and ILP32.
    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct wasm_val_t {
        pub kind: u8,
        pub of: u64,
    }

    #[repr(C)]
    pub struct wasm_val_vec_t {
        pub size: usize,
        pub data: *mut wasm_val_t,
    }

    #[repr(C)]
    pub struct wasm_extern_vec_t {
        pub size: usize,
        pub data: *mut *mut wasm_extern_t,
    }

    #[repr(C)]
    pub struct wasm_exporttype_vec_t {
        pub size: usize,
        pub data: *mut *mut wasm_exporttype_t,
    }

    #[repr(C)]
    pub struct wasm_valtype_vec_t {
        pub size: usize,
        pub data: *mut *mut wasm_valtype_t,
    }

    pub type wasm_func_callback_t = unsafe extern "C" fn(
        args: *const wasm_val_vec_t,
        results: *mut wasm_val_vec_t,
    ) -> *mut wasm_trap_t;

    extern "C" {
        pub fn wasm_engine_new() -> *mut wasm_engine_t;
        pub fn wasm_engine_delete(e: *mut wasm_engine_t);
        pub fn wasm_store_new(e: *mut wasm_engine_t) -> *mut wasm_store_t;
        pub fn wasm_store_delete(s: *mut wasm_store_t);

        pub fn wasm_byte_vec_new(out: *mut wasm_byte_vec_t, size: usize, data: *const u8);
        pub fn wasm_byte_vec_delete(v: *mut wasm_byte_vec_t);

        pub fn wasm_module_new(s: *mut wasm_store_t, binary: *const wasm_byte_vec_t)
            -> *mut wasm_module_t;
        pub fn wasm_module_delete(m: *mut wasm_module_t);
        pub fn wasm_module_exports(m: *const wasm_module_t, out: *mut wasm_exporttype_vec_t);
        pub fn wasm_exporttype_vec_delete(v: *mut wasm_exporttype_vec_t);
        pub fn wasm_exporttype_name(e: *const wasm_exporttype_t) -> *const wasm_byte_vec_t;

        pub fn wasm_instance_exports(i: *const wasm_instance_t, out: *mut wasm_extern_vec_t);
        pub fn wasm_instance_delete(i: *mut wasm_instance_t);
        pub fn wasm_extern_vec_delete(v: *mut wasm_extern_vec_t);
        pub fn wasm_extern_vec_new(out: *mut wasm_extern_vec_t, size: usize,
                                   data: *const *mut wasm_extern_t);
        pub fn wasm_extern_as_func(e: *mut wasm_extern_t) -> *mut wasm_func_t;
        pub fn wasm_extern_as_memory(e: *mut wasm_extern_t) -> *mut wasm_memory_t;
        pub fn wasm_memory_data(m: *mut wasm_memory_t) -> *mut u8;
        pub fn wasm_memory_data_size(m: *const wasm_memory_t) -> usize;

        pub fn wasm_func_call(f: *const wasm_func_t, args: *const wasm_val_vec_t,
                              results: *mut wasm_val_vec_t) -> *mut wasm_trap_t;
        pub fn wasm_func_new(s: *mut wasm_store_t, ty: *const wasm_functype_t,
                             cb: wasm_func_callback_t) -> *mut wasm_func_t;
        pub fn wasm_func_as_extern(f: *mut wasm_func_t) -> *mut wasm_extern_t;

        pub fn wasm_valtype_new(kind: u8) -> *mut wasm_valtype_t;
        pub fn wasm_valtype_vec_new(out: *mut wasm_valtype_vec_t, size: usize,
                                    data: *const *mut wasm_valtype_t);
        pub fn wasm_valtype_vec_new_empty(out: *mut wasm_valtype_vec_t);
        pub fn wasm_functype_new(params: *mut wasm_valtype_vec_t,
                                 results: *mut wasm_valtype_vec_t) -> *mut wasm_functype_t;
        pub fn wasm_functype_delete(t: *mut wasm_functype_t);

        pub fn wasm_trap_message(t: *const wasm_trap_t, out: *mut wasm_byte_vec_t);
        pub fn wasm_trap_delete(t: *mut wasm_trap_t);

        pub fn zwasm_instance_new_ex(s: *mut wasm_store_t, m: *const wasm_module_t,
                                     imports: *const wasm_extern_vec_t,
                                     trap_out: *mut *mut wasm_trap_t,
                                     engine_kind: u8) -> *mut wasm_instance_t;
        pub fn zwasm_instance_engine(i: *const wasm_instance_t, out: *mut i32) -> bool;
    }
}

use ffi::*;

// Zig 0.16's `std.debug.SelfInfo.MachO` references
// `_dyld_get_image_header_containing_address` for panic-time
// stack-walks. The symbol exists in iOS dyld but isn't in any public
// TBD, so the iOS link fails with "Undefined symbols". Provide a
// safe stub returning NULL — used only on a panic path we don't
// expect to hit, and matching dyld's documented "address not in any
// loaded image" return value.
#[cfg(all(target_vendor = "apple", any(target_os = "ios", target_os = "tvos", target_os = "watchos", target_os = "visionos")))]
mod ios_dyld_stub {
    use std::os::raw::c_void;
    #[unsafe(no_mangle)]
    pub extern "C" fn _dyld_get_image_header_containing_address(
        _addr: *const c_void,
    ) -> *const c_void {
        std::ptr::null()
    }
}

pub fn init() -> Result<()> {
    Ok(())
}

fn trap_text(trap: *mut wasm_trap_t) -> String {
    if trap.is_null() {
        return "(null trap)".to_string();
    }
    let mut msg = wasm_byte_vec_t { size: 0, data: std::ptr::null_mut() };
    unsafe { wasm_trap_message(trap, &mut msg) };
    let text = if msg.data.is_null() || msg.size == 0 {
        "(no trap message)".to_string()
    } else {
        let bytes = unsafe { std::slice::from_raw_parts(msg.data, msg.size) };
        let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
        String::from_utf8_lossy(bytes).into_owned()
    };
    unsafe {
        wasm_byte_vec_delete(&mut msg);
        wasm_trap_delete(trap);
    }
    text
}

/// One engine + store + module + interp-forced instance, torn down in
/// reverse order on drop.
pub(crate) struct Loaded {
    engine: *mut wasm_engine_t,
    store: *mut wasm_store_t,
    module: *mut wasm_module_t,
    instance: *mut wasm_instance_t,
    exports: wasm_extern_vec_t,
    export_names: Vec<String>,
    // Host funcs are owned by the store; their externs are reused by
    // every (re)instantiation.
    host_externs: Vec<*mut wasm_extern_t>,
}

impl Drop for Loaded {
    fn drop(&mut self) {
        unsafe {
            wasm_extern_vec_delete(&mut self.exports);
            if !self.instance.is_null() {
                wasm_instance_delete(self.instance);
            }
            if !self.module.is_null() {
                wasm_module_delete(self.module);
            }
            if !self.store.is_null() {
                wasm_store_delete(self.store);
            }
            if !self.engine.is_null() {
                wasm_engine_delete(self.engine);
            }
        }
    }
}

/// Signature of a host import to define: (params, results, callback).
pub(crate) struct HostImport {
    pub params: &'static [u8],
    pub results: &'static [u8],
    pub callback: wasm_func_callback_t,
}

fn functype(params: &[u8], results: &[u8]) -> *mut wasm_functype_t {
    unsafe {
        let mk = |kinds: &[u8]| -> wasm_valtype_vec_t {
            let mut v = wasm_valtype_vec_t { size: 0, data: std::ptr::null_mut() };
            if kinds.is_empty() {
                wasm_valtype_vec_new_empty(&mut v);
            } else {
                let tys: Vec<*mut wasm_valtype_t> =
                    kinds.iter().map(|k| wasm_valtype_new(*k)).collect();
                wasm_valtype_vec_new(&mut v, tys.len(), tys.as_ptr());
            }
            v
        };
        let mut p = mk(params);
        let mut r = mk(results);
        wasm_functype_new(&mut p, &mut r)
    }
}

impl Loaded {
    /// `imports` are positional, in the module's import order.
    pub(crate) fn new(wasm_bytes: &[u8], imports: &[HostImport]) -> Result<Self> {
        unsafe {
            let mut l = Loaded {
                engine: wasm_engine_new(),
                store: std::ptr::null_mut(),
                module: std::ptr::null_mut(),
                instance: std::ptr::null_mut(),
                exports: wasm_extern_vec_t { size: 0, data: std::ptr::null_mut() },
                export_names: Vec::new(),
                host_externs: Vec::new(),
            };
            if l.engine.is_null() {
                bail!("wasm_engine_new returned NULL");
            }
            l.store = wasm_store_new(l.engine);
            if l.store.is_null() {
                bail!("wasm_store_new returned NULL");
            }
            let mut bin = wasm_byte_vec_t { size: 0, data: std::ptr::null_mut() };
            wasm_byte_vec_new(&mut bin, wasm_bytes.len(), wasm_bytes.as_ptr());
            l.module = wasm_module_new(l.store, &bin);
            wasm_byte_vec_delete(&mut bin);
            if l.module.is_null() {
                bail!("wasm_module_new failed (invalid wasm or unsupported feature)");
            }
            for imp in imports {
                let ty = functype(imp.params, imp.results);
                let f = wasm_func_new(l.store, ty, imp.callback);
                wasm_functype_delete(ty);
                if f.is_null() {
                    bail!("wasm_func_new returned NULL");
                }
                l.host_externs.push(wasm_func_as_extern(f));
            }
            let mut etypes = wasm_exporttype_vec_t { size: 0, data: std::ptr::null_mut() };
            wasm_module_exports(l.module, &mut etypes);
            for i in 0..etypes.size {
                let name = &*wasm_exporttype_name(*etypes.data.add(i));
                let bytes = std::slice::from_raw_parts(name.data, name.size);
                l.export_names.push(String::from_utf8_lossy(bytes).into_owned());
            }
            wasm_exporttype_vec_delete(&mut etypes);
            l.instantiate()?;
            Ok(l)
        }
    }

    /// Delete the current instance, if any.
    pub(crate) fn release(&mut self) {
        unsafe {
            if !self.instance.is_null() {
                wasm_extern_vec_delete(&mut self.exports);
                self.exports = wasm_extern_vec_t { size: 0, data: std::ptr::null_mut() };
                wasm_instance_delete(self.instance);
                self.instance = std::ptr::null_mut();
            }
        }
    }

    /// Replace the current instance (if any) with a fresh, interp-forced
    /// one of the same module, reusing the store's host funcs.
    pub(crate) fn instantiate(&mut self) -> Result<()> {
        self.release();
        unsafe {
            // The vec only borrows the externs (the store owns the funcs).
            let import_vec = wasm_extern_vec_t {
                size: self.host_externs.len(),
                data: self.host_externs.as_ptr() as *mut *mut wasm_extern_t,
            };
            let mut trap: *mut wasm_trap_t = std::ptr::null_mut();
            self.instance = zwasm_instance_new_ex(self.store, self.module, &import_vec,
                                                  &mut trap, ZWASM_ENGINE_INTERP);
            if self.instance.is_null() {
                if !trap.is_null() {
                    bail!("zwasm_instance_new_ex trapped: {}", trap_text(trap));
                }
                bail!("zwasm_instance_new_ex failed (missing imports or unsupported feature)");
            }
            let mut engine_kind: i32 = -1;
            if !zwasm_instance_engine(self.instance, &mut engine_kind)
                || engine_kind != ZWASM_ENGINE_INTERP as i32
            {
                bail!("zwasm instance is not interpreter-backed (engine kind {engine_kind})");
            }
            wasm_instance_exports(self.instance, &mut self.exports);
            if self.exports.size != self.export_names.len() {
                bail!("zwasm export count mismatch ({} externs vs {} export types)",
                      self.exports.size, self.export_names.len());
            }
        }
        Ok(())
    }

    fn export(&self, name: &str) -> Result<*mut wasm_extern_t> {
        let idx = self
            .export_names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| anyhow!("export `{name}` not found"))?;
        Ok(unsafe { *self.exports.data.add(idx) })
    }

    pub(crate) fn func(&self, name: &str) -> Result<*mut wasm_func_t> {
        let f = unsafe { wasm_extern_as_func(self.export(name)?) };
        if f.is_null() {
            bail!("export `{name}` is not a function");
        }
        Ok(f)
    }

    /// Exported memory `name` itself (its data pointer can move on grow).
    #[allow(dead_code)]
    pub(crate) fn memory_handle(&self, name: &str) -> Result<*mut wasm_memory_t> {
        let m = unsafe { wasm_extern_as_memory(self.export(name)?) };
        if m.is_null() {
            bail!("export `{name}` is not a memory");
        }
        Ok(m)
    }

    /// Base pointer + byte length of exported memory `name`.
    #[allow(dead_code)]
    pub(crate) fn memory(&self, name: &str) -> Result<(*mut u8, usize)> {
        let m = unsafe { wasm_extern_as_memory(self.export(name)?) };
        if m.is_null() {
            bail!("export `{name}` is not a memory");
        }
        Ok(unsafe { (wasm_memory_data(m), wasm_memory_data_size(m)) })
    }
}

/// Call `f` with `args`, returning `n_results` raw values.
pub(crate) fn call_raw(f: *mut wasm_func_t, args: &[wasm_val_t], n_results: usize)
    -> Result<Vec<wasm_val_t>>
{
    let mut out = vec![wasm_val_t { kind: WASM_I32, of: 0 }; n_results];
    let a = wasm_val_vec_t { size: args.len(), data: args.as_ptr() as *mut wasm_val_t };
    let mut r = wasm_val_vec_t { size: out.len(), data: out.as_mut_ptr() };
    let trap = unsafe { wasm_func_call(f, &a, &mut r) };
    if !trap.is_null() {
        bail!("trap: {}", trap_text(trap));
    }
    Ok(out)
}

fn i32_val(v: i32) -> wasm_val_t {
    wasm_val_t { kind: WASM_I32, of: v as u32 as u64 }
}

/// Run `body` on a dedicated 8 MiB-stack thread. zwasm's load path has
/// overflowed the 272 KiB Swift dispatch-worker stack before; iOS/tvOS
/// pthread defaults are similarly tight.
fn on_big_stack<T: Send + 'static>(name: &str, body: impl FnOnce() -> Result<T> + Send + 'static)
    -> Result<T>
{
    crate::run_on_thread(name, 8 * 1024 * 1024, body)?
}

pub fn run_workload_zwasm_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    let bytes = wasm_bytes.to_vec();
    let name = fn_name.to_string();
    on_big_stack("zwasm-runner", move || {
        let load_start = Instant::now();
        let l = Loaded::new(&bytes, &[])?;
        let f = l.func(&name)?;
        let load_time = load_start.elapsed();
        crate::measure_calls(load_time, iters, || {
            let r = call_raw(f, &[i32_val(arg)], 1)
                .with_context(|| format!("`{name}({arg})` trapped"))?;
            Ok(r[0].of as u32 as i32)
        })
    })
}

pub fn run_workload_zwasm(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_zwasm_iters(wasm_bytes, fn_name, arg, 0)
}

/// `Shape::InstantiateEach` on zwasm: the previous instance is released
/// before the clock starts, then a fresh interp-forced instance is created
/// and called.
pub fn run_instantiate_each_zwasm(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    let bytes = wasm_bytes.to_vec();
    let name = fn_name.to_string();
    on_big_stack("zwasm-instantiate", move || {
        let load_start = Instant::now();
        let mut l = Loaded::new(&bytes, &[])?;
        let load_time = load_start.elapsed();
        crate::measure_samples(load_time, || {
            l.release();
            let t = Instant::now();
            l.instantiate()?;
            let r = call_raw(l.func(&name)?, &[i32_val(arg)], 1)
                .with_context(|| format!("`{name}({arg})` trapped"))?;
            Ok((t.elapsed(), r[0].of as u32 as i32))
        })
    })
}

unsafe extern "C" fn porf_print_stub(
    _args: *const wasm_val_vec_t,
    _results: *mut wasm_val_vec_t,
) -> *mut wasm_trap_t {
    std::ptr::null_mut()
}

/// Dedicated Porffor-graphql runner. The Porffor-compiled wasm exports
/// `m()` with signature `() → (f64, i32)` (multi-value) and imports
/// `("", "b") : (f64) → ()` for per-character host print.
pub fn run_graphql_validation_porf_zwasm(wasm_bytes: &[u8]) -> Result<RunReport> {
    let bytes = wasm_bytes.to_vec();
    on_big_stack("zwasm-porf", move || {
        let load_start = Instant::now();
        let l = Loaded::new(
            &bytes,
            &[HostImport { params: &[WASM_F64], results: &[], callback: porf_print_stub }],
        )?;
        let mut l = l;
        let load_time = load_start.elapsed();
        // Each sample instantiates a fresh instance and runs m() once:
        // instantiate + m() is the timed unit on every runtime, because
        // Porffor never frees and grows memory across calls. The previous
        // instance is released before the clock starts.
        crate::measure_samples(load_time, || {
            l.release();
            let t = Instant::now();
            l.instantiate()?;
            let r = call_raw(l.func("m")?, &[], 2).context("zwasm m() trapped")?;
            Ok((t.elapsed(), r[1].of as u32 as i32))
        })
    })
}

// --- femtovg E2E guest binding -------------------------------------------

#[cfg(feature = "femtovg-e2e")]
mod femtovg_binding {
    use std::cell::Cell;

    use super::*;
    use crate::femtovg_e2e as e2e;

    thread_local! {
        /// The guest's memory. wasm-c-api callbacks get no caller context,
        /// and the data pointer can move when the guest grows memory, so the
        /// callbacks re-read it through the handle on every call.
        static MEMORY: Cell<*mut wasm_memory_t> = const { Cell::new(std::ptr::null_mut()) };
    }

    unsafe fn memory<'a>() -> &'a [u8] {
        let m = MEMORY.with(Cell::get);
        if m.is_null() {
            return &[];
        }
        let base = wasm_memory_data(m);
        if base.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(base, wasm_memory_data_size(m))
        }
    }

    unsafe fn arg(args: *const wasm_val_vec_t, i: usize) -> i32 {
        (*(*args).data.add(i)).of as u32 as i32
    }

    unsafe fn ret(results: *mut wasm_val_vec_t, v: i32) {
        *(*results).data = wasm_val_t { kind: WASM_I32, of: v as u32 as u64 };
    }

    unsafe extern "C" fn set_size(a: *const wasm_val_vec_t, _r: *mut wasm_val_vec_t) -> *mut wasm_trap_t {
        e2e::host_set_size(arg(a, 0), arg(a, 1), arg(a, 2));
        std::ptr::null_mut()
    }
    unsafe extern "C" fn image_alloc(a: *const wasm_val_vec_t, r: *mut wasm_val_vec_t) -> *mut wasm_trap_t {
        ret(r, e2e::host_image_alloc(arg(a, 0), arg(a, 1), arg(a, 2), arg(a, 3)));
        std::ptr::null_mut()
    }
    unsafe extern "C" fn image_update(a: *const wasm_val_vec_t, r: *mut wasm_val_vec_t) -> *mut wasm_trap_t {
        let g = |i| arg(a, i);
        let rc = match e2e::guest_span(memory(), g(6), g(7).max(0) as usize) {
            Some(data) => e2e::host_image_update(g(0), g(1), g(2), g(3), g(4), g(5), data),
            None => -1,
        };
        ret(r, rc);
        std::ptr::null_mut()
    }
    unsafe extern "C" fn image_delete(a: *const wasm_val_vec_t, _r: *mut wasm_val_vec_t) -> *mut wasm_trap_t {
        e2e::host_image_delete(arg(a, 0));
        std::ptr::null_mut()
    }
    unsafe extern "C" fn render(a: *const wasm_val_vec_t, _r: *mut wasm_val_vec_t) -> *mut wasm_trap_t {
        let mem = memory();
        let verts = e2e::guest_span(mem, arg(a, 0), arg(a, 1).max(0) as usize * 16);
        let cmds = e2e::guest_span(mem, arg(a, 2), arg(a, 3).max(0) as usize * 4);
        if let (Some(v), Some(c)) = (verts, cmds) {
            e2e::host_render(v, c);
        }
        std::ptr::null_mut()
    }

    pub(crate) struct ZwasmGuest {
        funcs: [*mut wasm_func_t; 3],
        _loaded: Loaded,
    }

    impl ZwasmGuest {
        fn call(&mut self, which: usize, args: &[i32]) -> Result<i32> {
            let vals: Vec<wasm_val_t> = args.iter().map(|&v| i32_val(v)).collect();
            let r = call_raw(self.funcs[which], &vals, 1)?;
            Ok(r[0].of as u32 as i32)
        }
    }

    impl e2e::Guest for ZwasmGuest {
        fn init(&mut self, scene: i32, width: i32, height: i32) -> Result<i32> {
            self.call(0, &[scene, width, height])
        }
        fn frame(&mut self, index: i32, count: i32) -> Result<i32> {
            self.call(1, &[index, count])
        }
        fn mem_pages(&mut self) -> Result<i32> {
            self.call(2, &[])
        }
    }

    impl Drop for ZwasmGuest {
        fn drop(&mut self) {
            MEMORY.with(|m| m.set(std::ptr::null_mut()));
        }
    }

    const I: u8 = WASM_I32;

    pub(crate) fn guest(wasm: &[u8]) -> Result<Box<dyn e2e::Guest>> {
        let imports = e2e::import_names(wasm)?
            .into_iter()
            .map(|(module, name)| {
                if module != "fvg" {
                    bail!("unexpected import {module}.{name}");
                }
                Ok(match name.as_str() {
                    "set_size" => HostImport { params: &[I, I, I], results: &[], callback: set_size },
                    "image_alloc" => HostImport { params: &[I, I, I, I], results: &[I], callback: image_alloc },
                    "image_update" => {
                        HostImport { params: &[I, I, I, I, I, I, I, I], results: &[I], callback: image_update }
                    }
                    "image_delete" => HostImport { params: &[I], results: &[], callback: image_delete },
                    "render" => HostImport { params: &[I, I, I, I], results: &[], callback: render },
                    other => bail!("unexpected import fvg.{other}"),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let loaded = Loaded::new(wasm, &imports)?;
        MEMORY.with(|m| m.set(loaded.memory_handle("memory").unwrap_or(std::ptr::null_mut())));
        let funcs = [loaded.func("fvg_init")?, loaded.func("fvg_frame")?, loaded.func("fvg_mem_pages")?];
        Ok(Box::new(ZwasmGuest { funcs, _loaded: loaded }))
    }
}

#[cfg(feature = "femtovg-e2e")]
pub(crate) fn femtovg_guest(wasm: &'static [u8]) -> Result<Box<dyn crate::femtovg_e2e::Guest>> {
    femtovg_binding::guest(wasm)
}
