//! Pure-interpreter wasmtime+Pulley wrapper for the wasm-benchmark project.
//!
//! Two faces:
//!  - A Rust API used by the local CLI (`runwasm`) for fast iteration on
//!    macOS hosts.
//!  - A C-ABI surface that the SwiftUI watchOS / iOS / macOS app links into
//!    via `crates/benchmark-core/include/benchmark_core.h`.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use wasmtime::{Engine, Instance, Module, Store};

pub mod graphql_validation;
pub mod pac_probe;
pub mod sqlite3;

#[cfg(have_wamr)]
pub mod wamr;
// On targets where build.rs couldn't find libiwasm.a, expose a stub so
// callers can still mention `wamr::run_workload_wamr` by symbol — they
// just get a runtime error if they actually invoke it. Lets the iOS /
// watchOS device builds compile before WAMR has been cross-built.
#[cfg(not(have_wamr))]
pub mod wamr {
    use crate::RunReport;
    use anyhow::{anyhow, Result};
    pub fn init() -> Result<()> {
        Err(anyhow!("WAMR not linked into this build"))
    }
    pub fn run_workload_wamr(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
    ) -> Result<RunReport> {
        Err(anyhow!("WAMR not linked into this build"))
    }
    pub fn run_workload_wamr_iters(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
        _iters: u32,
    ) -> Result<RunReport> {
        Err(anyhow!("WAMR not linked into this build"))
    }
    pub fn run_graphql_validation_porf_wamr(_wasm_bytes: &[u8]) -> Result<RunReport> {
        Err(anyhow!("WAMR not linked into this build"))
    }
}

#[cfg(have_zwasm)]
pub mod zwasm;
#[cfg(not(have_zwasm))]
pub mod zwasm {
    use crate::RunReport;
    use anyhow::{anyhow, Result};
    pub fn init() -> Result<()> {
        Err(anyhow!("zwasm not linked into this build"))
    }
    pub fn run_workload_zwasm(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
    ) -> Result<RunReport> {
        Err(anyhow!("zwasm not linked into this build"))
    }
    pub fn run_workload_zwasm_iters(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
        _iters: u32,
    ) -> Result<RunReport> {
        Err(anyhow!("zwasm not linked into this build"))
    }
}

#[cfg(have_wasmz)]
pub mod wasmz;
#[cfg(not(have_wasmz))]
pub mod wasmz {
    use crate::RunReport;
    use anyhow::{anyhow, Result};
    pub fn init() -> Result<()> {
        Err(anyhow!("wasmz not linked into this build"))
    }
    pub fn run_workload_wasmz(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
    ) -> Result<RunReport> {
        Err(anyhow!("wasmz not linked into this build"))
    }
    pub fn run_workload_wasmz_iters(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
        _iters: u32,
    ) -> Result<RunReport> {
        Err(anyhow!("wasmz not linked into this build"))
    }
}

#[cfg(have_wasmedge)]
pub mod wasmedge;
#[cfg(not(have_wasmedge))]
pub mod wasmedge {
    use crate::RunReport;
    use anyhow::{anyhow, Result};
    pub fn init() -> Result<()> {
        Err(anyhow!("WasmEdge not linked into this build"))
    }
    pub fn run_workload_wasmedge(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
    ) -> Result<RunReport> {
        Err(anyhow!("WasmEdge not linked into this build"))
    }
    pub fn run_workload_wasmedge_iters(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
        _iters: u32,
    ) -> Result<RunReport> {
        Err(anyhow!("WasmEdge not linked into this build"))
    }
}

#[cfg(have_wasm3)]
pub mod wasm3;
// Stub for targets without libm3.a — same shape as the WAMR stub above.
#[cfg(not(have_wasm3))]
pub mod wasm3 {
    use crate::RunReport;
    use anyhow::{anyhow, Result};
    pub fn init() -> Result<()> {
        Err(anyhow!("wasm3 not linked into this build"))
    }
    pub fn run_workload_wasm3(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
    ) -> Result<RunReport> {
        Err(anyhow!("wasm3 not linked into this build"))
    }
    pub fn run_workload_wasm3_iters(
        _wasm_bytes: &[u8],
        _fn_name: &str,
        _arg: i32,
        _iters: u32,
    ) -> Result<RunReport> {
        Err(anyhow!("wasm3 not linked into this build"))
    }
}

/// Which interpreter runtime to dispatch a benchmark workload through.
///
/// All are pure-interpreter (no JIT / AOT / MAP_JIT) and so are
/// App-Store-eligible on iOS / watchOS / tvOS. `Pulley` is wasmtime's
/// portable interpreter; `Wamr` is the WAMR fast interpreter
/// (preprocessed bytecode mode); `Wasm3` is the wasm3 m3 interpreter
/// (C, used as a 3rd cross-runtime data point — missing SIMD, so the
/// matmul / Porffor / xmrsplayer rows return ERROR; that's signal).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Pulley = 0,
    Wamr = 1,
    Wasm3 = 2,
    WasmEdge = 3,
    Zwasm = 4,
    Wasmz = 5,
}

// ---- Apple `task_info` thin wrapper for CPU time / RSS / page faults ----
//
// The benchmarks run inside the app's main mach task, so a snapshot of
// task_info before and after each measurement loop gives us a useful
// proxy for what hardware-PMU sampling would otherwise show: CPU time
// in user vs system bands, peak resident set size, and page-fault counts.
// All of this is callable from any thread on iOS / watchOS / macOS
// without entitlements.
#[cfg(target_vendor = "apple")]
pub(crate) mod taskinfo {
    use std::os::raw::{c_int, c_uint};

    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    pub struct TimeValue {
        pub seconds: c_int,
        pub microseconds: c_int,
    }
    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    pub struct ThreadTimes {
        pub user_time: TimeValue,
        pub system_time: TimeValue,
    }
    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    pub struct MachTaskBasicInfo {
        pub virtual_size: u64,
        pub resident_size: u64,
        pub resident_size_max: u64,
        pub user_time: TimeValue,
        pub system_time: TimeValue,
        pub policy: c_int,
        pub suspend_count: c_int,
    }
    #[repr(C)]
    #[derive(Default, Copy, Clone)]
    pub struct TaskEventsInfo {
        pub faults: i32,
        pub pageins: i32,
        pub cow_faults: i32,
        pub messages_sent: i32,
        pub messages_received: i32,
        pub syscalls_mach: i32,
        pub syscalls_unix: i32,
        pub csw: i32,
    }

    pub const TASK_THREAD_TIMES_INFO: c_uint = 3;
    pub const MACH_TASK_BASIC_INFO: c_uint = 20;
    pub const TASK_EVENTS_INFO: c_uint = 2;

