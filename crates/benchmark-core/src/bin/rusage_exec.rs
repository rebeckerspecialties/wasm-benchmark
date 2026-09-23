//! Run a command and report its whole-process CPU accounting the way
//! `run_matrix` reports a timed window: user + system time, the part of it
//! spent on P-cores, instructions and cycles (rusage_info_v6), plus wall
//! time and exit status. For benchmarks driven through another program's
//! CLI (the component-model runs), so their E-core residency is measured
//! rather than assumed from `taskpolicy -b`.
//!
//!   rusage_exec [--jsonl FILE] [--label TEXT] -- CMD [ARGS...]
//!
//! stdin/stdout/stderr are inherited. The JSON line goes to FILE (appended)
//! or to stderr. The exit status is passed through.

use std::io::Write;
use std::os::raw::c_int;
use std::process::Command;
use std::time::Instant;

use benchmark_core::residency;

#[repr(C)]
struct SigInfo {
    _opaque: [u64; 13], // siginfo_t is 104 bytes on Darwin arm64
}

const P_PID: c_int = 1;
const WEXITED: c_int = 0x04;
const WNOWAIT: c_int = 0x20;

unsafe extern "C" {
    fn waitid(idtype: c_int, id: u32, info: *mut SigInfo, options: c_int) -> c_int;
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let sep = args.iter().position(|a| a == "--").expect("usage: rusage_exec [..] -- CMD ARGS");
    let opt = |flag: &str| {
        args[..sep].iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
    };
    let jsonl = opt("--jsonl");
    let label = opt("--label").unwrap_or_default();
    let cmd = &args[sep + 1..];
    assert!(!cmd.is_empty(), "no command after --");

    let t0 = Instant::now();
    let mut child = Command::new(&cmd[0]).args(&cmd[1..]).spawn().expect("spawn");
    let pid = child.id();
    // Wait for exit without reaping, so the child's rusage is still readable.
    let mut info = SigInfo { _opaque: [0; 13] };
    let rc = unsafe { waitid(P_PID, pid, &mut info, WEXITED | WNOWAIT) };
    let wall_ns = t0.elapsed().as_nanos() as u64;
    let usage = if rc == 0 { residency::proc_usage_of(pid as i32) } else { None };
    let status = child.wait().expect("wait");
    let code = status.code().unwrap_or(-1);

    let line = match usage {
        Some(u) => {
            let e_share = if u.cpu_ns > 0 { 1.0 - (u.p_cpu_ns as f64 / u.cpu_ns as f64).min(1.0) } else { f64::NAN };
            format!(
                "{{\"label\":{:?},\"exit\":{code},\"wall_ns\":{wall_ns},\"cpu_ns\":{},\"p_cpu_ns\":{},\
                 \"e_share\":{e_share:.4},\"instructions\":{},\"cycles\":{}}}",
                label, u.cpu_ns, u.p_cpu_ns, u.instructions, u.cycles
            )
        }
        None => format!("{{\"label\":{:?},\"exit\":{code},\"wall_ns\":{wall_ns},\"error\":\"no rusage\"}}", label),
    };
    match jsonl {
        Some(p) => {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).expect("jsonl");
            writeln!(f, "{line}").unwrap();
        }
        None => eprintln!("{line}"),
    }
    std::process::exit(code);
}
