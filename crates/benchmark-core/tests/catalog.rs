//! The app's catalog (catalog.rs): every case has a score reference, the
//! exclusion and skip lists name real cases, and the JSON lists everything.

use benchmark_core::cases::{self, CASES, RUNTIMES};
use benchmark_core::catalog;

#[test]
fn every_case_has_a_score_reference() {
    for c in CASES {
        assert!(cases::score_reference_ns(c.id).is_some(), "{} has no reference", c.id);
    }
}

#[test]
fn exclusions_and_skips_name_real_cases() {
    let known = |id: &str| CASES.iter().any(|c| c.id == id);
    for (id, _) in cases::APP_EXCLUDED_CASES.iter().chain(cases::WATCH_EXCLUDED_CASES) {
        assert!(known(id), "unknown case {id}");
    }
    for (_, id, _) in cases::APP_SKIPS {
        assert!(known(id), "unknown case {id}");
    }
}

#[test]
fn json_lists_every_engine_and_case() {
    let j = catalog::json();
    let v: serde_json::Value = serde_json::from_str(&j).expect("catalog is valid JSON");
    assert_eq!(v["engines"].as_array().unwrap().len(), RUNTIMES.len());
    assert_eq!(v["cases"].as_array().unwrap().len(), CASES.len());
    assert_eq!(v["skips"].as_array().unwrap().len(), cases::APP_SKIPS.len());
    let matmul = v["cases"].as_array().unwrap().iter().find(|c| c["id"] == "matmul_simd").unwrap();
    assert_eq!(matmul["label"], "matmul simd128 (64×64 f32)");
    let sqlite = v["cases"].as_array().unwrap().iter().find(|c| c["id"] == "sqlite3").unwrap();
    assert!(sqlite["app_excluded"].is_string());
    for e in v["engines"].as_array().unwrap() {
        assert!(e["name"].is_string() && e["prefix"].is_string() && e["linked"].is_boolean());
    }
}