    extern "C" {
        pub fn mach_task_self() -> c_uint;
        pub fn task_info(
            target_task: c_uint,
            flavor: c_uint,
            task_info_out: *mut c_int,
            task_info_count: *mut c_uint,
        ) -> c_int;
    }

    pub fn time_value_to_ns(t: TimeValue) -> u64 {
        (t.seconds as u64) * 1_000_000_000 + (t.microseconds as u64) * 1_000
    }

    pub fn thread_times() -> Option<ThreadTimes> {
        unsafe {
            let mut info = ThreadTimes::default();
            let mut cnt = (core::mem::size_of::<ThreadTimes>() / 4) as c_uint;
            if task_info(
                mach_task_self(),
                TASK_THREAD_TIMES_INFO,
                &mut info as *mut _ as *mut c_int,
                &mut cnt,
            ) == 0
            {
                Some(info)
            } else {
                None
            }
        }
    }
    pub fn basic_info() -> Option<MachTaskBasicInfo> {
        unsafe {
            let mut info = MachTaskBasicInfo::default();
            let mut cnt = (core::mem::size_of::<MachTaskBasicInfo>() / 4) as c_uint;
            if task_info(
                mach_task_self(),
                MACH_TASK_BASIC_INFO,
                &mut info as *mut _ as *mut c_int,
                &mut cnt,
            ) == 0
            {
                Some(info)
            } else {
                None
            }
        }
    }
    pub fn events_info() -> Option<TaskEventsInfo> {
        unsafe {
            let mut info = TaskEventsInfo::default();
            let mut cnt = (core::mem::size_of::<TaskEventsInfo>() / 4) as c_uint;
            if task_info(
                mach_task_self(),
                TASK_EVENTS_INFO,
                &mut info as *mut _ as *mut c_int,
                &mut cnt,
            ) == 0
            {
                Some(info)
            } else {
                None
            }
        }
    }
}

#[cfg(not(target_vendor = "apple"))]
pub(crate) mod taskinfo {
    pub struct ThreadTimes;
    pub struct MachTaskBasicInfo {
        pub resident_size_max: u64,
    }
    pub struct TaskEventsInfo {
        pub faults: i32,
    }
    pub fn thread_times() -> Option<ThreadTimes> { None }
    pub fn basic_info() -> Option<MachTaskBasicInfo> { None }
    pub fn events_info() -> Option<TaskEventsInfo> { None }
    pub fn time_value_to_ns(_: ()) -> u64 { 0 }
}

// `wasmtime::Error` does not implement `std::error::Error`; the conversion
// is gated on wasmtime's `anyhow` feature, which we enable. This adapter
// makes call sites read cleanly with `.with_context()`.
pub(crate) fn into_anyhow<T>(r: Result<T, wasmtime::Error>) -> Result<T> {
    r.map_err(anyhow::Error::from)
}

// -----------------------------------------------------------------------------
// Embedded workloads. Each .wasm is produced by `scripts/build-workloads.sh`
// from the matching `workloads-rs/*.rs` source file.
// -----------------------------------------------------------------------------

pub const FIB_WASM: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../workloads/fib.wasm"));
pub const FIB_TAIL_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/fib_tail.wasm"
));
pub const FACTORIAL_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/factorial.wasm"
));
pub const SIEVE_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/sieve.wasm"
));
pub const CRC32_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/crc32.wasm"
));
pub const MATMUL_SIMD_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/matmul_simd.wasm"
));
pub const MATMUL_FMA_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/matmul_fma.wasm"
));
pub const CONVOLUTION_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/convolution.wasm"
));
pub const AUDIO_DSP_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/audio_dsp.wasm"
));
pub const BULK_MEMORY_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/bulk_memory.wasm"
));
pub const CALL_INDIRECT_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/call_indirect.wasm"
));
/// SQLite speedtest1 from Sightglass (`benchmarks/sqlite3/sqlite3.wasm`).
/// Uses WASI preview-1 imports + `bench.start`/`bench.end` timing hooks.
/// Wired via `sqlite3::run_sqlite3()` (Pulley side) — needs an import
/// shim. WAMR side requires `WAMR_BUILD_LIBC_WASI=1` in the libiwasm.a
/// build (currently off; turn on as a follow-up).
pub const SQLITE3_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/sqlite3.wasm"
));
/// Hand-written graphql-js validation-shape benchmark, AssemblyScript port
/// (Option A). 61 KB, 0 imports, 13 `call_indirect`. Optimizer-friendly.
/// See `workloads/graphql-validation/ASSEMBLYSCRIPT-NOTES.md`.
pub const GRAPHQL_VALIDATION_AS_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/graphql-validation-as.wasm"
));
/// Hand-written graphql-js validation-shape benchmark, Porffor port
/// (Option C). 121 KB, 1 import (host print), 98 `call_indirect`. Preserves
/// graphql-js's megamorphic-dispatch shape much more faithfully than the
/// AS port — primary workload for `call_indirect` optimization work.
/// See `workloads/graphql-validation/PORFFOR-NOTES.md`.
pub const GRAPHQL_VALIDATION_PORF_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/graphql-validation-porf.wasm"
));
/// Real-world `call_indirect`-shaped workload: the `xmrsplayer` Rust
/// soundtracker player rendering `unreal.s3m` (a Scream Tracker 3
/// module) at 44 100 Hz stereo to a null sound driver. ~340 KB
/// (includes the embedded 245 KB S3M); 27 `call_indirect` sites lowered
/// from 12 `dyn`-trait dispatch sites in `xmrsplayer`'s per-tick effect
/// pipeline.
///
/// Single export `play_buffer(seed: i32) -> i32`; `seed` ignored,
/// returns a running i16-XOR checksum of every sample produced
/// across the lifetime of the wasm instance. Each call renders one
/// audio buffer of 1024 stereo frames (≈ 23 ms of audio, matching a
/// typical CoreAudio per-callback budget). Player state is persistent
/// across calls — call N continues exactly where call N-1 left off,
/// and the song loops indefinitely (xmrsplayer's
/// `max_loop_count = 0` default), so any number of harness
/// iterations works without exhausting the song's material. This
/// shape lets `pick_iters` land at a meaningful iter count on every
/// platform (≈ tens-to-thousands per `BENCH_TARGET_MS=200` window
/// depending on core class) and amortizes the one-shot parse +
/// player-init cost into `load_ns` instead of `run_ns_*`.
///
/// Source crate: `workloads-rs-cargo/xmrsplayer-bench` (LTO=fat,
/// opt-level=s, panic=abort, no_std + dlmalloc + xmrsplayer 0.11.0).
pub const XMRSPLAYER_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/xmrsplayer.wasm"
));
/// C++-style vtable dispatch through call_indirect — models the
/// StarlingMonkey `CryptoAlgorithm` hierarchy (pure-virtual base + 6
/// concrete algorithm impls). Four entry points sweep the IC's
/// polymorphism dimension:
///   - `vtable_mono`   100% monomorphic (best case for 1-way IC)
///   - `vtable_bi`     alternating bimodal (kills 1-way IC; 2-way wins)
///   - `vtable_poly4`  4-way rotation (1-way IC always misses)
///   - `vtable_poly6`  6-way rotation (worst case)
///
/// 200K iterations × 2 dispatches per iter = 400K call_indirect per
/// run, all against an immutable funcref table (Rust-emitted, no
/// `table.set`). IC eligibility: passes
/// `is_call_indirect_cacheable_table`.
pub const VTABLE_DISPATCH_WASM: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../workloads/vtable_dispatch.wasm"
));

