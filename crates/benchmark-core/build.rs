// Build script:
//   1. Wire `cargo:rerun-if-changed` for every checked-in `workloads/*.wasm`
//      so changing a workload triggers a rebuild.
//   2. Tell cargo where to find the platform-appropriate WAMR static
//      library (`libiwasm.a`) and link it.

use std::path::{Path, PathBuf};
use std::process::Command;

const WORKLOADS_DIR_REL: &str = "../../workloads";

/// `git` in `dir`, trimmed stdout on success.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Exports `BENCH_VERSION_<KEY>` (the release the runtime's submodule is
/// checked out at, plus the commits past it: "2.4.1+364"),
/// `BENCH_COMMIT_<KEY>` (8-hex short hash) and `BENCH_PATCHES_<KEY>` (the
/// number of patches in `patches/<patch_dir>`, which the build scripts
/// apply to the work tree only, so HEAD stays at the pinned commit).
fn submodule_version(repo: &Path, submodule: &str, patch_dir: &str, key: &str) {
    let dir = repo.join(submodule);
    if let Some(git_dir) = git(&dir, &["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={git_dir}/HEAD");
    }
    let patches = repo.join("patches").join(patch_dir);
    println!("cargo:rerun-if-changed={}", patches.display());
    let n_patches = std::fs::read_dir(&patches)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "patch"))
                .count()
        })
        .unwrap_or(0);
    let commit = git(&dir, &["rev-parse", "--short=8", "HEAD"]).unwrap_or_default();
    // `v49.0.0-9-g0d9aebd66d` -> `49.0.0+9`; shallow clones have no tags.
    let version = git(&dir, &["describe", "--tags", "--long", "HEAD"])
        .and_then(|d| {
            let mut parts = d.rsplitn(3, '-');
            let (_hash, ahead, tag) = (parts.next()?, parts.next()?, parts.next()?);
            let tag = tag.trim_start_matches("WAMR-").trim_start_matches('v');
            Some(if ahead == "0" { tag.to_string() } else { format!("{tag}+{ahead}") })
        })
        .unwrap_or_default();
    println!("cargo:rustc-env=BENCH_VERSION_{key}={version}");
    println!("cargo:rustc-env=BENCH_COMMIT_{key}={commit}");
    println!("cargo:rustc-env=BENCH_PATCHES_{key}={n_patches}");
}

/// tinywasm is a Cargo dependency: its version, and for a git dependency
/// the 8-hex commit, come from the lockfile.
fn locked_package(repo: &Path, package: &str) -> (String, String) {
    let lock = repo.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let text = std::fs::read_to_string(lock).unwrap_or_default();
    let Some(start) = text.find(&format!("name = \"{package}\"\n")) else {
        return (String::new(), String::new());
    };
    let entry = text[start..].split("\n\n").next().unwrap_or_default();
    let field = |key: &str| {
        entry
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key} = \"")))
            .and_then(|v| v.strip_suffix('"'))
            .unwrap_or_default()
            .to_string()
    };
    // source = "git+https://...?rev=<sha>#<sha>"
    let source = field("source");
    let commit = match source.strip_prefix("git+").and_then(|s| s.rsplit_once('#')) {
        Some((_, sha)) => sha.chars().take(8).collect(),
        None => String::new(),
    };
    (field("version"), commit)
}

