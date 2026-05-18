//! End-to-end correctness coverage for WAMR fast-interp's legacy-EH
//! lowering. Each test compiles inline wat with `wat::parse_str`,
//! instantiates on WAMR, calls one or more `fn() -> i32` exports,
//! and asserts the returned value.
//!
//! All cases use **void-result try-blocks** until the loader-side
//! try-body→block-dynamic-offset COPY-at-CATCH alignment lands (see
//! AGENTS.md's "Open follow-up — WAMR fast-interp legacy exception
//! handling" section).
//!
//! Run with: `cargo test -p benchmark-core --test eh_correctness`
//!
//! WAT syntax notes:
//!   * Rust's `wast` parser (and wabt's wat2wasm) only support the
//!     LINEAR form for legacy try/catch — `try / instr* / catch
//!     $tag / instr* / catch_all / instr* / end`. The wabt-style
//!     `(try (do ...) (catch ...))` folded syntax does NOT parse.
//!   * Inside the linear body of a try/catch block, instructions
//!     are separated by whitespace and execute in source order.
//!     Push operands BEFORE the consuming op: write
//!     `i32.const 99` then `global.set $g` (not the folded
//!     `(global.set $g (i32.const 99))`, which parses as two
//!     separate ops in linear context and trips a stack mismatch).
//!
//! Direct FFI rather than via the public `wamr` runner because that
//! one passes argc=1 (workloads are `fn(i32) -> i32`); these test
//! exports take no args.

use anyhow::{anyhow, Result};
use benchmark_core::wamr;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::sync::Once;

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

struct Module {
    // Owned wasm bytes — WAMR's `wasm_runtime_load` keeps a pointer
    // into this buffer for the lifetime of the module rather than
    // copying. If we drop the Vec before destroying the module/
    // instance, every export lookup returns NULL.
    _bytes: Vec<u8>,
    module: wasm_module_t,
    inst: wasm_module_inst_t,
    exec: wasm_exec_env_t,
}

