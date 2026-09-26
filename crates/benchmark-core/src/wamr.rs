//! Hand-written FFI bindings + thin runner for WebAssembly Micro Runtime
//! (WAMR) — used as the comparison runtime against wasmtime+Pulley.
//!
//! Built and linked by [`build.rs`] from
//! `wasm-micro-runtime/product-mini/platforms/<plat>/build/libiwasm.a`.
//! WAMR is configured with `WAMR_BUILD_FAST_INTERP=1` (their fast
//! preprocessed-bytecode interpreter — apples-to-apples vs Pulley) and
//! AOT/JIT off so it stays App-Store-eligible the same way Pulley does.

use std::os::raw::{c_char, c_void};
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::RunReport;

#[allow(non_camel_case_types)]
type wasm_module_t = *mut c_void;
#[allow(non_camel_case_types)]
type wasm_module_inst_t = *mut c_void;
#[allow(non_camel_case_types)]
type wasm_function_inst_t = *mut c_void;
#[allow(non_camel_case_types)]
type wasm_exec_env_t = *mut c_void;

extern "C" {
    fn wasm_runtime_init() -> bool;
    fn wasm_runtime_destroy();
    fn wasm_runtime_load(
        buf: *mut u8,
        size: u32,
        error_buf: *mut c_char,
        error_buf_size: u32,
    ) -> wasm_module_t;
    fn wasm_runtime_unload(module: wasm_module_t);
    fn wasm_runtime_instantiate(
        module: wasm_module_t,
        stack_size: u32,
        heap_size: u32,
        error_buf: *mut c_char,
        error_buf_size: u32,
    ) -> wasm_module_inst_t;
    fn wasm_runtime_deinstantiate(module_inst: wasm_module_inst_t);
    fn wasm_runtime_lookup_function(
        module_inst: wasm_module_inst_t,
        name: *const c_char,
    ) -> wasm_function_inst_t;
    fn wasm_runtime_create_exec_env(
        module_inst: wasm_module_inst_t,
        stack_size: u32,
    ) -> wasm_exec_env_t;
    fn wasm_runtime_destroy_exec_env(exec_env: wasm_exec_env_t);
    fn wasm_runtime_call_wasm(
        exec_env: wasm_exec_env_t,
        function: wasm_function_inst_t,
        argc: u32,
        argv: *mut u32,
    ) -> bool;
    fn wasm_runtime_get_exception(module_inst: wasm_module_inst_t) -> *const c_char;
}

static INIT: Once = Once::new();
static mut INIT_OK: bool = false;
fn ensure_init() -> Result<()> {
    INIT.call_once(|| {
        let ok = unsafe { wasm_runtime_init() };
        unsafe {
            INIT_OK = ok;
        }
    });
    if unsafe { INIT_OK } {
        Ok(())
    } else {
        Err(anyhow!("wasm_runtime_init failed"))
    }
}

/// Initialize the WAMR runtime. **Must be called from the main thread**
/// before any worker thread tries to load a module — WAMR's stack-guard
/// page setup is only permitted on the process's primary thread.
pub fn init() -> Result<()> {
    ensure_init()
}

