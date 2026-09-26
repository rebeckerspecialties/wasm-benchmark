//! Component-model async / WASI 0.3 benchmark. Self-timed with
//! `wasi:clocks/monotonic-clock@0.3.0.now`, so runtime startup and
//! compilation stay out of the numbers. Each phase runs `REPS` times and
//! prints one line per rep to stderr:
//!
//!   cm_async <phase> rep=<r> n=<ops> total_ns=<t> ns_per_op=<t/ops>
//!
//! Phases, each a hot path of the async canonical ABI:
//! - `wait_for_0`: sequential `monotonic-clock.wait-for(0)` calls, an
//!   async-lowered host import that is ready immediately (subtask start,
//!   return and waitable bookkeeping per call).
//! - `concurrent_wait`: 1000 `wait-for(0)` subtasks in flight at once,
//!   then joined (a waitable set with many members, guest task switching).
//! - `stdout_stream`: 2 MiB through `stdout.write-via-stream` as 8192
//!   `stream<u8>` writes of 256 bytes (stream.write plus the host's reads).
//!   The runner checks that stdout received exactly `REPS` × 2 MiB of the
//!   expected bytes. If the host stops accepting bytes the phase reports
//!   `stalled` instead of hanging.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;

use wasip3::cli::stdout;
use wasip3::clocks::monotonic_clock;
use wasip3::wit_bindgen::StreamResult;

#[global_allocator]
static ALLOC: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

wasip3::cli::command::export!(Bench);

struct Bench;

const REPS: u32 = 5;
const SEQ_CALLS: u32 = 20_000;
const CONCURRENT: u32 = 1_000;
const CHUNK: usize = 256;
const CHUNKS: u32 = 8_192;
/// Consecutive zero-progress writes after which the stream phase gives up.
const STALL_LIMIT: u32 = 1_000;

fn log(line: &str) {
    let err = wasip2::cli::stderr::get_stderr();
    let _ = err.blocking_write_and_flush(line.as_bytes());
    let _ = err.blocking_write_and_flush(b"\n");
}

fn report(phase: &str, rep: u32, n: u64, t: u64) {
    log(&format!("cm_async {phase} rep={rep} n={n} total_ns={t} ns_per_op={:.1}", t as f64 / n as f64));
}

/// Writes `CHUNKS` chunks; returns (bytes accepted, stalled). Chunk `i`
/// holds bytes `(i * 31 + j) as u8` for `j` in `0..CHUNK`.
async fn stream_to_stdout() -> (u64, bool) {
    let (mut tx, rx) = wasip3::wit_stream::new();
    let mut written: u64 = 0;
    let mut stalled = false;
    let (_res, ()) = futures::join!(async { stdout::write_via_stream(rx).await }, async {
        'outer: for i in 0..CHUNKS {
            let chunk: Vec<u8> = (0..CHUNK).map(|j| (i as usize * 31 + j) as u8).collect();
            let (mut status, mut buf) = tx.write(chunk).await;
            let mut idle = 0;
            loop {
                match status {
                    StreamResult::Complete(n) => {
                        written += n as u64;
                        idle = if n == 0 { idle + 1 } else { 0 };
                    }
                    StreamResult::Cancelled => idle += 1,
                    StreamResult::Dropped => {
                        stalled = true;
                        break 'outer;
                    }
                }
                if buf.remaining() == 0 {
                    break;
                }
                if idle >= STALL_LIMIT {
                    stalled = true;
                    break 'outer;
                }
                (status, buf) = tx.write_buf(buf).await;
            }
        }
        drop(tx);
    });
    (written, stalled)
}

impl wasip3::exports::cli::run::Guest for Bench {
    async fn run() -> Result<(), ()> {
        log("cm_async start");
        for rep in 0..REPS {
            let t0 = monotonic_clock::now();
            for _ in 0..SEQ_CALLS {
                monotonic_clock::wait_for(0).await;
            }
            report("wait_for_0", rep, SEQ_CALLS as u64, monotonic_clock::now() - t0);

            let t0 = monotonic_clock::now();
            let waits = (0..CONCURRENT).map(|_| monotonic_clock::wait_for(0));
            futures::future::join_all(waits).await;
            report("concurrent_wait", rep, CONCURRENT as u64, monotonic_clock::now() - t0);

            let t0 = monotonic_clock::now();
            let (written, stalled) = stream_to_stdout().await;
            let t = monotonic_clock::now() - t0;
            if stalled {
                log(&format!("cm_async stdout_stream rep={rep} stalled bytes_accepted={written}"));
            } else {
                report("stdout_stream", rep, CHUNKS as u64, t);
                log(&format!("cm_async stdout_stream rep={rep} bytes={written}"));
            }
        }
        Ok(())
    }
}
