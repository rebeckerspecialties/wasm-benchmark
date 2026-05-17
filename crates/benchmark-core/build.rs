// Build script:
//   1. Wire `cargo:rerun-if-changed` for every checked-in `workloads/*.wasm`
//      so changing a workload triggers a rebuild.
//   2. Tell cargo where to find the platform-appropriate WAMR static
//      library (`libiwasm.a`) and link it.

use std::path::{Path, PathBuf};

const WORKLOADS_DIR_REL: &str = "../../workloads";

/// Resolve the per-Apple-target output dir for a runtime that follows
/// the shared "host = build, cross = build-<triple>" layout that the
/// build-{wamr,wasm3,wasmedge,zwasm}.sh scripts use.
fn apple_target_subdir(target: &str) -> Option<String> {
    match target {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => Some("build".to_string()),
        "aarch64-apple-ios"
        | "aarch64-apple-ios-sim"
        | "arm64_32-apple-watchos"
        | "aarch64-apple-watchos-sim"
        | "aarch64-apple-tvos"
        | "aarch64-apple-tvos-sim" => Some(format!("build-{}", target)),
        _ => None,
    }
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest = Path::new(&manifest_dir);

    // -- workloads -----------------------------------------------------
    let workloads = manifest.join(WORKLOADS_DIR_REL);
    println!("cargo:rerun-if-changed={}", workloads.display());
    if let Ok(entries) = std::fs::read_dir(&workloads) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "wasm") {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    // -- WAMR static lib ------------------------------------------------
    // Per-target build output trees, written by scripts/build-wamr.sh.
    // Each directory contains a `libiwasm.a` produced with:
    //   WAMR_BUILD_FAST_INTERP=1 WAMR_BUILD_AOT=0 WAMR_BUILD_JIT=0
    //   WAMR_BUILD_SIMD=1 _BULK_MEMORY=1 _TAIL_CALL=1 _REF_TYPES=1
    // All Apple targets share the `darwin/CMakeLists.txt` build path
    // (the iOS sub-CMakeLists hardcodes a SHARED library and isn't
    // usable for our staticlib link — see scripts/build-wamr.sh). One
    // output dir per cargo TARGET triple keeps things simple.
    let target = std::env::var("TARGET").unwrap_or_default();
    let wamr_root = manifest.join("../../wasm-micro-runtime/product-mini/platforms/darwin");
    let wamr_dir: PathBuf = match target.as_str() {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => wamr_root.join("build"),
        "aarch64-apple-ios"
        | "aarch64-apple-ios-sim"
        | "arm64_32-apple-watchos"
        | "aarch64-apple-watchos-sim"
        | "aarch64-apple-tvos"
        | "aarch64-apple-tvos-sim" => wamr_root.join(format!("build-{}", target)),
        _ => wamr_root.join("build"),
    };
    let iwasm_a = wamr_dir.join("libiwasm.a");
    println!("cargo:rerun-if-changed={}", iwasm_a.display());
    if iwasm_a.exists() {
        println!("cargo:rustc-link-search=native={}", wamr_dir.display());
        println!("cargo:rustc-link-lib=static=iwasm");
        println!("cargo:rustc-cfg=have_wamr");
    } else {
        // No libiwasm.a for this target yet — skip the WAMR link. The
        // `wamr` module's FFI calls are still type-checked but the
        // produced staticlib is missing the WAMR symbols, so any caller
        // that actually invokes WAMR will fail to link at the app stage.
        // That's acceptable until scripts/build-wamr.sh fans out to
        // arm64_32-apple-watchos and the iOS targets.
        println!(
            "cargo:warning=libiwasm.a not found at {} — WAMR runtime unavailable for this target",
            iwasm_a.display()
        );
    }

    // -- wasm3 static lib ----------------------------------------------
    // Per-target trees written by scripts/build-wasm3.sh. Same naming
    // convention as WAMR: `wasm3/build` for the host, `wasm3/build-<triple>`
    // for cross builds. We compile only the 11 core m3 sources (no WASI /
    // tracer / uvwasi), so the output is a self-contained libm3.a.
    let wasm3_root = manifest.join("../../wasm3");
    let wasm3_subdir =
        apple_target_subdir(&target).unwrap_or_else(|| "build".to_string());
    let wasm3_dir: PathBuf = wasm3_root.join(&wasm3_subdir);
    let libm3_a = wasm3_dir.join("libm3.a");
    println!("cargo:rerun-if-changed={}", libm3_a.display());
    if libm3_a.exists() {
        println!("cargo:rustc-link-search=native={}", wasm3_dir.display());
        println!("cargo:rustc-link-lib=static=m3");
        println!("cargo:rustc-cfg=have_wasm3");
    } else {
        println!(
            "cargo:warning=libm3.a not found at {} — wasm3 runtime unavailable for this target",
            libm3_a.display()
        );
    }
}
