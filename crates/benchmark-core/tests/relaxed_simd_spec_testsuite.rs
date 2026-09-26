//! WAMR fast-interp conformance against the upstream relaxed-SIMD
//! spec testsuite. Companion to `relaxed_simd_diff_fuzz.rs`:
//!
//!   - `relaxed_simd_diff_fuzz` compares WAMR vs wasmtime
//!     deterministic-mode on boundary inputs we picked.
//!   - this test runs the *canonical* spec assertions against
//!     WAMR, honouring the spec's `(either ...)` semantics for
//!     implementation-defined ambiguity zones. If a result is in
//!     the spec-allowed set, the test passes regardless of which
//!     specific value our impl picked.
//!
//! Why this layer exists
//! ---------------------
//! The spec testsuite is the authoritative conformance source.
//! Wasmtime's deterministic mode is *one* spec-conformant
//! implementation; our diff-fuzz harness can only catch
//! divergences from *that specific* implementation. The spec
//! testsuite, by contrast, defines the spec-allowed set
//! explicitly via `(either ...)` constructs — so any element of
//! the set is conformant. The canonical-conformance check is
//! looser (we may match a different spec-allowed value than
//! wasmtime) but it's the source of truth for "is our impl
//! actually conformant".
//!
//! How it works
//! ------------
//! The upstream `.wast` files (vendored in
//! `wasmtime/tests/spec_testsuite/relaxed_*.wast`) are converted
//! to JSON by `wast2json --enable-relaxed-simd` at test time.
//! The JSON form has explicit `"expected"` and `"either"` fields
//! that the wast crate's Rust parser doesn't surface, so we
//! shell out to wast2json (from wabt) which is the same path
//! wasmtime's own runner uses.
//!
//! Each JSON file produces:
//!   - `module` commands: load the named `.wasm` blob via WAMR
//!   - `assert_return` commands: invoke the function, compare
//!     the returned v128 against either an exact pattern or a
//!     membership in an allowed set
//!   - everything else: skip (no `assert_invalid` etc. in the
//!     relaxed-SIMD suite that's interesting for fast-interp)
//!
//! Skip behavior
//! -------------
//! If `wast2json` isn't on PATH, the test is skipped with a
//! warning rather than failed — CI/dev machines without wabt
//! installed don't get a confusing red. CI must install wabt
//! to actually run this test.

#![allow(non_camel_case_types)]

use std::collections::HashSet;
use std::ffi::{c_char, c_void, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

use anyhow::{anyhow, Result};
use benchmark_core::wamr;
use serde_json::Value;

// ----- WAMR FFI (same shape as the other harnesses) -------------

type wasm_module_t = *mut c_void;
type wasm_module_inst_t = *mut c_void;
type wasm_function_inst_t = *mut c_void;
type wasm_exec_env_t = *mut c_void;

extern "C" {
    fn wasm_runtime_load(
        buf: *mut u8,
        size: u32,
        error_buf: *mut c_char,
        error_buf_size: u32,
    ) -> wasm_module_t;
    fn wasm_runtime_unload(module: wasm_module_t);
    fn wasm_runtime_instantiate(
        module: wasm_module_t,
        stack_size: u32,
        heap_size: u32,
        error_buf: *mut c_char,
        error_buf_size: u32,
    ) -> wasm_module_inst_t;
    fn wasm_runtime_deinstantiate(module_inst: wasm_module_inst_t);
    fn wasm_runtime_lookup_function(
        module_inst: wasm_module_inst_t,
        name: *const c_char,
    ) -> wasm_function_inst_t;
    fn wasm_runtime_create_exec_env(
        module_inst: wasm_module_inst_t,
        stack_size: u32,
    ) -> wasm_exec_env_t;
    fn wasm_runtime_destroy_exec_env(exec_env: wasm_exec_env_t);
    fn wasm_runtime_call_wasm(
        exec_env: wasm_exec_env_t,
        function: wasm_function_inst_t,
        argc: u32,
        argv: *mut u32,
    ) -> bool;
    fn wasm_runtime_get_exception(module_inst: wasm_module_inst_t) -> *const c_char;
    fn wasm_runtime_clear_exception(module_inst: wasm_module_inst_t);
}

static INIT: Once = Once::new();

fn ensure_init() {
    INIT.call_once(|| {
        wamr::init().expect("wamr::init failed");
    });
}

// ----- v128 + argv glue -----------------------------------------

/// 128-bit value: 16 raw bytes, little-endian by lane.
#[derive(Clone, Copy, PartialEq, Eq)]
struct V128([u8; 16]);

impl std::fmt::Debug for V128 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v128(")?;
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                write!(f, "_")?;
            }
            write!(f, "{:02x}", b)?;
        }
        write!(f, ")")
    }
}