/// Run `fn_name(arg) -> i32` on `wasm_bytes` via WAMR fast-interp,
/// auto-tuning iteration count (or honoring `iters` if non-zero).
/// Mirrors `crate::run_workload_iters` for the wasmtime path.
pub fn run_workload_wamr_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    ensure_init()?;

    let mut err_buf = [0i8; 256];

    let load_start = Instant::now();
    let mut bytes_owned = wasm_bytes.to_vec();
    let module = unsafe {
        wasm_runtime_load(
            bytes_owned.as_mut_ptr(),
            bytes_owned.len() as u32,
            err_buf.as_mut_ptr(),
            err_buf.len() as u32,
        )
    };
    if module.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err_buf.as_ptr()) }
            .to_string_lossy();
        return Err(anyhow!("wasm_runtime_load failed: {msg}"));
    }
    // 32 KiB stack, 8 MiB initial heap. The 64 KiB heap that earlier
    // workloads used was fine for the static-buffer benchmarks
    // (call_indirect / xmrsplayer / vtable / fib / sieve / crc32 /
    // matmul / convolution / audio_dsp / bulk_memory) but too small
    // for AS-compiled workloads — AS's TLSF allocator `~start` first
    // tries to `memory.grow` to its initial slab and traps `unreachable`
    // if the grow fails. 8 MiB is comfortably above what
    // `graphql-validation (AS)` needs and still small relative to
    // arm64_32-apple-watchos's overall memory pressure.
    let module_inst = unsafe {
        wasm_runtime_instantiate(
            module,
            32 * 1024,
            8 * 1024 * 1024,
            err_buf.as_mut_ptr(),
            err_buf.len() as u32,
        )
    };
    if module_inst.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err_buf.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        unsafe { wasm_runtime_unload(module) };
        return Err(anyhow!("wasm_runtime_instantiate failed: {msg}"));
    }
    let cname = std::ffi::CString::new(fn_name)?;
    let func = unsafe { wasm_runtime_lookup_function(module_inst, cname.as_ptr()) };
    if func.is_null() {
        unsafe {
            wasm_runtime_deinstantiate(module_inst);
            wasm_runtime_unload(module);
        }
        return Err(anyhow!("export `{fn_name}` not found"));
    }
    let exec_env = unsafe { wasm_runtime_create_exec_env(module_inst, 32 * 1024) };
    if exec_env.is_null() {
        unsafe {
            wasm_runtime_deinstantiate(module_inst);
            wasm_runtime_unload(module);
        }
        return Err(anyhow!("wasm_runtime_create_exec_env failed"));
    }
    let load_time = load_start.elapsed();

    let mut call = |arg: i32| -> Result<i32> {
        let mut argv = [arg as u32; 1];
        let ok = unsafe { wasm_runtime_call_wasm(exec_env, func, 1, argv.as_mut_ptr()) };
        if !ok {
            let msg_ptr = unsafe { wasm_runtime_get_exception(module_inst) };
            let msg = if msg_ptr.is_null() {
                "(no exception text)".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(msg_ptr) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(anyhow!("WAMR call_wasm trap: {msg}"));
        }
        Ok(argv[0] as i32)
    };

    // Two warmup calls so workloads with lazy first-call init
    // (xmrsplayer, ...) have their per-instance setup paid against
    // `load_ns` rather than inflating the `pick_iters` budget. See
    // the matching change in the Pulley `run_workload_iters` for the
    // detailed rationale.
    let mut result = call(arg).context("WAMR init-warmup call failed")?;
    let warm_start = Instant::now();
    result = call(arg).context("WAMR steady-warmup call failed")?;
    let warm = warm_start.elapsed();
    let n = if iters == 0 {
        crate::pick_iters(warm, Duration::from_millis(200))
    } else {
        iters
    };

    let window = crate::Window::start();

    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        result = call(arg)?;
        samples.push(it_start.elapsed().as_nanos() as u64);
    }

    unsafe {
        wasm_runtime_destroy_exec_env(exec_env);
        wasm_runtime_deinstantiate(module_inst);
        wasm_runtime_unload(module);
    }

    Ok(window.finish(result, n, load_time, samples))
}

pub fn run_workload_wamr(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_wamr_iters(wasm_bytes, fn_name, arg, 0)
}

fn exception_text(module_inst: wasm_module_inst_t) -> String {
    let msg_ptr = unsafe { wasm_runtime_get_exception(module_inst) };
    if msg_ptr.is_null() {
        "(no exception text)".to_string()
    } else {
        unsafe { std::ffi::CStr::from_ptr(msg_ptr) }.to_string_lossy().into_owned()
    }
}

fn load_module(bytes: &mut [u8]) -> Result<wasm_module_t> {
    let mut err_buf = [0i8; 256];
    let module = unsafe {
        wasm_runtime_load(bytes.as_mut_ptr(), bytes.len() as u32, err_buf.as_mut_ptr(),
                          err_buf.len() as u32)
    };
    if module.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err_buf.as_ptr()) }.to_string_lossy();
        return Err(anyhow!("wasm_runtime_load failed: {msg}"));
    }
    Ok(module)
}

/// One sample on a fresh instance: instantiate + `func` + call are timed,
/// the exec env and instance are destroyed after the clock stops. `call`
/// gets the exec env and function and returns the i32 result.
fn instantiate_sample(
    module: wasm_module_t,
    fn_name: &std::ffi::CStr,
    stack_size: u32,
    heap_size: u32,
    call: impl FnOnce(wasm_exec_env_t, wasm_function_inst_t) -> bool,
    result: impl FnOnce() -> i32,
) -> Result<(Duration, i32)> {
    let mut err_buf = [0i8; 256];
    let t = Instant::now();
    let inst = unsafe {
        wasm_runtime_instantiate(module, stack_size, heap_size, err_buf.as_mut_ptr(),
                                 err_buf.len() as u32)
    };
    if inst.is_null() {
        let msg = unsafe { std::ffi::CStr::from_ptr(err_buf.as_ptr()) }.to_string_lossy();
        return Err(anyhow!("wasm_runtime_instantiate failed: {msg}"));
    }
    let func = unsafe { wasm_runtime_lookup_function(inst, fn_name.as_ptr()) };
    if func.is_null() {
        unsafe { wasm_runtime_deinstantiate(inst) };
        return Err(anyhow!("export `{}` not found", fn_name.to_string_lossy()));
    }
    let exec_env = unsafe { wasm_runtime_create_exec_env(inst, stack_size) };
    if exec_env.is_null() {
        unsafe { wasm_runtime_deinstantiate(inst) };
        return Err(anyhow!("wasm_runtime_create_exec_env failed"));
    }
    let ok = call(exec_env, func);
    let elapsed = t.elapsed();
    let r = if ok { Ok((elapsed, result())) } else {
        Err(anyhow!("WAMR call_wasm trap: {}", exception_text(inst)))
    };
    unsafe {
        wasm_runtime_destroy_exec_env(exec_env);
        wasm_runtime_deinstantiate(inst);
    }
    r
}

