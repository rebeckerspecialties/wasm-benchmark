//! femtovg E2E CLI: one runtime per process (so the peak footprint is that
//! runtime's), the guest build that runtime supports best unless told
//! otherwise, the ABI's fixed zoom schedule on a 1024×1024 offscreen Metal
//! texture. Prints one JSON line (and appends it to --jsonl).
//!
//!   run_femtovg_e2e --runtime pulley [--variant best|simd128|relaxed|scalar]
//!                   [--scene 0] [--frames 121] [--size 1024] [--passes 2]
//!                   [--png final.png] [--jsonl results.jsonl]
//!
//! Wrap with `taskpolicy -b` for the E-cluster; the JSON records the
//! measured pass's E-core share.

use std::io::Write;

use benchmark_core::cases::RUNTIMES;
use benchmark_core::femtovg_e2e::{run_e2e, E2eConfig, Variant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let token = get("--runtime").unwrap_or_else(|| "pulley".into());
    let (rt, _, _) = *RUNTIMES
        .iter()
        .find(|r| r.1 == token)
        .unwrap_or_else(|| panic!("unknown runtime `{token}`"));
    let variant = match get("--variant").as_deref() {
        None | Some("best") => Variant::best_for(rt),
        Some(v) => Variant::parse(v).unwrap_or_else(|| panic!("unknown variant `{v}`")),
    };
    let num = |flag: &str, d: u32| get(flag).map(|v| v.parse().expect(flag)).unwrap_or(d);
    let size = num("--size", 1024);
    let cfg = E2eConfig {
        scene: num("--scene", 0),
        frames: num("--frames", 121),
        width: size,
        height: size,
        passes: num("--passes", 2),
    };
    // WAMR's stack-guard setup must run on the main thread first.
    let _ = benchmark_core::wamr::init();
    let png = get("--png").map(std::path::PathBuf::from);
    match run_e2e(rt, variant, cfg, png.as_deref()) {
        Ok(report) => {
            let line = report.to_json();
            println!("{line}");
            if let Some(p) = get("--jsonl") {
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).expect("--jsonl");
                writeln!(f, "{line}").unwrap();
            }
        }
        Err(e) => {
            let line = format!(
                "{{\"runtime\":\"{token}\",\"variant\":\"{}\",\"scene\":{},\"error\":{:?}}}",
                variant.name(),
                cfg.scene,
                format!("{e:#}")
            );
            println!("{line}");
            if let Some(p) = get("--jsonl") {
                let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).expect("--jsonl");
                writeln!(f, "{line}").unwrap();
            }
            std::process::exit(1);
        }
    }
}
