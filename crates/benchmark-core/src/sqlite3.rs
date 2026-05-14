//! Sightglass `sqlite3` benchmark runner.
//!
//! sqlite3.wasm is the SQLite `speedtest1` benchmark, ported by the
//! Bytecode Alliance from JetStream 3 for use with Sightglass. It
//! exercises a much larger and more realistic wasm workload than our
//! microbenchmarks: ~850 KiB of wasm, mixed integer/floating-point/
//! memory work, deeply branchy control flow, dispatch via wasm
//! function tables (vtable-style C lowerings), and significant heap
//! traffic.
//!
//! The module imports:
//!   - `wasi_snapshot_preview1.{fd_close, fd_write, fd_read, fd_sync,
//!     environ_sizes_get, environ_get, fd_seek, fd_fdstat_get}`
//!   - `bench.{start, end}` — Sightglass timing hooks, no-ops here.
//!
//! For benchmarking we don't need real WASI: the speedtest1 harness
//! runs an in-memory SQLite database, all `fd_*` calls are either
//! "write progress to stdout" (we discard) or "read environ" (we
//! return empty). The `_start` export drives the whole benchmark to
//! completion; we time the whole `_start` invocation.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use wasmtime::{Caller, Engine, Linker, Module, Store};

use crate::{into_anyhow, taskinfo, RunReport};

/// Run the sqlite3 benchmark via Pulley. Returns a `RunReport` where
/// `result` is always 0 (sqlite3 doesn't return a numeric result via
/// `_start`); `iterations` is always 1 (the workload is too big for
/// auto-tuning — one `_start` call already takes ~minutes on weaker
/// hardware, ~5s on M4).
pub fn run_sqlite3(wasm_bytes: &[u8]) -> Result<RunReport> {
    let load_start = Instant::now();
    let pulley_target = if cfg!(target_pointer_width = "64") {
        "pulley64"
    } else {
        "pulley32"
    };
    let mut config = wasmtime::Config::new();
    into_anyhow(config.target(pulley_target).map(|_| ()))
        .with_context(|| format!("Config::target({pulley_target}) failed"))?;
    config.wasm_simd(true);
    config.wasm_relaxed_simd(true);
    config.relaxed_simd_deterministic(false);
    config.wasm_tail_call(true);
    config.wasm_bulk_memory(true);

    let engine =
        into_anyhow(Engine::new(&config)).context("Engine::new failed")?;
    let module = into_anyhow(Module::from_binary(&engine, wasm_bytes))
        .context("sqlite3 Module::from_binary failed")?;

    // Linker with stub WASI + `bench.*` imports. All stubs are no-ops
    // returning success; this is sufficient for the in-memory speedtest1
    // workload which doesn't actually need real I/O.
    let mut linker: Linker<()> = Linker::new(&engine);
    register_stub_wasi(&mut linker)?;
    register_stub_bench(&mut linker)?;

    let mut store = Store::new(&engine, ());
    let instance = into_anyhow(linker.instantiate(&mut store, &module))
        .context("sqlite3 instantiate failed (linker missing import?)")?;

    let start = into_anyhow(instance.get_typed_func::<(), ()>(&mut store, "_start"))
        .context("sqlite3 export `_start` not found")?;
    let load_time = load_start.elapsed();

    // Single-shot run. _start drives the full speedtest1 sequence.
    let cpu_before = taskinfo::thread_times();
    let events_before = taskinfo::events_info();

    let it_start = Instant::now();
    into_anyhow(start.call(&mut store, ())).context("sqlite3 _start trapped")?;
    let elapsed = it_start.elapsed();

    let cpu_after = taskinfo::thread_times();
    let events_after = taskinfo::events_info();
    let basic = taskinfo::basic_info();

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
        result: 0,
        iterations: 1,
        load_time,
        run_min: elapsed,
        run_median: elapsed,
        run_p99: elapsed,
        cpu_user_ns,
        cpu_system_ns,
        rss_peak_bytes,
        page_faults,
    })
}