/// `Shape::InstantiateEach` on WAMR: the module is loaded once, every
/// sample instantiates it afresh. The app heap is 0 here (the generic
/// runner's 8 MiB heap is for AssemblyScript's allocator); a heap WAMR
/// would carve into linear memory on every instantiation is not part of
/// what the case measures.
pub fn run_instantiate_each_wamr(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    ensure_init()?;
    let load_start = Instant::now();
    // WAMR keeps pointers into the bytes for the module's lifetime; the
    // guard (declared after the bytes) unloads before they are freed.
    let mut bytes_owned = wasm_bytes.to_vec();
    struct ModGuard(wasm_module_t);
    impl Drop for ModGuard {
        fn drop(&mut self) {
            unsafe { wasm_runtime_unload(self.0) };
        }
    }
    let module = ModGuard(load_module(&mut bytes_owned)?);
    let cname = std::ffi::CString::new(fn_name)?;
    let load_time = load_start.elapsed();
    crate::measure_samples(load_time, || {
        let mut argv = [arg as u32; 1];
        let argv_ptr = argv.as_mut_ptr();
        instantiate_sample(
            module.0,
            &cname,
            32 * 1024,
            0,
            |env, func| unsafe { wasm_runtime_call_wasm(env, func, 1, argv_ptr) },
            || unsafe { *argv_ptr as i32 },
        )
    })
}

// --- femtovg E2E guest binding -------------------------------------------

#[cfg(feature = "femtovg-e2e")]
mod femtovg_binding {
    use super::*;
    use crate::femtovg_e2e as e2e;

    #[repr(C)]
    struct NativeSymbol {
        symbol: *const c_char,
        func_ptr: *mut c_void,
        signature: *const c_char,
        attachment: *mut c_void,
    }

    extern "C" {
        fn wasm_runtime_register_natives(
            module_name: *const c_char,
            native_symbols: *mut NativeSymbol,
            n_native_symbols: u32,
        ) -> bool;
        fn wasm_runtime_get_module_inst(exec_env: wasm_exec_env_t) -> wasm_module_inst_t;
        fn wasm_runtime_validate_app_addr(inst: wasm_module_inst_t, app_offset: u64, size: u64) -> bool;
        fn wasm_runtime_addr_app_to_native(inst: wasm_module_inst_t, app_offset: u64) -> *mut c_void;
    }

    /// `[ptr, ptr + len)` of the calling instance's linear memory.
    unsafe fn span<'a>(env: wasm_exec_env_t, ptr: i32, len: usize) -> Option<&'a [u8]> {
        let inst = wasm_runtime_get_module_inst(env);
        let off = u32::try_from(ptr).ok()? as u64;
        if !wasm_runtime_validate_app_addr(inst, off, len as u64) {
            return None;
        }
        let p = wasm_runtime_addr_app_to_native(inst, off) as *const u8;
        (!p.is_null()).then(|| std::slice::from_raw_parts(p, len))
    }

    extern "C" fn set_size(_env: wasm_exec_env_t, w: i32, h: i32, dpi: i32) {
        e2e::host_set_size(w, h, dpi)
    }
    extern "C" fn image_alloc(_env: wasm_exec_env_t, w: i32, h: i32, f: i32, fl: i32) -> i32 {
        e2e::host_image_alloc(w, h, f, fl)
    }
    #[allow(clippy::too_many_arguments)]
    extern "C" fn image_update(
        env: wasm_exec_env_t, hd: i32, x: i32, y: i32, w: i32, h: i32, f: i32, p: i32, n: i32,
    ) -> i32 {
        match unsafe { span(env, p, n.max(0) as usize) } {
            Some(data) => e2e::host_image_update(hd, x, y, w, h, f, data),
            None => -1,
        }
    }
    extern "C" fn image_delete(_env: wasm_exec_env_t, hd: i32) {
        e2e::host_image_delete(hd)
    }
    extern "C" fn render(env: wasm_exec_env_t, vp: i32, vn: i32, cp: i32, cn: i32) {
        let verts = unsafe { span(env, vp, vn.max(0) as usize * 16) };
        let cmds = unsafe { span(env, cp, cn.max(0) as usize * 4) };
        if let (Some(v), Some(c)) = (verts, cmds) {
            e2e::host_render(v, c)
        }
    }

