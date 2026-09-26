//! Relaxed-SIMD abuse-case integration suite. Each test compiles a
//! small wasm module that exercises a spec-allowed implementation-
//! defined boundary in the relaxed-SIMD opcode family, runs it
//! through WAMR fast-interp (the runtime our PR enables), and
//! asserts the result is bit-exactly what WAMR produces today on
//! aarch64 (M4 / Apple Silicon).
//!
//! Categories are derived from a survey of:
//!
//!   * the upstream wasm-spec testsuite at
//!     `wasmtime/tests/spec_testsuite/relaxed_*.wast`
//!   * V8 / SpiderMonkey relaxed-SIMD conformance tests
//!   * Closed PRs / issues in wasmtime + WAMR touching SIMD lowering
//!
//! 19 cases, all spec-conformant. Of those, 16 produce values
//! bit-identical to wasmtime's Cranelift JIT. 1 (`trunc_f64_zero`)
//! diverges in a spec-allowed way — SIMDe's aarch64 lowering uses
//! `vcvtq_s64_f64 + vmovn_s64` (saturate-to-INT64_MAX then truncate
//! low 32 bits → `0xffffffff`) while Cranelift directly saturates
//! to `INT32_MAX`. Both are conformant under the relaxed-SIMD spec
//! for non-finite inputs; the test pins WAMR's current observable
//! behavior so a future SIMDe upgrade or lowering change is
//! caught.
//!
//! A second wave of boundary tests (Categories 1b, 2b, 6b, 6c, 7b,
//! 7c, 8b) was added after the chatgpt-codex-connector code review
//! on PR #3 caught an i16-intermediate-truncation bug in
//! `i32x4.relaxed_dot_i8x16_i7x16_add_s` that none of the original
//! 19 cases exercised. Each new test targets a multi-step spec
//! operation where collapsing the steps changes the result, or
//! pins an implementation-defined ambiguity that the original
//! coverage didn't.
//!
//! These tests double as ASan / UBSan smoke tests. Build WAMR with
//! `-fsanitize=address,undefined` via the existing
//! `build-asan/` cmake config and run `cargo test --test
//! relaxed_simd_abuse` — every case must complete without
//! triggering a sanitizer report.

use std::ffi::{c_char, c_void, CString};
use std::sync::Once;

use anyhow::{anyhow, Result};
use benchmark_core::wamr;

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

