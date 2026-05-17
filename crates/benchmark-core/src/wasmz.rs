//! Hand-written FFI bindings + thin runner for wasmz
//! (`Ray-D-Song/wasmz`) — a Zig WebAssembly interpreter with a small
//! C API (`include/wasmz.h`). The upstream sources require Zig 0.15.2
//! but Zig 0.15's build runner segfaults on macOS 26 Tahoe (its
//! bundled libSystem TBDs are missing `_realpath$DARWIN_EXTSN` etc.),
//! so we maintain a small Zig-0.16 port as
//! `patches/wasmz/0001-zig-0.16-stdlib-port.patch`. See
//! `scripts/build-wasmz.sh` for the build wiring.
//!
//! arm64_32-apple-watchos is supported via
//! `patches/wasmz/0002-arm64_32-apple-watchos-support.patch`. Zig 0.16
//! spells the triple `aarch64-watchos-ilp32`; the legacy `arm64_32`
//! arch was removed in ziglang/zig PR #20820. The patch flips
//! `single_threaded = true` for the static lib and adds a
//! self-contained `panic`/`logFn` to avoid pulling
//! `std.Io.Threaded` (which doesn't compile under ILP32 — u64 →
//! usize narrowing in dirReadDarwin / pwrite).
//!
//! C ABI: `include/wasmz.h` documents the surface. Values are passed
//! via the `wasmz_val_t` tagged-union struct (kind + 16-byte payload);
//! results come back the same way. For the benchmarks we only ever
//! need i32 in / i32 out.

use std::os::raw::{c_char, c_int, c_void};
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::{taskinfo, RunReport};

// Opaque pointer types — wasmz.h documents them as opaque structs.
#[allow(non_camel_case_types)]
type wasmz_engine_t = c_void;
#[allow(non_camel_case_types)]
type wasmz_store_t = c_void;
#[allow(non_camel_case_types)]
type wasmz_module_t = c_void;
#[allow(non_camel_case_types)]
type wasmz_instance_t = c_void;
#[allow(non_camel_case_types)]
type wasmz_error_t = c_void;

// `wasmz_val_t` from include/wasmz.h. Layout is fixed by the header:
// 4-byte enum kind, 4-byte padding, then a 16-byte union.
#[repr(C)]
#[derive(Copy, Clone)]
struct WasmzVal {
    kind: c_int,
    _pad: [u8; 4],
    of: [u8; 16],
}

const WASMZ_VAL_I32: c_int = 0;

impl WasmzVal {
    fn i32_val(v: i32) -> Self {
        let mut s = WasmzVal { kind: WASMZ_VAL_I32, _pad: [0; 4], of: [0; 16] };
        s.of[..4].copy_from_slice(&v.to_le_bytes());
        s
    }
    fn as_i32(&self) -> i32 {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&self.of[..4]);
        i32::from_le_bytes(buf)
    }
}

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
// zwasm.rs declares the same stub under the same conditions. When
// both adapters are linked in the same staticlib the wasmz one would
// be a duplicate-symbol error, so we gate this definition on
// `not(have_zwasm)`: if the zwasm runtime is also linked, its stub
// already covers wasmz too.
#[cfg(all(
    target_vendor = "apple",
    any(target_os = "ios", target_os = "tvos", target_os = "watchos"),
    not(have_zwasm)
))]
mod ios_dyld_stub {
    use std::os::raw::c_void;
    #[unsafe(no_mangle)]
    pub extern "C" fn _dyld_get_image_header_containing_address(
        _addr: *const c_void,
    ) -> *const c_void {
        std::ptr::null()
    }
}