    static REGISTER: Once = Once::new();

    /// WAMR keeps the symbol array (and its strings) for the process
    /// lifetime and resolves `fvg.*` imports against it at load time.
    fn register() {
        REGISTER.call_once(|| {
            let sym = |name: &str, f: *mut c_void, sig: &str| NativeSymbol {
                symbol: std::ffi::CString::new(name).unwrap().into_raw(),
                func_ptr: f,
                signature: std::ffi::CString::new(sig).unwrap().into_raw(),
                attachment: std::ptr::null_mut(),
            };
            let syms = Box::leak(Box::new([
                sym("set_size", set_size as *mut c_void, "(iii)"),
                sym("image_alloc", image_alloc as *mut c_void, "(iiii)i"),
                sym("image_update", image_update as *mut c_void, "(iiiiiiii)i"),
                sym("image_delete", image_delete as *mut c_void, "(i)"),
                sym("render", render as *mut c_void, "(iiii)"),
            ]));
            let module = std::ffi::CString::new("fvg").unwrap().into_raw();
            unsafe { wasm_runtime_register_natives(module, syms.as_mut_ptr(), syms.len() as u32) };
        });
    }

    pub(crate) struct WamrGuest {
        exec_env: wasm_exec_env_t,
        inst: wasm_module_inst_t,
        module: wasm_module_t,
        funcs: [wasm_function_inst_t; 3],
        // WAMR keeps pointers into the bytes for the module's lifetime.
        _bytes: Vec<u8>,
    }

    impl WamrGuest {
        fn call(&mut self, which: usize, args: &[i32]) -> Result<i32> {
            let mut argv = [0u32; 3];
            for (a, v) in argv.iter_mut().zip(args) {
                *a = *v as u32;
            }
            let ok = unsafe {
                wasm_runtime_call_wasm(self.exec_env, self.funcs[which], args.len() as u32, argv.as_mut_ptr())
            };
            if !ok {
                return Err(anyhow!("WAMR call_wasm trap: {}", exception_text(self.inst)));
            }
            Ok(argv[0] as i32)
        }
    }

    impl e2e::Guest for WamrGuest {
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

    impl Drop for WamrGuest {
        fn drop(&mut self) {
            unsafe {
                wasm_runtime_destroy_exec_env(self.exec_env);
                wasm_runtime_deinstantiate(self.inst);
                wasm_runtime_unload(self.module);
            }
        }
    }

    pub(crate) fn guest(wasm: &[u8]) -> Result<Box<dyn e2e::Guest>> {
        ensure_init()?;
        register();
        let mut bytes = wasm.to_vec();
        let module = load_module(&mut bytes)?;
        let mut err_buf = [0i8; 256];
        // 1 MiB wasm stack: usvg's parser and femtovg's path code recurse
        // more than the 32 KiB the micro-benchmarks use. No app heap: the
        // guest brings its own allocator.
        let stack = 1024 * 1024;
        let inst = unsafe {
            wasm_runtime_instantiate(module, stack, 0, err_buf.as_mut_ptr(), err_buf.len() as u32)
        };
        if inst.is_null() {
            let msg = unsafe { std::ffi::CStr::from_ptr(err_buf.as_ptr()) }.to_string_lossy().into_owned();
            unsafe { wasm_runtime_unload(module) };
            return Err(anyhow!("wasm_runtime_instantiate failed: {msg}"));
        }
        let mut funcs = [std::ptr::null_mut(); 3];
        for (f, name) in funcs.iter_mut().zip(["fvg_init", "fvg_frame", "fvg_mem_pages"]) {
            let c = std::ffi::CString::new(name)?;
            *f = unsafe { wasm_runtime_lookup_function(inst, c.as_ptr()) };
            if f.is_null() {
                unsafe {
                    wasm_runtime_deinstantiate(inst);
                    wasm_runtime_unload(module);
                }
                return Err(anyhow!("export `{name}` not found"));
            }
        }
        let exec_env = unsafe { wasm_runtime_create_exec_env(inst, stack) };
        if exec_env.is_null() {
            unsafe {
                wasm_runtime_deinstantiate(inst);
                wasm_runtime_unload(module);
            }
            return Err(anyhow!("wasm_runtime_create_exec_env failed"));
        }
        Ok(Box::new(WamrGuest { exec_env, inst, module, funcs, _bytes: bytes }))
    }
}

#[cfg(feature = "femtovg-e2e")]
pub(crate) fn femtovg_guest(wasm: &'static [u8]) -> Result<Box<dyn crate::femtovg_e2e::Guest>> {
    femtovg_binding::guest(wasm)
}