impl Module {
    fn from_wat(wat_src: &str) -> Result<Self> {
        ensure_init();
        let mut bytes =
            wat::parse_str(wat_src).map_err(|e| anyhow!("wat parse failed: {e}"))?;
        let mut err = [0i8; 256];
        let module = unsafe {
            wasm_runtime_load(
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                err.as_mut_ptr(),
                err.len() as u32,
            )
        };
        if module.is_null() {
            let s = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(anyhow!("wasm_runtime_load failed: {s}"));
        }
        let inst = unsafe {
            wasm_runtime_instantiate(
                module,
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
            unsafe { wasm_runtime_unload(module) };
            return Err(anyhow!("wasm_runtime_instantiate failed: {s}"));
        }
        let exec = unsafe { wasm_runtime_create_exec_env(inst, 64 * 1024) };
        if exec.is_null() {
            unsafe {
                wasm_runtime_deinstantiate(inst);
                wasm_runtime_unload(module);
            }
            return Err(anyhow!("wasm_runtime_create_exec_env failed"));
        }
        Ok(Self {
            _bytes: bytes,
            module,
            inst,
            exec,
        })
    }

    /// Call an export `fn() -> i32`. Clears any pending exception from
    /// a prior failed call first so the test order doesn't matter.
    fn call_i32(&self, name: &str) -> Result<i32> {
        unsafe { wasm_runtime_clear_exception(self.inst) };
        let cn = CString::new(name)?;
        let f = unsafe { wasm_runtime_lookup_function(self.inst, cn.as_ptr()) };
        if f.is_null() {
            // Also surface any pending exception WAMR set on the
            // instance — e.g. if instantiate silently flagged a bad
            // section but returned non-null. (Without this the test
            // failure message is just "export not found", which is
            // misleading when the real issue is a load/instantiate
            // error.)
            let p = unsafe { wasm_runtime_get_exception(self.inst) };
            let m = if p.is_null() {
                "(no exception)".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(anyhow!(
                "export `{name}` not found (instance exception: {m})"
            ));
        }
        let mut argv = [0u32; 1];
        let ok =
            unsafe { wasm_runtime_call_wasm(self.exec, f, 0, argv.as_mut_ptr()) };
        if !ok {
            let p = unsafe { wasm_runtime_get_exception(self.inst) };
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

    /// Call an export `fn() -> i64`. Same shape as `call_i32` but
    /// wires through the 2-slot WAMR `argv` (low u32 in argv[0],
    /// high in argv[1]) per WAMR's ABI for multi-cell return
    /// values via `wasm_runtime_call_wasm`.
    fn call_i64(&self, name: &str) -> Result<i64> {
        unsafe { wasm_runtime_clear_exception(self.inst) };
        let cn = CString::new(name)?;
        let f = unsafe { wasm_runtime_lookup_function(self.inst, cn.as_ptr()) };
        if f.is_null() {
            return Err(anyhow!("export `{name}` not found"));
        }
        let mut argv = [0u32; 2];
        let ok =
            unsafe { wasm_runtime_call_wasm(self.exec, f, 0, argv.as_mut_ptr()) };
        if !ok {
            let p = unsafe { wasm_runtime_get_exception(self.inst) };
            let m = if p.is_null() {
                "(no exception text)".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(anyhow!("trap: {m}"));
        }
        let lo = argv[0] as u64;
        let hi = argv[1] as u64;
        Ok((lo | (hi << 32)) as i64)
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            wasm_runtime_destroy_exec_env(self.exec);
            wasm_runtime_deinstantiate(self.inst);
            wasm_runtime_unload(self.module);
        }
    }
}

/* ------------------------------------------------------------------ */
/* Same-function dispatch — the simplest commit-3 paths.              */
/* ------------------------------------------------------------------ */

/// Minimal sanity: a no-try, no-tag module compiles and runs.
#[test]
fn minimal_sanity_no_eh() {
    let m = Module::from_wat(
        r#"
(module
  (func (export "t") (result i32)
    i32.const 42))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 42);
}

/// Sanity: same idea but load a wasm-tools-stripped binary that
/// matches exactly what `wat2wasm` would produce — no custom name
/// section. Validates that the FFI plumbing is sound; pinpoints
/// whether the custom name section trips WAMR.
#[test]
fn minimal_sanity_handcrafted_bytes() {
    ensure_init();
    // The same `(module (func (export "t") (result i32) i32.const 42))`
    // compiled by hand (no custom name section). 30 bytes.
    let mut bytes: Vec<u8> = vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
        // Type section: 1 type, () -> i32
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
        // Function section: func[0] uses type 0
        0x03, 0x02, 0x01, 0x00,
        // Export section: 1 export, name "t", kind func, idx 0
        0x07, 0x05, 0x01, 0x01, b't', 0x00, 0x00,
        // Code section: func[0] = i32.const 42; end
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
    ];
    let mut err = [0i8; 256];
    let module = unsafe {
        wasm_runtime_load(
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            err.as_mut_ptr(),
            err.len() as u32,
        )
    };
    eprintln!("handcrafted: load returned {:?}", module);
    assert!(!module.is_null(), "wasm_runtime_load failed");
    let inst = unsafe {
        wasm_runtime_instantiate(
            module,
            64 * 1024,
            64 * 1024,
            err.as_mut_ptr(),
            err.len() as u32,
        )
    };
    eprintln!("handcrafted: inst returned {:?}", inst);
    assert!(!inst.is_null(), "wasm_runtime_instantiate failed");
    let exec = unsafe { wasm_runtime_create_exec_env(inst, 64 * 1024) };
    assert!(!exec.is_null(), "create_exec_env failed");
    let cn = std::ffi::CString::new("t").unwrap();
    let f = unsafe { wasm_runtime_lookup_function(inst, cn.as_ptr()) };
    eprintln!("handcrafted: lookup returned {:?}", f);
    assert!(!f.is_null(), "lookup_function returned null for handcrafted wasm");
    let mut argv = [0u32; 1];
    let ok = unsafe { wasm_runtime_call_wasm(exec, f, 0, argv.as_mut_ptr()) };
    assert!(ok, "wasm_runtime_call_wasm failed");
    assert_eq!(argv[0] as i32, 42);
    unsafe {
        wasm_runtime_destroy_exec_env(exec);
        wasm_runtime_deinstantiate(inst);
        wasm_runtime_unload(module);
    }
}

#[test]
fn typed_catch_same_function() {
    let wat_src = r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      throw $err
    catch $err
      i32.const 99
      global.set $g
    end
    global.get $g))
"#;
    let bytes = wat::parse_str(wat_src).unwrap();
    std::fs::write("/tmp/from_wat_crate.wasm", &bytes).unwrap();
    eprintln!("typed_catch_same_function: wasm = {} bytes", bytes.len());
    let m = Module::from_wat(wat_src).unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 99);
}

#[test]
fn catch_all_when_no_typed_match() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      throw $err
    catch_all
      i32.const 77
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 77);
}

/// No throw fires — CATCH op is reached via normal flow and the
/// runtime handler pops + branches past END.
#[test]
fn no_throw_catch_skipped() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 99))
  (func (export "t") (result i32)
    try
      nop
    catch $err
      i32.const 11
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 99);
}

/* ------------------------------------------------------------------ */
/* Inter-function unwind — return_func's exception hook.              */
/* ------------------------------------------------------------------ */

#[test]
fn inter_function_unwind() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func $thr throw $err)
  (func (export "t") (result i32)
    try
      call $thr
    catch $err
      i32.const 55
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 55);
}