extern "C" {
    fn wasmz_engine_new() -> *mut wasmz_engine_t;
    fn wasmz_engine_delete(engine: *mut wasmz_engine_t);

    fn wasmz_store_new(engine: *mut wasmz_engine_t) -> *mut wasmz_store_t;
    fn wasmz_store_delete(store: *mut wasmz_store_t);

    fn wasmz_module_new(
        engine: *mut wasmz_engine_t,
        bytes: *const u8,
        len: usize,
        out_module: *mut *mut wasmz_module_t,
    ) -> *mut wasmz_error_t;
    fn wasmz_module_delete(m: *mut wasmz_module_t);

    fn wasmz_instance_new(
        store: *mut wasmz_store_t,
        module: *mut wasmz_module_t,
        out_instance: *mut *mut wasmz_instance_t,
    ) -> *mut wasmz_error_t;
    fn wasmz_instance_delete(inst: *mut wasmz_instance_t);

    fn wasmz_instance_call(
        inst: *mut wasmz_instance_t,
        func_name: *const c_char,
        args: *const WasmzVal,
        args_len: usize,
        results: *mut WasmzVal,
        results_len: usize,
    ) -> *mut wasmz_error_t;

    fn wasmz_error_delete(err: *mut wasmz_error_t);
    fn wasmz_error_message(err: *const wasmz_error_t) -> *const c_char;

    // Host-import linker surface — used to stub the Porffor host
    // print so graphql-validation-Porffor can load + run.
    fn wasmz_linker_new() -> *mut wasmz_linker_t;
    fn wasmz_linker_delete(linker: *mut wasmz_linker_t);
    fn wasmz_linker_define_func(
        linker: *mut wasmz_linker_t,
        module_name: *const c_char,
        func_name: *const c_char,
        param_kinds: *const c_int,
        param_count: usize,
        result_kinds: *const c_int,
        result_count: usize,
        func: WasmzFunc,
        host_data: *mut c_void,
    ) -> *mut wasmz_error_t;
    fn wasmz_instance_new_with_linker(
        store: *mut wasmz_store_t,
        module: *mut wasmz_module_t,
        linker: *mut wasmz_linker_t,
        out_instance: *mut *mut wasmz_instance_t,
    ) -> *mut wasmz_error_t;
}

#[allow(non_camel_case_types)]
type wasmz_linker_t = c_void;
#[allow(non_camel_case_types)]
type wasmz_ctx_t = c_void;

// wasmz_func_t per wasmz.h: int(*)(void *host_data, void *ctx,
// const wasmz_val_t *params, size_t param_count,
// wasmz_val_t *results, size_t result_count).
type WasmzFunc = extern "C" fn(
    host_data: *mut c_void,
    ctx: *mut wasmz_ctx_t,
    params: *const WasmzVal,
    param_count: usize,
    results: *mut WasmzVal,
    result_count: usize,
) -> c_int;

// Porffor host-print stub for wasmz. Returns 0 = success.
extern "C" fn wasmz_porf_b(
    _host_data: *mut c_void,
    _ctx: *mut wasmz_ctx_t,
    _params: *const WasmzVal,
    _param_count: usize,
    _results: *mut WasmzVal,
    _result_count: usize,
) -> c_int {
    0
}

fn err_msg(err: *mut wasmz_error_t) -> String {
    if err.is_null() {
        return "(no error)".to_string();
    }
    let p = unsafe { wasmz_error_message(err) };
    let s = if p.is_null() {
        "(no error message)".to_string()
    } else {
        unsafe { std::ffi::CStr::from_ptr(p) }
            .to_string_lossy()
            .into_owned()
    };
    unsafe { wasmz_error_delete(err) };
    s
}

static INIT: Once = Once::new();

pub fn init() -> Result<()> {
    INIT.call_once(|| {});
    Ok(())
}

pub fn run_workload_wasmz_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    // Same workaround as the zwasm adapter: Swift dispatch workers
    // get a 272 KiB stack, and the wasmz parse/load path easily
    // overflows that. iOS/tvOS pthread defaults are similarly tight.
    // Route the whole runner through a dedicated thread with an 8 MiB
    // stack so the load + call paths have headroom matching what the
    // main thread (the standalone C test environment) provides.
    let wasm_bytes_owned = wasm_bytes.to_vec();
    let fn_name_owned = fn_name.to_string();
    std::thread::Builder::new()
        .name("wasmz-runner".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || run_workload_wasmz_iters_inner(&wasm_bytes_owned, &fn_name_owned, arg, iters))
        .context("wasmz: failed to spawn dedicated 8 MiB-stack thread")?
        .join()
        .map_err(|_| anyhow!("wasmz-runner thread panicked"))?
}

