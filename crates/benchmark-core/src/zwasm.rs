//! Hand-written FFI bindings + thin runner for zwasm
//! (`clojurewasm/zwasm`) — a Zig WebAssembly runtime built with
//! `-Djit=false` so it runs strictly as an interpreter (no native
//! codegen, no MAP_JIT). The README's "supported hosts" list omits
//! iOS / watchOS-sim / tvOS, but Zig 0.16's `-target aarch64-ios`
//! etc. cross-compile the static lib straight out of the box. See
//! `scripts/build-zwasm.sh`.
//!
//! arm64_32-apple-watchos is **not** supported by Zig 0.16 (no
//! target) and zwasm assumes 64-bit pointers anyway, so the
//! watchOS-device build is skipped — see the watchos arm in
//! `scripts/build-zwasm.sh`.
//!
//! C ABI: zwasm.h documents the surface — all values are passed
//! through `uint64_t` arrays with the raw Wasm encoding (i32 is
//! zero-extended).

use std::os::raw::{c_char, c_void};
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::{taskinfo, RunReport};

// Opaque pointer types — zwasm.h documents them as opaque structs.
#[allow(non_camel_case_types)]
type zwasm_module_t = c_void;
#[allow(non_camel_case_types)]
type zwasm_config_t = c_void;

// Zig 0.16's `std.debug.SelfInfo.MachO` references
// `_dyld_get_image_header_containing_address` for panic-time
// stack-walks. The symbol exists in iOS dyld but isn't in any public
// TBD, so the iOS link fails with "Undefined symbols". Provide a
// safe stub returning NULL — used only on a panic path we don't
// expect to hit, and matching dyld's documented "address not in any
// loaded image" return value. macOS picks up the real dyld symbol;
// the duplicate-definition is resolved by `-l` ordering (our static
// lib's symbol wins on iOS, dyld's wins on macOS).
//
// Scoped to the apple targets where this matters; on Linux the Zig
// build doesn't reach this code path.
#[cfg(all(target_vendor = "apple", any(target_os = "ios", target_os = "tvos", target_os = "watchos")))]
mod ios_dyld_stub {
    use std::os::raw::c_void;
    #[unsafe(no_mangle)]
    pub extern "C" fn _dyld_get_image_header_containing_address(
        _addr: *const c_void,
    ) -> *const c_void {
        std::ptr::null()
    }
}

// Opaque pointer for the imports collection.
#[allow(non_camel_case_types)]
type zwasm_imports_t = c_void;

extern "C" {
    fn zwasm_config_new() -> *mut zwasm_config_t;
    fn zwasm_config_delete(cfg: *mut zwasm_config_t);
    fn zwasm_config_set_force_interpreter(cfg: *mut zwasm_config_t, on: bool);
    fn zwasm_module_new_configured(
        wasm_ptr: *const u8,
        len: usize,
        cfg: *mut zwasm_config_t,
    ) -> *mut zwasm_module_t;
    fn zwasm_module_new_with_imports(
        wasm_ptr: *const u8,
        len: usize,
        imports: *mut zwasm_imports_t,
    ) -> *mut zwasm_module_t;
    fn zwasm_module_delete(m: *mut zwasm_module_t);
    fn zwasm_module_invoke(
        m: *mut zwasm_module_t,
        name: *const c_char,
        args: *const u64,
        nargs: u32,
        results: *mut u64,
        nresults: u32,
    ) -> bool;
    fn zwasm_last_error_message() -> *const c_char;

    fn zwasm_import_new() -> *mut zwasm_imports_t;
    fn zwasm_import_delete(imports: *mut zwasm_imports_t);
    fn zwasm_import_add_fn(
        imports: *mut zwasm_imports_t,
        module_name: *const c_char,
        func_name: *const c_char,
        callback: ZwasmHostFn,
        env: *mut c_void,
        param_count: u32,
        result_count: u32,
    );
}

type ZwasmHostFn =
    extern "C" fn(env: *mut c_void, args: *const u64, results: *mut u64) -> bool;

// Porffor host-print stub. zwasm's callback signature passes args /
// results as raw u64; we don't read either. Returns true (no trap).
extern "C" fn zwasm_porf_b(_env: *mut c_void, _args: *const u64, _results: *mut u64) -> bool {
    true
}

fn last_error() -> String {
    let p = unsafe { zwasm_last_error_message() };
    if p.is_null() {
        "(no error message)".to_string()
    } else {
        unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned()
    }
}

static INIT: Once = Once::new();

pub fn init() -> Result<()> {
    INIT.call_once(|| {});
    Ok(())
}

pub fn run_workload_zwasm_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    // Crash diagnostic on the Swift dispatch worker (272 KiB stack)
    // pointed at `types.WasmModule.loadCore`'s memmove of the parsed
    // module bytes — zwasm's load path overflows the small dispatch
    // worker stack. iOS/tvOS pthread defaults are similarly tight.
    // Route the whole runner through a dedicated thread with an 8 MiB
    // stack so the load + call paths have headroom matching what the
    // main thread (the standalone C test environment) provides.
    let wasm_bytes_owned = wasm_bytes.to_vec();
    let fn_name_owned = fn_name.to_string();
    std::thread::Builder::new()
        .name("zwasm-runner".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || run_workload_zwasm_iters_inner(&wasm_bytes_owned, &fn_name_owned, arg, iters))
        .context("zwasm: failed to spawn dedicated 8 MiB-stack thread")?
        .join()
        .map_err(|_| anyhow!("zwasm-runner thread panicked"))?
}