/// Multi-frame unwind: the deepest callee throws and the throw has
/// to propagate through three intermediate frames before reaching
/// the outermost function's catch.
#[test]
fn multi_frame_unwind() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func $a throw $err)
  (func $b call $a)
  (func $c call $b)
  (func $d call $c)
  (func (export "t") (result i32)
    try
      call $d
    catch $err
      i32.const 91
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 91);
}

/// Recursive throw: the base case throws, the throw has to walk back
/// through `depth` intermediate frames before the outermost catch
/// fires. Stresses the eh-stack + return_func hook across many
/// frames simultaneously.
#[test]
fn recursive_throw_unwind() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func $rec (param $n i32)
    local.get $n
    i32.eqz
    if
      throw $err
    else
      local.get $n
      i32.const 1
      i32.sub
      call $rec
    end)
  (func (export "t") (result i32)
    try
      i32.const 50
      call $rec
    catch $err
      i32.const 1234
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 1234);
}

/* ------------------------------------------------------------------ */
/* Nested try-regions — EH_TRY_CATCH_STATE_BIT correctness.           */
/* ------------------------------------------------------------------ */

#[test]
fn nested_inner_catch_wins() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $err
      catch $err
        i32.const 33
        global.set $g
      end
    catch $err
      i32.const 99
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 33);
}

#[test]
fn three_level_nested() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        try
          throw $err
        catch $err
          i32.const 7
          global.set $g
        end
      catch $err
        i32.const 99
        global.set $g
      end
    catch $err
      i32.const 100
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 7);
}

/// A throw raised from inside a catch body propagates outward — the
/// in-progress entry has state=CATCH so the same try-region's
/// handlers don't re-fire.
#[test]
fn throw_inside_catch_propagates_outward() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $err
      catch $err
        i32.const 1
        global.set $g
        throw $err
      end
    catch $err
      global.get $g
      i32.const 10
      i32.add
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    // inner catch runs (g=1, re-throws); outer catch fires (g+=10 → 11).
    assert_eq!(m.call_i32("t").unwrap(), 11);
}

/* ------------------------------------------------------------------ */
/* Multiple catches in one try-region.                                */
/* ------------------------------------------------------------------ */

#[test]
fn multiple_catches_pick_by_tag() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a)
  (tag $b)
  (global $g (mut i32) (i32.const 0))
  (func (export "throws_a") (result i32)
    try
      throw $a
    catch $a
      i32.const 11
      global.set $g
    catch $b
      i32.const 22
      global.set $g
    end
    global.get $g)
  (func (export "throws_b") (result i32)
    i32.const 0
    global.set $g
    try
      throw $b
    catch $a
      i32.const 11
      global.set $g
    catch $b
      i32.const 22
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("throws_a").unwrap(), 11);
    assert_eq!(m.call_i32("throws_b").unwrap(), 22);
}

#[test]
fn typed_catches_then_catch_all_fallback() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a)
  (tag $b)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      throw $b
    catch $a
      i32.const 11
      global.set $g
    catch_all
      i32.const 99
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 99);
}

/* ------------------------------------------------------------------ */
/* Uncaught throws / no-catch try-regions.                            */
/* ------------------------------------------------------------------ */

#[test]
fn uncaught_throw_traps_to_host() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (func (export "t")
    throw $err))
"#,
    )
    .unwrap();
    let e = m.call_i32("t").unwrap_err();
    let s = format!("{e:#}");
    assert!(
        s.contains("wasm exception thrown"),
        "expected trap message, got: {s}"
    );
}

#[test]
fn try_without_catches_no_throw() {
    let m = Module::from_wat(
        r#"
(module
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      i32.const 42
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 42);
}

/* ------------------------------------------------------------------ */
/* Try-region as inner-most of other control flow.                    */
/* ------------------------------------------------------------------ */

#[test]
fn try_inside_loop_repeated() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    (local $i i32)
    loop $L
      try
        throw $err
      catch $err
        global.get $g
        i32.const 1
        i32.add
        global.set $g
      end
      local.get $i
      i32.const 1
      i32.add
      local.set $i
      local.get $i
      i32.const 5
      i32.lt_s
      br_if $L
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 5);
}

#[test]
fn try_inside_if_taken_branch() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    i32.const 1
    if
      try
        throw $err
      catch $err
        i32.const 7
        global.set $g
      end
    else
      i32.const 100
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 7);
}

/* ------------------------------------------------------------------ */
/* Repeated invocation / sequential try-regions.                      */
/* ------------------------------------------------------------------ */

#[test]
fn sequential_try_regions() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      throw $err
    catch $err
      i32.const 1
      global.set $g
    end
    try
      throw $err
    catch $err
      global.get $g
      i32.const 10
      i32.add
      global.set $g
    end
    try
      throw $err
    catch $err
      global.get $g
      i32.const 100
      i32.add
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 111);
}

