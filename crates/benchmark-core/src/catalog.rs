//! What the app lists: every engine with the version it was built from,
//! every case with its score reference, which cases the app and the watch
//! run, and the rows it leaves out. `bench_catalog_json` hands this to
//! Swift as one JSON document.

use crate::cases::{self, Case, CASES, RUNTIMES};
use crate::Runtime;

/// One engine as the app shows it.
pub struct Engine {
    pub runtime: Runtime,
    /// `RUNTIMES=` token.
    pub token: &'static str,
    /// Row prefix of the app's console lines (`[Pulley]`).
    pub prefix: &'static str,
    pub name: &'static str,
    /// Project the engine ships in when that differs from its name.
    pub upstream: &'static str,
    /// Release plus commits past it ("2.4.1+364"); empty when unknown.
    pub version: &'static str,
    /// Short hash of the pinned commit; empty for a crates.io release.
    pub commit: &'static str,
    /// Patches the build scripts apply on top of `commit`.
    pub patches: u32,
    /// Whether this build links the runtime (build.rs found its library).
    pub linked: bool,
}

fn count(s: &str) -> u32 {
    s.parse().unwrap_or(0)
}

pub fn engines() -> Vec<Engine> {
    RUNTIMES
        .iter()
        .map(|&(runtime, token, prefix)| {
            let (name, upstream, version, commit, patches, linked) = match runtime {
                Runtime::Pulley => (
                    "Pulley",
                    "wasmtime",
                    env!("BENCH_VERSION_PULLEY"),
                    env!("BENCH_COMMIT_PULLEY"),
                    count(env!("BENCH_PATCHES_PULLEY")),
                    true,
                ),
                Runtime::Wamr => (
                    "WAMR",
                    "",
                    env!("BENCH_VERSION_WAMR"),
                    env!("BENCH_COMMIT_WAMR"),
                    count(env!("BENCH_PATCHES_WAMR")),
                    cfg!(have_wamr),
                ),
                Runtime::Wasm3 => (
                    "wasm3",
                    "",
                    env!("BENCH_VERSION_WASM3"),
                    env!("BENCH_COMMIT_WASM3"),
                    count(env!("BENCH_PATCHES_WASM3")),
                    cfg!(have_wasm3),
                ),
                Runtime::WasmEdge => (
                    "WasmEdge",
                    "",
                    env!("BENCH_VERSION_WASMEDGE"),
                    env!("BENCH_COMMIT_WASMEDGE"),
                    count(env!("BENCH_PATCHES_WASMEDGE")),
                    cfg!(have_wasmedge),
                ),
                Runtime::Zwasm => (
                    "zwasm",
                    "",
                    env!("BENCH_VERSION_ZWASM"),
                    env!("BENCH_COMMIT_ZWASM"),
                    count(env!("BENCH_PATCHES_ZWASM")),
                    cfg!(have_zwasm),
                ),
                Runtime::Wasmz => (
                    "wasmz",
                    "",
                    env!("BENCH_VERSION_WASMZ"),
                    env!("BENCH_COMMIT_WASMZ"),
                    count(env!("BENCH_PATCHES_WASMZ")),
                    cfg!(have_wasmz),
                ),
                Runtime::Tinywasm => (
                    "tinywasm",
                    "",
                    env!("BENCH_VERSION_TINYWASM"),
                    env!("BENCH_COMMIT_TINYWASM"),
                    0,
                    true,
                ),
            };
            Engine { runtime, token, prefix, name, upstream, version, commit, patches, linked }
        })
        .collect()
}

fn excluded(list: &[(&str, &'static str)], case: &Case) -> Option<&'static str> {
    list.iter().find(|e| e.0 == case.id).map(|e| e.1)
}

/// JSON string literal for `s`.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt(s: Option<&str>) -> String {
    s.map(quote).unwrap_or_else(|| "null".to_string())
}

/// The catalog as JSON:
///
/// ```json
/// {"engines": [{"id": 0, "token": "pulley", "prefix": "[Pulley]", "name": "Pulley",
///               "upstream": "wasmtime", "version": "49.0.0+9", "commit": "0d9aebd6",
///               "patches": 0, "linked": true}, ...],
///  "cases": [{"id": "fib", "label": "fib(30)", "reference_ns": 314731068,
///             "app_excluded": null, "watch_excluded": null}, ...],
///  "skips": [{"engine": "zwasm", "case": "xmrsplayer", "reason": "..."}, ...]}
/// ```
pub fn json() -> String {
    let engines: Vec<String> = engines()
        .iter()
        .map(|e| {
            format!(
                "{{\"id\":{},\"token\":{},\"prefix\":{},\"name\":{},\"upstream\":{},\
                 \"version\":{},\"commit\":{},\"patches\":{},\"linked\":{}}}",
                e.runtime as u32,
                quote(e.token),
                quote(e.prefix),
                quote(e.name),
                quote(e.upstream),
                quote(e.version),
                quote(e.commit),
                e.patches,
                e.linked
            )
        })
        .collect();
    let cases: Vec<String> = CASES
        .iter()
        .map(|c| {
            format!(
                "{{\"id\":{},\"label\":{},\"reference_ns\":{},\"app_excluded\":{},\
                 \"watch_excluded\":{}}}",
                quote(c.id),
                quote(c.label),
                cases::score_reference_ns(c.id)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                opt(excluded(cases::APP_EXCLUDED_CASES, c)),
                opt(excluded(cases::WATCH_EXCLUDED_CASES, c)),
            )
        })
        .collect();
    let skips: Vec<String> = cases::APP_SKIPS
        .iter()
        .map(|&(rt, id, reason)| {
            format!(
                "{{\"engine\":{},\"case\":{},\"reason\":{}}}",
                quote(cases::runtime_token(rt)),
                quote(id),
                quote(reason)
            )
        })
        .collect();
    format!(
        "{{\"engines\":[{}],\"cases\":[{}],\"skips\":[{}]}}",
        engines.join(","),
        cases.join(","),
        skips.join(",")
    )
}

