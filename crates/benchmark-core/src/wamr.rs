//! Hand-written FFI bindings + thin runner for WebAssembly Micro Runtime
//! (WAMR) — used as the comparison runtime against wasmtime+Pulley.
//!
//! Built and linked by [`build.rs`] from
//! `wasm-micro-runtime/product-mini/platforms/<plat>/build/libiwasm.a`.
//! WAMR is configured with `WAMR_BUILD_FAST_INTERP=1` (their fast
//! preprocessed-bytecode interpreter — apples-to-apples vs Pulley) and
//! AOT/JIT off so it stays App-Store-eligible the same way Pulley does.

use std::os::raw::{c_char, c_int, c_void};
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::{taskinfo, RunReport};

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
    // 8 KiB stack, 64 KiB initial heap — enough for the static-buffer
    // workloads, more than enough for fib_tail / fib.
    let module_inst = unsafe {
        wasm_runtime_instantiate(
            module,
            8 * 1024,
            64 * 1024,
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
    let exec_env = unsafe { wasm_runtime_create_exec_env(module_inst, 8 * 1024) };
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

    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();

    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        result = call(arg)?;
        samples.push(it_start.elapsed().as_nanos() as u64);
    }

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

    unsafe {
        wasm_runtime_destroy_exec_env(exec_env);
        wasm_runtime_deinstantiate(module_inst);
        wasm_runtime_unload(module);
    }

    Ok(RunReport {
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
    })
}

pub fn run_workload_wamr(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_wamr_iters(wasm_bytes, fn_name, arg, 0)
}