#[test]
fn try_function_called_multiple_times() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func $once
    try
      throw $err
    catch $err
      global.get $g
      i32.const 1
      i32.add
      global.set $g
    end)
  (func (export "t") (result i32)
    call $once
    call $once
    call $once
    call $once
    call $once
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 5);
}

/* ------------------------------------------------------------------ */
/* Stress: many tags, many try-regions across a module.               */
/* ------------------------------------------------------------------ */

/* ------------------------------------------------------------------ */
/* RETHROW.                                                            */
/* ------------------------------------------------------------------ */

/// rethrow 0 — re-raise the immediately-enclosing catch's tag. The
/// re-raise propagates outward and the outer catch sees the same tag.
#[test]
fn rethrow_depth_zero() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $a
      catch $a
        i32.const 1
        global.set $g
        rethrow 0
      end
    catch $a
      global.get $g
      i32.const 10
      i32.add
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    // inner catch sets g=1, then rethrow; outer catch fires (g += 10 → 11).
    assert_eq!(m.call_i32("t").unwrap(), 11);
}

/// rethrow preserves the tag (an outer catch_all WOULD also match,
/// but we verify the right typed catch fires).
#[test]
fn rethrow_preserves_tag() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a)
  (tag $b)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $b
      catch $a
        i32.const 100
        global.set $g
      catch $b
        i32.const 1
        global.set $g
        rethrow 0
      end
    catch $a
      i32.const 200
      global.set $g
    catch $b
      global.get $g
      i32.const 10
      i32.add
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    // inner catch $b fires (g=1), then rethrow $b;
    // outer catch $b fires (g += 10 → 11).
    assert_eq!(m.call_i32("t").unwrap(), 11);
}

/// rethrow with depth 1 — re-raise the tag caught by the *outer*
/// catch from inside an inner catch body. Verifies the eh-stack walk
/// correctly counts state=CATCH entries.
#[test]
fn rethrow_depth_one() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a)
  (tag $b)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $a
      catch $a
        try
          throw $b
        catch $b
          ;; depth 1: re-raise the outer-outer's caught tag ($a)
          i32.const 1
          global.set $g
          rethrow 1
        end
      end
    catch $a
      global.get $g
      i32.const 10
      i32.add
      global.set $g
    catch $b
      i32.const 999
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    // Innermost throws $b, caught by innermost; that body sets g=1
    // and `rethrow 1` re-raises the tag from the depth-1 catch ($a).
    // The outermost catch $a fires (g += 10 → 11). The outermost
    // catch $b would set g=999; we verify $a wins.
    assert_eq!(m.call_i32("t").unwrap(), 11);
}

/* ------------------------------------------------------------------ */
/* Stress: many tags, many try-regions across a module.               */
/* ------------------------------------------------------------------ */

/* ------------------------------------------------------------------ */
/* Tag-with-params — payload routing from throw site to catch body.    */
/* ------------------------------------------------------------------ */

/// Tag with a single i32 param. `throw $err (i32.const 42)` hands
/// the value to the catch body's operand stack at entry; the
/// catch then stores it via `global.set`. Exercises the simplest
/// payload-routing shape: 1 cell, same function, typed catch.
#[test]
fn tag_single_i32_param() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      i32.const 42
      throw $err
    catch $err
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 42);
}

/// Tag with two i32 params — `throw $err (i32.const 10) (i32.const 32)`.
/// Verifies the catch body sees the params in source order with
/// the last-pushed on top of the operand stack (so `i32.add`
/// computes 10+32=42).
#[test]
fn tag_two_i32_params() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32 i32))
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      i32.const 10
      i32.const 32
      throw $err
    catch $err
      ;; catch body sees [10, 32] with 32 on top.
      i32.add
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 42);
}

/// Tag with a single i64 param — exercises the 2-cell-per-param
/// width handling. The loader records 2 cells in
/// `param_cell_num`, the throw emits 2 src offsets, the catch
/// allocates 2 dst slots, and the walker copies cell-wise.
#[test]
fn tag_single_i64_param() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i64))
  (global $g (mut i64) (i64.const 0))
  (func (export "t") (result i32)
    try
      i64.const 0x1234_5678_9abc_def0
      throw $err
    catch $err
      global.set $g
    end
    global.get $g
    i64.const 0x1234_5678_9abc_def0
    i64.eq))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 1);
}

