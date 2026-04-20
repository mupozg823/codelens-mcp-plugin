//! Workflow quality gate — explicit contract tests for the three workflow
//! tools called out in the enterprise roadmap:
//!
//!   - `prepare_harness_session` (bootstrap)
//!   - `analyze_change_request`  (plan)
//!   - `impact_report`           (review)
//!
//! Each tool already has rich inline assertions in `workflow.rs`. The contract
//! module adds one compact, named invariant per tool so that a regression in
//! the response envelope shape is caught by a focused test whose failure
//! message names the violated contract — the 20 inline asserts scattered in
//! `workflow.rs` do not offer that framing on their own.
//!
//! Latency budgets are deliberately **not** part of this module: assigning a
//! budget without a reproducible baseline would be guesswork, and guessed
//! budgets break CI on unrelated hardware. That belongs in a separate
//! benchmark-driven gate.

use super::*;

/// Universal envelope invariants every successful workflow tool response must
/// satisfy. Separate from schema-version checks because not every workflow
/// tool publishes a `schema_version` field (e.g. `prepare_harness_session`'s
/// response is shape-typed via Rust types, not a versioned JSON schema).
pub(super) fn assert_workflow_envelope(tool_name: &str, payload: &serde_json::Value) {
    assert_eq!(
        payload["success"],
        json!(true),
        "{tool_name}: expected success==true, got {}",
        payload["success"]
    );
    let data = &payload["data"];
    assert!(
        data.is_object() || data.is_array(),
        "{tool_name}: expected `data` object/array, got {data}"
    );
}

/// Assert that a workflow tool response carries the exact schema version the
/// caller expects. Looks at `payload.schema_version` first, then
/// `payload.data.schema_version`. Fails loudly if either (a) the field is
/// missing everywhere, or (b) the value drifts from `expected`.
pub(super) fn assert_schema_version(tool_name: &str, payload: &serde_json::Value, expected: &str) {
    let actual = payload
        .get("schema_version")
        .or_else(|| payload.get("data").and_then(|d| d.get("schema_version")))
        .and_then(|v| v.as_str());
    assert_eq!(
        actual,
        Some(expected),
        "{tool_name}: schema_version drift — expected {expected:?}, got {actual:?}"
    );
}

// ── prepare_harness_session ─────────────────────────────────────────

#[test]
fn workflow_contract_prepare_harness_session_happy_path() {
    let project = project_root();
    let state = make_state(&project);
    let payload = call_tool(&state, "prepare_harness_session", json!({}));
    assert_workflow_envelope("prepare_harness_session", &payload);
    // Bootstrap contract: must declare the active surface back to the caller
    // and report a project activation status. These two fields are the
    // documented bootstrap handshake and changing them silently breaks every
    // harness adapter.
    assert!(
        payload["data"]["active_surface"].is_string(),
        "prepare_harness_session: active_surface missing, got {}",
        payload["data"]
    );
    assert_eq!(
        payload["data"]["project"]["activated"],
        json!(true),
        "prepare_harness_session: project.activated != true"
    );
}

// ── P0-3: auto-attach LSP prewarm on bootstrap ──────────────────────

#[test]
fn prepare_harness_session_lsp_auto_attach_skips_non_persistent_transport() {
    // Contract: by default (stdio transport, CLI one-shot), auto-attach
    // is gated off because the LSP process spawned would not outlive the
    // request. The field is still present with enabled=false and a
    // stable machine-readable disabled_reason so downstream harness
    // adapters can key off it.
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "def greet(name):\n    return 'hi'\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "prepare_harness_session", json!({}));
    assert_workflow_envelope("prepare_harness_session", &payload);
    let auto = &payload["data"]["lsp_auto_attach"];
    assert_eq!(auto["enabled"], json!(false));
    assert_eq!(auto["disabled_reason"], json!("non_persistent_transport"));
    assert_eq!(auto["detected_languages"], json!([]));
    assert_eq!(auto["prewarm_fired"], json!([]));
}

#[test]
fn prepare_harness_session_reports_lsp_auto_attach_contract_when_opted_in() {
    // Contract: with CODELENS_LSP_AUTO=true the language detection scan
    // runs and `detected_languages` includes any language whose
    // extensions are present in the project tree. We don't assert on
    // `prewarm_fired` because CI environments may not have the server
    // binary installed; the contract is on the *detection* step.
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "def greet(name: str) -> str:\n    return f'hi {name}'\n",
    )
    .unwrap();
    unsafe {
        std::env::set_var("CODELENS_LSP_AUTO", "true");
    }
    let state = make_state(&project);
    let payload = call_tool(&state, "prepare_harness_session", json!({}));
    unsafe {
        std::env::remove_var("CODELENS_LSP_AUTO");
    }
    assert_workflow_envelope("prepare_harness_session", &payload);

    let auto = &payload["data"]["lsp_auto_attach"];
    assert!(
        auto.is_object(),
        "prepare_harness_session: lsp_auto_attach must be an object, got {auto}"
    );
    assert_eq!(
        auto["enabled"],
        json!(true),
        "opt-in must flip enabled=true"
    );
    assert!(
        auto["detected_languages"].is_array(),
        "lsp_auto_attach.detected_languages must be array"
    );
    assert!(
        auto["prewarm_fired"].is_array(),
        "lsp_auto_attach.prewarm_fired must be array"
    );
    let detected: Vec<String> = auto["detected_languages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert!(
        detected.iter().any(|lang| lang == "python"),
        "Python sample file must trigger detected_languages; got {detected:?}"
    );
}

