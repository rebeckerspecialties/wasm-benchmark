//! Hand-written FFI bindings + thin runner for WasmEdge — the incumbent
//! production runtime for the user's WatchOS audio app. Pure-interpreter
//! mode (`WASMEDGE_USE_LLVM=OFF` at cmake time + a 24-patch Apple-mobile
//! enablement stack, see `patches/wasmedge/`); no JIT / AOT, so it is
//! App-Store-eligible the same way Pulley / WAMR / wasm3 are.
//!
//! Linked from `WasmEdge/build-<triple>/libwasmedge.a` (produced by
//! `scripts/build-wasmedge.sh`). Patches handle the Apple-mobile memory
//! guard fallbacks needed when the default 4-GiB-guard allocator can't
//! reserve enough address space on iOS/watchOS/tvOS, plus a stack of
//! interpreter super-instructions that close some of WasmEdge's gap
//! vs Pulley/WAMR on dense-dispatch workloads.

use std::os::raw::{c_char, c_uint, c_void};
use std::sync::Once;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::{taskinfo, RunReport};

// All WasmEdge contexts are opaque pointers from the caller's POV.
#[allow(non_camel_case_types)]
type WasmEdgeConfigureContext = c_void;
#[allow(non_camel_case_types)]
type WasmEdgeVMContext = c_void;
#[allow(non_camel_case_types)]
type WasmEdgeStoreContext = c_void;

// Mirrors the C structs in wasmedge_basic.h. Layout is documented and
// stable in the C header; we keep field order verbatim. `#[repr(C)]` is
// the binding contract.
#[repr(C)]
#[derive(Clone, Copy)]
struct WasmEdgeString {
    length: u32,
    buf: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WasmEdgeBytes {
    length: u32,
    buf: *const u8,
}

// `WasmEdge_Result { uint32_t Code; }` — single 4-byte field. The
// helper accessors `WasmEdge_ResultOK` / `WasmEdge_ResultGetMessage`
// take this by value.
#[repr(C)]
#[derive(Clone, Copy)]
struct WasmEdgeResult {
    code: u32,
}

// `WasmEdge_Value { uint128_t Value; uint8_t Data[8] Type; }`. We never
// poke at the fields from Rust — we construct via `WasmEdge_ValueGenI32`
// and read back via `WasmEdge_ValueGetI32`. We just need an
// ABI-compatible POD here so the FFI signatures line up. uint128_t is
// `__int128` on aarch64 clang (16-byte aligned, 16 bytes), so the
// whole struct is 24 bytes / 16-aligned.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct WasmEdgeValue {
    _bytes: [u8; 24],
}

// `WasmEdge_Proposal` enum values (subset we want enabled). Matches the
// `P(...)` entries in `WasmEdge/include/api/wasmedge/enum.inc`:
//   0  ImportExportMutGlobals
//   1  NonTrapFloatToIntConversions
//   2  SignExtensionOperators
//   3  MultiValue
//   4  BulkMemoryOperations
//   5  ReferenceTypes
//   6  SIMD
//   7  TailCall
//   8  ExtendedConst
//   9  FunctionReferences
//  10  GC
//  11  MultiMemories
//  12  RelaxSIMD
//  13  Annotations
//  14  ExceptionHandling
//  15  Memory64
//  16  Threads
//  17  Component
const PROP_MULTI_VALUE: u32 = 3;
const PROP_BULK_MEMORY: u32 = 4;
const PROP_REFERENCE_TYPES: u32 = 5;
const PROP_SIMD: u32 = 6;
const PROP_TAIL_CALL: u32 = 7;
const PROP_EXTENDED_CONST: u32 = 8;
const PROP_RELAX_SIMD: u32 = 12;
const PROP_EXCEPTION_HANDLING: u32 = 14;