/// Tag with mixed i32 + i64 params — exercises the loader's
/// per-param cell synthesis: the i64 occupies 2 consecutive
/// cells in frame_lp but `wasm_loader_push_frame_offset` only
/// writes a meaningful first-cell offset to `frame_offset[]`
/// (the second cell entry is left uninitialized). The CATCH /
/// THROW emits now reconstruct the consecutive cell offsets
/// `(first, first+1)` per multi-cell param so the runtime
/// walker copies the right cells. Test would fail if the copy
/// ended up shifted by one cell.
#[test]
fn tag_mixed_i32_i64_params() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32 i64))
  (global $gi (mut i32) (i32.const 0))
  (global $gl (mut i64) (i64.const 0))
  (func (export "t") (result i32)
    try
      i32.const 100
      i64.const 0x7fff_ffff_ffff_ff00
      throw $err
    catch $err
      ;; catch body sees [i32=100, i64=0x7fff...]. Pop the
      ;; i64 first (top), then the i32.
      global.set $gl
      global.set $gi
    end
    global.get $gi
    i32.const 100
    i32.eq
    global.get $gl
    i64.const 0x7fff_ffff_ffff_ff00
    i64.eq
    i32.and))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 1);
}

/// Two catches with different tag signatures — the throw matches
/// the second one, verifying that the loader correctly records
/// per-catch param_cell_num / param_dst_offsets, and the walker
/// picks the matching entry via tag_index, not by signature
/// position.
#[test]
fn multiple_catches_with_params_pick_by_tag() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a (param i32))
  (tag $b (param i32 i32))
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      i32.const 7
      i32.const 13
      throw $b           ;; matches the (param i32 i32) catch
    catch $a
      ;; should not fire — drop the would-be param so the void
      ;; try-region's check_block_stack at the catch-to-catch
      ;; transition sees a balanced stack
      drop
      i32.const 999
      global.set $g
    catch $b
      i32.add
      global.set $g      ;; expected: 7 + 13 = 20
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 20);
}

/// Nested try-regions, both with tag-with-params catches. The
/// inner catch handles the throw and its payload, then the
/// outer try-region falls through with no exception. Verifies
/// dst slots are per-catch (no cross-region aliasing).
///
/// The outer catch (which never runs) drops its incoming param
/// before pushing 999 so the catch body's loader validation
/// sees a balanced operand stack — the catch body's loader
/// validation now correctly sees the tag's param on the operand
/// stack at entry (was a latent bug; before tag-with-params
/// support landed, the PUSH_TYPE-only code in CATCH let the
/// param "slip past" loader validation).
#[test]
fn nested_try_with_params_inner_wins() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        i32.const 55
        throw $err
      catch $err
        ;; inner catch fires; payload = 55
        global.set $g
      end
      ;; falls through to outer try normal flow
    catch $err
      drop               ;; discard the would-be payload
      i32.const 999      ;; never runs
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 55);
}

/// Rethrow with payload — outer catch sees the same payload the
/// inner catch received. Exercises the RETHROW handler's
/// `throw_src_offsets = match->param_dst_offsets` re-point: the
/// inner catch's dst slots are read as the new src for the outer
/// catch's copy.
#[test]
fn rethrow_preserves_payload() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        i32.const 88
        throw $err
      catch $err
        ;; inner catch — pop and discard the payload, then
        ;; rethrow. The original payload (88) lives in the
        ;; inner catch's dst slots even though the operand
        ;; stack consumed it; RETHROW reads from dst slots.
        drop
        rethrow 0
      end
    catch $err
      ;; outer catch — expects the same payload (88).
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 88);
}

/// Catch_all with a thrown payload — the payload is dropped per
/// spec ("catch_all has no exception values"). The catch_all
/// body simply runs without seeing any params. This is the
/// orthogonal case to `tag_single_i32_param`.
#[test]
fn catch_all_drops_payload() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      i32.const 42
      throw $err
    catch_all
      ;; payload dropped; just observe that the catch fired.
      i32.const 7
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 7);
}

/// Try-region invoked repeatedly — the dst slots are reused
/// each iteration (function entry doesn't reset the underlying
/// frame_lp slots, but a fresh throw populates them anew). Acts
/// as a smoke test for stale-payload bleed across invocations.
#[test]
fn repeated_throw_with_payload() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (func $once (param $v i32) (result i32)
    try
      local.get $v
      throw $err
    catch $err
      ;; payload is on stack — return it
      return
    end
    i32.const 0)
  (func (export "t") (result i32)
    i32.const 11
    call $once
    i32.const 22
    call $once
    i32.const 33
    call $once
    i32.add
    i32.add))
"#,
    )
    .unwrap();
    /* 11 + 22 + 33 = 66 */
    assert_eq!(m.call_i32("t").unwrap(), 66);
}

/// **Documented gap**: cross-function throw with payload. The
/// callee's source slots are torn down by `return_func` before
/// the caller's `find_a_catch_handler` runs, so the payload is
/// silently dropped. The caller's catch still fires (the tag
/// match works via `frame->tag_index`), but the operand stack
/// at catch entry contains uninitialized garbage instead of the
/// thrown values. Ignored until the cross-frame payload buffer
/// design is in.
#[test]
#[ignore = "cross-function tag-with-params: callee's source frame is freed before caller's walker runs — see AGENTS.md"]
fn cross_function_tag_with_params() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (func $inner
    i32.const 42
    throw $err)
  (func (export "t") (result i32)
    try
      call $inner
      i32.const 0
    catch $err
      ;; expects 42 on stack — currently sees garbage
    end))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 42);
}