// Single inline WAT for all 19 abuse cases. Each function returns
// an i64 (the low 64 bits of a v128 result via `i64x2.extract_lane 0`),
// which the host pulls out via the standard WAMR call_wasm ABI.
const ABUSE_WAT: &str = r#"
(module
  (memory (export "mem") 1)

  ;; Category 1: NaN propagation in relaxed_min / relaxed_max.
  (func (export "min_f32_nan_lo") (result i64)
    v128.const f32x4 nan 1.0 2.0 nan
    v128.const f32x4 1.0 nan nan 2.0
    f32x4.relaxed_min
    i64x2.extract_lane 0)
  (func (export "max_f64_nan") (result i64)
    v128.const f64x2 nan 1.0
    v128.const f64x2 2.0 nan
    f64x2.relaxed_max
    i64x2.extract_lane 0)

  ;; Category 2: signed-zero asymmetry in min/max.
  (func (export "min_signed_zero") (result i64)
    v128.const f32x4 +0.0 -0.0 +0.0 -0.0
    v128.const f32x4 -0.0 +0.0 +0.0 -0.0
    f32x4.relaxed_min
    i64x2.extract_lane 0)

  ;; Category 3: FMA single-vs-double rounding.
  (func (export "madd_rounding") (result i64)
    v128.const f32x4 0x1.000004p+0 0 0 0
    v128.const f32x4 0x1.0002p+0   0 0 0
    v128.const f32x4 0x1.000204p+0 0 0 0
    f32x4.relaxed_nmadd
    i64x2.extract_lane 0)
  (func (export "madd_overflow") (result i64)
    v128.const f32x4 0x1.fffffep+127 0 0 0
    v128.const f32x4 2.0 0 0 0
    v128.const f32x4 0x1.fffffep+127 0 0 0
    f32x4.relaxed_nmadd
    i64x2.extract_lane 0)

  ;; Category 3b: relaxed_madd with (Inf, 0, c) — IEEE 754 invalid
  ;; multiply. Both fused fma(Inf, 0, c) and unfused Inf*0 + c
  ;; produce a NaN regardless of c; bit pattern is impl-defined.
  (func (export "madd_inf_times_zero_lo") (result i64)
    v128.const f32x4 inf inf inf inf
    v128.const f32x4 0 0 0 0
    v128.const f32x4 1.0 2.0 3.0 4.0
    f32x4.relaxed_madd
    i64x2.extract_lane 0)
  (func (export "madd_inf_times_zero_hi") (result i64)
    v128.const f32x4 inf inf inf inf
    v128.const f32x4 0 0 0 0
    v128.const f32x4 1.0 2.0 3.0 4.0
    f32x4.relaxed_madd
    i64x2.extract_lane 1)

  ;; Category 4: relaxed_swizzle out-of-range indices.
  (func (export "swizzle_oob_lo") (result i64)
    v128.const i8x16 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25
    v128.const i8x16 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31
    i8x16.relaxed_swizzle
    i64x2.extract_lane 0)
  (func (export "swizzle_oob_hi_bit") (result i64)
    v128.const i8x16 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25
    v128.const i8x16 0x80 0x81 0x82 0x83 0x84 0x85 0x86 0x87
                     0x88 0x89 0x8a 0x8b 0x8c 0x8d 0x8e 0x8f
    i8x16.relaxed_swizzle
    i64x2.extract_lane 0)

  ;; Category 5: q15mulr_s INT16_MIN * INT16_MIN overflow.
  (func (export "q15mulr_overflow") (result i64)
    v128.const i16x8 -32768 -32767 32767 0 0 0 0 0
    v128.const i16x8 -32768 -32768 32767 0 0 0 0 0
    i16x8.relaxed_q15mulr_s
    i64x2.extract_lane 0)

  ;; Category 6: relaxed_laneselect mask interpretation.
  (func (export "laneselect_i16_mask_0x80") (result i64)
    v128.const i16x8 0x1111 0x2222 0x3333 0x4444 0 0 0 0
    v128.const i16x8 0xaaaa 0xbbbb 0xcccc 0xdddd 0 0 0 0
    v128.const i16x8 0x0080 0xff00 0x8000 0x0000 0 0 0 0
    i16x8.relaxed_laneselect
    i64x2.extract_lane 0)
  (func (export "laneselect_i8_mixed") (result i64)
    v128.const i8x16 0x11 0x22 0x33 0x44 0x55 0x66 0x77 0x88
                     0x11 0x22 0x33 0x44 0x55 0x66 0x77 0x88
    v128.const i8x16 0xaa 0xbb 0xcc 0xdd 0xee 0xff 0x00 0x11
                     0xaa 0xbb 0xcc 0xdd 0xee 0xff 0x00 0x11
    v128.const i8x16 0x80 0x7f 0x00 0xff 0x81 0x01 0xfe 0x7e
                     0x80 0x7f 0x00 0xff 0x81 0x01 0xfe 0x7e
    i8x16.relaxed_laneselect
    i64x2.extract_lane 0)

  ;; Category 7: i8 * i7 dot-product ambiguity.
  (func (export "dot_i8_i7_extreme") (result i64)
    v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128
                     -128 -128 -128 -128 -128 -128 -128 -128
    v128.const i8x16 -127 -127 -127 -127 -127 -127 -127 -127
                     -127 -127 -127 -127 -127 -127 -127 -127
    i16x8.relaxed_dot_i8x16_i7x16_s
    i64x2.extract_lane 0)
  (func (export "dot_add_normal") (result i64)
    v128.const i8x16 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1
    v128.const i8x16 2 2 2 2 2 2 2 2 2 2 2 2 2 2 2 2
    v128.const i32x4 100 200 300 400
    i32x4.relaxed_dot_i8x16_i7x16_add_s
    i64x2.extract_lane 0)

  ;; Category 8: relaxed_trunc on non-finite (no-crash + pin shape).
  (func (export "trunc_nan_inf") (result i64)
    v128.const f32x4 nan inf -inf 0
    i32x4.relaxed_trunc_f32x4_s
    i64x2.extract_lane 0)
  (func (export "trunc_huge") (result i64)
    v128.const f32x4 0x1p+62 -0x1p+62 0x1.fffffep+127 -0x1.fffffep+127
    i32x4.relaxed_trunc_f32x4_u
    i64x2.extract_lane 0)
  (func (export "trunc_f64_zero") (result i64)
    v128.const f64x2 nan inf
    i32x4.relaxed_trunc_f64x2_s_zero
    i64x2.extract_lane 0)

  ;; Category 9: determinism across repeated calls.
  (func (export "determinism_madd") (result i64)
    v128.const f32x4 1.5 2.5 3.5 4.5
    v128.const f32x4 10 20 30 40
    v128.const f32x4 100 200 300 400
    f32x4.relaxed_madd
    v128.const f32x4 1.5 2.5 3.5 4.5
    v128.const f32x4 10 20 30 40
    v128.const f32x4 100 200 300 400
    f32x4.relaxed_madd
    v128.xor
    i64x2.extract_lane 0)

  ;; Category 11: relaxed op inside an if-block exercises the
  ;; loader's `wasm_loader_find_block_addr` skipper across the
  ;; widened 2-byte SIMD sub-opcode.
  (func (export "relaxed_inside_if") (param i32) (result i64)
    (if (result v128) (local.get 0)
      (then
        v128.const f32x4 1.0 2.0 3.0 4.0
        v128.const f32x4 5.0 6.0 7.0 8.0
        v128.const f32x4 0.5 0.5 0.5 0.5
        f32x4.relaxed_madd)
      (else
        v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
        v128.const i8x16 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0
        i8x16.relaxed_swizzle))
    i64x2.extract_lane 0)

  ;; Category 12: legacy i8x16.swizzle then relaxed_swizzle — both
  ;; share the 0xfd prefix; this exercises the 1-byte vs 2-byte
  ;; sub-opcode loader path in the rewritten IR.
  (func (export "legacy_then_relaxed") (result i64)
    v128.const i8x16 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15
    v128.const i8x16 15 14 13 12 11 10 9 8 7 6 5 4 3 2 1 0
    i8x16.swizzle
    v128.const i8x16 7 6 5 4 3 2 1 0 15 14 13 12 11 10 9 8
    i8x16.relaxed_swizzle
    i64x2.extract_lane 0)

  ;; Bonus: misaligned v128 load + relaxed_madd (UBSan tripwire).
  (func (export "load_unaligned") (result i64)
    i32.const 5
    v128.const i32x4 0x11111111 0x22222222 0x33333333 0x44444444
    v128.store
    i32.const 5
    v128.load
    v128.const f32x4 1 2 3 4
    v128.const f32x4 0.1 0.2 0.3 0.4
    f32x4.relaxed_madd
    i64x2.extract_lane 0)

  ;; --------------------------------------------------------------
  ;; Boundary tests added after the chatgpt-codex-connector review
  ;; on PR #3 caught a missing i16-intermediate truncation in
  ;; `i32x4.relaxed_dot_i8x16_i7x16_add_s` — see the commit
  ;; "fast-interp: i32x4.relaxed_dot_i8x16_i7x16_add_s preserve i16
  ;; intermediate". Each block below targets a multi-step spec
  ;; operation where collapsing the steps changes the result, or
  ;; an implementation-defined ambiguity zone where we pin
  ;; observable behavior.
  ;; --------------------------------------------------------------

  ;; Category 7b: i16-intermediate overflow boundary for
  ;; `i32x4.relaxed_dot_i8x16_i7x16_add_s`. With a = b = 0x80 (i8 =
  ;; -128) in all 16 bytes, c = 0:
  ;;   pair_sum = (-128 * -128) + (-128 * -128) = 32768  (overflows i16)
  ;;   wrap → (int16)32768 = -32768
  ;;   ext_pair = i32(-32768) + i32(-32768) = -65536
  ;;   + c (0)  = -65536  per i32 lane
  ;;
  ;; Direct-i32-sum implementation (the pre-fix bug):
  ;;   4 * 16384 = 65536  per lane  ← OUTSIDE spec-allowed set
  ;;
  ;; Spec-allowed set per lane (any wrap/sat × wrap/sat combination
  ;; of the two pair sums): {-65536, -1, 65534}. 65536 is *not* in
  ;; the set.
  ;;
  ;; Our wrap-on-both impl pins to -65536 per lane.
  ;;   lane0 = lane1 = -65536 = (i32) 0xffff0000
  ;;   low i64 = (lane1 << 32) | lane0 = 0xffff0000_ffff0000
  (func (export "dot_add_i16_overflow") (result i64)
    v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128
                     -128 -128 -128 -128 -128 -128 -128 -128
    v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128
                     -128 -128 -128 -128 -128 -128 -128 -128
    v128.const i32x4 0 0 0 0
    i32x4.relaxed_dot_i8x16_i7x16_add_s
    i64x2.extract_lane 0)

  ;; Category 7c: pin the `i16x8.relaxed_dot_i8x16_i7x16_s` impl
  ;; at the same overflow boundary. The current impl correctly
  ;; truncates to i16 via the assignment `result.i16x8[lane] =
  ;; (int16)sum` (wasm_interp_fast.c:8103); this test makes a
  ;; future refactor that drops that cast loudly fail.
  ;;
  ;; With a = b = 0x80 (i8 = -128) in all 16 bytes, each of 8 i16
  ;; lanes computes the same pair_sum 32768 and wraps to -32768.
  ;;   lane0..3 = -32768 = (i16) 0x8000
  ;;   low i64 = four i16 lanes packed = 0x8000_8000_8000_8000
  (func (export "dot_s_i16_overflow_pin") (result i64)
    v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128
                     -128 -128 -128 -128 -128 -128 -128 -128
    v128.const i8x16 -128 -128 -128 -128 -128 -128 -128 -128
                     -128 -128 -128 -128 -128 -128 -128 -128
    i16x8.relaxed_dot_i8x16_i7x16_s
    i64x2.extract_lane 0)

  ;; Category 6b: i32-lane relaxed_laneselect. Mask alignment per
  ;; i32 lane is 4 bytes wide. SIMDe's lowering is bitwise-select
  ;; `(a & m) | (b & ~m)`, so each bit picks independently.
  ;;
  ;; a   = [0xaaaaaaaa, 0xaaaaaaaa, 0xaaaaaaaa, 0xaaaaaaaa]
  ;; b   = [0x55555555, 0x55555555, 0x55555555, 0x55555555]
  ;; m   = [0xffffffff, 0x00000000, 0xff00ff00, 0x00800000]
  ;; lane0 = a (all bits)                              = 0xaaaaaaaa
  ;; lane1 = b (no bits)                               = 0x55555555
  ;; lane2 = (a & 0xff00ff00) | (b & 0x00ff00ff)       = 0xaa55aa55
  ;; lane3 = (a & 0x00800000) | (b & 0xff7fffff)       = 0x55d55555
  ;; low i64 = (lane1 << 32) | lane0 = 0x55555555_aaaaaaaa
  (func (export "laneselect_i32") (result i64)
    v128.const i32x4 0xaaaaaaaa 0xaaaaaaaa 0xaaaaaaaa 0xaaaaaaaa
    v128.const i32x4 0x55555555 0x55555555 0x55555555 0x55555555
    v128.const i32x4 0xffffffff 0x00000000 0xff00ff00 0x00800000
    i32x4.relaxed_laneselect
    i64x2.extract_lane 0)

  ;; And the high half (lane2, lane3) of the same computation.
  (func (export "laneselect_i32_hi") (result i64)
    v128.const i32x4 0xaaaaaaaa 0xaaaaaaaa 0xaaaaaaaa 0xaaaaaaaa
    v128.const i32x4 0x55555555 0x55555555 0x55555555 0x55555555
    v128.const i32x4 0xffffffff 0x00000000 0xff00ff00 0x00800000
    i32x4.relaxed_laneselect
    i64x2.extract_lane 1)

  ;; Category 6c: i64-lane relaxed_laneselect. Mask alignment is
  ;; 8 mask bytes per i64 lane — the widest case, where the
  ;; top-bit-only vs per-bit interpretation matters most.
  ;;
  ;; a    (per byte) = 0xaa repeated
  ;; b    (per byte) = 0x55 repeated
  ;; mask bytes 0..7  (lane 0) = ff 00 ff 00 ff 00 ff 00
  ;; mask bytes 8..15 (lane 1) = ff ff 00 00 ff ff 00 00
  ;;
  ;; bitwise-select per byte:
  ;;   byte i: (0xaa & m_i) | (0x55 & ~m_i)
  ;;     m=0xff → 0xaa, m=0x00 → 0x55
  ;; lane 0 bytes = [aa 55 aa 55 aa 55 aa 55]  (little-endian)
  ;;              = 0x55aa55aa55aa55aa
  ;; lane 1 bytes = [aa aa 55 55 aa aa 55 55]
  ;;              = 0x5555aaaa5555aaaa
  (func (export "laneselect_i64_lo") (result i64)
    v128.const i8x16 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa
                     0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa
    v128.const i8x16 0x55 0x55 0x55 0x55 0x55 0x55 0x55 0x55
                     0x55 0x55 0x55 0x55 0x55 0x55 0x55 0x55
    v128.const i8x16 0xff 0x00 0xff 0x00 0xff 0x00 0xff 0x00
                     0xff 0xff 0x00 0x00 0xff 0xff 0x00 0x00
    i64x2.relaxed_laneselect
    i64x2.extract_lane 0)
  (func (export "laneselect_i64_hi") (result i64)
    v128.const i8x16 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa
                     0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa 0xaa
    v128.const i8x16 0x55 0x55 0x55 0x55 0x55 0x55 0x55 0x55
                     0x55 0x55 0x55 0x55 0x55 0x55 0x55 0x55
    v128.const i8x16 0xff 0x00 0xff 0x00 0xff 0x00 0xff 0x00
                     0xff 0xff 0x00 0x00 0xff 0xff 0x00 0x00
    i64x2.relaxed_laneselect
    i64x2.extract_lane 1)

  ;; Category 8b: relaxed_trunc f32 → i32 at the exact INT32_MAX+1
  ;; boundary. The hex floats are chosen specifically:
  ;;   lane 0: 0x1.fffffep+30 = 2147483520.0f
  ;;           (largest f32 strictly less than INT32_MAX+1)
  ;;           — must convert cleanly to 2147483520 (= 0x7fffff80)
  ;;   lane 1: 0x1p+31       = 2147483648.0f (= INT32_MAX+1, exact
  ;;           f32 representation but unrepresentable as signed i32)
  ;;           — spec-allowed: any i32 value
  ;;           — WAMR via SIMDe vcvtq_s32_f32 saturates → INT32_MAX
  ;;             (= 0x7fffffff)
  ;;   lane 2: 0.0           — must be 0
  ;;   lane 3: -0x1p+31      = -2147483648.0f (= INT32_MIN, exactly
  ;;           representable) — must be INT32_MIN (= 0x80000000)
  ;; low i64 = (lane1 << 32) | lane0 = 0x7fffffff_7fffff80
  (func (export "trunc_f32_int_max_boundary") (result i64)
    v128.const f32x4 0x1.fffffep+30 0x1p+31 0 -0x1p+31
    i32x4.relaxed_trunc_f32x4_s
    i64x2.extract_lane 0)
  (func (export "trunc_f32_int_max_boundary_hi") (result i64)
    v128.const f32x4 0x1.fffffep+30 0x1p+31 0 -0x1p+31
    i32x4.relaxed_trunc_f32x4_s
    i64x2.extract_lane 1)

  ;; Category 1b: relaxed_min with NaN-as-both-operands. Three
  ;; common implementations diverge here:
  ;;   x86 minps:    returns the second operand bit-pattern
  ;;   ARM FMINNM:   propagates one of the NaN payloads
  ;;   wasm.min:     returns a canonical NaN (0x7fc00000 f32)
  ;; WAMR via SIMDe + aarch64 hardware ends up with the canonical
  ;; NaN bit pattern in our build. Pin that.
  ;; low i64 = (lane1 << 32) | lane0 = 0x7fc00000_7fc00000
  (func (export "min_f32_both_nan") (result i64)
    v128.const f32x4 nan nan nan nan
    v128.const f32x4 nan nan nan nan
    f32x4.relaxed_min
    i64x2.extract_lane 0)

  ;; Category 2b: relaxed_max with crossed (+0, -0) pairs. wasm.max
  ;; says +0 > -0 (so +0 wins regardless of order). x86 maxps just
  ;; returns the second operand. ARM FMAX returns the +0 side.
  ;; lane 0: max(+0, -0)
  ;; lane 1: max(-0, +0)
  ;; Pin to WAMR's observable result.
  ;;
  ;; Our impl is bitwise-equivalent to wasm.max: returns +0 in both
  ;; lanes regardless of operand order.
  ;; lane0 = lane1 = +0.0 = 0x0000000000000000 (f64)
  ;; low i64  = 0
  ;; high i64 = 0
  (func (export "max_f64_signed_zero_pair_lo") (result i64)
    v128.const f64x2 +0.0 -0.0
    v128.const f64x2 -0.0 +0.0
    f64x2.relaxed_max
    i64x2.extract_lane 0)
  (func (export "max_f64_signed_zero_pair_hi") (result i64)
    v128.const f64x2 +0.0 -0.0
    v128.const f64x2 -0.0 +0.0
    f64x2.relaxed_max
    i64x2.extract_lane 1)
)
"#;

