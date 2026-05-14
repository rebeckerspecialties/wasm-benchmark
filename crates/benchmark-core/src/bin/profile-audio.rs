//! Single-workload steady-state runner for profiling.
//!
//! Runs `audio_dsp` repeatedly in a tight loop after one warmup so an
//! attached `xctrace` capture sees a long stable measurement window.
//!
//!   taskpolicy -b -t 5 -- target/release/profile-audio 50
//!     ^^ background-throttle pins this on M4 E-cores
//!
//! Bench harness picks the workload and iteration count via argv:
//!   profile-audio [iters=20]   audio_dsp
//!   profile-audio matmul_fma 50
//!   profile-audio convolution 50

use std::time::Instant;

use anyhow::Result;
use benchmark_core::{
    run_audio_dsp, run_convolution, run_matmul_fma, run_matmul_simd, run_crc32, run_fib,
    run_fib_tail, run_sieve, RunReport,
};

fn dispatch(name: &str) -> Result<fn() -> Result<RunReport>> {
    Ok(match name {
        "audio_dsp" => || run_audio_dsp(0x5C7),
        "matmul_simd" => || run_matmul_simd(0xBEEF),
        "matmul_fma" => || run_matmul_fma(0xBEEF),
        "convolution" => || run_convolution(0xCAFE),
        "crc32" => || run_crc32(0xC0FFEE),
        "fib" => || run_fib(30),
        "fib_tail" => || run_fib_tail(100_000),
        "sieve" => || run_sieve(10_000),
        other => anyhow::bail!("unknown workload: {other}"),
    })
}

// Print ASLR slide of the main image so atos / our offline analyzer can
// translate runtime PC samples back to static binary offsets.
#[cfg(target_vendor = "apple")]
fn print_slide() {
    extern "C" {
        fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
    }
    let slide = unsafe { _dyld_get_image_vmaddr_slide(0) };
    eprintln!("aslr_slide  : 0x{slide:x}");
}
#[cfg(not(target_vendor = "apple"))]
fn print_slide() {}

fn main() -> Result<()> {
    print_slide();

    let mut args = std::env::args().skip(1);
    let workload = args.next().unwrap_or_else(|| "audio_dsp".to_string());
    let iters: u32 = args
        .next()
        .as_deref()
        .map(str::parse)
        .transpose()?
        .unwrap_or(20);

    let runner = dispatch(&workload)?;

    eprintln!("workload   : {workload}");
    eprintln!("iterations : {iters}");
    eprintln!("(running 1 warmup, then measured loop)");

    // Warm up.
    let _ = runner()?;

    let mut total_load_ns: u128 = 0;
    let mut total_run_ns: u128 = 0;
    let mut last_result: i32 = 0;
    let wall = Instant::now();
    for i in 0..iters {
        let r = runner()?;
        total_load_ns += r.load_time.as_nanos();
        total_run_ns += r.run_median.as_nanos();
        last_result = r.result;
        if i % 5 == 0 {
            eprintln!("  iter {i}/{iters}  result={}", r.result);
        }
    }
    let wall_total = wall.elapsed();

    eprintln!();
    eprintln!("result          : {last_result}");
    eprintln!("wall total      : {wall_total:.3?}");
    eprintln!("avg load+lower  : {:.3} ms", total_load_ns as f64 / 1e6 / iters as f64);
    eprintln!("avg run-median  : {:.3} ms", total_run_ns as f64 / 1e6 / iters as f64);
    Ok(())
}