fn run_workload_zwasm_iters_inner(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    init()?;

    let load_start = Instant::now();

    // Config — even though we built with `-Djit=false`, also flip
    // the runtime force_interpreter flag for belt-and-braces. zwasm's
    // documented behavior is that interpreter is forced if EITHER the
    // build-time flag or the runtime flag is set.
    let cfg = unsafe { zwasm_config_new() };
    if cfg.is_null() {
        return Err(anyhow!("zwasm_config_new returned NULL"));
    }
    struct CfgGuard(*mut zwasm_config_t);
    impl Drop for CfgGuard {
        fn drop(&mut self) {
            unsafe { zwasm_config_delete(self.0) };
        }
    }
    let _cfg_g = CfgGuard(cfg);
    unsafe { zwasm_config_set_force_interpreter(cfg, true) };

    // Module — zwasm copies the wasm bytes, so we don't need to keep
    // our copy alive after `_configured` returns.
    let module =
        unsafe { zwasm_module_new_configured(wasm_bytes.as_ptr(), wasm_bytes.len(), cfg) };
    if module.is_null() {
        return Err(anyhow!("zwasm_module_new_configured failed: {}", last_error()));
    }
    struct ModGuard(*mut zwasm_module_t);
    impl Drop for ModGuard {
        fn drop(&mut self) {
            unsafe { zwasm_module_delete(self.0) };
        }
    }
    let _mod_g = ModGuard(module);

    let cname = std::ffi::CString::new(fn_name)?;

    let load_time = load_start.elapsed();

    let call = |arg: i32| -> Result<i32> {
        // zwasm passes args as zero-extended u64.
        let args: [u64; 1] = [(arg as u32) as u64];
        let mut results: [u64; 1] = [0];
        let ok = unsafe {
            zwasm_module_invoke(
                module,
                cname.as_ptr(),
                args.as_ptr(),
                1,
                results.as_mut_ptr(),
                1,
            )
        };
        if !ok {
            return Err(anyhow!("zwasm_module_invoke trap: {}", last_error()));
        }
        Ok(results[0] as u32 as i32)
    };

    // Two warmups — same rationale as the other adapters.
    let mut result = call(arg).context("zwasm init-warmup failed")?;
    let warm_start = Instant::now();
    result = call(arg).context("zwasm steady-warmup failed")?;
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

pub fn run_workload_zwasm(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_zwasm_iters(wasm_bytes, fn_name, arg, 0)
}

/// Dedicated Porffor-graphql runner. The Porffor-compiled wasm
/// exports `m()` with signature `() → (f64, i32)` (multi-value) and
/// imports `("", "b") : (f64) → ()` for per-character host print. The
/// generic i32→i32 runner can't drive either shape; this function
/// wires the imports + uses the right call shape. Mirrors
/// `wamr::run_graphql_validation_porf_wamr`.
pub fn run_graphql_validation_porf_zwasm(wasm_bytes: &[u8]) -> Result<RunReport> {
    let wasm_bytes_owned = wasm_bytes.to_vec();
    std::thread::Builder::new()
        .name("zwasm-porf".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || run_graphql_validation_porf_zwasm_inner(&wasm_bytes_owned))
        .context("zwasm: failed to spawn 8 MiB-stack thread")?
        .join()
        .map_err(|_| anyhow!("zwasm-porf thread panicked"))?
}

fn run_graphql_validation_porf_zwasm_inner(wasm_bytes: &[u8]) -> Result<RunReport> {
    init()?;

    let load_start = Instant::now();

    let imports = unsafe { zwasm_import_new() };
    if imports.is_null() {
        return Err(anyhow!("zwasm_import_new returned NULL"));
    }
    struct ImpGuard(*mut zwasm_imports_t);
    impl Drop for ImpGuard {
        fn drop(&mut self) {
            unsafe { zwasm_import_delete(self.0) };
        }
    }
    let _ig = ImpGuard(imports);
    let modname = b"\0".as_ptr() as *const c_char;
    let funcname = b"b\0".as_ptr() as *const c_char;
    // f64 in, no results — Porffor's print-char shape.
    unsafe {
        zwasm_import_add_fn(imports, modname, funcname, zwasm_porf_b, std::ptr::null_mut(), 1, 0);
    }

    let module = unsafe {
        zwasm_module_new_with_imports(wasm_bytes.as_ptr(), wasm_bytes.len(), imports)
    };
    if module.is_null() {
        return Err(anyhow!(
            "zwasm_module_new_with_imports failed: {}",
            last_error()
        ));
    }
    struct ModGuard(*mut zwasm_module_t);
    impl Drop for ModGuard {
        fn drop(&mut self) {
            unsafe { zwasm_module_delete(self.0) };
        }
    }
    let _mg = ModGuard(module);

    let load_time = load_start.elapsed();

    // m() → (f64, i32). We don't use either; just measure wallclock.
    let cname = std::ffi::CString::new("m")?;
    let call = || -> Result<i32> {
        let mut results: [u64; 2] = [0; 2];
        let ok = unsafe {
            zwasm_module_invoke(
                module,
                cname.as_ptr(),
                std::ptr::null(),
                0,
                results.as_mut_ptr(),
                2,
            )
        };
        if !ok {
            return Err(anyhow!("zwasm m() trap: {}", last_error()));
        }
        Ok(results[1] as u32 as i32)
    };

    let mut result = call().context("zwasm porf warmup failed")?;
    let warm_start = Instant::now();
    result = call().context("zwasm porf steady-warmup failed")?;
    let warm = warm_start.elapsed();
    let n = crate::pick_iters(warm, Duration::from_millis(200));

    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();
    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        result = call()?;
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