extern "C" {
    fn WasmEdge_ConfigureCreate() -> *mut WasmEdgeConfigureContext;
    fn WasmEdge_ConfigureDelete(cxt: *mut WasmEdgeConfigureContext);
    fn WasmEdge_ConfigureAddProposal(cxt: *mut WasmEdgeConfigureContext, prop: u32);

    fn WasmEdge_VMCreate(
        conf: *const WasmEdgeConfigureContext,
        store: *mut WasmEdgeStoreContext,
    ) -> *mut WasmEdgeVMContext;
    fn WasmEdge_VMDelete(vm: *mut WasmEdgeVMContext);
    fn WasmEdge_VMLoadWasmFromBytes(
        vm: *mut WasmEdgeVMContext,
        bytes: WasmEdgeBytes,
    ) -> WasmEdgeResult;
    fn WasmEdge_VMValidate(vm: *mut WasmEdgeVMContext) -> WasmEdgeResult;
    fn WasmEdge_VMInstantiate(vm: *mut WasmEdgeVMContext) -> WasmEdgeResult;
    fn WasmEdge_VMExecute(
        vm: *mut WasmEdgeVMContext,
        func_name: WasmEdgeString,
        params: *const WasmEdgeValue,
        param_len: u32,
        returns: *mut WasmEdgeValue,
        return_len: u32,
    ) -> WasmEdgeResult;

    fn WasmEdge_BytesCreate(buf: *const u8, len: u32) -> WasmEdgeBytes;
    fn WasmEdge_StringCreateByCString(s: *const c_char) -> WasmEdgeString;
    fn WasmEdge_StringDelete(s: WasmEdgeString);

    fn WasmEdge_ResultOK(r: WasmEdgeResult) -> bool;
    fn WasmEdge_ResultGetMessage(r: WasmEdgeResult) -> *const c_char;

    fn WasmEdge_ValueGenI32(val: i32) -> WasmEdgeValue;
    fn WasmEdge_ValueGetI32(val: WasmEdgeValue) -> i32;
}

fn err_from(r: WasmEdgeResult, ctx: &'static str) -> Result<()> {
    if unsafe { WasmEdge_ResultOK(r) } {
        Ok(())
    } else {
        let p = unsafe { WasmEdge_ResultGetMessage(r) };
        let msg = if p.is_null() {
            "(null error message)".to_string()
        } else {
            unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned()
        };
        Err(anyhow!("WasmEdge {ctx}: {msg}"))
    }
}

static INIT: Once = Once::new();

/// WasmEdge has no `wasm_runtime_init()`-equivalent — its global state
/// is lazily set up on first `WasmEdge_VMCreate`. We expose `init()`
/// only so callers can mirror the WAMR/wasm3 init shape.
pub fn init() -> Result<()> {
    INIT.call_once(|| {});
    Ok(())
}

/// Run `fn_name(arg: i32) -> i32` on `wasm_bytes` via WasmEdge's
/// pure-interpreter VM. Mirrors `wamr::run_workload_wamr_iters` —
/// two warmup calls, auto-tuned iter budget (or honor `iters`), Apple
/// task-info CPU/RSS/page-fault deltas around the measurement loop.
pub fn run_workload_wasmedge_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    // arm64_32-apple-watchos: WasmEdge_VMInstantiate consistently SIGTRAPs
    // on Watch SE2 S8 hardware. Traced via per-step `eprintln!` markers
    // in this adapter: ConfigureCreate ✓, VMCreate ✓, LoadWasmFromBytes
    // ✓, Validate ✓, Instantiate → BRK (SIGTRAP). Likely an
    // `assuming(x)` in lib/executor/instantiate/* that's false on the
    // 32-bit ABI — `assuming()` in NDEBUG builds is
    // `x ? : __builtin_unreachable()`, which clang/arm64_32 compiles to
    // a brk. The Apple-mobile guarded allocator path is already
    // disabled on arm64_32 via patches/wasmedge/0028, but the trap is
    // beyond the allocator — somewhere in module instantiation
    // pointer math. Returning a clean error here is what keeps the
    // rest of the watch benchmark suite from aborting; a follow-up
    // WasmEdge patch is required to actually run workloads on
    // arm64_32-apple-watchos.
    #[cfg(all(target_vendor = "apple", target_os = "watchos", not(target_pointer_width = "64")))]
    {
        let _ = (wasm_bytes, fn_name, arg, iters);
        return Err(anyhow!(
            "WasmEdge VMInstantiate SIGTRAPs on arm64_32-apple-watchos \
             (assuming(x) UB in instantiate path beyond the allocator); \
             needs a follow-up patch — see crates/benchmark-core/src/wasmedge.rs"
        ));
    }
    #[cfg(not(all(target_vendor = "apple", target_os = "watchos", not(target_pointer_width = "64"))))]
    {
        run_workload_wasmedge_iters_inner(wasm_bytes, fn_name, arg, iters)
    }
}