/// Resolve the per-Apple-target output dir for a runtime that follows
/// the shared "host = build, cross = build-<triple>" layout that the
/// build-{wamr,wasm3,wasmedge,zwasm}.sh scripts use.
fn apple_target_subdir(target: &str) -> Option<String> {
    match target {
        "aarch64-apple-darwin" | "x86_64-apple-darwin" => Some("build".to_string()),
        "aarch64-apple-ios"
        | "aarch64-apple-ios-sim"
        | "arm64_32-apple-watchos"
        | "aarch64-apple-watchos"
        | "aarch64-apple-watchos-sim"
        | "aarch64-apple-tvos"
        | "aarch64-apple-tvos-sim"
        | "aarch64-apple-visionos"
        | "aarch64-apple-visionos-sim" => Some(format!("build-{}", target)),
        _ => None,
    }
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest = Path::new(&manifest_dir);

    // Set below when the runtime's static library exists for the target.
    for cfg in ["have_wamr", "have_wasmedge", "have_wasmz", "have_zwasm", "have_wasm3"] {
        println!("cargo::rustc-check-cfg=cfg({cfg})");
    }

    // -- engine versions (shown by the app) -----------------------------
    let repo = manifest.join("../..");
    submodule_version(&repo, "wasmtime", "wasmtime", "PULLEY");
    submodule_version(&repo, "wasm-micro-runtime", "wasm-micro-runtime", "WAMR");
    submodule_version(&repo, "wasm3", "wasm3", "WASM3");
    submodule_version(&repo, "WasmEdge", "wasmedge", "WASMEDGE");
    submodule_version(&repo, "zwasm", "zwasm", "ZWASM");
    submodule_version(&repo, "wasmz", "wasmz", "WASMZ");
    let (version, commit) = locked_package(&repo, "tinywasm");
    println!("cargo:rustc-env=BENCH_VERSION_TINYWASM={version}");
    println!("cargo:rustc-env=BENCH_COMMIT_TINYWASM={commit}");

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
    let wamr_dir: PathBuf =
        wamr_root.join(apple_target_subdir(&target).unwrap_or_else(|| "build".to_string()));
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

    // -- WasmEdge static lib -------------------------------------------
    // Per-target trees written by scripts/build-wasmedge.sh, mirroring
    // the WAMR / wasm3 layout. Built with WASMEDGE_USE_LLVM=OFF (pure
    // interp) + the 27-patch Apple-mobile patch series in
    // patches/wasmedge/. Output is `libwasmedge.a` per target.
    let wasmedge_root = manifest.join("../../WasmEdge");
    let wasmedge_subdir =
        apple_target_subdir(&target).unwrap_or_else(|| "build".to_string());
    let wasmedge_dir: PathBuf = wasmedge_root.join(&wasmedge_subdir);
    let libwasmedge_a = wasmedge_dir.join("libwasmedge.a");
    println!("cargo:rerun-if-changed={}", libwasmedge_a.display());
    if libwasmedge_a.exists() {
        println!("cargo:rustc-link-search=native={}", wasmedge_dir.display());
        // The patched build emits a single archive (libwasmedge.a)
        // assembled from libwasmedgeVM.a + libwasmedgeCAPI.a + ... +
        // libfmt.a + libspdlog.a (see lib/api/CMakeLists.txt). We link
        // it whole — no need to enumerate sub-libs.
        println!("cargo:rustc-link-lib=static=wasmedge");
        // C++ runtime — WasmEdge is C++17. Apple's libc++ ships with
        // the OS, no extra search path needed.
        println!("cargo:rustc-link-lib=c++");
        println!("cargo:rustc-cfg=have_wasmedge");
    } else {
        println!(
            "cargo:warning=libwasmedge.a not found at {} — WasmEdge runtime unavailable for this target",
            libwasmedge_a.display()
        );
    }

    // -- wasmz static lib ----------------------------------------------
    // Per-target trees written by scripts/build-wasmz.sh via Zig 0.16
    // `zig build static-lib -Doptimize=ReleaseFast`. Patched (see
    // patches/wasmz/0001-zig-0.16-stdlib-port.patch + 0002-arm64_32-
    // apple-watchos-support.patch) so the sources build with 0.16
    // instead of the upstream-required 0.15.2 (Zig 0.15's build runner
    // segfaults on macOS 26 Tahoe). arm64_32-apple-watchos is enabled
    // via patch 0002 (single_threaded + self-contained panic/logFn);
    // Zig 0.16 spells the triple `aarch64-watchos-ilp32`.
    let wasmz_root = manifest.join("../../wasmz");
    let wasmz_subdir =
        apple_target_subdir(&target).unwrap_or_else(|| "build".to_string());
    let wasmz_dir: PathBuf = wasmz_root.join(&wasmz_subdir);
    let libwasmz_a = wasmz_dir.join("libwasmz.a");
    println!("cargo:rerun-if-changed={}", libwasmz_a.display());
    if libwasmz_a.exists() {
        println!("cargo:rustc-link-search=native={}", wasmz_dir.display());
        println!("cargo:rustc-link-lib=static=wasmz");
        println!("cargo:rustc-cfg=have_wasmz");
    } else {
        println!(
            "cargo:warning=libwasmz.a not found at {} — wasmz runtime unavailable for this target",
            libwasmz_a.display()
        );
    }

    // -- zwasm static lib ----------------------------------------------
    // Per-target trees written by scripts/build-zwasm.sh via Zig 0.16
    // `zig build static-lib -Djit=false`. macOS uses bare `build/`,
    // cross targets use `build-<triple>`. arm64_32-apple-watchos is
    // enabled via patches/zwasm/0001-arm64_32-apple-watchos-support
    // (single_threaded + ILP32 narrowing fixes + self-contained
    // panic/logFn); Zig 0.16 spells the triple `aarch64-watchos-ilp32`.
    let zwasm_root = manifest.join("../../zwasm");
    let zwasm_subdir =
        apple_target_subdir(&target).unwrap_or_else(|| "build".to_string());
    let zwasm_dir: PathBuf = zwasm_root.join(&zwasm_subdir);
    let libzwasm_a = zwasm_dir.join("libzwasm.a");
    println!("cargo:rerun-if-changed={}", libzwasm_a.display());
    if libzwasm_a.exists() {
        println!("cargo:rustc-link-search=native={}", zwasm_dir.display());
        println!("cargo:rustc-link-lib=static=zwasm");
        println!("cargo:rustc-cfg=have_zwasm");
    } else {
        println!(
            "cargo:warning=libzwasm.a not found at {} — zwasm runtime unavailable for this target",
            libzwasm_a.display()
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