/// Build a V128 from a `{"type": "v128", "lane_type": "...", "value": [...]}`
/// JSON object as emitted by wast2json.
fn v128_from_json(v: &Value) -> Result<V128> {
    let lane_type = v
        .get("lane_type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("v128 missing lane_type: {v}"))?;
    let value = v
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("v128 missing value: {v}"))?;
    let mut bytes = [0u8; 16];
    match lane_type {
        "i8" => {
            if value.len() != 16 {
                return Err(anyhow!("i8 v128 needs 16 lanes, got {}", value.len()));
            }
            for (i, lane) in value.iter().enumerate() {
                let s = lane.as_str().ok_or_else(|| anyhow!("lane not string: {lane}"))?;
                bytes[i] = parse_lane_u8(s)?;
            }
        }
        "i16" => {
            if value.len() != 8 {
                return Err(anyhow!("i16 v128 needs 8 lanes, got {}", value.len()));
            }
            for (i, lane) in value.iter().enumerate() {
                let s = lane.as_str().ok_or_else(|| anyhow!("lane not string: {lane}"))?;
                let v: u16 = parse_lane_u16(s)?;
                bytes[i * 2..i * 2 + 2].copy_from_slice(&v.to_le_bytes());
            }
        }
        "i32" => {
            if value.len() != 4 {
                return Err(anyhow!("i32 v128 needs 4 lanes, got {}", value.len()));
            }
            for (i, lane) in value.iter().enumerate() {
                let s = lane.as_str().ok_or_else(|| anyhow!("lane not string: {lane}"))?;
                let v: u32 = parse_lane_u32(s)?;
                bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        "i64" => {
            if value.len() != 2 {
                return Err(anyhow!("i64 v128 needs 2 lanes, got {}", value.len()));
            }
            for (i, lane) in value.iter().enumerate() {
                let s = lane.as_str().ok_or_else(|| anyhow!("lane not string: {lane}"))?;
                let v: u64 = parse_lane_u64(s)?;
                bytes[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        "f32" => {
            if value.len() != 4 {
                return Err(anyhow!("f32 v128 needs 4 lanes, got {}", value.len()));
            }
            for (i, lane) in value.iter().enumerate() {
                let s = lane.as_str().ok_or_else(|| anyhow!("lane not string: {lane}"))?;
                let v: u32 = parse_lane_f32_bits(s)?;
                bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
        "f64" => {
            if value.len() != 2 {
                return Err(anyhow!("f64 v128 needs 2 lanes, got {}", value.len()));
            }
            for (i, lane) in value.iter().enumerate() {
                let s = lane.as_str().ok_or_else(|| anyhow!("lane not string: {lane}"))?;
                let v: u64 = parse_lane_f64_bits(s)?;
                bytes[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        other => return Err(anyhow!("unsupported lane_type: {other}")),
    }
    Ok(V128(bytes))
}

fn parse_lane_u8(s: &str) -> Result<u8> {
    // wast2json emits unsigned-decimal strings for i8 lanes.
    s.parse::<u8>()
        .or_else(|_| s.parse::<i8>().map(|v| v as u8))
        .map_err(|e| anyhow!("parse u8 from `{s}`: {e}"))
}

fn parse_lane_u16(s: &str) -> Result<u16> {
    s.parse::<u16>()
        .or_else(|_| s.parse::<i16>().map(|v| v as u16))
        .map_err(|e| anyhow!("parse u16 from `{s}`: {e}"))
}

fn parse_lane_u32(s: &str) -> Result<u32> {
    s.parse::<u32>()
        .or_else(|_| s.parse::<i32>().map(|v| v as u32))
        .map_err(|e| anyhow!("parse u32 from `{s}`: {e}"))
}

fn parse_lane_u64(s: &str) -> Result<u64> {
    s.parse::<u64>()
        .or_else(|_| s.parse::<i64>().map(|v| v as u64))
        .map_err(|e| anyhow!("parse u64 from `{s}`: {e}"))
}

/// Parse a f32 lane string. wast2json emits bit-pattern decimal
/// (so "1078523331" → u32 0x404ccccd, which is f32 3.2). It also
/// emits `nan:canonical` and `nan:arithmetic` for NaN-as-pattern.
/// For NaN patterns, the spec allows any NaN bit pattern matching
/// the kind, so we return a sentinel and let the comparator know.
/// For now, treat NaN-pattern as "any NaN" — we accept the WAMR
/// result if it's a NaN, regardless of bit pattern.
fn parse_lane_f32_bits(s: &str) -> Result<u32> {
    if s.starts_with("nan") {
        // Sentinel for "any NaN" — represented as the canonical
        // f32 NaN bit pattern; the comparator special-cases this.
        return Ok(0x7fc0_0000);
    }
    s.parse::<u32>()
        .map_err(|e| anyhow!("parse f32 bits from `{s}`: {e}"))
}

fn parse_lane_f64_bits(s: &str) -> Result<u64> {
    if s.starts_with("nan") {
        return Ok(0x7ff8_0000_0000_0000);
    }
    s.parse::<u64>()
        .map_err(|e| anyhow!("parse f64 bits from `{s}`: {e}"))
}

/// True if the JSON lane is a NaN-pattern (`"nan:canonical"` or
/// `"nan:arithmetic"`) — used to allow any NaN bit pattern in
/// the WAMR result.
fn is_nan_pattern(s: &str) -> bool {
    s.starts_with("nan")
}

/// Returns true if the given v128 (interpreted as f32x4 / f64x2)
/// contains the expected value at every lane, allowing any NaN
/// bit pattern where the expected lane is a NaN-pattern.
fn v128_eq_nan_aware(actual: &V128, expected: &Value) -> bool {
    let lane_type = match expected.get("lane_type").and_then(Value::as_str) {
        Some(t) => t,
        None => return false,
    };
    let value = match expected.get("value").and_then(Value::as_array) {
        Some(v) => v,
        None => return false,
    };
    match lane_type {
        "f32" => {
            if value.len() != 4 {
                return false;
            }
            for (i, lane) in value.iter().enumerate() {
                let s = match lane.as_str() {
                    Some(s) => s,
                    None => return false,
                };
                let actual_bits: u32 = {
                    let arr: [u8; 4] = actual.0[i * 4..i * 4 + 4].try_into().unwrap();
                    u32::from_le_bytes(arr)
                };
                if is_nan_pattern(s) {
                    // Any NaN: top 23 bits set + non-zero significand
                    let exp = (actual_bits >> 23) & 0xff;
                    let frac = actual_bits & 0x7f_ffff;
                    if exp != 0xff || frac == 0 {
                        return false;
                    }
                } else {
                    let expected_bits: u32 = match s.parse() {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    if actual_bits != expected_bits {
                        return false;
                    }
                }
            }
            true
        }
        "f64" => {
            if value.len() != 2 {
                return false;
            }
            for (i, lane) in value.iter().enumerate() {
                let s = match lane.as_str() {
                    Some(s) => s,
                    None => return false,
                };
                let actual_bits: u64 = {
                    let arr: [u8; 8] = actual.0[i * 8..i * 8 + 8].try_into().unwrap();
                    u64::from_le_bytes(arr)
                };
                if is_nan_pattern(s) {
                    let exp = (actual_bits >> 52) & 0x7ff;
                    let frac = actual_bits & 0x000f_ffff_ffff_ffff;
                    if exp != 0x7ff || frac == 0 {
                        return false;
                    }
                } else {
                    let expected_bits: u64 = match s.parse() {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    if actual_bits != expected_bits {
                        return false;
                    }
                }
            }
            true
        }
        _ => {
            // Non-float lanes: exact byte equality after parsing.
            match v128_from_json(expected) {
                Ok(e) => actual.0 == e.0,
                Err(_) => false,
            }
        }
    }
}

// ----- WAMR module wrapper --------------------------------------

struct WamrModule {
    module: wasm_module_t,
    inst: wasm_module_inst_t,
    exec: wasm_exec_env_t,
    _bytes: Vec<u8>,
}

impl WamrModule {
    fn load(bytes: Vec<u8>) -> Result<Self> {
        ensure_init();
        let mut bytes = bytes;
        let mut err = [0i8; 256];
        let module = unsafe {
            wasm_runtime_load(
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                err.as_mut_ptr() as *mut c_char,
                err.len() as u32,
            )
        };
        if module.is_null() {
            let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr() as *const c_char) }
                .to_string_lossy()
                .into_owned();
            return Err(anyhow!("load failed: {msg}"));
        }
        let inst = unsafe {
            wasm_runtime_instantiate(
                module,
                64 * 1024,
                64 * 1024,
                err.as_mut_ptr() as *mut c_char,
                err.len() as u32,
            )
        };
        if inst.is_null() {
            let msg = unsafe { std::ffi::CStr::from_ptr(err.as_ptr() as *const c_char) }
                .to_string_lossy()
                .into_owned();
            unsafe { wasm_runtime_unload(module) };
            return Err(anyhow!("instantiate failed: {msg}"));
        }
        let exec = unsafe { wasm_runtime_create_exec_env(inst, 64 * 1024) };
        if exec.is_null() {
            unsafe {
                wasm_runtime_deinstantiate(inst);
                wasm_runtime_unload(module);
            }
            return Err(anyhow!("exec_env create failed"));
        }
        Ok(Self {
            module,
            inst,
            exec,
            _bytes: bytes,
        })
    }

    /// Invoke `field` with the given v128 arguments and a v128
    /// return. WAMR's call_wasm ABI packs v128 as 4 contiguous
    /// uint32 slots in argv.
    fn invoke_v128(&self, field: &str, args: &[V128]) -> Result<V128> {
        unsafe { wasm_runtime_clear_exception(self.inst) };
        let cn = CString::new(field)?;
        let f = unsafe { wasm_runtime_lookup_function(self.inst, cn.as_ptr()) };
        if f.is_null() {
            return Err(anyhow!("lookup `{field}` failed"));
        }
        // argv must be at least max(4 * args.len(), 4) for the v128 return.
        let argv_len = std::cmp::max(4 * args.len(), 4);
        let mut argv: Vec<u32> = vec![0; argv_len];
        for (i, arg) in args.iter().enumerate() {
            for j in 0..4 {
                let arr: [u8; 4] = arg.0[j * 4..j * 4 + 4].try_into().unwrap();
                argv[i * 4 + j] = u32::from_le_bytes(arr);
            }
        }
        let ok =
            unsafe { wasm_runtime_call_wasm(self.exec, f, (4 * args.len()) as u32, argv.as_mut_ptr()) };
        if !ok {
            let p = unsafe { wasm_runtime_get_exception(self.inst) };
            let m = if p.is_null() {
                "(no msg)".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(anyhow!("trap: {m}"));
        }
        // Result is in argv[0..4] (low to high i32 lanes).
        let mut out = [0u8; 16];
        for j in 0..4 {
            out[j * 4..j * 4 + 4].copy_from_slice(&argv[j].to_le_bytes());
        }
        Ok(V128(out))
    }
}

impl Drop for WamrModule {
    fn drop(&mut self) {
        unsafe {
            wasm_runtime_destroy_exec_env(self.exec);
            wasm_runtime_deinstantiate(self.inst);
            wasm_runtime_unload(self.module);
        }
    }
}

// ----- The test driver ------------------------------------------

const SPEC_DIR: &str = "wasmtime/tests/spec_testsuite";
const WAST_FILES: &[&str] = &[
    "i16x8_relaxed_q15mulr_s.wast",
    "i32x4_relaxed_trunc.wast",
    "i8x16_relaxed_swizzle.wast",
    "relaxed_dot_product.wast",
    "relaxed_laneselect.wast",
    "relaxed_madd_nmadd.wast",
    "relaxed_min_max.wast",
];

/// Skip-list — assertions we can't run from this harness yet.
/// Each entry is (file basename, function name). Anything tagged
/// here is documented; the goal is to drive this list to empty.
fn should_skip_field(file: &str, field: &str) -> Option<&'static str> {
    // The "_cmp" helpers in relaxed_dot_product.wast and
    // relaxed_min_max.wast call the same op twice and i32x4.eq
    // the results. They test *determinism* (same inputs →
    // identical outputs) — interesting in general, but always
    // pass on a single-threaded fast-interp since there's no
    // source of nondeterminism within one process. Accept.
    let _ = (file, field);
    None
}

#[derive(Debug)]
struct Failure {
    file: String,
    line: u64,
    field: String,
    args: String,
    actual: V128,
    expected_kind: &'static str,
    expected_set: String,
}

fn run_wast_json(json_path: &Path, file_name: &str) -> Result<Vec<Failure>> {
    let json_text = fs::read_to_string(json_path)?;
    let parsed: Value = serde_json::from_str(&json_text)?;
    let commands = parsed
        .get("commands")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("no commands in JSON"))?;
    let json_dir = json_path.parent().unwrap();

    let mut current: Option<WamrModule> = None;
    let mut failures = Vec::new();
    let mut asserts_attempted = 0usize;
    let mut asserts_skipped: HashSet<String> = HashSet::new();

    for cmd in commands {
        let cmd_type = cmd.get("type").and_then(Value::as_str).unwrap_or("");
        match cmd_type {
            "module" => {
                let filename = cmd
                    .get("filename")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("module command missing filename"))?;
                let wasm_path = json_dir.join(filename);
                let bytes = fs::read(&wasm_path)
                    .map_err(|e| anyhow!("read {wasm_path:?}: {e}"))?;
                current = Some(WamrModule::load(bytes).map_err(|e| {
                    anyhow!("[{file_name}] load module {filename}: {e}")
                })?);
            }
            "assert_return" => {
                let line = cmd.get("line").and_then(Value::as_u64).unwrap_or(0);
                let action = cmd
                    .get("action")
                    .ok_or_else(|| anyhow!("assert_return missing action"))?;
                if action.get("type").and_then(Value::as_str) != Some("invoke") {
                    continue;
                }
                let field = action
                    .get("field")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("invoke missing field"))?;

                if let Some(reason) = should_skip_field(file_name, field) {
                    asserts_skipped.insert(format!("{field}: {reason}"));
                    continue;
                }

                let args_json = action
                    .get("args")
                    .and_then(Value::as_array)
                    .ok_or_else(|| anyhow!("invoke missing args"))?;
                let args: Vec<V128> = args_json
                    .iter()
                    .map(v128_from_json)
                    .collect::<Result<_>>()
                    .map_err(|e| anyhow!("[{file_name}/{field}/line {line}] parse args: {e}"))?;

                let module = current
                    .as_ref()
                    .ok_or_else(|| anyhow!("assert_return before module"))?;

                let actual = match module.invoke_v128(field, &args) {
                    Ok(v) => v,
                    Err(e) => {
                        return Err(anyhow!(
                            "[{file_name}/{field}/line {line}] invoke: {e}"
                        ))
                    }
                };
                asserts_attempted += 1;

                // The JSON has either "expected" (exact) or "either" (any-of).
                let (expected_set_json, expected_kind) = if let Some(e) = cmd.get("expected") {
                    (e.as_array().cloned().unwrap_or_default(), "expected")
                } else if let Some(e) = cmd.get("either") {
                    (e.as_array().cloned().unwrap_or_default(), "either")
                } else {
                    continue;
                };
                if expected_set_json.is_empty() {
                    continue;
                }

                // Single-result functions only — relaxed-SIMD funcs all
                // return one v128.
                let matched = expected_set_json.iter().any(|e| {
                    if e.get("type").and_then(Value::as_str) != Some("v128") {
                        // Could be other return types — bail out
                        // conservatively.
                        return false;
                    }
                    v128_eq_nan_aware(&actual, e)
                });

                if !matched {
                    let args_dbg: Vec<String> =
                        args.iter().map(|a| format!("{a:?}")).collect();
                    let expected_dbg: Vec<String> =
                        expected_set_json.iter().map(|e| e.to_string()).collect();
                    failures.push(Failure {
                        file: file_name.to_string(),
                        line,
                        field: field.to_string(),
                        args: args_dbg.join(", "),
                        actual,
                        expected_kind,
                        expected_set: expected_dbg.join(" | "),
                    });
                }
            }
            // Skipped command types — none of the relaxed-SIMD
            // .wast files use them in failure-relevant ways.
            "assert_invalid"
            | "assert_malformed"
            | "assert_trap"
            | "assert_exhaustion"
            | "assert_uninstantiable"
            | "assert_unlinkable"
            | "register"
            | "action" => {}
            _ => {}
        }
    }

    eprintln!(
        "  {} assertions checked ({} failures, {} skipped)",
        asserts_attempted,
        failures.len(),
        asserts_skipped.len()
    );
    for s in &asserts_skipped {
        eprintln!("    skip: {s}");
    }
    Ok(failures)
}

fn wast2json_path() -> Option<PathBuf> {
    // Search PATH for `wast2json`. Skipped silently if not found.
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("wast2json");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn spec_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/benchmark-core → repo root → wasmtime/tests/spec_testsuite
    manifest.join("..").join("..").join(SPEC_DIR)
}

#[test]
fn spec_testsuite_relaxed_simd_all_files() {
    let Some(wast2json) = wast2json_path() else {
        eprintln!(
            "[spec-testsuite] wast2json not found in PATH — skipping. \
             Install wabt (`brew install wabt` on macOS) to run this test."
        );
        return;
    };
    let dir = spec_dir();
    if !dir.is_dir() {
        eprintln!(
            "[spec-testsuite] spec dir not found at {dir:?} — skipping. \
             (Expected the wasmtime submodule to be checked out.)"
        );
        return;
    }

    let tmp = tempdir();
    let mut total_failures = Vec::new();

    for wast_file in WAST_FILES {
        let input = dir.join(wast_file);
        if !input.is_file() {
            eprintln!("[spec-testsuite] {input:?} missing — skipping");
            continue;
        }
        let json_path = tmp.join(format!("{wast_file}.json"));
        let status = Command::new(&wast2json)
            .arg("--enable-relaxed-simd")
            .arg(&input)
            .arg("-o")
            .arg(&json_path)
            .status()
            .expect("wast2json failed to spawn");
        assert!(
            status.success(),
            "wast2json {wast_file} failed with status {status}"
        );

        eprintln!("[{}]", wast_file);
        match run_wast_json(&json_path, wast_file) {
            Ok(fails) => total_failures.extend(fails),
            Err(e) => panic!("harness error on {wast_file}: {e}"),
        }
    }

    if !total_failures.is_empty() {
        eprintln!(
            "\n=== {} spec-testsuite conformance failures ===",
            total_failures.len()
        );
        for f in &total_failures {
            eprintln!(
                "  {}:{} {}({}): got {actual:?}, {kind}={set}",
                f.file,
                f.line,
                f.field,
                f.args,
                actual = f.actual,
                kind = f.expected_kind,
                set = f.expected_set,
            );
        }
        panic!(
            "WAMR fast-interp diverges from spec on {} assertion(s)",
            total_failures.len()
        );
    }
}

/// Create a fresh temporary directory under the system temp root.
/// We don't use the `tempfile` crate to avoid pulling in another
/// dep — the harness only needs a unique-per-run path and we
/// clean it ourselves implicitly (test process exit removes it
/// on most platforms; on macOS the OS cleans `/tmp` periodically).
fn tempdir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let p = std::env::temp_dir().join(format!("wamr-spec-{pid}-{nanos}"));
    fs::create_dir_all(&p).expect("mkdir tempdir");
    p
}