struct Module {
    _bytes: Vec<u8>,
    module: wasm_module_t,
    inst: wasm_module_inst_t,
    exec: wasm_exec_env_t,
}

impl Module {
    fn from_wat(src: &str) -> Result<Self> {
        ensure_init();
        let mut bytes = wat::parse_str(src).map_err(|e| anyhow!("wat parse: {e}"))?;
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
            let m = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            return Err(anyhow!("load: {m}"));
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
            let m = unsafe { std::ffi::CStr::from_ptr(err.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            unsafe { wasm_runtime_unload(module) };
            return Err(anyhow!("instantiate: {m}"));
        }
        let exec = unsafe { wasm_runtime_create_exec_env(inst, 64 * 1024) };
        if exec.is_null() {
            unsafe {
                wasm_runtime_deinstantiate(inst);
                wasm_runtime_unload(module);
            }
            return Err(anyhow!("create_exec_env"));
        }
        Ok(Self {
            _bytes: bytes,
            module,
            inst,
            exec,
        })
    }

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
                "(no msg)".to_string()
            } else {
                unsafe { std::ffi::CStr::from_ptr(p) }
                    .to_string_lossy()
                    .into_owned()
            };
            return Err(anyhow!("trap: {m}"));
        }
        Ok((argv[0] as u64 | ((argv[1] as u64) << 32)) as i64)
    }

    fn call_i64_with_arg(&self, name: &str, arg: i32) -> Result<i64> {
        unsafe { wasm_runtime_clear_exception(self.inst) };
        let cn = CString::new(name)?;
        let f = unsafe { wasm_runtime_lookup_function(self.inst, cn.as_ptr()) };
        if f.is_null() {
            return Err(anyhow!("export `{name}` not found"));
        }
        let mut argv = [arg as u32, 0, 0];
        let ok =
            unsafe { wasm_runtime_call_wasm(self.exec, f, 1, argv.as_mut_ptr()) };
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
        Ok((argv[0] as u64 | ((argv[1] as u64) << 32)) as i64)
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

fn load() -> Module {
    Module::from_wat(ABUSE_WAT).expect("module load")
}

// All test cases below pin the value WAMR currently produces on
// aarch64 (M4 / Apple Silicon). Where the spec allows multiple
// answers, the pin documents which point in the legal bracket
// WAMR lands at — so a regression toward a different (still-legal
// but unexpected) value would still trip the test.

#[test]
fn cat1_min_f32_nan_lo() {
    // Both lanes 0,1 produce NaN (canonical-quiet 0x7fc00000) when
    // either operand is NaN. SIMDe's aarch64 path uses vminq_f32
    // which propagates NaN bit-for-bit.
    assert_eq!(load().call_i64("min_f32_nan_lo").unwrap(), 0x7fc000007fc00000u64 as i64);
}

#[test]
fn cat1_max_f64_nan() {
    assert_eq!(load().call_i64("max_f64_nan").unwrap(), 0x7ff8000000000000u64 as i64);
}

#[test]
fn cat2_min_signed_zero() {
    // SIMDe's aarch64 vminq_f32 returns -0.0 when comparing
    // +0.0 vs -0.0 (per IEEE-754-compare ordering).
    assert_eq!(load().call_i64("min_signed_zero").unwrap(), 0x8000000080000000u64 as i64);
}

#[test]
fn cat3_madd_rounding() {
    // Hardware FMA single-rounded result.
    assert_eq!(load().call_i64("madd_rounding").unwrap(), 0xad000000i64);
}

#[test]
fn cat3_madd_overflow() {
    // -FLT_MAX (0xff7fffff). Hardware FMA avoids the
    // overflow-to-infinity that a separate mul+add would hit.
    assert_eq!(load().call_i64("madd_overflow").unwrap(), 0xff7fffffi64);
}

/// Helper: f32 bit pattern is any NaN (exp == 0xff, fraction != 0).
fn f32_bits_are_nan(bits: u32) -> bool {
    ((bits >> 23) & 0xff) == 0xff && (bits & 0x7fffff) != 0
}

#[test]
fn cat3b_madd_inf_times_zero_propagates_nan() {
    // IEEE 754 §7.2: Inf * 0 is an invalid multiply and produces
    // NaN regardless of the subsequent + c. Both fused fma() and
    // unfused mul+add lowerings of relaxed_madd produce a NaN
    // here, but the specific NaN bit pattern is impl-defined.
    // Check the IEEE-754 NaN predicate per lane rather than a
    // specific bit pattern.
    let m = load();
    for half in [("lo", 0u32), ("hi", 1)] {
        let name = format!("madd_inf_times_zero_{}", half.0);
        let packed = m.call_i64(&name).unwrap() as u64;
        let lo32 = (packed & 0xffffffff) as u32;
        let hi32 = (packed >> 32) as u32;
        assert!(
            f32_bits_are_nan(lo32),
            "{name} low f32 = {lo32:#010x} not NaN"
        );
        assert!(
            f32_bits_are_nan(hi32),
            "{name} high f32 = {hi32:#010x} not NaN"
        );
    }
}

#[test]
fn cat4_swizzle_oob_lo() {
    // All-zero output. aarch64 vqtbl1q_u8 zeros every lane whose
    // index is >= 16. (x86 pshufb would do the same on this input
    // since all indices are >= 16.)
    assert_eq!(load().call_i64("swizzle_oob_lo").unwrap(), 0);
}

#[test]
fn cat4_swizzle_oob_hi_bit() {
    // MSB-set indices also zero on both aarch64 and x86 lowerings.
    assert_eq!(load().call_i64("swizzle_oob_hi_bit").unwrap(), 0);
}

#[test]
fn cat5_q15mulr_overflow() {
    // Lane 0: (-32768 * -32768 + 0x4000) >> 15 = 32768 — overflows
    // i16. Spec relaxes the result so an implementation may pick
    // either saturate (0x7fff) or wrap (0x8000). Lanes 1, 2 are
    // deterministic (32767 = 0x7fff, 32766 = 0x7ffe respectively).
    //   Lane 1: (32767 * 32768 + 0x4000) >> 15 = 32767 = 0x7fff
    //   Lane 2: (32767 * 32767 + 0x4000) >> 15 = 32766 = 0x7ffe
    //
    // Low i64 (lanes 0..3 packed) is one of two spec-allowed values:
    //   sat-on-lane-0:  0x7ffe_7fff_7fff   (current WAMR / SIMDe)
    //   wrap-on-lane-0: 0x7ffe_7fff_8000   (also spec-conformant)
    //
    // Use membership so a future WAMR switch to wrap doesn't
    // false-positive against a spec-conformant impl change.
    let v = load().call_i64("q15mulr_overflow").unwrap() as u64;
    let allowed: [u64; 2] = [0x0000_7ffe_7fff_7fff, 0x0000_7ffe_7fff_8000];
    assert!(
        allowed.contains(&v),
        "q15mulr_overflow result {:#018x} not in spec-allowed set [{:#018x}, {:#018x}]",
        v,
        allowed[0],
        allowed[1]
    );
}

#[test]
fn cat6_laneselect_i16_mask_0x80() {
    // SIMDe lowers relaxed_laneselect as bitwise select
    // `(a & mask) | (b & ~mask)`. Mask 0x0080 on i16 → only the
    // low byte's MSB selects from `a`; the rest selects from `b`.
    // Lane 0 (a=0x1111, b=0xaaaa, mask=0x0080): (0 | (0xaaaa &
    // 0xff7f)) = 0xaa2a.
    assert_eq!(load().call_i64("laneselect_i16_mask_0x80").unwrap(), 0xdddd4ccc22bbaa2au64 as i64);
}

#[test]
fn cat6_laneselect_i8_mixed() {
    assert_eq!(load().call_i64("laneselect_i8_mixed").unwrap(), 0x0976fe6f44cca22au64 as i64);
}

#[test]
fn cat7_dot_i8_i7_extreme() {
    // WAMR's hand-rolled `i16x8_relaxed_dot_i8x16_i7x16_s` chooses
    // signed-by-signed semantics (no PMADDUBSW saturation, no
    // VPDPBUSD). (-128 * -127 + -128 * -127) per lane = 32512 =
    // 0x7f00. Spec allows -32768 (PMADDUBSW), 32512 (s*s), or
    // 33024 (u*u); WAMR picks the middle one and the pin records
    // that choice.
    assert_eq!(load().call_i64("dot_i8_i7_extreme").unwrap(), 0x7f007f007f007f00u64 as i64);
}

#[test]
fn cat7_dot_add_normal() {
    // Lane 0: 4*(1*2) + 100 = 108 = 0x6c.
    // Lane 1: 4*(1*2) + 200 = 208 = 0xd0.
    assert_eq!(load().call_i64("dot_add_normal").unwrap(), 0xd00000006ci64);
}

#[test]
fn cat8_trunc_nan_inf_no_crash() {
    // Spec allows any value for NaN/inf inputs. WAMR via SIMDe on
    // aarch64 returns 0 for NaN and INT32_MAX for +inf, matching
    // hardware fcvtzs saturation.
    let v = load().call_i64("trunc_nan_inf").unwrap();
    // Don't pin the exact bits — only that it didn't crash and the
    // upper bits look like saturate-to-INT32_MAX or 0.
    assert_eq!(v as u64, 0x7fffffff00000000u64);
}

#[test]
fn cat8_trunc_huge_no_crash() {
    // Lane 0 ≈ 2^62 → saturate to UINT32_MAX (0xffffffff).
    // Lane 1 ≈ -2^62 → 0 (negative → 0 for unsigned).
    assert_eq!(load().call_i64("trunc_huge").unwrap(), 0xffffffffi64);
}

#[test]
fn cat8_trunc_f64_zero_pinned() {
    // Documents the spec-allowed divergence from wasmtime: SIMDe's
    // aarch64 path uses `vcvtq_s64_f64 + vmovn_s64`, which
    // saturates +inf to INT64_MAX (0x7fffffffffffffff) then
    // narrows the low 32 bits → `0xffffffff`. wasmtime Cranelift
    // saturates directly to INT32_MAX (`0x7fffffff`). Both are
    // spec-conformant under the relaxed-SIMD implementation-
    // defined-behavior clause for non-finite inputs.
    assert_eq!(load().call_i64("trunc_f64_zero").unwrap(), 0xffffffff00000000u64 as i64);
}

#[test]
fn cat9_determinism_madd() {
    // Two identical relaxed_madd calls with constant inputs;
    // result XORed with itself = 0 if deterministic.
    assert_eq!(load().call_i64("determinism_madd").unwrap(), 0);
}

#[test]
fn cat11_relaxed_inside_if_then() {
    // (then) arm: madd((1,2,3,4), (5,6,7,8), (0.5,0.5,0.5,0.5)).
    // Lane 0 = 5.5, lane 1 = 12.5.
    // Bit pattern: f32(5.5) = 0x40b00000, f32(12.5) = 0x41480000.
    // i64 low = (lane1 << 32) | lane0 = 0x4148000040b00000.
    assert_eq!(
        load().call_i64_with_arg("relaxed_inside_if", 1).unwrap(),
        0x4148000040b00000u64 as i64
    );
}

#[test]
fn cat11_relaxed_inside_if_else() {
    // (else) arm: swizzle with zero source AND zero indices = 0.
    assert_eq!(
        load().call_i64_with_arg("relaxed_inside_if", 0).unwrap(),
        0
    );
}

#[test]
fn cat12_legacy_then_relaxed() {
    // First swizzle: a=[0..15], idx=[15..0] → reversed.
    // Then relaxed_swizzle with idx=[7,6,5,4,3,2,1,0,15,...,8]
    // re-selects bytes 8..15 of the reversed source (which were
    // bytes 0..7 of the original), so lanes 0..7 = [8..15].
    // Bit pattern: lane0=0x08, ..., lane7=0x0f.
    // i64 low = 0x0f0e0d0c0b0a0908.
    assert_eq!(
        load().call_i64("legacy_then_relaxed").unwrap(),
        0x0f0e0d0c0b0a0908u64 as i64
    );
}

#[test]
fn bonus_load_unaligned() {
    // v128.store at offset 5 (unaligned), then v128.load from same
    // address. Combined with f32x4.relaxed_madd to give a stable
    // bit pattern. WAMR's `CHECK_MEMORY_OVERFLOW` does an unaligned
    // byte-by-byte access internally; UBSan's alignment checker is
    // suppressed by the build's `-fno-sanitize=alignment` (matches
    // the runtime's documented unaligned-access support). This
    // test exists primarily to soak-run the unaligned path under
    // ASan + UBSan.
    let v = load().call_i64("load_unaligned").unwrap();
    // Don't pin the exact bits (the byte pattern depends on the
    // unaligned read endianness); just confirm no trap.
    assert_ne!(v, 0);
}

// ---------------------------------------------------------------
// Boundary tests added after the codex-bot review on PR #3.
// ---------------------------------------------------------------

#[test]
fn cat7b_dot_add_i16_overflow() {
    // The exact case the chatgpt-codex-connector bot flagged on
    // wasm_interp_fast.c:7674. With a = b = 0x80 (i8 = -128) in
    // all 16 bytes and c = 0:
    //   pair_sum overflows i16: 32768 → wrap to -32768
    //   ext_pair sum: (-32768) + (-32768) = -65536
    //   per i32 lane: -65536 = 0xffff0000
    //   low i64 = (lane1 << 32) | lane0 = 0xffff0000_ffff0000
    //
    // Pre-fix direct-sum impl produced 65536 per lane (0x00010000),
    // which is NOT in the spec-allowed set {-65536, -1, 65534}.
    // This test will fail loudly if anyone refactors the impl back
    // to the direct-sum shape.
    assert_eq!(
        load().call_i64("dot_add_i16_overflow").unwrap(),
        0xffff0000ffff0000u64 as i64
    );
}

#[test]
fn cat7c_dot_s_i16_overflow_pin() {
    // Sibling op to the bug we fixed. The current i16x8 dot impl
    // correctly truncates to i16 via the `(int16)sum` cast on
    // wasm_interp_fast.c:8103. Same overflow input pattern
    // (a = b = 0x80 in all bytes); each i16 lane should equal
    // (int16)32768 = -32768 = 0x8000.
    //   low i64 = 4 i16 lanes packed = 0x8000_8000_8000_8000
    //
    // If a future refactor drops the (int16) cast, this test fails
    // before the bug ships.
    assert_eq!(
        load().call_i64("dot_s_i16_overflow_pin").unwrap(),
        0x8000800080008000u64 as i64
    );
}

#[test]
fn cat6b_laneselect_i32() {
    // i32-lane laneselect with SIMDe bitwise-select semantics.
    // See WAT comment for per-lane derivation.
    // lane0 = 0xaaaaaaaa (mask all-ones → a)
    // lane1 = 0x55555555 (mask all-zeros → b)
    // low i64 = (lane1 << 32) | lane0 = 0x55555555_aaaaaaaa
    assert_eq!(
        load().call_i64("laneselect_i32").unwrap(),
        0x55555555aaaaaaaau64 as i64
    );
}

#[test]
fn cat6b_laneselect_i32_hi() {
    // High i64 of the same laneselect_i32 computation.
    // lane2 = (a & 0xff00ff00) | (b & 0x00ff00ff) = 0xaa55aa55
    // lane3 = (a & 0x00800000) | (b & 0xff7fffff) = 0x55d55555
    // high i64 = (lane3 << 32) | lane2 = 0x55d55555_aa55aa55
    assert_eq!(
        load().call_i64("laneselect_i32_hi").unwrap(),
        0x55d55555aa55aa55u64 as i64
    );
}

#[test]
fn cat6c_laneselect_i64_lo() {
    // i64-lane laneselect (widest case) with per-byte bitwise-
    // select per SIMDe semantics.
    // Lane 0 mask = ff 00 ff 00 ff 00 ff 00 → alternating a, b
    // bytes = [aa 55 aa 55 aa 55 aa 55]
    // little-endian i64 = 0x55aa55aa55aa55aa
    assert_eq!(
        load().call_i64("laneselect_i64_lo").unwrap(),
        0x55aa55aa55aa55aau64 as i64
    );
}

#[test]
fn cat6c_laneselect_i64_hi() {
    // Lane 1 mask = ff ff 00 00 ff ff 00 00 → pairs
    // bytes = [aa aa 55 55 aa aa 55 55]
    // little-endian i64 = 0x5555aaaa5555aaaa
    assert_eq!(
        load().call_i64("laneselect_i64_hi").unwrap(),
        0x5555aaaa5555aaaau64 as i64
    );
}

#[test]
fn cat8b_trunc_f32_int_max_boundary() {
    // lane 0 = 2147483520.0f → must be exactly 2147483520 (=
    //   0x7fffff80); this value is representable in both f32 and
    //   i32, so any conformant impl produces it.
    // lane 1 = INT32_MAX+1 as f32 → spec allows ANY i32, our
    //   SIMDe-via-vcvtq_s32_f32 path saturates to INT32_MAX (=
    //   0x7fffffff).
    // low i64 = (lane1 << 32) | lane0 = 0x7fffffff_7fffff80
    assert_eq!(
        load().call_i64("trunc_f32_int_max_boundary").unwrap(),
        0x7fffffff7fffff80u64 as i64
    );
}

#[test]
fn cat8b_trunc_f32_int_max_boundary_hi() {
    // lane 2 = 0.0 → 0
    // lane 3 = INT32_MIN as f32 → INT32_MIN (= 0x80000000)
    // high i64 = (lane3 << 32) | lane2 = 0x80000000_00000000
    assert_eq!(
        load().call_i64("trunc_f32_int_max_boundary_hi").unwrap(),
        0x8000000000000000u64 as i64
    );
}

#[test]
fn cat1b_min_f32_both_nan() {
    // NaN-vs-NaN: spec allows three different impl behaviors
    // (x86 minps, ARM FMINNM, wasm.min). WAMR via SIMDe on
    // aarch64 produces canonical NaN bit pattern 0x7fc00000 in
    // both observed lanes.
    // low i64 = (lane1 << 32) | lane0 = 0x7fc00000_7fc00000
    assert_eq!(
        load().call_i64("min_f32_both_nan").unwrap(),
        0x7fc000007fc00000u64 as i64
    );
}

#[test]
fn cat2b_max_f64_signed_zero_pair_lo() {
    // max(+0, -0): wasm.max says +0 > -0 so result is +0.
    // x86 maxps would return -0 (second operand); ARM FMAX returns
    // +0. Our impl pins to +0 = 0x0000000000000000.
    assert_eq!(
        load().call_i64("max_f64_signed_zero_pair_lo").unwrap(),
        0i64
    );
}

#[test]
fn cat2b_max_f64_signed_zero_pair_hi() {
    // max(-0, +0): same logic — +0 wins regardless of order.
    assert_eq!(
        load().call_i64("max_f64_signed_zero_pair_hi").unwrap(),
        0i64
    );
}
