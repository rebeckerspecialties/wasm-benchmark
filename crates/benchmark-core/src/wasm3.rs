//! Hand-written FFI bindings + thin runner for wasm3 — a pure C
//! interpreter (no JIT, no AOT, no MAP_JIT — App-Store-eligible the
//! same way Pulley and WAMR are).
//!
//! Built and linked by [`build.rs`] from
//! `wasm3/build-<triple>/libm3.a` (produced by
//! `scripts/build-wasm3.sh`). Only the core 11 sources are compiled in —
//! WASI / uvwasi / meta-wasi / tracer are omitted, since our workloads
//! are wasm32-unknown-unknown and don't use them; including them would
//! pull unresolved syscall symbols into the app link.
//!
//! Feature coverage:
//! * `return_call` / `return_call_indirect` — yes (compiled in via
//!   `m3_compile.c`'s op table).
//! * SIMD (`v128`) — **no**. `matmul_simd` and `matmul_fma` load-fail
//!   on wasm3; the runner surfaces the wasm3 error string and the
//!   workload row reports ERROR. Treat as data.
//! * Wasm exceptions — no. `graphql-validation (Porffor)` load-fails.
//! * WASI — not linked. `sqlite3.wasm` load-fails (missing imports).

use std::os::raw::{c_char, c_uint, c_void};
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::{taskinfo, RunReport};

// Opaque wasm3 types. The headers declare them as `struct M3* / IM3*`
// but we only ever pass them around as pointers.
#[allow(non_camel_case_types)]
type IM3Environment = *mut c_void;
#[allow(non_camel_case_types)]
type IM3Runtime = *mut c_void;
#[allow(non_camel_case_types)]
type IM3Module = *mut c_void;
#[allow(non_camel_case_types)]
type IM3Function = *mut c_void;
#[allow(non_camel_case_types)]
type M3Result = *const c_char;

extern "C" {
    fn m3_NewEnvironment() -> IM3Environment;
    fn m3_FreeEnvironment(env: IM3Environment);
    fn m3_NewRuntime(env: IM3Environment, stack_bytes: c_uint, userdata: *mut c_void) -> IM3Runtime;
    fn m3_FreeRuntime(runtime: IM3Runtime);
    fn m3_ParseModule(
        env: IM3Environment,
        out_module: *mut IM3Module,
        wasm_bytes: *const u8,
        n_bytes: c_uint,
    ) -> M3Result;
    fn m3_FreeModule(module: IM3Module);
    fn m3_LoadModule(runtime: IM3Runtime, module: IM3Module) -> M3Result;
    fn m3_FindFunction(
        out_function: *mut IM3Function,
        runtime: IM3Runtime,
        function_name: *const c_char,
    ) -> M3Result;
    fn m3_Call(
        function: IM3Function,
        argc: c_uint,
        argv: *const *const c_void,
    ) -> M3Result;
    fn m3_GetResults(
        function: IM3Function,
        retc: c_uint,
        retptrs: *const *mut c_void,
    ) -> M3Result;
}

// wasm3 returns errors as static C string pointers; NULL = success.
fn m3_ok(r: M3Result) -> Result<(), String> {
    if r.is_null() {
        Ok(())
    } else {
        let msg = unsafe { std::ffi::CStr::from_ptr(r) }
            .to_string_lossy()
            .into_owned();
        Err(msg)
    }
}

static INIT: Once = Once::new();
static mut INIT_OK: bool = false;

/// One-shot wasm3 init. Currently a no-op (wasm3 has no
/// process-global state to set up — unlike WAMR's stack-guard pages),
/// but we keep the same `init() -> Result<()>` shape so the Swift app
/// can call it symmetrically.
pub fn init() -> Result<()> {
    INIT.call_once(|| {
        unsafe {
            INIT_OK = true;
        }
    });
    Ok(())
}

/// Default stack budget for a wasm3 runtime. wasm3 splits this between
/// its operand stack and the call stack — 256 KiB is comfortably above
/// what our deepest workload (xmrsplayer's per-tick effect pipeline)
/// has needed in practice.
const WASM3_STACK_BYTES: c_uint = 256 * 1024;