/* ------------------------------------------------------------------ */
/* DELEGATE — `try ... delegate N` forwards to the Nth outer block.   */
/* ------------------------------------------------------------------ */

/// Baseline: inner try delegates a throw to the outer try.
///
/// At the eh-table level this exercises the simplest delegate
/// dispatch — `delta = 0` (no try-blocks strictly between the
/// inner try and the outer try) — so the walker's "mark consumed,
/// continue to next eh-stack entry" path is the one under test.
#[test]
fn delegate_forwards_to_outer() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $err
      delegate 0  ;; forward to the outer try
    catch $err
      i32.const 88
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 88);
}

/// Normal-flow path through `delegate`: the try body doesn't throw,
/// so the DELEGATE runtime handler runs (popping the eh-stack
/// entry) instead of the throw walker. A second try-region
/// immediately after proves the pop landed in the right slot —
/// otherwise the new TRY's push would overlap stale state.
#[test]
fn delegate_normal_flow_pops_eh_stack() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      ;; no throw — fall through delegate
      nop
    delegate 0
    ;; A second try-region in the same function. If the first
    ;; region's eh-stack entry weren't popped, this TRY's push
    ;; would land on top of stale state and the throw below
    ;; would find the wrong catch index.
    try
      throw $err
    catch $err
      i32.const 55
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 55);
}

/// `delegate N` with `N > 0` — the delegate's try is nested
/// inside a non-try block on its way to the target outer try.
/// `delta` is still 0 (the block in between isn't a try), so the
/// walker's behaviour is identical to depth=0, but the loader
/// must correctly handle `csp_num - 1 > N` for the depth check.
#[test]
fn delegate_through_block_to_outer_try() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      block
        try
          throw $err
        delegate 1  ;; skip the (block), land in outer try
      end
    catch $err
      i32.const 77
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 77);
}

/// `delegate` SKIPS a nested try-with-catches between the
/// delegate and the target. The skipped try has a catch for the
/// thrown tag, but spec semantics say it doesn't get to see the
/// throw — only the outermost try (the delegate's target) does.
///
/// This is the test that proves the walker's `i -= delta`
/// short-circuit is correct: if delta were wrong (or zero), the
/// middle try's catch would fire and the result would be 22.
#[test]
fn delegate_skips_middle_try() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        try
          throw $err
        delegate 1   ;; target = outer try; middle try is skipped
      catch $err
        ;; This must NOT fire — delegate forwarded past us.
        i32.const 22
        global.set $g
      end
    catch $err
      ;; This is where the spec says the throw lands.
      i32.const 99
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 99);
}

/// `delegate` targeting the function block — exception escapes
/// the function. The walker's "delta + 1 >= i" guard fires here
/// (all active try-blocks are inside the delegate's reach, so
/// there's no eh-stack entry to fall onto) and we return-with-
/// exception out to the host.
#[test]
fn delegate_to_function_block_escapes() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (func (export "t") (result i32)
    try
      throw $err
    delegate 0   ;; target = function block — escapes to host
    i32.const 1))
"#,
    )
    .unwrap();
    let err = m.call_i32("t").unwrap_err().to_string();
    assert!(
        err.contains("wasm exception thrown") || err.contains("uncaught"),
        "expected uncaught-exception trap, got: {err}"
    );
}

/// Delegate from a CALLEE — uncaught exception in the callee
/// (its delegate targets a non-try function block, so the
/// exception escapes the callee) is caught by the CALLER's try
/// surrounding the call. Exercises the interaction between
/// delegate-forwarding and the caller-frame return_func hook
/// (find_a_catch_handler's "prev_frame && prev_frame->ip" path).
#[test]
fn delegate_in_callee_caught_by_caller() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func $inner
    try
      throw $err
    delegate 0)  ;; target = function block → escapes $inner
  (func (export "t") (result i32)
    try
      call $inner
    catch $err
      i32.const 44
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 44);
}

/// Nested delegates — three layers, each `delegate 0`. Tests that
/// the walker correctly chains through consecutive delegate
/// entries (each "mark consumed + advance"); only the outermost
/// non-delegate try should match.
#[test]
fn nested_delegates_chain() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        try
          try
            throw $err
          delegate 0  ;; layer 3 forwards to layer 2
        delegate 0    ;; layer 2 forwards to layer 1
      delegate 0      ;; layer 1 forwards to outermost
    catch $err
      i32.const 17
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 17);
}

/// Delegate forwards to a `catch_all` (no typed catch). Exercises
/// the walker's fall-through from "no typed match" to
/// `entry->catch_all_pc` in the outer try after the inner
/// delegate consumes its entry.
#[test]
fn delegate_forwards_to_catch_all() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      try
        throw $err
      delegate 0
    catch_all
      i32.const 33
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 33);
}

