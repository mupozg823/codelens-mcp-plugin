//! Bench gate — CI-enforceable thresholds from
//! `docs/plans/PLAN_extreme-efficiency.md`.
//!
//! These tests lock baseline measurements taken at the start of the
//! PLAN_extreme-efficiency work so that subsequent phases can assert
//! concrete deltas (e.g. "find_symbol bytes 2800 → 900 after P1"). The
//! thresholds come from `benchmarks/baselines/extreme-efficiency.json`
//! so the plan file, the bench gate, and the CI are driven by a single
//! source of truth.
//!
//! Three RED-style tests are included, one per phase we care about
//! *now*:
//!   * `bench_gate_find_symbol_primitive_payload_fits_budget` — current
//!     baseline is ~2.8 KB; the threshold is set to 3 KB so it passes
//!     today but clamps regressions. After Phase P1 (primitive mode)
//!     we tighten the threshold to 900 bytes.
//!   * `bench_gate_review_architecture_cold_latency_under_threshold` —
//!     loose gate (<60 s) so an accidental 5× blow-up fails loudly;
//!     Phase P2 tightens to <3 s.
//!   * `bench_gate_cross_tool_semantic_ready_consistency` — marked
//!     `#[ignore]` so it stays RED for Phase P3 without breaking
//!     `cargo test`, but still runnable via
//!     `cargo test -- --ignored bench_gate_cross_tool`. Once P3 lands
//!     we flip the annotation.

use super::*;
use serde_json::json;
use std::fs;
use std::time::Instant;

fn baseline_json() -> serde_json::Value {
    // Resolve relative to the repo root (two directories above the
    // `codelens-mcp` crate manifest) so `cargo test -p codelens-mcp`
    // works from any cwd.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let repo_root = std::path::Path::new(manifest)
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR should have a repo-root ancestor");
    let path = repo_root.join("benchmarks/baselines/extreme-efficiency.json");
    let content = fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "bench gate could not read baseline file {}: {err}",
            path.display()
        )
    });
    serde_json::from_str(&content).unwrap_or_else(|err| {
        panic!(
            "bench gate could not parse baseline JSON {}: {err}",
            path.display()
        )
    })
}

fn threshold_usize(baseline: &serde_json::Value, task: &str, field: &str) -> usize {
    baseline["tasks"][task]["thresholds"][field]
        .as_u64()
        .unwrap_or_else(|| panic!("baseline threshold missing: tasks.{task}.thresholds.{field}"))
        as usize
}

fn fixture_with_widget() -> codelens_engine::ProjectRoot {
    let project = project_root();
    fs::write(
        project.as_path().join("widget.rs"),
        "/// Emit a decorated widget tag.\n\
         pub fn widget_fn(input: &str) -> String {\n    \
             format!(\"<{input}>\")\n\
         }\n\
         \n\
         /// Pre-compute the widget index.\n\
         pub fn widget_index() -> usize {\n    \
             42\n\
         }\n",
    )
    .unwrap();
    project
}

#[test]
fn bench_gate_find_symbol_primitive_payload_fits_budget() {
    // P0 baseline gate. Asserts that a find_symbol call with
    // include_body=true — the headline workflow documented in the
    // Phase 1-7 commit — stays under the payload ceiling defined in
    // the baseline JSON. After Phase P1 lands the ceiling drops to
    // 900 bytes (primitive response mode). Today it guards against
    // regression past ~3 KB.
    let baseline = baseline_json();
    let max_bytes = threshold_usize(&baseline, "find_symbol_body", "payload_bytes_max");

    let project = fixture_with_widget();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "widget_fn",
            "include_body": true,
            "exact_match": true,
            "max_matches": 5,
        }),
    );
    assert_eq!(payload["success"], json!(true));

    let serialized = serde_json::to_string(&payload).expect("serialize payload");
    let bytes = serialized.len();
    assert!(
        bytes <= max_bytes,
        "find_symbol payload {bytes} bytes exceeds baseline ceiling {max_bytes}; \
         investigate regression or raise the baseline in \
         benchmarks/baselines/extreme-efficiency.json with justification"
    );
}

#[test]
fn bench_gate_review_architecture_cold_latency_under_threshold() {
    // P0 loose gate for review_architecture. Cold p95 is ~47 s in prod
    // with a large import graph; this test uses a tiny fixture (~2
    // files) so the cold call is dominated by PageRank setup rather
    // than graph size. We pin the ceiling to the baseline's
    // `latency_ms_cold_p95_max` (60 s today) so a pathological
    // regression (e.g. infinite loop in the hybrid ranker) fails the
    // gate loudly.
    let baseline = baseline_json();
    let max_ms = threshold_usize(&baseline, "review_architecture", "latency_ms_cold_p95_max");

    let project = fixture_with_widget();
    let state = make_state(&project);

    let start = Instant::now();
    let payload = call_tool(&state, "review_architecture", json!({}));
    let elapsed_ms = start.elapsed().as_millis() as usize;

    // Some test environments may not have a semantic index loaded;
    // either response shape (success=true or success=false with a
    // cleanly degraded payload) is fine for the latency gate — we
    // only care about wall-clock budget.
    assert!(
        payload.is_object(),
        "expected object payload, got: {payload}"
    );
    assert!(
        elapsed_ms <= max_ms,
        "review_architecture took {elapsed_ms} ms, exceeding baseline \
         ceiling {max_ms} ms; investigate regression or warm the cache \
         in subsequent phases"
    );
}

#[test]
#[ignore = "RED gate for Phase P3 (state unification). Fails today \
            because review_architecture omits the `loaded` field \
            while get_ranked_context emits `semantic_ready:false`. \
            Run via `cargo test -- --ignored` to see the gap; the \
            ignore flag flips off when P3 ships a unified \
            AppState::embedding_status() shared by both handlers."]
fn bench_gate_cross_tool_semantic_ready_consistency() {
    // P3 invariant: review_architecture.data.semantic.loaded must
    // agree with get_ranked_context.retrieval.semantic_ready for the
    // same session. The current live-session divergence (observed
    // 2026-04-21: loaded=true from review_architecture vs
    // semantic_ready=false from get_ranked_context) reproduces here
    // even on a minimal fixture because the two handlers consult
    // different sources — the on-disk index vs the in-memory engine
    // handle. Phase P3 replaces both with a single source of truth.
    let project = fixture_with_widget();
    let state = make_state(&project);

    let architecture = call_tool(&state, "review_architecture", json!({}));
    let ranked = call_tool(
        &state,
        "get_ranked_context",
        json!({ "query": "widget", "max_tokens": 512 }),
    );

    let loaded_from_architecture = architecture["data"]["semantic"]["loaded"].as_bool();
    let ready_from_ranked = ranked["data"]["retrieval"]["semantic_ready"].as_bool();

    assert_eq!(
        loaded_from_architecture, ready_from_ranked,
        "semantic readiness diverged: review_architecture.data.semantic.loaded={:?} \
         vs get_ranked_context.retrieval.semantic_ready={:?}. \
         Phase P3 unifies both behind AppState::embedding_status() — \
         if this test fails before P3 lands, it means an unrelated \
         regression leaked into the session-level cache.",
        loaded_from_architecture, ready_from_ranked
    );
}
