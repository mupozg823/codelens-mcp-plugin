//! Phase O1 — per-symbol compression levels L0 / L1 / L2.
//!
//! `docs/plans/PLAN_opus47-alignment.md` Tier A ships a per-symbol
//! presentation enum so the response shape adapts to query intent and
//! rank instead of the flat "top-N get bodies" policy the previous
//! `compact_symbol_bodies` implemented. This matters because Opus 4.7
//! `output_config.task_budget` puts a hard ceiling on per-call tokens,
//! so responses need to degrade smoothly rather than either include
//! everything or truncate blindly.
//!
//! The three tests below pin the contract from the harness's
//! perspective:
//!
//! * L1 default: `find_symbol(name=...)` without `include_body=true`
//!   produces symbols carrying at least a signature (no body) — that is,
//!   `SymbolPresentation::Signature` — so each symbol fits in ~300B.
//! * L2 opt-in: `include_body=true` promotes the top-N symbols to
//!   `SignatureBody` (300-1500B each) while later symbols stay at L1.
//! * L0 cap: symbols beyond the body cap AND without explicit body
//!   request drop to `IdOnly` (name + file + line + kind only), so a
//!   deep `max_matches=50` call does not balloon the response.

use super::*;
use serde_json::json;

fn write_compression_fixture(project: &codelens_engine::ProjectRoot) {
    // Several distinct `widget_*` functions so a non-exact `find_symbol`
    // query returns multiple matches we can inspect by rank.
    fs::write(
        project.as_path().join("widget.rs"),
        "/// Widget alpha.\n\
         pub fn widget_alpha(x: &str) -> String {\n    \
             format!(\"<{x}>\")\n\
         }\n\
         \n\
         /// Widget beta.\n\
         pub fn widget_beta(x: &str) -> String {\n    \
             format!(\"[{x}]\")\n\
         }\n\
         \n\
         /// Widget gamma.\n\
         pub fn widget_gamma(x: &str) -> String {\n    \
             format!(\"({x})\")\n\
         }\n\
         \n\
         /// Widget delta.\n\
         pub fn widget_delta(x: &str) -> String {\n    \
             format!(\"{{{x}}}\")\n\
         }\n",
    )
    .unwrap();
}

#[test]
fn identifier_lookup_defaults_to_l1_signature_per_symbol() {
    // Without include_body, the response must carry a signature per
    // matched symbol (L1) but no body (L2). Contract: every returned
    // symbol has a non-empty `signature`, zero `body`, and a
    // `presentation_level` field naming the level.
    let project = project_root();
    write_compression_fixture(&project);
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "widget",
            "exact_match": false,
            "max_matches": 4,
        }),
    );
    assert_eq!(payload["success"], json!(true), "payload={payload}");

    let symbols = payload["data"]["symbols"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!symbols.is_empty(), "expected matches; payload={payload}");

    for (idx, sym) in symbols.iter().enumerate() {
        let level = sym["presentation_level"].as_str().unwrap_or("<missing>");
        let has_body = sym.get("body").is_some_and(|b| !b.is_null());
        let has_sig = sym
            .get("signature")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        assert_eq!(
            level, "signature",
            "symbol[{idx}] must be presentation_level=signature without include_body; sym={sym}"
        );
        assert!(has_sig, "symbol[{idx}] must carry a signature; sym={sym}");
        assert!(
            !has_body,
            "symbol[{idx}] must NOT carry a body when include_body=false; sym={sym}"
        );
    }
}

#[test]
fn body_requested_explicit_emits_l2_per_symbol() {
    // With include_body=true, the top-N (default 3) get L2
    // (signature + body). Contract: at least one symbol reports
    // `presentation_level == "signature_body"` AND carries a
    // non-empty `body`.
    let project = project_root();
    write_compression_fixture(&project);
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "widget",
            "exact_match": false,
            "include_body": true,
            "max_matches": 4,
        }),
    );
    assert_eq!(payload["success"], json!(true), "payload={payload}");

    let symbols = payload["data"]["symbols"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let l2_count = symbols
        .iter()
        .filter(|sym| {
            sym["presentation_level"].as_str() == Some("signature_body")
                && sym
                    .get("body")
                    .and_then(|b| b.as_str())
                    .is_some_and(|s| !s.is_empty())
        })
        .count();
    assert!(
        l2_count >= 1,
        "expected at least one L2 (signature_body) symbol with include_body=true; payload={payload}"
    );
}

#[test]
fn symbols_beyond_cap_drop_to_l0_id_only() {
    // When the match set exceeds the body-cap (3 by default) AND
    // include_body=true is NOT set, later symbols degrade to
    // `id_only` — name/file/line/kind only, no signature, no body.
    // This keeps a wide fuzzy search (max_matches=20) bounded.
    let project = project_root();
    write_compression_fixture(&project);
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "widget",
            "exact_match": false,
            "max_matches": 4,
            "_presentation_cap": 2,
        }),
    );
    assert_eq!(payload["success"], json!(true), "payload={payload}");

    let symbols = payload["data"]["symbols"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        symbols.len() >= 3,
        "fixture must produce at least 3 matches; got {}",
        symbols.len()
    );

    // Beyond cap (index ≥ 2): level must be id_only, no signature.
    let tail_levels: Vec<String> = symbols
        .iter()
        .skip(2)
        .filter_map(|sym| sym["presentation_level"].as_str().map(ToOwned::to_owned))
        .collect();
    assert!(
        !tail_levels.is_empty(),
        "expected symbols beyond cap; symbols={symbols:?}"
    );
    assert!(
        tail_levels.iter().all(|level| level == "id_only"),
        "symbols beyond presentation cap must drop to id_only; got {tail_levels:?}"
    );
}
