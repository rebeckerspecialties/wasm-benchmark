//! Standalone tinywasm runner: per-call instructions and cycles, and the
//! dispatched opcode-pair histogram behind the 2026-09-26 tinywasm report.
//!
//! Build (the harness's nightly, same codegen flags as the device libs):
//!   RUSTFLAGS="-C target-cpu=apple-a12" cargo +nightly-2026-07-05 build --release
//! Histogram build: apply op-histogram.patch to a tinywasm checkout that has
//! upstream #64 (the patch counts in #64's dispatch macro), then add
//!   --features hist,tinywasm/op-histogram \
//!   --config 'patch."https://github.com/explodingcamera/tinywasm".tinywasm.path="<checkout>/crates/tinywasm"'
//!
//! Usage: twrun bench|hist <module.wasm> <export> <i32 arg> <calls> [top]
//!   `export` must be (i32) -> i32. One warm-up call, then `calls` counted
//!   calls; counters are the process's (proc_pid_rusage, RUSAGE_INFO_V4), so
//!   run it under `taskpolicy -b` for the E-cores. The module may import
//!   `env.f: (i32) -> i32` (a typed host function returning its argument + 1).
use std::time::Instant;
use tinywasm::{HostFunction, Imports, ModuleInstance, Store};

unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
    fn getpid() -> i32;
}
/// (instructions, cycles) of this process so far (RUSAGE_INFO_V4).
fn counters() -> (u64, u64) {
    let mut b = [0u64; 64];
    unsafe { proc_pid_rusage(getpid(), 4, b.as_mut_ptr()) };
    (b[31], b[32])
}

fn instantiate(store: &mut Store, wasm: &[u8]) -> ModuleInstance {
    let module = tinywasm::parse_bytes(wasm).expect("parse");
    let mut imports = Imports::new();
    imports.define("env", "f", HostFunction::from(|_ctx, x: i32| Ok(x.wrapping_add(1))));
    ModuleInstance::instantiate(store, &module, Some(&imports)).expect("instantiate")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args[1].as_str();
    let wasm = std::fs::read(&args[2]).expect("read wasm");
    let func_name = &args[3];
    let arg: i32 = args[4].parse().unwrap();
    let calls: u32 = args[5].parse().unwrap();
    let mut store = Store::default();
    let instance = instantiate(&mut store, &wasm);
    let func = instance.func::<i32, i32>(&store, func_name).expect("export");
    // warm-up call, not counted
    let mut r = func.call(&mut store, arg).expect("call");
    #[cfg(feature = "hist")]
    let _ = tinywasm::op_histogram::take();
    let (i0, c0) = counters();
    let t = Instant::now();
    for _ in 0..calls {
        r = func.call(&mut store, arg).expect("call");
    }
    let el = t.elapsed();
    let (i1, c1) = counters();
    let n = calls as f64;
    println!("{func_name}({arg}) = {r}; per call: {:.3} ms, {:.0} instructions, {:.0} cycles, IPC {:.2}",
        el.as_secs_f64() * 1e3 / n, (i1 - i0) as f64 / n, (c1 - c0) as f64 / n, (i1 - i0) as f64 / (c1 - c0) as f64);
    #[cfg(feature = "hist")]
    if mode == "hist" {
        let pairs = tinywasm::op_histogram::take();
        let total: u64 = pairs.iter().map(|p| p.2).sum();
        println!("dispatches per call: {:.0}; instructions per dispatch: {:.1}", total as f64 / n, (i1 - i0) as f64 / total as f64);
        let mut ops: std::collections::BTreeMap<&str, u64> = Default::default();
        for &(_, next, c) in &pairs { *ops.entry(next).or_default() += c; }
        let mut ops: Vec<_> = ops.into_iter().collect();
        ops.sort_by(|a, b| b.1.cmp(&a.1));
        let top: usize = args.get(6).map(|s| s.parse().unwrap()).unwrap_or(40);
        println!("\n{:>6} {:>6}  op", "share", "cum");
        let mut cum = 0.0;
        for (op, c) in ops.iter().take(top) {
            let s = *c as f64 / total as f64; cum += s;
            println!("{:>5.1}% {:>5.1}%  {op}", 100.0 * s, 100.0 * cum);
        }
        let mut pairs = pairs;
        pairs.sort_by(|a, b| b.2.cmp(&a.2));
        println!("\n{:>6}  pair", "share");
        for (p, nx, c) in pairs.iter().take(top) {
            println!("{:>5.1}%  {p} -> {nx}", 100.0 * *c as f64 / total as f64);
        }
    }
    let _ = mode;
}