/// Delegate appears inside a CATCH body — the catch is handling
/// an earlier throw, and then a fresh try-delegate inside the
/// catch's body forwards a new exception outward. Verifies that
/// the EH_TRY_CATCH_STATE_BIT (set on the outer entry when its
/// catch first matched) still suppresses re-match after the
/// delegate consumes the inner entry, so the second throw
/// escapes the function instead of looping back into the outer
/// catch.
#[test]
fn delegate_inside_catch_body_escapes() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (func (export "t") (result i32)
    try
      throw $err
    catch $err
      ;; we're now inside the outer catch — its eh-stack entry
      ;; has EH_TRY_CATCH_STATE_BIT set.
      try
        throw $err
      delegate 0   ;; target = the catch body's surrounding block,
                   ;; which is the (already-consumed) outer try.
                   ;; Walker should NOT re-match it; exception
                   ;; must escape the function.
    end
    i32.const 1))
"#,
    )
    .unwrap();
    let err = m.call_i32("t").unwrap_err().to_string();
    assert!(
        err.contains("wasm exception thrown") || err.contains("uncaught"),
        "expected uncaught-exception trap, got: {err}"
    );
}

/* ------------------------------------------------------------------ */
/* BR out of a try-region — known limitation flagged in AGENTS.md.    */
/* ------------------------------------------------------------------ */

/// `br N` jumping out of a try-region — the eh-stack entry from
/// the try-block's TRY needs to be popped before control leaves the
/// region, otherwise a subsequent try-region in the same function
/// inherits stale state. Currently the loader's `br` patches its
/// target at the post-END position, bypassing the runtime END
/// handler's pop. Documenting as a known limitation; commit 6
/// would address by either (a) emitting a synthetic pop op before
/// the br jump, or (b) patching `br N` to land on the END byte for
/// EH targets so the pop runs there.
#[test]
#[ignore = "br across try-region boundary leaks eh-stack — see AGENTS.md follow-up"]
fn br_out_of_try_pops_eh_stack() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    block $outer
      try
        br $outer  ;; jump out of the try without throwing
      catch $err
        i32.const 99
        global.set $g
      end
    end
    ;; Now we're past the outer block. A second try-region must
    ;; start with a fresh eh-stack count, but the leaked entry
    ;; from above prevents that.
    try
      throw $err
    catch $err
      i32.const 11
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 11);
}

/* ------------------------------------------------------------------ */
/* Stress: deep recursive throws + repeated function entries.          */
/* ------------------------------------------------------------------ */

/// Drives the eh_count reset path on every function entry across a
/// deep recursion. If `frame->eh_count = 0` were ever skipped, this
/// would corrupt state in later calls.
#[test]
fn deep_recursion_with_try_and_throw() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func $rec (param $n i32)
    try
      ;; Throw on every level — proves the catch fires for every frame.
      local.get $n
      i32.eqz
      if
        throw $err
      else
        local.get $n
        i32.const 1
        i32.sub
        call $rec
      end
    catch $err
      global.get $g
      i32.const 1
      i32.add
      global.set $g
      ;; rethrow so the next outer frame's catch also fires
      rethrow 0
    end)
  (func (export "t") (result i32)
    try
      i32.const 100
      call $rec
    catch $err
      ;; pass — every level's catch already incremented g.
      nop
    end
    global.get $g))
"#,
    )
    .unwrap();
    // 101 frames each increment g by 1 in their catch body (the
    // base case throws + 100 recursive callers' catches each fire),
    // then the top-level catch absorbs the final rethrow.
    assert_eq!(m.call_i32("t").unwrap(), 101);
}

/// Many try-regions in one function — exercises eh_idx accounting
/// up to a moderate count.
#[test]
fn ten_sequential_try_regions() {
    let mut src = String::from(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
"#,
    );
    for _ in 0..10 {
        src.push_str(
            r#"    try
      throw $err
    catch $err
      global.get $g
      i32.const 1
      i32.add
      global.set $g
    end
"#,
        );
    }
    src.push_str("    global.get $g))\n");
    let m = Module::from_wat(&src).unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 10);
}

/// 32 try-regions in one function — bigger eh_idx range; checks the
/// 24-bit packing of eh_idx (low 31 bits, well within range) and
/// the per-function exception_handlers[] alloc.
#[test]
fn thirty_two_sequential_try_regions() {
    let mut src = String::from(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
"#,
    );
    for _ in 0..32 {
        src.push_str(
            r#"    try
      throw $err
    catch $err
      global.get $g
      i32.const 1
      i32.add
      global.set $g
    end
"#,
        );
    }
    src.push_str("    global.get $g))\n");
    let m = Module::from_wat(&src).unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 32);
}