pub fn run_workload_wasm3_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    init()?;

    let load_start = Instant::now();

    let env: IM3Environment = unsafe { m3_NewEnvironment() };
    if env.is_null() {
        return Err(anyhow!("m3_NewEnvironment returned NULL"));
    }
    // Owns env across the call; freed on every exit path below.
    struct EnvGuard(IM3Environment);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe { m3_FreeEnvironment(self.0) };
        }
    }
    let _env_guard = EnvGuard(env);

    let runtime: IM3Runtime =
        unsafe { m3_NewRuntime(env, WASM3_STACK_BYTES, std::ptr::null_mut()) };
    if runtime.is_null() {
        return Err(anyhow!("m3_NewRuntime returned NULL"));
    }
    struct RuntimeGuard(IM3Runtime);
    impl Drop for RuntimeGuard {
        fn drop(&mut self) {
            unsafe { m3_FreeRuntime(self.0) };
        }
    }
    let _runtime_guard = RuntimeGuard(runtime);

    // wasm3 requires the wasm bytes to outlive the module — see the
    // comment on `m3_ParseModule` in wasm3.h. We hand it our owned copy
    // and keep it alive for the rest of this function.
    let bytes_owned = wasm_bytes.to_vec();
    let mut module: IM3Module = std::ptr::null_mut();
    m3_ok(unsafe {
        m3_ParseModule(
            env,
            &mut module as *mut IM3Module,
            bytes_owned.as_ptr(),
            bytes_owned.len() as c_uint,
        )
    })
    .map_err(|e| anyhow!("wasm3 m3_ParseModule failed: {e}"))?;
    if module.is_null() {
        return Err(anyhow!("m3_ParseModule returned NULL module"));
    }

    // m3_LoadModule transfers ownership of `module` to the runtime on
    // success, so we only free it ourselves if the load fails.
    if let Err(e) = m3_ok(unsafe { m3_LoadModule(runtime, module) }) {
        unsafe { m3_FreeModule(module) };
        return Err(anyhow!("wasm3 m3_LoadModule failed: {e}"));
    }

    let cname = std::ffi::CString::new(fn_name)?;
    let mut func: IM3Function = std::ptr::null_mut();
    m3_ok(unsafe {
        m3_FindFunction(&mut func as *mut IM3Function, runtime, cname.as_ptr())
    })
    .map_err(|e| anyhow!("wasm3 m3_FindFunction({fn_name}) failed: {e}"))?;
    if func.is_null() {
        return Err(anyhow!("export `{fn_name}` not found"));
    }

    let load_time = load_start.elapsed();

    // wasm3's m3_Call takes argv as an array of `const void *`, each
    // pointing at the actual argument. For a single i32 arg the
    // argv pattern is `[&arg]`.
    let call = |arg: i32| -> Result<i32> {
        let arg_owned: i32 = arg;
        let argv: [*const c_void; 1] = [&arg_owned as *const i32 as *const c_void];
        m3_ok(unsafe { m3_Call(func, 1, argv.as_ptr()) })
            .map_err(|e| anyhow!("wasm3 m3_Call trap: {e}"))?;
        let mut ret: i32 = 0;
        let retptrs: [*mut c_void; 1] = [&mut ret as *mut i32 as *mut c_void];
        m3_ok(unsafe { m3_GetResults(func, 1, retptrs.as_ptr()) })
            .map_err(|e| anyhow!("wasm3 m3_GetResults failed: {e}"))?;
        Ok(ret)
    };

    // Two warmups — same rationale as the Pulley / WAMR runners.
    let mut result = call(arg).context("wasm3 init-warmup call failed")?;
    let warm_start = Instant::now();
    result = call(arg).context("wasm3 steady-warmup call failed")?;
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

    // bytes_owned + module/runtime/env are released by the guards
    // dropping in reverse order at end of scope.
    let _ = bytes_owned;

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

pub fn run_workload_wasm3(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_wasm3_iters(wasm_bytes, fn_name, arg, 0)
}