fn run_workload_wasmz_iters_inner(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    init()?;

    let load_start = Instant::now();

    // Engine. wasmz uses libc allocators internally; the engine is
    // an opaque heap-allocated handle.
    let engine = unsafe { wasmz_engine_new() };
    if engine.is_null() {
        return Err(anyhow!("wasmz_engine_new returned NULL"));
    }
    struct EngGuard(*mut wasmz_engine_t);
    impl Drop for EngGuard {
        fn drop(&mut self) {
            unsafe { wasmz_engine_delete(self.0) };
        }
    }
    let _eg = EngGuard(engine);

    let store = unsafe { wasmz_store_new(engine) };
    if store.is_null() {
        return Err(anyhow!("wasmz_store_new returned NULL"));
    }
    struct StoreGuard(*mut wasmz_store_t);
    impl Drop for StoreGuard {
        fn drop(&mut self) {
            unsafe { wasmz_store_delete(self.0) };
        }
    }
    let _sg = StoreGuard(store);

    let mut module: *mut wasmz_module_t = std::ptr::null_mut();
    let err = unsafe {
        wasmz_module_new(engine, wasm_bytes.as_ptr(), wasm_bytes.len(), &mut module)
    };
    if !err.is_null() {
        return Err(anyhow!("wasmz_module_new failed: {}", err_msg(err)));
    }
    if module.is_null() {
        return Err(anyhow!("wasmz_module_new: module pointer is NULL"));
    }
    struct ModGuard(*mut wasmz_module_t);
    impl Drop for ModGuard {
        fn drop(&mut self) {
            unsafe { wasmz_module_delete(self.0) };
        }
    }
    let _mg = ModGuard(module);

    let mut instance: *mut wasmz_instance_t = std::ptr::null_mut();
    let err = unsafe { wasmz_instance_new(store, module, &mut instance) };
    if !err.is_null() {
        return Err(anyhow!("wasmz_instance_new failed: {}", err_msg(err)));
    }
    if instance.is_null() {
        return Err(anyhow!("wasmz_instance_new: instance pointer is NULL"));
    }
    struct InstGuard(*mut wasmz_instance_t);
    impl Drop for InstGuard {
        fn drop(&mut self) {
            unsafe { wasmz_instance_delete(self.0) };
        }
    }
    let _ig = InstGuard(instance);

    let cname = std::ffi::CString::new(fn_name)?;

    let load_time = load_start.elapsed();

    let call = |arg: i32| -> Result<i32> {
        let args: [WasmzVal; 1] = [WasmzVal::i32_val(arg)];
        // Result kind matters — the FFI layer keys off `results[0].kind`
        // (it writes the right union variant based on the caller-supplied
        // kind). Pre-fill with WASMZ_VAL_I32 so the i32 branch fires.
        let mut results: [WasmzVal; 1] = [WasmzVal::i32_val(0)];
        let err = unsafe {
            wasmz_instance_call(
                instance,
                cname.as_ptr(),
                args.as_ptr(),
                1,
                results.as_mut_ptr(),
                1,
            )
        };
        if !err.is_null() {
            return Err(anyhow!("wasmz_instance_call trap: {}", err_msg(err)));
        }
        Ok(results[0].as_i32())
    };

    // Two warmups — same rationale as the other adapters.
    let mut result = call(arg).context("wasmz init-warmup failed")?;
    let warm_start = Instant::now();
    result = call(arg).context("wasmz steady-warmup failed")?;
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

pub fn run_workload_wasmz(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_wasmz_iters(wasm_bytes, fn_name, arg, 0)
}

/// Dedicated Porffor-graphql runner. Wires the `("", "b") : (f64) → ()`
/// host print stub via wasmz's linker, then calls `m() → (f64, i32)`
/// (multi-value). Same shape as wamr / wasmedge / zwasm dedicated
/// runners.
pub fn run_graphql_validation_porf_wasmz(wasm_bytes: &[u8]) -> Result<RunReport> {
    let wasm_bytes_owned = wasm_bytes.to_vec();
    std::thread::Builder::new()
        .name("wasmz-porf".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || run_graphql_validation_porf_wasmz_inner(&wasm_bytes_owned))
        .context("wasmz: failed to spawn 8 MiB-stack thread")?
        .join()
        .map_err(|_| anyhow!("wasmz-porf thread panicked"))?
}

fn run_graphql_validation_porf_wasmz_inner(wasm_bytes: &[u8]) -> Result<RunReport> {
    init()?;

    let load_start = Instant::now();

    let engine = unsafe { wasmz_engine_new() };
    if engine.is_null() {
        return Err(anyhow!("wasmz_engine_new returned NULL"));
    }
    struct EngGuard(*mut wasmz_engine_t);
    impl Drop for EngGuard {
        fn drop(&mut self) {
            unsafe { wasmz_engine_delete(self.0) };
        }
    }
    let _eg = EngGuard(engine);

    let store = unsafe { wasmz_store_new(engine) };
    if store.is_null() {
        return Err(anyhow!("wasmz_store_new returned NULL"));
    }
    struct StoreGuard(*mut wasmz_store_t);
    impl Drop for StoreGuard {
        fn drop(&mut self) {
            unsafe { wasmz_store_delete(self.0) };
        }
    }
    let _sg = StoreGuard(store);

    let linker = unsafe { wasmz_linker_new() };
    if linker.is_null() {
        return Err(anyhow!("wasmz_linker_new returned NULL"));
    }
    struct LinkGuard(*mut wasmz_linker_t);
    impl Drop for LinkGuard {
        fn drop(&mut self) {
            unsafe { wasmz_linker_delete(self.0) };
        }
    }
    let _lg = LinkGuard(linker);

    // WASMZ_VAL_F64 = 3 per wasmz.h.
    let param_kinds: [c_int; 1] = [3];
    let modname = b"\0".as_ptr() as *const c_char;
    let funcname = b"b\0".as_ptr() as *const c_char;
    let link_err = unsafe {
        wasmz_linker_define_func(
            linker,
            modname,
            funcname,
            param_kinds.as_ptr(),
            1,
            std::ptr::null(),
            0,
            wasmz_porf_b,
            std::ptr::null_mut(),
        )
    };
    if !link_err.is_null() {
        return Err(anyhow!("wasmz_linker_define_func failed: {}", err_msg(link_err)));
    }

    let mut module: *mut wasmz_module_t = std::ptr::null_mut();
    let err = unsafe {
        wasmz_module_new(engine, wasm_bytes.as_ptr(), wasm_bytes.len(), &mut module)
    };
    if !err.is_null() {
        return Err(anyhow!("wasmz_module_new failed: {}", err_msg(err)));
    }
    struct ModGuard(*mut wasmz_module_t);
    impl Drop for ModGuard {
        fn drop(&mut self) {
            unsafe { wasmz_module_delete(self.0) };
        }
    }
    let _mg = ModGuard(module);

    let mut instance: *mut wasmz_instance_t = std::ptr::null_mut();
    let err = unsafe {
        wasmz_instance_new_with_linker(store, module, linker, &mut instance)
    };
    if !err.is_null() {
        return Err(anyhow!(
            "wasmz_instance_new_with_linker failed: {}",
            err_msg(err)
        ));
    }
    struct InstGuard(*mut wasmz_instance_t);
    impl Drop for InstGuard {
        fn drop(&mut self) {
            unsafe { wasmz_instance_delete(self.0) };
        }
    }
    let _ig = InstGuard(instance);

    let cname = std::ffi::CString::new("m")?;

    let load_time = load_start.elapsed();

    // m() → (f64, i32). 0 params, 2 results.
    let call = || -> Result<i32> {
        let mut results: [WasmzVal; 2] = [WasmzVal { kind: WASMZ_VAL_I32, _pad: [0; 4], of: [0; 16] }; 2];
        // Pre-fill result kinds so the underlying call knows how to
        // marshal them. Per wasmz.h convention.
        results[0].kind = 3; // F64
        results[1].kind = WASMZ_VAL_I32;
        let err = unsafe {
            wasmz_instance_call(
                instance,
                cname.as_ptr(),
                std::ptr::null(),
                0,
                results.as_mut_ptr(),
                2,
            )
        };
        if !err.is_null() {
            return Err(anyhow!("wasmz m() trap: {}", err_msg(err)));
        }
        Ok(results[1].as_i32())
    };

    let mut result = call().context("wasmz porf init-warmup failed")?;
    let warm_start = Instant::now();
    result = call().context("wasmz porf steady-warmup failed")?;
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