/// Catch body that *itself* contains a try-region. Verifies the
/// EH-frame stack push/pop pairs correctly when control enters a
/// new try while already inside a catch handler.
#[test]
fn try_inside_catch_body() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (global $g (mut i32) (i32.const 0))
  (func (export "t") (result i32)
    try
      throw $err
    catch $err
      i32.const 1
      global.set $g
      ;; nested try inside the catch
      try
        throw $err
      catch $err
        global.get $g
        i32.const 10
        i32.add
        global.set $g
      end
    end
    global.get $g))
"#,
    )
    .unwrap();
    // Outer catch sets g=1; inner try-catch fires; inner catch adds 10 → g=11.
    assert_eq!(m.call_i32("t").unwrap(), 11);
}

/* ------------------------------------------------------------------ */
/* Stress: many tags, many try-regions across a module.               */
/* ------------------------------------------------------------------ */

#[test]
fn many_tags_match_by_index() {
    let m = Module::from_wat(
        r#"
(module
  (tag $t0)
  (tag $t1)
  (tag $t2)
  (tag $t3)
  (tag $t4)
  (tag $t5)
  (global $g (mut i32) (i32.const 0))
  (func (export "throws_3") (result i32)
    try
      throw $t3
    catch $t0
      i32.const 100
      global.set $g
    catch $t1
      i32.const 101
      global.set $g
    catch $t2
      i32.const 102
      global.set $g
    catch $t3
      i32.const 103
      global.set $g
    catch $t4
      i32.const 104
      global.set $g
    catch $t5
      i32.const 105
      global.set $g
    end
    global.get $g))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("throws_3").unwrap(), 103);
}

/* ------------------------------------------------------------------ */
/* Result-typed try-regions — `try (result T)` and `try (result T)... */
/* catch ... end` deposit each body's value at the block's            */
/* dynamic_offset slot. Loader injects a COPY at every CATCH /         */
/* CATCH_ALL transition (and the existing END handler emits the       */
/* final body's COPY).                                                */
/* ------------------------------------------------------------------ */

/// Try-region with i32 result, try body completes normally. The
/// catch never fires, but the loader still has to align the try
/// body's last value with the block's result slot. Tests the
/// loader-side COPY-at-CATCH emit for the normal-flow path.
#[test]
fn try_result_i32_no_throw() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (func (export "t") (result i32)
    try (result i32)
      i32.const 7
    catch $err
      drop
      i32.const 11
    end))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 7);
}

/// Same shape but the try body throws — catch body's result
/// (consumed-param then re-push) reaches the function return.
/// Tests the throw-dispatch path through the catch body's END
/// COPY.
#[test]
fn try_result_i32_with_throw() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (func (export "t") (result i32)
    try (result i32)
      i32.const 99
      throw $err
    catch $err
      ;; catch param i32 already on stack; pass through
    end))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 99);
}

/// i64 result — exercises the 2-cell COPY (EXT_OP_COPY_STACK_TOP_I64).
#[test]
fn try_result_i64_no_throw() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (func (export "t") (result i64)
    try (result i64)
      i64.const 0x1234_5678_9abc_def0
    catch $err
      i64.const 0
    end))
"#,
    )
    .unwrap();
    let v = m.call_i64("t").unwrap();
    assert_eq!(v, 0x1234_5678_9abc_def0u64 as i64);
}

/// Multiple catches with i32 result — only the second catch
/// fires (throws $b). Each catch transition has to emit its own
/// COPY for the previous body's last value, so the dst slot
/// converges no matter which path runs.
#[test]
fn try_result_i32_multi_catch() {
    let m = Module::from_wat(
        r#"
(module
  (tag $a (param i32))
  (tag $b (param i32))
  (func (export "t") (result i32)
    try (result i32)
      i32.const 1
      throw $b
    catch $a
      ;; param $a on stack — should NOT fire
    catch $b
      ;; param $b on stack — pass through (= 1)
    end))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 1);
}

/// Try-result with catch_all — the all-catch fallback receives
/// no params, so the body must push its own i32 result.
#[test]
fn try_result_i32_catch_all() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err)
  (func (export "t") (result i32)
    try (result i32)
      throw $err
    catch_all
      i32.const 33
    end))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 33);
}

/// Try-result inside a function with locals — verifies the
/// loader's `dynamic_offset` slot allocation interacts cleanly
/// with the existing local-slot range (no aliasing with locals
/// 0..N-1).
#[test]
fn try_result_with_locals() {
    let m = Module::from_wat(
        r#"
(module
  (tag $err (param i32))
  (func (export "t") (result i32) (local $x i32) (local $y i32)
    i32.const 10
    local.set $x
    i32.const 20
    local.set $y
    try (result i32)
      local.get $x
      local.get $y
      i32.add
    catch $err
      drop
      i32.const 0
    end))
"#,
    )
    .unwrap();
    assert_eq!(m.call_i32("t").unwrap(), 30);
}
