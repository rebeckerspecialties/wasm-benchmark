//! Throw-firing correctness test for WAMR fast-interp legacy-EH.
//!
//! Loads /tmp/eh_void.wasm (compiled from /tmp/eh_void.wat via
//! `wat2wasm --enable-exceptions`) and calls each exported test
//! function. All five test cases use **void-result try-blocks** so
//! they don't depend on the loader-side try-body-result COPY-at-CATCH
//! alignment that's still pending (see AGENTS.md's open follow-up).
//!
//! Expected results — verify the runtime throw walker dispatches into
//! the right handler and the global side-effect lands correctly:
//!
//!   test_local_throw  → 99  (typed catch in same function)
//!   test_catch_all    → 77  (catch_all fallback)
//!   test_inter_fn     → 55  (callee throws, caller catches)
//!   test_nested       → 33  (inner catch wins, outer never fires)
//!   test_no_throw     → 11  (CATCH-skip on normal flow, untouched)
//!
//! Direct FFI rather than via the public `wamr` runner because that
//! one passes argc=1 (workloads are `fn(i32) -> i32`); these test
//! exports take no args.

use anyhow::{anyhow, Result};
use benchmark_core::wamr;
use std::ffi::CString;
use std::fs;
use std::os::raw::{c_char, c_void};

#[allow(non_camel_case_types)]
type wasm_module_t = *mut c_void;
#[allow(non_camel_case_types)]
type wasm_module_inst_t = *mut c_void;
#[allow(non_camel_case_types)]
type wasm_function_inst_t = *mut c_void;
#[allow(non_camel_case_types)]
type wasm_exec_env_t = *mut c_void;

extern "C" {
    fn wasm_runtime_load(
        buf: *mut u8,
        size: u32,
        error_buf: *mut c_char,
        error_buf_size: u32,
    ) -> wasm_module_t;
    fn wasm_runtime_instantiate(
        module: wasm_module_t,
        stack_size: u32,
        heap_size: u32,
        error_buf: *mut c_char,
        error_buf_size: u32,
    ) -> wasm_module_inst_t;
    fn wasm_runtime_lookup_function(
        module_inst: wasm_module_inst_t,
        name: *const c_char,
    ) -> wasm_function_inst_t;
    fn wasm_runtime_create_exec_env(
        module_inst: wasm_module_inst_t,
        stack_size: u32,
    ) -> wasm_exec_env_t;
    fn wasm_runtime_call_wasm(
        exec_env: wasm_exec_env_t,
        function: wasm_function_inst_t,
        argc: u32,
        argv: *mut u32,
    ) -> bool;
    fn wasm_runtime_get_exception(module_inst: wasm_module_inst_t) -> *const c_char;
    fn wasm_runtime_clear_exception(module_inst: wasm_module_inst_t);
}

fn call(
    inst: wasm_module_inst_t,
    exec: wasm_exec_env_t,
    name: &str,
) -> Result<i32> {
    let cn = CString::new(name)?;
    let f = unsafe { wasm_runtime_lookup_function(inst, cn.as_ptr()) };
    if f.is_null() {
        return Err(anyhow!("export `{name}` not found"));
    }
    let mut argv = [0u32; 1];
    let ok = unsafe { wasm_runtime_call_wasm(exec, f, 0, argv.as_mut_ptr()) };
    if !ok {
        let p = unsafe { wasm_runtime_get_exception(inst) };
        let m = if p.is_null() {
            "(no exception text)".to_string()
        } else {
            unsafe { std::ffi::CStr::from_ptr(p) }
                .to_string_lossy()
                .into_owned()
        };
        return Err(anyhow!("trap: {m}"));
    }
    Ok(argv[0] as i32)
}

fn main() -> Result<()> {
    wamr::init()?;
    let mut bytes = fs::read("/tmp/eh_void.wasm")?;
    let mut err = [0i8; 256];
    let m = unsafe {
        wasm_runtime_load(
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            err.as_mut_ptr(),
            err.len() as u32,
        )
    };
    if m.is_null() {
        let s = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        return Err(anyhow!("load: {s}"));
    }
    let inst = unsafe {
        wasm_runtime_instantiate(
            m,
            64 * 1024,
            64 * 1024,
            err.as_mut_ptr(),
            err.len() as u32,
        )
    };
    if inst.is_null() {
        let s = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        return Err(anyhow!("instantiate: {s}"));
    }
    let exec = unsafe { wasm_runtime_create_exec_env(inst, 64 * 1024) };

    let cases: &[(&str, i32)] = &[
        ("test_local_throw", 99),
        ("test_catch_all", 77),
        ("test_inter_fn", 55),
        ("test_nested", 33),
        ("test_no_throw", 11),
    ];
    let mut all_pass = true;
    for (name, want) in cases {
        // Clear any pending exception from a prior failed call — once
        // wasm_runtime_call_wasm leaves an exception set on the
        // instance, subsequent calls short-circuit until it's cleared.
        unsafe { wasm_runtime_clear_exception(inst) };
        match call(inst, exec, name) {
            Ok(got) if got == *want => {
                println!("{name}: PASS (got {got})");
            }
            Ok(got) => {
                println!("{name}: FAIL — got {got}, want {want}");
                all_pass = false;
            }
            Err(e) => {
                println!("{name}: ERROR — {e:#}");
                all_pass = false;
            }
        }
    }
    if !all_pass {
        std::process::exit(1);
    }
    Ok(())
}
