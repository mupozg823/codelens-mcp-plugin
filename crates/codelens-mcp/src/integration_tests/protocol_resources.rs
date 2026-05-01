use super::*;

// ── Resource URI handling and canonical/alias resolution ───────────────────────

#[test]
fn symbiote_uri_alias_matches_codelens_response() {
    // ADR-0007 Phase 2: clients can address resources under either
    // `codelens://` (canonical) or `symbiote://` (rebrand alias). Both
    // must resolve to the same payload without any dispatch difference.
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let codelens_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://project/overview"})),
        },
    )
    .expect("codelens:// uri must resolve");

    let symbiote_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "symbiote://project/overview"})),
        },
    )
    .expect("symbiote:// alias must resolve");

    let codelens_body = serde_json::to_value(&codelens_response)
        .unwrap()
        .get("result")
        .cloned()
        .unwrap_or_default();
    let symbiote_body = serde_json::to_value(&symbiote_response)
        .unwrap()
        .get("result")
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        codelens_body, symbiote_body,
        "symbiote:// alias must return the same resource payload as codelens://"
    );
}
