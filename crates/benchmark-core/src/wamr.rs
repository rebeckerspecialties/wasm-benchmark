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

// --- graphql-validation-porf runner --------------------------------
//
// Porffor's `m()` export returns `(f64, i32)` (multi-return) and the
// module imports a host print function `("", "b")` taking an f64.
// The generic runner above can't handle either, so we have a dedicated
// runner here. We use WAMR's `call_wasm_v` (variadic) form so we can
// declare the result-count up front; the return values themselves are
// discarded (we only care about wallclock).
//
// Per-iteration: we destroy and re-instantiate to match Porffor's
// no-GC memory model (the wasmtime side does the same via fresh
// `Store` per iter). Module is loaded once.

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
    fn wasm_runtime_call_wasm_v(
        exec_env: wasm_exec_env_t,
        function: wasm_function_inst_t,
        num_results: u32,
        results: *mut WasmVal,
        num_args: u32,
        ...
    ) -> bool;
}

#[repr(C)]
#[derive(Copy, Clone)]
union WasmValPayload {
    i32_: i32,
    i64_: i64,
    f32_: f32,
    f64_: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct WasmVal {
    kind: u32, // WASM_I32=0, WASM_I64=1, WASM_F32=2, WASM_F64=3
    _pad: u32,
    payload: WasmValPayload,
}

extern "C" fn porf_b_native(_exec_env: wasm_exec_env_t, _ch: f64) {
    // Stubbed host print — Porffor calls this per character to write
    // its `validate: errors=N` line. We swallow the output; only the
    // wallclock per validation matters.
}

static REGISTER_ONCE: Once = Once::new();

fn ensure_porf_natives_registered() {
    REGISTER_ONCE.call_once(|| {
        // These must outlive every module load — WAMR's
        // wasm_runtime_register_natives stores the NativeSymbol array
        // pointer in a global linked list and dereferences it later
        // at module-load time. If we pass a stack-local array, the
        // pointer dangles after this function returns. So leak both
        // the strings AND the symbol array.
        let module = std::ffi::CString::new("").unwrap().into_raw() as *const c_char;
        let symbol = std::ffi::CString::new("b").unwrap().into_raw() as *const c_char;
        let signature = std::ffi::CString::new("(F)").unwrap().into_raw() as *const c_char;
        let sym_box = Box::new(NativeSymbol {
            symbol,
            func_ptr: porf_b_native as *mut c_void,
            signature,
            attachment: std::ptr::null_mut(),
        });
        let sym_ptr: *mut NativeSymbol = Box::leak(sym_box);
        let _ok = unsafe { wasm_runtime_register_natives(module, sym_ptr, 1) };
    });
}

pub fn run_graphql_validation_porf_wamr(wasm_bytes: &[u8]) -> Result<RunReport> {
    ensure_init()?;
    ensure_porf_natives_registered();

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
            .to_string_lossy()
            .into_owned();
        return Err(anyhow!("wasm_runtime_load failed: {msg}"));
    }

    // Helper: spin up an instance, run `m()` once, tear down. Heap is
    // sized generously (4 MiB) because Porffor allocates without GC.
    let cname_m = std::ffi::CString::new("m")?;
    let run_once = |timed: bool, err_buf: &mut [i8; 256]| -> Result<Duration> {
        // 1 MB wasm operand stack — Porffor compiles JS to deeply
        // recursive wasm with no inlining, so the per-frame slot
        // allocations add up across the graphql-validation call tree.
        // 8 KB / 64 KB both overflow mid-validation with
        // "wasm operand stack overflow".
        let module_inst = unsafe {
            wasm_runtime_instantiate(
                module,
                1024 * 1024,
                4 * 1024 * 1024,
                err_buf.as_mut_ptr(),
                err_buf.len() as u32,
            )
        };
        if module_inst.is_null() {
            let msg = unsafe { std::ffi::CStr::from_ptr(err_buf.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(anyhow!("wasm_runtime_instantiate failed: {msg}"));
        }
        let func = unsafe { wasm_runtime_lookup_function(module_inst, cname_m.as_ptr()) };
        if func.is_null() {
            unsafe { wasm_runtime_deinstantiate(module_inst) };
            return Err(anyhow!("export `m` not found"));
        }
        // 1 MB exec-env wasm stack — same reasoning as the
        // wasm_runtime_instantiate call above. The stack here is what
        // backs the per-frame slot allocations across the recursive
        // call chain.
        let exec_env = unsafe { wasm_runtime_create_exec_env(module_inst, 1024 * 1024) };
        if exec_env.is_null() {
            unsafe { wasm_runtime_deinstantiate(module_inst) };
            return Err(anyhow!("wasm_runtime_create_exec_env failed"));
        }
        // `m()` returns (f64, i32) — 2 results, 0 args. Use the
        // variadic call form.
        let mut results: [WasmVal; 2] = [WasmVal {
            kind: 0,
            _pad: 0,
            payload: WasmValPayload { i64_: 0 },
        }; 2];
        let it_start = if timed { Some(Instant::now()) } else { None };
        let ok = unsafe {
            wasm_runtime_call_wasm_v(exec_env, func, 2, results.as_mut_ptr(), 0)
        };
        let elapsed = it_start.map(|t| t.elapsed()).unwrap_or_default();
        if !ok {
            let msg_ptr = unsafe { wasm_runtime_get_exception(module_inst) };
            let msg = if msg_ptr.is_null() {
                "(no exception text)".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(msg_ptr) }
                    .to_string_lossy()
                    .into_owned()
            };
            unsafe {
                wasm_runtime_destroy_exec_env(exec_env);
                wasm_runtime_deinstantiate(module_inst);
            }
            return Err(anyhow!("WAMR m() trap: {msg}"));
        }
        unsafe {
            wasm_runtime_destroy_exec_env(exec_env);
            wasm_runtime_deinstantiate(module_inst);
        }
        Ok(elapsed)
    };

    let load_time = load_start.elapsed();

    // Warmup → budget.
    let warm = run_once(true, &mut err_buf).context("WAMR porf warmup failed")?;
    let n = crate::pick_iters(warm, Duration::from_millis(200));

    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();

    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let t = run_once(true, &mut err_buf).context("WAMR porf iter failed")?;
        samples.push(t.as_nanos() as u64);
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
        wasm_runtime_unload(module);
    }

    Ok(RunReport {
        result: 0,
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
