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
    /// `export(arg: i32) -> i32` on a fresh instance per sample: instantiate
    /// + call is timed, teardown is not. For features whose hot path runs
    /// at instantiation (extended constant expressions).
    InstantiateEach,
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
    // Wasm 3.0 feature benchmarks, one per feature at least two shipped
    // runtimes support (docs/runtime-comparison-2026-09-22.md has the
    // support matrix). `.<twin>` rows are the same program without the
    // feature.
    c("tailcall_fsm", "tail-call FSM (65536 return_call)", TAILCALL_FSM_WASM, "tailcall_fsm", 7,
      Some(EXPECTED_TAILCALL_FSM)),
    c("eh_parser_exnref", "EH parser, exnref (4096 stmts, 25% throw)", EH_PARSER_EXNREF_WASM,
      "eh_parser_exnref", 7, Some(EXPECTED_EH_PARSER)),
    c("eh_parser_legacy", "EH parser, legacy try/catch (4096 stmts)", EH_PARSER_LEGACY_WASM,
      "eh_parser_legacy", 7, Some(EXPECTED_EH_PARSER)),
    c("gc_trees", "GC binary trees (~130K struct.new)", GC_TREES_WASM, "gc_trees", 7,
      Some(EXPECTED_GC_TREES)),
    c("callref_dispatch", "call_ref dispatch (200K, typed table)", CALLREF_DISPATCH_WASM,
      "callref_dispatch", 7, Some(EXPECTED_CALLREF_DISPATCH)),
    c("callref_dispatch.indirect", "call_ref twin: call_indirect (200K)",
      CALLREF_DISPATCH_INDIRECT_WASM, "callref_dispatch", 7, Some(EXPECTED_CALLREF_DISPATCH)),
    c("relaxed_dot", "relaxed-SIMD int8 dot (64×64×256)", RELAXED_KERNELS_WASM, "relaxed_dot", 7,
      Some(EXPECTED_RELAXED_DOT)),
    c("relaxed_madd", "relaxed-SIMD FMA Horner (16K pts)", RELAXED_KERNELS_WASM, "relaxed_madd",
      7, Some(EXPECTED_RELAXED_MADD)),
    c("mem64_chase", "memory64 pointer chase (64 MiB, 256K hops)", MEM64_CHASE_WASM,
      "mem64_chase", 7, Some(EXPECTED_MEM64_CHASE)),
    c("mem64_chase.mem32", "memory64 twin: 32-bit memory", MEM64_CHASE_MEM32_WASM, "mem64_chase",
      7, Some(EXPECTED_MEM64_CHASE)),
    c("multimem_transform", "multi-memory transform (3 memories)", MULTIMEM_TRANSFORM_WASM,
      "multimem_transform", 7, Some(EXPECTED_MULTIMEM_TRANSFORM)),
    c("multimem_transform.single", "multi-memory twin: one memory",
      MULTIMEM_TRANSFORM_SINGLE_WASM, "multimem_transform", 7, Some(EXPECTED_MULTIMEM_TRANSFORM)),
    Case {
        id: "extconst_init",
        label: "extended-const instantiate (2560 globals)",
        wasm: EXTCONST_INIT_WASM,
        func: "extconst_init",
        arg: 7,
        expected: Some(EXPECTED_EXTCONST_INIT),
        shape: Shape::InstantiateEach,
    },
    Case {
        id: "extconst_init.mvp",
        label: "extended-const twin: MVP consts",
        wasm: EXTCONST_INIT_MVP_WASM,
        func: "extconst_init",
        arg: 7,
        expected: Some(EXPECTED_EXTCONST_INIT),
        shape: Shape::InstantiateEach,
    },
    Case {
        id: "graphql_porf",
        label: "graphql-validation (Porffor)",
        wasm: GRAPHQL_VALIDATION_PORF_WASM,
        func: "m",
        arg: 0,
        expected: None,
        shape: Shape::PorfforMain,
    },
    // The same JS with graphql-js's real `try { visit() } catch (e) { if (e
    // !== abortObj) throw e; }`: one legacy try/catch around the visit, the
    // visitor's throws unwinding into it.
    Case {
        id: "graphql_porf_trycatch",
        label: "graphql-validation (Porffor try/catch)",
        wasm: GRAPHQL_VALIDATION_PORF_ACCURATE_WASM,
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

/// (runtime, case id, reason) pairs that crash the whole process (SIGSEGV
/// inside the runtime), which would take every later row of an app launch
/// or CLI run down with them. They are reported as N/A with the reason,
/// never silently dropped. Re-check on every runtime bump.
pub const KNOWN_CRASHES: &[(Runtime, &str, &str)] = &[
    (Runtime::Wasmz, "sieve",
     "wasmz v0.1.4 segfaults on this module's v128 ops (the scalar build runs)"),
    (Runtime::Wasmz, "crc32",
     "wasmz v0.1.4 segfaults on this module's v128 ops (the scalar build runs)"),
    (Runtime::Zwasm, "gc_trees",
     "zwasm v2.7.0 never reclaims GC structs and grows its GC heap far faster than the \
      allocation rate (124 MB for 31K 24-byte structs); the process segfaults at ~2.3 GB"),
];

pub fn known_crash(rt: Runtime, case_id: &str) -> Option<&'static str> {
    KNOWN_CRASHES.iter().find(|k| k.0 == rt && k.1 == case_id).map(|k| k.2)
}

