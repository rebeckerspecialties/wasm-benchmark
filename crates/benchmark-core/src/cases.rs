//! Runtime-independent case table: every (wasm, export, arg) the harness
//! measures, plus how to drive it and the cross-runtime-consensus result.
//! `run_case` dispatches one case to one runtime and turns a wrong result
//! into an error, the same contract as `report_from_checked` on the C ABI
//! side.

use anyhow::{anyhow, Result};

use crate::*;

/// How a case is driven.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `export(arg: i32) -> i32`, no imports.
    I32ToI32,
    /// Porffor's `m() -> (f64, i32)` with a `("", "b"): (f64) -> ()` host
    /// print import; each runtime has a dedicated runner.
    PorfforMain,
    /// Sightglass sqlite3 speedtest1: WASI preview-1 + `bench.*` imports.
    /// Only the Pulley side has the import shim.
    Sqlite3,
}

#[derive(Clone, Copy, Debug)]
pub struct Case {
    /// Stable machine name (what `WORKLOADS=` matches and what the result
    /// files key on).
    pub id: &'static str,
    /// Human label, identical to the Swift app's row text after the
    /// runtime prefix.
    pub label: &'static str,
    pub wasm: &'static [u8],
    pub func: &'static str,
    pub arg: i32,
    /// Cross-runtime consensus result; `None` when the result depends on
    /// the iteration count (xmrsplayer) or there is no reference.
    pub expected: Option<i32>,
    pub shape: Shape,
}

const fn c(
    id: &'static str,
    label: &'static str,
    wasm: &'static [u8],
    func: &'static str,
    arg: i32,
    expected: Option<i32>,
) -> Case {
    Case { id, label, wasm, func, arg, expected, shape: Shape::I32ToI32 }
}

pub const CASES: &[Case] = &[
    c("fib", "fib(30)", FIB_WASM, "fib", 30, Some(EXPECTED_FIB_30)),
    c("fib_tail", "fib_tail(100000) [return_call]", FIB_TAIL_WASM, "fib_tail", 100000,
      Some(EXPECTED_FIB_TAIL_100K)),
    c("factorial", "factorial(20)", FACTORIAL_WASM, "factorial", 20, Some(EXPECTED_FACTORIAL_20)),
    c("sieve", "sieve(10000)", SIEVE_WASM, "sieve", 10000, Some(EXPECTED_SIEVE_10K)),
    c("crc32", "crc32(64KB)", CRC32_WASM, "crc32", 0xC0FFEE, Some(EXPECTED_CRC32_C0FFEE)),
    c("matmul_simd", "matmul simd128 (64×64 f32)", MATMUL_SIMD_WASM, "matmul", 0xBEEF,
      Some(EXPECTED_MATMUL_SIMD)),
    c("matmul_fma", "matmul relaxed-simd FMA", MATMUL_FMA_WASM, "matmul_fma", 0xBEEF,
      Some(EXPECTED_MATMUL_FMA)),
    c("convolution", "convolution 256×256", CONVOLUTION_WASM, "convolve", 0xCAFE,
      Some(EXPECTED_CONVOLUTION)),
    c("audio_dsp", "audio DSP (1000 frames × 512)", AUDIO_DSP_WASM, "audio_dsp", 0x5C7,
      Some(EXPECTED_AUDIO_DSP)),
    c("bulk_memory", "bulk_memory (memory.copy/fill)", BULK_MEMORY_WASM, "bulk_memory", 0xB0CC,
      Some(EXPECTED_BULK_MEMORY)),
    c("call_indirect", "call_indirect (200K dispatches)", CALL_INDIRECT_WASM, "call_indirect",
      0xC1AA, Some(EXPECTED_CALL_INDIRECT)),
    c("factorial.scalar", "factorial(20) [scalar build]", FACTORIAL_SCALAR_WASM, "factorial", 20,
      Some(EXPECTED_FACTORIAL_20)),
    c("sieve.scalar", "sieve(10000) [scalar build]", SIEVE_SCALAR_WASM, "sieve", 10000,
      Some(EXPECTED_SIEVE_10K)),
    c("crc32.scalar", "crc32(64KB) [scalar build]", CRC32_SCALAR_WASM, "crc32", 0xC0FFEE,
      Some(EXPECTED_CRC32_C0FFEE)),
    c("convolution.scalar", "convolution 256×256 [scalar build]", CONVOLUTION_SCALAR_WASM,
      "convolve", 0xCAFE, Some(EXPECTED_CONVOLUTION)),
    c("bulk_memory.scalar", "bulk_memory (memory.copy/fill) [scalar build]",
      BULK_MEMORY_SCALAR_WASM, "bulk_memory", 0xB0CC, Some(EXPECTED_BULK_MEMORY)),
    c("xmrsplayer", "xmrsplayer (1024-frame buffer)", XMRSPLAYER_WASM, "play_buffer", 0, None),
    c("vtable_mono", "vtable_mono (200K)", VTABLE_DISPATCH_WASM, "vtable_mono", 0xC1AA,
      Some(EXPECTED_VTABLE_MONO)),
    c("vtable_bi", "vtable_bi (200K)", VTABLE_DISPATCH_WASM, "vtable_bi", 0xC1AA,
      Some(EXPECTED_VTABLE_BI)),
    c("vtable_poly4", "vtable_poly4 (200K)", VTABLE_DISPATCH_WASM, "vtable_poly4", 0xC1AA,
      Some(EXPECTED_VTABLE_POLY4)),
    c("vtable_poly6", "vtable_poly6 (200K)", VTABLE_DISPATCH_WASM, "vtable_poly6", 0xC1AA,
      Some(EXPECTED_VTABLE_POLY6)),
    c("graphql_as", "graphql-validation (AS)", GRAPHQL_VALIDATION_AS_WASM, "validate_once", 0,
      Some(EXPECTED_GRAPHQL_AS)),
    Case {
        id: "graphql_porf",
        label: "graphql-validation (Porffor)",
        wasm: GRAPHQL_VALIDATION_PORF_WASM,
        func: "m",
        arg: 0,
        expected: None,
        shape: Shape::PorfforMain,
    },
    Case {
        id: "sqlite3",
        label: "sqlite3 speedtest1 (in-mem)",
        wasm: SQLITE3_WASM,
        func: "_start",
        arg: 0,
        expected: None,
        shape: Shape::Sqlite3,
    },
];