#[derive(Debug, Clone)]
pub struct RunReport {
    pub result: i32,
    pub iterations: u32,
    pub load_time: Duration,
    pub run_min: Duration,
    pub run_median: Duration,
    pub run_p99: Duration,
    pub cpu_user_ns: u64,
    pub cpu_system_ns: u64,
    pub rss_peak_bytes: u64,
    pub page_faults: u64,
}

/// Heuristic: pick an iteration count so the total measurement window is
/// in the ballpark of `target_total` (default 200 ms). Caller can override.
///
/// `BENCH_TARGET_MS` env var, when set to a positive integer, overrides
/// `target_total` for every call site. Use this to scale every workload's
/// iteration budget at once — e.g. `BENCH_TARGET_MS=2000` runs 10x more
/// iterations to tighten the noise floor (√N improvement) and amortize
/// one-off compile/cache-warm costs across the run window.
pub(crate) fn pick_iters(first_run: Duration, target_total: Duration) -> u32 {
    let single_ns = first_run.as_nanos().max(1) as u64;
    let target_ns = std::env::var("BENCH_TARGET_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .map(|ms| ms.saturating_mul(1_000_000))
        .unwrap_or_else(|| target_total.as_nanos() as u64);
    let raw = (target_ns / single_ns).max(1);
    raw.min(16_384) as u32
}

/// Generic runner: load `wasm_bytes` via Pulley, look up `fn_name`, then run
/// it `iters` times (auto-tuned if `iters == 0`) and report per-phase
/// timings + Apple task-info derived CPU / RSS / page-fault deltas.
pub fn run_workload(wasm_bytes: &[u8], fn_name: &str, arg: i32) -> Result<RunReport> {
    run_workload_iters(wasm_bytes, fn_name, arg, 0)
}

/// Same shape as `run_workload`, but routes through the chosen runtime.
pub fn run_workload_with(
    rt: Runtime,
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
) -> Result<RunReport> {
    match rt {
        Runtime::Pulley => run_workload(wasm_bytes, fn_name, arg),
        Runtime::Wamr => wamr::run_workload_wamr(wasm_bytes, fn_name, arg),
        Runtime::Wasm3 => wasm3::run_workload_wasm3(wasm_bytes, fn_name, arg),
        Runtime::WasmEdge => wasmedge::run_workload_wasmedge(wasm_bytes, fn_name, arg),
        Runtime::Zwasm => zwasm::run_workload_zwasm(wasm_bytes, fn_name, arg),
        Runtime::Wasmz => wasmz::run_workload_wasmz(wasm_bytes, fn_name, arg),
    }
}

pub fn run_workload_iters(
    wasm_bytes: &[u8],
    fn_name: &str,
    arg: i32,
    iters: u32,
) -> Result<RunReport> {
    let load_start = Instant::now();
    let pulley_target = if cfg!(target_pointer_width = "64") {
        "pulley64"
    } else {
        "pulley32"
    };
    let mut config = wasmtime::Config::new();
    into_anyhow(config.target(pulley_target).map(|_| ()))
        .with_context(|| format!("Config::target({pulley_target}) failed"))?;
    // simd128 + relaxed-simd both on (defaults, made explicit). Non-
    // deterministic relaxed-simd lets Pulley use Vfma32x4/Vfma64x2.
    config.wasm_simd(true);
    config.wasm_relaxed_simd(true);
    config.relaxed_simd_deterministic(false);
    // Tail calls: defaults to true except with Winch (we don't use Winch).
    // Made explicit so the `fib_tail` workload's `return_call` opcodes are
    // accepted regardless of any future default change.
    config.wasm_tail_call(true);
    // Bulk memory is default-on; explicit for the bulk_memory workload.
    config.wasm_bulk_memory(true);
    // Note: `wasm_reference_types` is gated behind wasmtime's `gc` feature
    // (which we don't enable). The plain wasm 1.0 `call_indirect` op our
    // workload uses works fine without it.
    let engine = into_anyhow(Engine::new(&config))
        .context("Engine::new failed")?;
    let module = into_anyhow(Module::from_binary(&engine, wasm_bytes))
        .context("Module::from_binary failed — invalid wasm or unsupported feature?")?;
    let mut store = Store::new(&engine, ());
    let instance = into_anyhow(Instance::new(&mut store, &module, &[]))
        .context("Instance::new failed — missing imports?")?;
    let typed = into_anyhow(instance.get_typed_func::<i32, i32>(&mut store, fn_name))
        .with_context(|| format!("export `{fn_name}` not found or wrong signature"))?;
    let load_time = load_start.elapsed();

    // Two warmup calls before measurement:
    //
    // 1. The first call may pay one-time per-instance init costs that
    //    a workload defers to first-call laziness — e.g. xmrsplayer
    //    parses its embedded `unreal.s3m` and constructs the
    //    `XmrsPlayer` on the first invocation. Including that in the
    //    `pick_iters` sizing budget would massively underestimate
    //    iter count (you'd land at iter=2 on a workload whose
    //    steady-state per-call is single-digit ms).
    //
    // 2. The second call is the real warmup — wasm code is hot, ICache
    //    is warm, the workload's per-call cost reflects its
    //    steady-state shape. Use *its* wallclock for `pick_iters`.
    //
    // For workloads without lazy-init costs (the rest of the suite —
    // fib, sieve, crc32, ...) the two warmups just pay an extra
    // sub-millisecond beat that doesn't show up against the
    // measurement window.
    let mut result = into_anyhow(typed.call(&mut store, arg))
        .with_context(|| format!("`{fn_name}({arg})` trapped (init warmup)"))?;
    let warm_start = Instant::now();
    result = into_anyhow(typed.call(&mut store, arg))
        .with_context(|| format!("`{fn_name}({arg})` trapped (steady warmup)"))?;
    let warm = warm_start.elapsed();
    let n = if iters == 0 {
        pick_iters(warm, Duration::from_millis(200))
    } else {
        iters
    };

    // Snapshot before measurement window.
    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();

    // Measurement loop. Record every per-iteration wall-clock for percentiles.
    let mut samples: Vec<u64> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let it_start = Instant::now();
        let r = into_anyhow(typed.call(&mut store, arg))
            .with_context(|| format!("`{fn_name}({arg})` trapped"))?;
        samples.push(it_start.elapsed().as_nanos() as u64);
        result = r;
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

// -----------------------------------------------------------------------------
// Per-workload Rust convenience wrappers + host references.
// -----------------------------------------------------------------------------

pub fn run_fib(n: i32) -> Result<RunReport> {
    run_workload(FIB_WASM, "fib", n)
}

pub fn fib_reference(n: i32) -> i32 {
    let (mut a, mut b) = (0i32, 1i32);
    for _ in 0..n {
        let next = a.wrapping_add(b);
        a = b;
        b = next;
    }
    a
}

/// Tail-recursive accumulator-passing Fibonacci, exercising wasm `return_call`.
pub fn run_fib_tail(n: i32) -> Result<RunReport> {
    run_workload(FIB_TAIL_WASM, "fib_tail", n)
}

pub fn run_factorial(n: i32) -> Result<RunReport> {
    run_workload(FACTORIAL_WASM, "factorial", n)
}

pub fn factorial_reference(n: i32) -> i32 {
    let mut acc: i32 = 1;
    let mut i: i32 = 2;
    while i <= n {
        acc = acc.wrapping_mul(i);
        i = i.wrapping_add(1);
    }
    acc
}

pub fn run_sieve(n: i32) -> Result<RunReport> {
    run_workload(SIEVE_WASM, "sieve", n)
}

pub fn sieve_reference(n: i32) -> i32 {
    if n < 2 {
        return 0;
    }
    let n = n as usize;
    let mut sieve = vec![false; n + 1];
    let mut p = 2usize;
    while p * p <= n {
        if !sieve[p] {
            let mut m = p * p;
            while m <= n {
                sieve[m] = true;
                m += p;
            }
        }
        p += 1;
    }
    (2..=n).filter(|&i| !sieve[i]).count() as i32
}

pub fn run_crc32(seed: i32) -> Result<RunReport> {
    run_workload(CRC32_WASM, "crc32", seed)
}

pub fn crc32_reference(seed: i32) -> i32 {
    const N: usize = 64 * 1024;
    const POLY: u32 = 0xEDB88320;
    // Same LCG as the wasm side.
    let mut s = seed as u32;
    let mut input = vec![0u8; N];
    for byte in input.iter_mut() {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        *byte = (s >> 24) as u8;
    }
    let mut table = [0u32; 256];
    for i in 0..256 {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { POLY ^ (c >> 1) } else { c >> 1 };
        }
        table[i] = c;
    }
    let mut crc: u32 = 0xFFFFFFFF;
    for &b in &input {
        crc = (crc >> 8) ^ table[((crc ^ b as u32) & 0xFF) as usize];
    }
    !crc as i32
}

pub fn run_matmul_simd(seed: i32) -> Result<RunReport> {
    run_workload(MATMUL_SIMD_WASM, "matmul", seed)
}

pub fn matmul_simd_reference(seed: i32) -> i32 {
    const N: usize = 64;
    let mut a = vec![0f32; N * N];
    let mut b = vec![0f32; N * N];
    let mut c = vec![0f32; N * N];
    for i in 0..N {
        for j in 0..N {
            a[i * N + j] = ((i as i32 + seed) ^ (j as i32 * 7)) as f32 * 0.001;
            b[i * N + j] = (((i as i32) * 13 + j as i32 + seed) & 0xFF) as f32 * 0.002;
        }
    }
    for i in 0..N {
        for j in 0..N {
            let mut acc: f32 = 0.0;
            for k in 0..N {
                acc += a[i * N + k] * b[k * N + j];
            }
            c[i * N + j] = acc;
        }
    }
    let trace: f32 = (0..N).map(|i| c[i * N + i]).sum();
    trace as i32
}

pub fn run_matmul_fma(seed: i32) -> Result<RunReport> {
    run_workload(MATMUL_FMA_WASM, "matmul_fma", seed)
}

pub fn matmul_fma_reference(seed: i32) -> i32 {
    // Same fill pattern as matmul_simd, but uses `f32::mul_add` to match
    // Pulley's single-rounding `Vfma32x4` lowering of `f32x4_relaxed_madd`.
    const N: usize = 64;
    let mut a = vec![0f32; N * N];
    let mut b = vec![0f32; N * N];
    let mut c = vec![0f32; N * N];
    for i in 0..N {
        for j in 0..N {
            a[i * N + j] = ((i as i32 + seed) ^ (j as i32 * 7)) as f32 * 0.001;
            b[i * N + j] = (((i as i32) * 13 + j as i32 + seed) & 0xFF) as f32 * 0.002;
        }
    }
    for i in 0..N {
        for j in 0..N {
            let mut acc: f32 = 0.0;
            for k in 0..N {
                acc = (a[i * N + k]).mul_add(b[k * N + j], acc);
            }
            c[i * N + j] = acc;
        }
    }
    let trace: f32 = (0..N).map(|i| c[i * N + i]).sum();
    trace as i32
}

pub fn run_convolution(seed: i32) -> Result<RunReport> {
    run_workload(CONVOLUTION_WASM, "convolve", seed)
}

pub fn convolution_reference(seed: i32) -> i32 {
    const W: usize = 256;
    const H: usize = 256;
    let mut input = vec![0u8; W * H];
    let mut output = vec![0u8; W * H];
    let mut s = seed as u32;
    for byte in input.iter_mut() {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        *byte = (s >> 24) as u8;
    }
    for y in 1..H - 1 {
        for x in 1..W - 1 {
            let mut acc: u32 = 0;
            for dy in -1i32..=1 {
                for dx in -1i32..=1 {
                    let yi = (y as i32 + dy) as usize;
                    let xi = (x as i32 + dx) as usize;
                    acc += input[yi * W + xi] as u32;
                }
            }
            output[y * W + x] = (acc / 9) as u8;
        }
    }
    let sum: u32 = output.iter().fold(0u32, |a, &b| a.wrapping_add(b as u32));
    (sum & ((1u32 << 30) - 1)) as i32
}

pub fn run_audio_dsp(seed: i32) -> Result<RunReport> {
    run_workload(AUDIO_DSP_WASM, "audio_dsp", seed)
}

pub fn run_bulk_memory(seed: i32) -> Result<RunReport> {
    run_workload(BULK_MEMORY_WASM, "bulk_memory", seed)
}

pub fn bulk_memory_reference(seed: i32) -> i32 {
    const N: usize = 64 * 1024;
    const ROUNDS: usize = 200;
    let mut src = vec![0u8; N];
    let mut dst = vec![0u8; N];
    let mut s = seed as u32;
    for byte in src.iter_mut() {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        *byte = (s >> 24) as u8;
    }
    let sizes = [16usize, 64, 256, 1024, 4096];
    for round in 0..ROUNDS {
        let size = sizes[round % sizes.len()];
        let mut off = 0usize;
        while off + size <= N {
            let src_off = (off + (round * 17)) & (N - size);
            dst[off..off + size].copy_from_slice(&src[src_off..src_off + size]);
            off += size * 2;
        }
        let fill_off = (round * 1024) & (N - size);
        let fill_byte = (round & 0xFF) as u8;
        dst[fill_off..fill_off + size].fill(fill_byte);
    }
    let sum: u32 = dst.iter().fold(0u32, |a, &b| a.wrapping_add(b as u32));
    (sum & 0x7FFF_FFFF) as i32
}

pub fn run_call_indirect(seed: i32) -> Result<RunReport> {
    run_workload(CALL_INDIRECT_WASM, "call_indirect", seed)
}

/// Render audio through `xmrsplayer` to a null sound driver. The
/// wasm side keeps `XmrsPlayer` state in a static cell so each call
/// just synthesizes one buffer (1024 stereo frames ≈ 23 ms of audio
/// at 44.1 kHz) and returns; parse + module-init happens lazily on
/// the first call and is billed against `load_ns`. `pick_iters` lands
/// at a meaningful iter count on every platform — tens to hundreds
/// per `BENCH_TARGET_MS=200` window on watch-class E-cores, thousands
/// on M4 P-cores — so per-iter min/median/p99 reflect a real
/// distribution. The song (`unreal.s3m`) loops indefinitely thanks to
/// xmrsplayer's `max_loop_count = 0` default, so any iteration count
/// is safe; the cumulative audio rendered across iterations grows
/// linearly with `BENCH_TARGET_MS`.
pub fn run_xmrsplayer(seed: i32) -> Result<RunReport> {
    run_workload(XMRSPLAYER_WASM, "play_buffer", seed)
}

pub fn run_vtable_mono(seed: i32) -> Result<RunReport> {
    run_workload(VTABLE_DISPATCH_WASM, "vtable_mono", seed)
}

pub fn run_vtable_bi(seed: i32) -> Result<RunReport> {
    run_workload(VTABLE_DISPATCH_WASM, "vtable_bi", seed)
}

pub fn run_vtable_poly4(seed: i32) -> Result<RunReport> {
    run_workload(VTABLE_DISPATCH_WASM, "vtable_poly4", seed)
}

pub fn run_vtable_poly6(seed: i32) -> Result<RunReport> {
    run_workload(VTABLE_DISPATCH_WASM, "vtable_poly6", seed)
}

pub fn call_indirect_reference(seed: i32) -> i32 {
    const ITERS: usize = 200_000;
    let mut acc: i32 = seed;
    let mut s: u32 = seed as u32;
    let ops: [fn(i32, i32) -> i32; 16] = [
        |a, b| a.wrapping_add(b),
        |a, b| a.wrapping_sub(b),
        |a, b| a.wrapping_mul(b),
        |a, b| a ^ b,
        |a, b| a & b,
        |a, b| a | b,
        |a, b| a.wrapping_shl((b & 31) as u32),
        |a, b| (a as u32).wrapping_shr((b & 31) as u32) as i32,
        |a, b| (a as u32).rotate_left((b & 31) as u32) as i32,
        |a, b| (a as u32).rotate_right((b & 31) as u32) as i32,
        |a, b| if a < b { a } else { b },
        |a, b| if a > b { a } else { b },
        |a, _| (a as u32).leading_zeros() as i32,
        |a, _| (a as u32).trailing_zeros() as i32,
        |a, _| (a as u32).count_ones() as i32,
        |a, b| (!a) ^ b,
    ];
    for _ in 0..ITERS {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let idx = (s >> 28) as usize;
        acc = ops[idx](acc, s as i32);
    }
    acc
}

// Re-implements the SPC-DSP-like workload on the host to verify the
// wasm side's output matches.
pub fn audio_dsp_reference(seed: i32) -> i32 {
    const VOICES: usize = 8;
    const SAMPLE_MEM: usize = 4096;
    const FRAMES: usize = 1000;
    const SAMPLES_PER_FRAME: usize = 512;

    #[derive(Copy, Clone)]
    struct V {
        sample_pos: i32,
        sample_inc: i32,
        env: i32,
        env_phase: u8,
        sample_off: u16,
    }

    let mut s = seed as u32;
    let mut sample = vec![0i16; SAMPLE_MEM];
    for v in sample.iter_mut() {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        *v = ((s as i32) >> 16) as i16;
    }
    let mut voices = [V {
        sample_pos: 0,
        sample_inc: 0,
        env: 0,
        env_phase: 0,
        sample_off: 0,
    }; VOICES];
    for v in 0..VOICES {
        voices[v] = V {
            sample_pos: 0,
            sample_inc: 0x800 + (v as i32 * 0x80),
            env: 0,
            env_phase: 0,
            sample_off: ((v * SAMPLE_MEM / VOICES) & 0xFFFF) as u16,
        };
    }
    let mut lpf: i32 = 0;
    let mut checksum: i32 = 0;
    for frame in 0..FRAMES {
        for _ in 0..SAMPLES_PER_FRAME {
            let mut mix: i32 = 0;
            for v in 0..VOICES {
                let voice = &mut voices[v];
                let int_pos = (voice.sample_pos >> 12) as usize;
                let off = voice.sample_off as usize;
                let s0 = sample[(off + int_pos) & (SAMPLE_MEM - 1)] as i32;
                let s1 = sample[(off + int_pos + 1) & (SAMPLE_MEM - 1)] as i32;
                let s2 = sample[(off + int_pos + 2) & (SAMPLE_MEM - 1)] as i32;
                let s3 = sample[(off + int_pos + 3) & (SAMPLE_MEM - 1)] as i32;
                let interp = (s0 + 2 * s1 + 2 * s2 + s3) / 6;
                match voice.env_phase {
                    0 => {
                        voice.env += 0x10;
                        if voice.env >= 0x7FF {
                            voice.env = 0x7FF;
                            voice.env_phase = 1;
                        }
                    }
                    1 => {
                        voice.env -= 0x4;
                        if voice.env < 0x500 {
                            voice.env = 0x500;
                            voice.env_phase = 2;
                        }
                    }
                    2 => {}
                    _ => {
                        voice.env -= 0x2;
                        if voice.env < 0 {
                            voice.env = 0;
                        }
                    }
                }
                mix += (interp * voice.env) >> 11;
                voice.sample_pos = voice.sample_pos.wrapping_add(voice.sample_inc);
                if voice.sample_pos >= ((SAMPLE_MEM as i32) << 12) {
                    voice.sample_pos -= (SAMPLE_MEM as i32) << 12;
                }
            }
            lpf = lpf + ((mix - lpf) >> 2);
            let out = lpf.clamp(-32768, 32767) as i16;
            checksum = checksum.wrapping_add(out as i32);
        }
        if frame == FRAMES / 4 {
            for v in voices.iter_mut() { v.env_phase = 1; }
        } else if frame == FRAMES / 2 {
            for v in voices.iter_mut() { v.env_phase = 2; }
        } else if frame == 3 * FRAMES / 4 {
            for v in voices.iter_mut() { v.env_phase = 3; }
        }
    }
    checksum & 0x7FFFFFFF
}

// -----------------------------------------------------------------------------
// C ABI surface — what the Swift app talks to. Stable layout (`#[repr(C)]`).
// -----------------------------------------------------------------------------

#[repr(C)]
pub struct BenchReport {
    pub result: i32,
    pub ok: u8,
    pub iterations: u32,
    pub load_ns: u64,
    pub run_ns_min: u64,
    pub run_ns_median: u64,
    pub run_ns_p99: u64,
    pub cpu_user_ns: u64,
    pub cpu_system_ns: u64,
    pub rss_peak_bytes: u64,
    pub page_faults: u64,
    pub error_msg: *mut std::os::raw::c_char,
}

fn report_from(r: Result<RunReport>) -> BenchReport {
    match r {
        Ok(r) => BenchReport {
            result: r.result,
            ok: 1,
            iterations: r.iterations,
            load_ns: r.load_time.as_nanos() as u64,
            run_ns_min: r.run_min.as_nanos() as u64,
            run_ns_median: r.run_median.as_nanos() as u64,
            run_ns_p99: r.run_p99.as_nanos() as u64,
            cpu_user_ns: r.cpu_user_ns,
            cpu_system_ns: r.cpu_system_ns,
            rss_peak_bytes: r.rss_peak_bytes,
            page_faults: r.page_faults,
            error_msg: std::ptr::null_mut(),
        },
        Err(e) => {
            let msg = format!("{e:#}");
            let cstring = std::ffi::CString::new(msg).unwrap_or_else(|_| {
                std::ffi::CString::new("error message contained nul bytes").unwrap()
            });
            BenchReport {
                result: 0,
                ok: 0,
                iterations: 0,
                load_ns: 0,
                run_ns_min: 0,
                run_ns_median: 0,
                run_ns_p99: 0,
                cpu_user_ns: 0,
                cpu_system_ns: 0,
                rss_peak_bytes: 0,
                page_faults: 0,
                error_msg: cstring.into_raw(),
            }
        }
    }
}

/// Initialize WAMR. **Must be called on the process's main thread**
/// (the Swift app's `App` body runs on main, so call from there).
/// Returns 1 on success, 0 if WAMR is unavailable in this build.
#[unsafe(no_mangle)]
pub extern "C" fn bench_init_wamr() -> u8 {
    match wamr::init() {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

// Pulley path (default).
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib(n: i32) -> BenchReport {
    report_from(run_fib(n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_tail(n: i32) -> BenchReport {
    report_from(run_fib_tail(n))
}

// WAMR path. Each function calls into the WAMR fast-interpreter.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_wamr(n: i32) -> BenchReport {
    report_from(wamr::run_workload_wamr(FIB_WASM, "fib", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_tail_wamr(n: i32) -> BenchReport {
    report_from(wamr::run_workload_wamr(FIB_TAIL_WASM, "fib_tail", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_factorial_wamr(n: i32) -> BenchReport {
    report_from(wamr::run_workload_wamr(FACTORIAL_WASM, "factorial", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sieve_wamr(n: i32) -> BenchReport {
    report_from(wamr::run_workload_wamr(SIEVE_WASM, "sieve", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_crc32_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(CRC32_WASM, "crc32", 0xC0FFEE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_simd_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(MATMUL_SIMD_WASM, "matmul", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_fma_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(MATMUL_FMA_WASM, "matmul_fma", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_convolution_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(CONVOLUTION_WASM, "convolve", 0xCAFE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_audio_dsp_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(AUDIO_DSP_WASM, "audio_dsp", 0x5C7))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_bulk_memory_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(BULK_MEMORY_WASM, "bulk_memory", 0xB0CC))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_call_indirect_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(CALL_INDIRECT_WASM, "call_indirect", 0xC1AA))
}

/// xmrsplayer rendering 15 s of `unreal.s3m` to a null sound driver.
/// Real-world `call_indirect`-shaped workload (31 sites lowered from
/// 12 `dyn`-trait dispatches in xmrsplayer's per-tick effect pipeline).
/// `seed` is ignored by the wasm side. Per-call wallclock is dominated
/// by the 15 s of audio synthesis, so the harness usually settles on
/// 1 iteration at the default `BENCH_TARGET_MS=200`.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_xmrsplayer_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(XMRSPLAYER_WASM, "play_buffer", 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_factorial(n: i32) -> BenchReport {
    report_from(run_factorial(n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sieve(n: i32) -> BenchReport {
    report_from(run_sieve(n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_crc32() -> BenchReport {
    report_from(run_crc32(0xC0FFEE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_simd() -> BenchReport {
    report_from(run_matmul_simd(0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_fma() -> BenchReport {
    report_from(run_matmul_fma(0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_convolution() -> BenchReport {
    report_from(run_convolution(0xCAFE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_audio_dsp() -> BenchReport {
    report_from(run_audio_dsp(0x5C7))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_bulk_memory() -> BenchReport {
    report_from(run_bulk_memory(0xB0CC))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_call_indirect() -> BenchReport {
    report_from(run_call_indirect(0xC1AA))
}

/// xmrsplayer rendering 15 s of `unreal.s3m` to a null sound driver.
/// Pulley-side counterpart to `bench_run_xmrsplayer_wamr`. See the
/// WAMR variant's doc comment for context on iteration sizing.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_xmrsplayer() -> BenchReport {
    report_from(run_xmrsplayer(0))
}

/// vtable_dispatch — C++-style virtual dispatch through call_indirect.
/// 200K iterations × 2 dispatches per iter. Four entry points sweep
/// the IC's polymorphism dimension. See `VTABLE_DISPATCH_WASM` doc.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_mono() -> BenchReport {
    report_from(run_vtable_mono(0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_bi() -> BenchReport {
    report_from(run_vtable_bi(0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly4() -> BenchReport {
    report_from(run_vtable_poly4(0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly6() -> BenchReport {
    report_from(run_vtable_poly6(0xC1AA))
}

// vtable_* WAMR variants — same wasm + same per-shape entry points,
// just routed through the WAMR fast-interp instead of Pulley. Side-by-
// side data point for "how does WAMR's load-time IR rewrite handle
// the same vtable-dispatch shape Pulley's call_indirect lazy-init
// fusion targets". WAMR has no IC / type-feedback — its win on these
// shapes (if any) comes from fewer per-dispatch instructions in its
// preprocessed-bytecode interpreter, not from specialization.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_mono_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(VTABLE_DISPATCH_WASM, "vtable_mono", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_bi_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(VTABLE_DISPATCH_WASM, "vtable_bi", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly4_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(VTABLE_DISPATCH_WASM, "vtable_poly4", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly6_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(VTABLE_DISPATCH_WASM, "vtable_poly6", 0xC1AA))
}

/// graphql-validation AS port on WAMR. The AS wasm has 0 imports, so
/// it loads cleanly under the generic `run_workload_wamr` runner
/// (which doesn't register any host imports). `validate_once(0)`
/// returns an `i32` error count; the harness reports the median
/// per-iter time.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_as_wamr() -> BenchReport {
    report_from(wamr::run_workload_wamr(GRAPHQL_VALIDATION_AS_WASM, "validate_once", 0))
}

/// graphql-validation Porffor port on WAMR. Porffor compiles JS
/// try/catch to the wasm exceptions proposal, which is not enabled
/// in our WAMR build (`WAMR_BUILD_EXCE_HANDLING=0`). The
/// `run_graphql_validation_porf_wamr` runner attempts the load
/// anyway; the harness reports the wasm-level error string from
/// `wasm_runtime_get_exception` if WAMR refuses the module. Treat as
/// a "WAMR can't run this shape" data point — Pulley side runs both.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_porf_wamr() -> BenchReport {
    report_from(wamr::run_graphql_validation_porf_wamr(GRAPHQL_VALIDATION_PORF_WASM))
}

/// Sightglass `sqlite3` benchmark (speedtest1 against an in-memory DB).
/// Single-shot, ~minutes on weak cores, ~5-30 s on M4.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sqlite3() -> BenchReport {
    report_from(sqlite3::run_sqlite3(SQLITE3_WASM))
}

// ---------------------------------------------------------------------
// wasm3 (m3) — pure C interpreter, no SIMD / no exceptions. Same
// `(wasm_bytes, fn_name, arg)` signature as the WAMR exports above.
// Workloads that require features wasm3 lacks (matmul_simd /
// matmul_fma / xmrsplayer / graphql-validation-porf / sqlite3) will
// fail at load time; that's data, not a regression — the harness
// surfaces the wasm3 error string in the workload row.
// ---------------------------------------------------------------------

/// Initialize wasm3 (currently a no-op — wasm3 has no process-global
/// state to set up, unlike WAMR — but exposed symmetrically so the
/// Swift app can call all three init functions from main.
#[unsafe(no_mangle)]
pub extern "C" fn bench_init_wasm3() -> u8 {
    match wasm3::init() {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_wasm3(n: i32) -> BenchReport {
    report_from(wasm3::run_workload_wasm3(FIB_WASM, "fib", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_tail_wasm3(n: i32) -> BenchReport {
    report_from(wasm3::run_workload_wasm3(FIB_TAIL_WASM, "fib_tail", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_factorial_wasm3(n: i32) -> BenchReport {
    report_from(wasm3::run_workload_wasm3(FACTORIAL_WASM, "factorial", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sieve_wasm3(n: i32) -> BenchReport {
    report_from(wasm3::run_workload_wasm3(SIEVE_WASM, "sieve", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_crc32_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(CRC32_WASM, "crc32", 0xC0FFEE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_simd_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(MATMUL_SIMD_WASM, "matmul", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_fma_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(MATMUL_FMA_WASM, "matmul_fma", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_convolution_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(CONVOLUTION_WASM, "convolve", 0xCAFE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_audio_dsp_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(AUDIO_DSP_WASM, "audio_dsp", 0x5C7))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_bulk_memory_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(BULK_MEMORY_WASM, "bulk_memory", 0xB0CC))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_call_indirect_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(CALL_INDIRECT_WASM, "call_indirect", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_xmrsplayer_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(XMRSPLAYER_WASM, "play_buffer", 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_mono_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(VTABLE_DISPATCH_WASM, "vtable_mono", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_bi_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(VTABLE_DISPATCH_WASM, "vtable_bi", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly4_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(VTABLE_DISPATCH_WASM, "vtable_poly4", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly6_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(VTABLE_DISPATCH_WASM, "vtable_poly6", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_as_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(GRAPHQL_VALIDATION_AS_WASM, "validate_once", 0))
}

/// graphql-validation Porffor on wasm3. Porffor compiles JS try/catch
/// to the wasm exceptions proposal; wasm3 doesn't implement exceptions,
/// so this will fail at load with `unknownOpcode` or similar.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_porf_wasm3() -> BenchReport {
    report_from(wasm3::run_workload_wasm3(
        GRAPHQL_VALIDATION_PORF_WASM,
        "m",
        0,
    ))
}

// ---------------------------------------------------------------------
// WasmEdge — pure-interpreter mode (WASMEDGE_USE_LLVM=OFF) with the
// 27-patch Apple-mobile enablement stack. Unlike WAMR, WasmEdge's
// interpreter supports SIMD + wasm-exceptions simultaneously, so
// Porffor's graphql-validation should run on this path (subject to the
// host-print import; the runner uses no imports, so it'll trap on the
// missing import — same shape as Pulley would without the host stub).
// ---------------------------------------------------------------------

/// Initialize WasmEdge. No process-global state to set up — kept
/// symmetrical with `bench_init_wamr` / `bench_init_wasm3`. Returns 1
/// if WasmEdge is linked into this build, 0 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn bench_init_wasmedge() -> u8 {
    match wasmedge::init() {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_wasmedge(n: i32) -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(FIB_WASM, "fib", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_tail_wasmedge(n: i32) -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(FIB_TAIL_WASM, "fib_tail", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_factorial_wasmedge(n: i32) -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(FACTORIAL_WASM, "factorial", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sieve_wasmedge(n: i32) -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(SIEVE_WASM, "sieve", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_crc32_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(CRC32_WASM, "crc32", 0xC0FFEE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_simd_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(MATMUL_SIMD_WASM, "matmul", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_fma_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(MATMUL_FMA_WASM, "matmul_fma", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_convolution_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(CONVOLUTION_WASM, "convolve", 0xCAFE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_audio_dsp_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(AUDIO_DSP_WASM, "audio_dsp", 0x5C7))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_bulk_memory_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(BULK_MEMORY_WASM, "bulk_memory", 0xB0CC))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_call_indirect_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(CALL_INDIRECT_WASM, "call_indirect", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_xmrsplayer_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(XMRSPLAYER_WASM, "play_buffer", 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_mono_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(VTABLE_DISPATCH_WASM, "vtable_mono", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_bi_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(VTABLE_DISPATCH_WASM, "vtable_bi", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly4_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(VTABLE_DISPATCH_WASM, "vtable_poly4", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly6_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(VTABLE_DISPATCH_WASM, "vtable_poly6", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_as_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(
        GRAPHQL_VALIDATION_AS_WASM,
        "validate_once",
        0,
    ))
}

/// graphql-validation Porffor on WasmEdge. Unlike WAMR, WasmEdge's
/// interpreter has both SIMD and wasm-exceptions enabled simultaneously,
/// so the load should succeed. The wasm imports a `b` print function
/// in module `""`; we don't register a host stub here yet, so the call
/// will trap on missing import — log surfaces the import-name from the
/// WasmEdge error string. Wiring the host stub is a follow-up.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_porf_wasmedge() -> BenchReport {
    report_from(wasmedge::run_workload_wasmedge(
        GRAPHQL_VALIDATION_PORF_WASM,
        "m",
        0,
    ))
}

// ---------------------------------------------------------------------
// zwasm (clojurewasm/zwasm) — Zig pure-interpreter mode, built with
// `-Djit=false`. arm64_32-apple-watchos device builds are not
// supported (zwasm assumes 64-bit pointers; Zig 0.16 has no arm64_32
// target); the rows will report ERROR on watchOS device, which is
// what we want — the data point is "zwasm doesn't run on this target."
// ---------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn bench_init_zwasm() -> u8 {
    match zwasm::init() {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_zwasm(n: i32) -> BenchReport {
    report_from(zwasm::run_workload_zwasm(FIB_WASM, "fib", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_tail_zwasm(n: i32) -> BenchReport {
    report_from(zwasm::run_workload_zwasm(FIB_TAIL_WASM, "fib_tail", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_factorial_zwasm(n: i32) -> BenchReport {
    report_from(zwasm::run_workload_zwasm(FACTORIAL_WASM, "factorial", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sieve_zwasm(n: i32) -> BenchReport {
    report_from(zwasm::run_workload_zwasm(SIEVE_WASM, "sieve", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_crc32_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(CRC32_WASM, "crc32", 0xC0FFEE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_simd_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(MATMUL_SIMD_WASM, "matmul", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_fma_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(MATMUL_FMA_WASM, "matmul_fma", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_convolution_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(CONVOLUTION_WASM, "convolve", 0xCAFE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_audio_dsp_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(AUDIO_DSP_WASM, "audio_dsp", 0x5C7))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_bulk_memory_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(BULK_MEMORY_WASM, "bulk_memory", 0xB0CC))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_call_indirect_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(CALL_INDIRECT_WASM, "call_indirect", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_xmrsplayer_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(XMRSPLAYER_WASM, "play_buffer", 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_mono_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(VTABLE_DISPATCH_WASM, "vtable_mono", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_bi_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(VTABLE_DISPATCH_WASM, "vtable_bi", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly4_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(VTABLE_DISPATCH_WASM, "vtable_poly4", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly6_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(VTABLE_DISPATCH_WASM, "vtable_poly6", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_as_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(
        GRAPHQL_VALIDATION_AS_WASM,
        "validate_once",
        0,
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_porf_zwasm() -> BenchReport {
    report_from(zwasm::run_workload_zwasm(GRAPHQL_VALIDATION_PORF_WASM, "m", 0))
}

// ---------------------------------------------------------------------
// wasmz (Ray-D-Song/wasmz) — Zig pure-interpreter, ported to Zig 0.16.
// Same arm64_32-apple-watchos caveat as zwasm: wasmz assumes 64-bit
// pointers and Zig 0.16 has no arm64_32 target, so watchOS device
// rows report ERROR.
// ---------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn bench_init_wasmz() -> u8 {
    match wasmz::init() {
        Ok(()) => 1,
        Err(_) => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_wasmz(n: i32) -> BenchReport {
    report_from(wasmz::run_workload_wasmz(FIB_WASM, "fib", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_fib_tail_wasmz(n: i32) -> BenchReport {
    report_from(wasmz::run_workload_wasmz(FIB_TAIL_WASM, "fib_tail", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_factorial_wasmz(n: i32) -> BenchReport {
    report_from(wasmz::run_workload_wasmz(FACTORIAL_WASM, "factorial", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_sieve_wasmz(n: i32) -> BenchReport {
    report_from(wasmz::run_workload_wasmz(SIEVE_WASM, "sieve", n))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_crc32_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(CRC32_WASM, "crc32", 0xC0FFEE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_simd_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(MATMUL_SIMD_WASM, "matmul", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_matmul_fma_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(MATMUL_FMA_WASM, "matmul_fma", 0xBEEF))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_convolution_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(CONVOLUTION_WASM, "convolve", 0xCAFE))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_audio_dsp_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(AUDIO_DSP_WASM, "audio_dsp", 0x5C7))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_bulk_memory_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(BULK_MEMORY_WASM, "bulk_memory", 0xB0CC))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_call_indirect_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(CALL_INDIRECT_WASM, "call_indirect", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_xmrsplayer_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(XMRSPLAYER_WASM, "play_buffer", 0))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_mono_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(VTABLE_DISPATCH_WASM, "vtable_mono", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_bi_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(VTABLE_DISPATCH_WASM, "vtable_bi", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly4_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(VTABLE_DISPATCH_WASM, "vtable_poly4", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_vtable_poly6_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(VTABLE_DISPATCH_WASM, "vtable_poly6", 0xC1AA))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_as_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(
        GRAPHQL_VALIDATION_AS_WASM,
        "validate_once",
        0,
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_porf_wasmz() -> BenchReport {
    report_from(wasmz::run_workload_wasmz(GRAPHQL_VALIDATION_PORF_WASM, "m", 0))
}

/// Hand-written graphql-js validation-shape benchmark, AssemblyScript port.
/// 13 `call_indirect`s — the optimizer-friendly comparison baseline.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_as() -> BenchReport {
    report_from(graphql_validation::run_graphql_validation_as(
        GRAPHQL_VALIDATION_AS_WASM,
    ))
}

/// Hand-written graphql-js validation-shape benchmark, Porffor port.
/// 98 `call_indirect`s — preserves graphql-js's megamorphic-dispatch shape.
/// Primary workload for `call_indirect` optimization work.
#[unsafe(no_mangle)]
pub extern "C" fn bench_run_graphql_validation_porf() -> BenchReport {
    report_from(graphql_validation::run_graphql_validation_porf(
        GRAPHQL_VALIDATION_PORF_WASM,
    ))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_free_error_msg(ptr: *mut std::os::raw::c_char) {
    if ptr.is_null() {
        return;
    }
    drop(unsafe { std::ffi::CString::from_raw(ptr) });
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_fib_wasm_size() -> usize {
    FIB_WASM.len()
}