/// (case id, reason) cases the app's leaderboard does not run.
pub const APP_EXCLUDED_CASES: &[(&str, &str)] = &[(
    "sqlite3",
    "only Pulley has the WASI import shim, and one call takes 86 s on an iPhone XS",
)];

/// (case id, reason) cases the watch app also leaves out, on top of
/// `APP_EXCLUDED_CASES`: one call takes minutes on an S8, or the row's
/// footprint does not fit the watch's memory limit.
pub const WATCH_EXCLUDED_CASES: &[(&str, &str)] = &[
    ("audio_dsp", "one call takes up to 30 s on an Apple Watch S8"),
    ("gc_trees", "WasmEdge and wasmz never collect GC structs"),
    ("mem64_chase", "touches a 64 MiB linear memory"),
    ("mem64_chase.mem32", "touches a 64 MiB linear memory"),
    ("graphql_porf", "peaks at 586 MB on zwasm and 1 GB on tinywasm (macOS)"),
    ("graphql_porf_trycatch", "same program as graphql_porf"),
];

/// (runtime, case id, reason) rows the app leaves out because the runtime
/// keeps the row's memory for as long as the process lives. The app runs
/// every engine in one process, so the footprint would carry into every
/// later row or get the app killed by jetsam. The device pass runs these
/// rows in launches of their own instead (HEAVY_ROWS in
/// scripts/run-device-pass.sh).
pub const APP_SKIPS: &[(Runtime, &str, &str)] = &[
    (Runtime::Zwasm, "xmrsplayer",
     "zwasm v2.7.0 keeps memory per call; the footprint reached 1.2 GB on an iPhone XS"),
    (Runtime::Zwasm, "graphql_porf",
     "zwasm v2.7.0 keeps memory per call; the iPhone XS killed the app (jetsam) in this row"),
    (Runtime::Zwasm, "tailcall_fsm",
     "zwasm v2.7.0's return_call is not constant-space (~50 B per call)"),
    (Runtime::Zwasm, "mem64_chase",
     "zwasm v2.7.0 keeps an instance's 64 MiB memory after the instance is deleted"),
    (Runtime::Zwasm, "mem64_chase.mem32",
     "zwasm v2.7.0 keeps an instance's 64 MiB memory after the instance is deleted"),
    (Runtime::Zwasm, "extconst_init",
     "zwasm v2.7.0 keeps each instance's memory; this row reached 2.6 GB on the M4"),
    (Runtime::Zwasm, "extconst_init.mvp",
     "zwasm v2.7.0 keeps each instance's memory; this row reached 2.0 GB on the M4"),
];

/// Score reference for a case: nanoseconds per call on the reference
/// device (see `score_reference`).
pub fn score_reference_ns(case_id: &str) -> Option<u64> {
    crate::score_reference::SCORE_REFERENCE_NS
        .iter()
        .find(|r| r.0 == case_id)
        .map(|r| r.1)
}

/// Run one case on one runtime. A result that disagrees with the
/// consensus reference is an error: a fast wrong answer is not a result.
pub fn run_case(rt: Runtime, case: &Case) -> Result<RunReport> {
    if let Some(reason) = known_crash(rt, case.id) {
        return Err(anyhow!("N/A — not run: {reason}"));
    }
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
        Shape::InstantiateEach => run_instantiate_each_with(rt, case.wasm, case.func, case.arg),
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