/// Every runtime the harness links, with its `RUNTIMES=` token and the
/// bracketed row prefix the Swift app uses.
pub const RUNTIMES: &[(Runtime, &str, &str)] = &[
    (Runtime::Pulley, "pulley", "[Pulley]"),
    (Runtime::Wamr, "wamr", "[ WAMR ]"),
    (Runtime::Wasm3, "wasm3", "[wasm3 ]"),
    (Runtime::WasmEdge, "wasmedge", "[WE    ]"),
    (Runtime::Zwasm, "zwasm", "[zwasm ]"),
    (Runtime::Wasmz, "wasmz", "[wasmz ]"),
    (Runtime::Tinywasm, "tinywasm", "[tinywm]"),
];

pub fn runtime_token(rt: Runtime) -> &'static str {
    RUNTIMES.iter().find(|r| r.0 == rt).map(|r| r.1).unwrap_or("?")
}

/// Run one case on one runtime. A result that disagrees with the
/// consensus reference is an error: a fast wrong answer is not a result.
pub fn run_case(rt: Runtime, case: &Case) -> Result<RunReport> {
    let r = match case.shape {
        Shape::I32ToI32 => run_workload_with(rt, case.wasm, case.func, case.arg),
        Shape::PorfforMain => match rt {
            Runtime::Pulley => graphql_validation::run_graphql_validation_porf(case.wasm),
            Runtime::Wamr => wamr::run_graphql_validation_porf_wamr(case.wasm),
            Runtime::WasmEdge => wasmedge::run_graphql_validation_porf_wasmedge(case.wasm),
            Runtime::Zwasm => zwasm::run_graphql_validation_porf_zwasm(case.wasm),
            Runtime::Wasmz => wasmz::run_graphql_validation_porf_wasmz(case.wasm),
            Runtime::Tinywasm => tinywasm::run_graphql_validation_porf_tinywasm(case.wasm),
            // wasm3 has no exception handling or multi-value host call path.
            Runtime::Wasm3 => wasm3::run_workload_wasm3(case.wasm, case.func, case.arg),
        },
        Shape::Sqlite3 => match rt {
            Runtime::Pulley => sqlite3::run_sqlite3(case.wasm),
            _ => Err(anyhow!(
                "N/A — the harness only has a WASI preview-1 + bench.* import shim for Pulley"
            )),
        },
    }?;
    if let Some(exp) = case.expected {
        if r.result != exp {
            return Err(anyhow!(
                "wrong result: got {}, expected {} (cross-runtime consensus)",
                r.result,
                exp
            ));
        }
    }
    Ok(r)
}