/// Stub WASI preview-1 functions. All return 0 (success) and do
/// nothing useful with their arguments — the in-memory speedtest1
/// path doesn't depend on real I/O.
fn register_stub_wasi(linker: &mut Linker<()>) -> Result<()> {
    let module = "wasi_snapshot_preview1";

    // fd_close(fd: i32) -> errno: i32
    into_anyhow(linker.func_wrap(module, "fd_close", |_caller: Caller<'_, ()>, _fd: i32| 0i32))
        .context("link fd_close")?;

    // fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr) -> errno
    // We need to populate `nwritten_ptr` with a plausible byte count
    // (the sum of iov lengths) so the caller doesn't loop. Read the
    // iovec lengths from linear memory and write the total back.
    into_anyhow(linker.func_wrap(
        module,
        "fd_write",
        |mut caller: Caller<'_, ()>,
         _fd: i32,
         iovs_ptr: i32,
         iovs_len: i32,
         nwritten_ptr: i32|
         -> i32 {
            let mem = match caller.get_export("memory").and_then(|e| e.into_memory()) {
                Some(m) => m,
                None => return 28, // ENOSPC; arbitrary, just avoid 0
            };
            let data = mem.data_mut(&mut caller);
            let mut total: u32 = 0;
            for i in 0..iovs_len {
                let entry = (iovs_ptr as usize) + (i as usize) * 8;
                if entry + 8 > data.len() {
                    return 28;
                }
                // iovec layout: u32 buf_ptr, u32 buf_len.
                let len = u32::from_le_bytes(data[entry + 4..entry + 8].try_into().unwrap());
                total = total.saturating_add(len);
            }
            let np = nwritten_ptr as usize;
            if np + 4 <= data.len() {
                data[np..np + 4].copy_from_slice(&total.to_le_bytes());
            }
            0
        },
    ))
    .context("link fd_write")?;

    // fd_read: behaves like EOF — wrote 0 bytes, return success.
    into_anyhow(linker.func_wrap(
        module,
        "fd_read",
        |mut caller: Caller<'_, ()>,
         _fd: i32,
         _iovs_ptr: i32,
         _iovs_len: i32,
         nread_ptr: i32|
         -> i32 {
            if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                let data = mem.data_mut(&mut caller);
                let np = nread_ptr as usize;
                if np + 4 <= data.len() {
                    data[np..np + 4].copy_from_slice(&0u32.to_le_bytes());
                }
            }
            0
        },
    ))
    .context("link fd_read")?;

    // fd_sync, fd_fdstat_get: no-op success.
    into_anyhow(linker.func_wrap(module, "fd_sync", |_c: Caller<'_, ()>, _fd: i32| 0i32))
        .context("link fd_sync")?;
    into_anyhow(linker.func_wrap(
        module,
        "fd_fdstat_get",
        |_c: Caller<'_, ()>, _fd: i32, _stat_ptr: i32| 0i32,
    ))
    .context("link fd_fdstat_get")?;

    // environ_sizes_get(num_envvars_ptr, envvars_buf_size_ptr) -> errno
    // Report zero env vars.
    into_anyhow(linker.func_wrap(
        module,
        "environ_sizes_get",
        |mut caller: Caller<'_, ()>, num_ptr: i32, sz_ptr: i32| -> i32 {
            if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                let data = mem.data_mut(&mut caller);
                for &p in &[num_ptr, sz_ptr] {
                    let p = p as usize;
                    if p + 4 <= data.len() {
                        data[p..p + 4].copy_from_slice(&0u32.to_le_bytes());
                    }
                }
            }
            0
        },
    ))
    .context("link environ_sizes_get")?;

    // environ_get: no env to copy; success.
    into_anyhow(linker.func_wrap(
        module,
        "environ_get",
        |_c: Caller<'_, ()>, _envp: i32, _buf: i32| 0i32,
    ))
    .context("link environ_get")?;

    // fd_seek(fd, offset, whence, newoffset_ptr) -> errno; report newoffset = 0.
    into_anyhow(linker.func_wrap(
        module,
        "fd_seek",
        |mut caller: Caller<'_, ()>,
         _fd: i32,
         _offset: i64,
         _whence: i32,
         newoffset_ptr: i32|
         -> i32 {
            if let Some(mem) = caller.get_export("memory").and_then(|e| e.into_memory()) {
                let data = mem.data_mut(&mut caller);
                let p = newoffset_ptr as usize;
                if p + 8 <= data.len() {
                    data[p..p + 8].copy_from_slice(&0u64.to_le_bytes());
                }
            }
            0
        },
    ))
    .context("link fd_seek")?;

    Ok(())
}

/// `bench.start` and `bench.end` are Sightglass's timing hooks. We do
/// our own timing around the whole `_start` invocation, so these
/// are no-ops.
fn register_stub_bench(linker: &mut Linker<()>) -> Result<()> {
    into_anyhow(linker.func_wrap("bench", "start", |_c: Caller<'_, ()>| {}))
        .context("link bench.start")?;
    into_anyhow(linker.func_wrap("bench", "end", |_c: Caller<'_, ()>| {}))
        .context("link bench.end")?;
    Ok(())
}