fn run_workload_wasmedge_iters_inner(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    init()?;

    let load_start = Instant::now();

    let conf = unsafe { WasmEdge_ConfigureCreate() };
    if conf.is_null() {
        return Err(anyhow!("WasmEdge_ConfigureCreate returned NULL"));
    }
    struct ConfGuard(*mut WasmEdgeConfigureContext);
    impl Drop for ConfGuard {
        fn drop(&mut self) {
            unsafe { WasmEdge_ConfigureDelete(self.0) };
        }
    }
    let _conf_g = ConfGuard(conf);

    // Enable every wasm proposal the workload set needs. The interpreter
    // supports all of these out of the box — unlike WAMR, exceptions
    // + SIMD coexist here, so Porffor's graphql-validation should run
    // (subject to host-import availability, same as Pulley).
    for prop in [
        PROP_MULTI_VALUE,
        PROP_BULK_MEMORY,
        PROP_REFERENCE_TYPES,
        PROP_SIMD,
        PROP_TAIL_CALL,
        PROP_EXTENDED_CONST,
        PROP_RELAX_SIMD,
        PROP_EXCEPTION_HANDLING,
    ] {
        unsafe { WasmEdge_ConfigureAddProposal(conf, prop) };
    }

    let vm = unsafe { WasmEdge_VMCreate(conf, std::ptr::null_mut()) };
    if vm.is_null() {
        return Err(anyhow!("WasmEdge_VMCreate returned NULL"));
    }
    struct VmGuard(*mut WasmEdgeVMContext);
    impl Drop for VmGuard {
        fn drop(&mut self) {
            unsafe { WasmEdge_VMDelete(self.0) };
        }
    }
    let _vm_g = VmGuard(vm);

    // Own the bytes for the VM's lifetime — `BytesCreate` wraps the
    // buffer without copying. The Rust side keeps `bytes_owned` alive
    // until the function returns.
    let bytes_owned = wasm_bytes.to_vec();
    let bytes = unsafe {
        WasmEdge_BytesCreate(bytes_owned.as_ptr(), bytes_owned.len() as u32)
    };

    err_from(
        unsafe { WasmEdge_VMLoadWasmFromBytes(vm, bytes) },
        "VMLoadWasmFromBytes",
    )?;
    err_from(unsafe { WasmEdge_VMValidate(vm) }, "VMValidate")?;
    err_from(unsafe { WasmEdge_VMInstantiate(vm) }, "VMInstantiate")?;

    let cname = std::ffi::CString::new(fn_name)?;
    let func_name = unsafe { WasmEdge_StringCreateByCString(cname.as_ptr()) };
    // StringCreateByCString copies, so we own the WasmEdge_String. Free
    // at end of scope.
    struct StringGuard(WasmEdgeString);
    impl Drop for StringGuard {
        fn drop(&mut self) {
            unsafe { WasmEdge_StringDelete(self.0) };
        }
    }
    let _name_g = StringGuard(func_name);

    let load_time = load_start.elapsed();

    let call = |arg: i32| -> Result<i32> {
        let params: [WasmEdgeValue; 1] = [unsafe { WasmEdge_ValueGenI32(arg) }];
        let mut returns: [WasmEdgeValue; 1] = [WasmEdgeValue { _bytes: [0u8; 24] }];
        err_from(
            unsafe {
                WasmEdge_VMExecute(
                    vm,
                    func_name,
                    params.as_ptr(),
                    1,
                    returns.as_mut_ptr(),
                    1,
                )
            },
            "VMExecute",
        )?;
        Ok(unsafe { WasmEdge_ValueGetI32(returns[0]) })
    };

    // Two warmups — same rationale as the Pulley / WAMR / wasm3 runners.
    let mut result = call(arg).context("WasmEdge init-warmup failed")?;
    let warm_start = Instant::now();
    result = call(arg).context("WasmEdge steady-warmup failed")?;
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

    let _ = bytes_owned;
    let _ = c_uint::default; // keep `c_uint` import live without warning

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

pub fn run_workload_wasmedge(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
) -> Result<RunReport> {
    run_workload_wasmedge_iters(wasm_bytes, fn_name, arg, 0)
}