#[test]
fn prepare_harness_session_honors_lsp_auto_opt_out_env() {
    // Contract: CODELENS_LSP_AUTO=false disables detection + prewarm
    // entirely. The payload still includes the field so downstream
    // harness code can rely on the key always being present, but
    // `enabled=false` and `disabled_reason="user_opt_out"`.
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "def greet():\n    return 'hi'\n",
    )
    .unwrap();
    // SAFETY: env vars are a process-global; the test relies on being
    // run single-threaded within this test function. If other prepare
    // tests interleave, the env flag may leak — but cargo test honours
    // the test harness and inner scope resets before returning.
    unsafe {
        std::env::set_var("CODELENS_LSP_AUTO", "false");
    }
    let state = make_state(&project);
    let payload = call_tool(&state, "prepare_harness_session", json!({}));
    unsafe {
        std::env::remove_var("CODELENS_LSP_AUTO");
    }
    assert_workflow_envelope("prepare_harness_session", &payload);
    let auto = &payload["data"]["lsp_auto_attach"];
    assert_eq!(auto["enabled"], json!(false));
    assert_eq!(auto["disabled_reason"], json!("user_opt_out"));
    assert_eq!(auto["detected_languages"], json!([]));
    assert_eq!(auto["prewarm_fired"], json!([]));
}

#[test]
fn prepare_harness_session_detects_deeply_nested_rust_sources() {
    // Regression contract: before the `LSP_AUTO_ATTACH_SAMPLE_LIMIT`
    // bump to 800 / `LSP_AUTO_ATTACH_MAX_DEPTH` bump to 4, the shallow
    // language walker filled its 120-file quota with near-root Python
    // / JSON / shell files (the bench tree on this repo hit 103
    // non-Rust files at depth ≤ 3 before touching a single `.rs`), so
    // `detected_languages` for a Rust workspace silently dropped the
    // `rust` entry. This test pins the language detection on a Rust
    // source file placed at `crates/<crate>/src/deep/<...>.rs` — the
    // shape every repo in this workspace uses.
    let project = project_root();
    let nested = project
        .as_path()
        .join("crates")
        .join("workflow_contract_fixture_crate")
        .join("src")
        .join("deep");
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        nested.join("lib.rs"),
        "pub fn greet() -> &'static str { \"hi\" }\n",
    )
    .unwrap();
    // A pile of cheap non-Rust files near the root so the walker has
    // to decide between "fill up on the easy ones" and "reach into the
    // Rust tree". This mirrors what benchmarks/ + docs/ + scripts/ do
    // in the real codelens-mcp-plugin checkout.
    let noise = project.as_path().join("workflow_contract_noise");
    fs::create_dir_all(&noise).unwrap();
    for idx in 0..150 {
        fs::write(noise.join(format!("row_{idx:03}.py")), "x = 1\n").unwrap();
    }

    unsafe {
        std::env::set_var("CODELENS_LSP_AUTO", "true");
    }
    let state = make_state(&project);
    let payload = call_tool(&state, "prepare_harness_session", json!({}));
    unsafe {
        std::env::remove_var("CODELENS_LSP_AUTO");
    }
    let auto = &payload["data"]["lsp_auto_attach"];
    let detected: Vec<String> = auto["detected_languages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert!(
        detected.iter().any(|lang| lang == "rust"),
        "deeply-nested Rust sources must still register as detected; got {detected:?}"
    );
}

// ── analyze_change_request ──────────────────────────────────────────

#[test]
fn workflow_contract_analyze_change_request_happy_path() {
    let project = project_root();
    fs::write(
        project.as_path().join("workflow_contract_fixture.py"),
        "def search_users(query):\n    return []\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "audit workflow contract fixture"}),
    );
    assert_workflow_envelope("analyze_change_request", &payload);
    // Plan contract: must return a handle callers can use to fetch sections
    // later, plus a risk_level enum and a blockers array. This is the minimum
    // shape `analysis_job` consumers rely on.
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id missing");
    assert!(
        analysis_id.starts_with("analysis-"),
        "analyze_change_request: analysis_id must start with 'analysis-', got {analysis_id}"
    );
    assert!(
        matches!(
            payload["data"]["risk_level"].as_str(),
            Some("low" | "medium" | "high")
        ),
        "analyze_change_request: risk_level must be low|medium|high, got {}",
        payload["data"]["risk_level"]
    );
    assert!(
        payload["data"]["blockers"].is_array(),
        "analyze_change_request: blockers must be an array"
    );
}

// ── impact_report ───────────────────────────────────────────────────

#[test]
fn workflow_contract_impact_report_ci_audit_schema_is_pinned() {
    // Review contract: impact_report under the `ci-audit` profile emits a
    // machine-readable payload whose shape downstream CI tooling depends on.
    // Any drift in `schema_version` or `report_kind` is a breaking change,
    // so the values are pinned here as a regression guard.
    let project = project_root();
    fs::write(
        project.as_path().join("workflow_contract_impact.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));

    let payload = call_tool(
        &state,
        "impact_report",
        json!({"path": "workflow_contract_impact.py"}),
    );
    assert_workflow_envelope("impact_report", &payload);
    assert_schema_version("impact_report", &payload, "codelens-ci-audit-v1");
    assert_eq!(
        payload["data"]["report_kind"],
        json!("impact_report"),
        "impact_report: report_kind drifted"
    );
    assert_eq!(
        payload["data"]["profile"],
        json!("ci-audit"),
        "impact_report: profile drifted"
    );
}
