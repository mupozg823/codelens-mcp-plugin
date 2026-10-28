use super::*;
use serde_json::Value;

#[tokio::test]
async fn analysis_lite_http_oversized_evidence_is_clipped_with_explicit_omission_never_a_sampled_zero()
 {
    let state = candidate_state();
    let source = format!("export function sink() {{}}\n{}", "sink();\n".repeat(100));
    std::fs::write(state.project().as_path().join("a.ts"), source).unwrap();
    let project = state.project().as_path().display().to_string();
    let app = build_router(state);
    let sid = candidate_initialize(&app, Some(&project), "ClaudeCode", "analysis-lite", None).await;
    let body = candidate_call(
        &app,
        &sid,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink","max_results":100}),
    )
    .await;
    // 101 references (declaration + 100 calls) cannot fit 18000 bytes: whole
    // rows are dropped, never to zero, and the drop is counted.
    assert_ne!(body["result"]["isError"], true, "{body}");
    let kept = data(&body)["results"]["references"]
        .as_array()
        .unwrap()
        .len();
    let wire = &data(&body)["truncation"]["wire_bound"];
    assert!(kept > 0, "{body}");
    assert_eq!(
        kept + wire["references_omitted"].as_u64().unwrap() as usize,
        100,
        "max_results 100 rows before clipping: {body}"
    );
    assert_eq!(wire["limit_bytes"], 18000);
    assert!(wire["unclipped_bytes"].as_u64().unwrap() > 18000, "{body}");
    assert_eq!(data(&body)["truncation"]["results"], true);
    // Clipping rewrites only the dropped arrays; the other result arrays keep
    // their schema shape.
    assert!(data(&body)["results"]["nodes"].is_array(), "{body}");
    assert!(data(&body)["results"]["edges"].is_array(), "{body}");
    assert!(serde_json::to_vec(&data(&body)).unwrap().len() <= 18000);
    let narrowed = candidate_call(
        &app,
        &sid,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink","max_results":2}),
    )
    .await;
    assert_ne!(narrowed["result"]["isError"], true, "{narrowed}");
    assert_eq!(
        data(&narrowed)["results"]["references"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(data(&narrowed)["truncation"]["results"], true);
}

fn candidate_state() -> Arc<AppState> {
    let state = test_state();
    std::fs::write(state.project().as_path().join("package.json"), "{}").unwrap();
    std::fs::write(
        state.project().as_path().join("a.ts"),
        "export function sink() {}\nsink();\n",
    )
    .unwrap();
    std::fs::write(state.project().as_path().join("b.ts"), "import { sink as useSink } from './a';\nexport function caller() { useSink(); useSink(); }\n").unwrap();
    state
}

async fn candidate_rpc(
    app: &axum::Router,
    sid: Option<&str>,
    request: Value,
    project_header: Option<&str>,
) -> (Value, Option<String>) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json");
    if let Some(sid) = sid {
        builder = builder.header("mcp-session-id", sid);
    }
    if let Some(project) = project_header {
        builder = builder.header("x-codelens-project", project);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(axum::body::Body::from(request.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .map(|value| value.to_str().unwrap().to_owned());
    (
        serde_json::from_str(&body_string(response).await).unwrap(),
        session_id,
    )
}

async fn candidate_initialize(
    app: &axum::Router,
    project: Option<&str>,
    client: &str,
    profile: &str,
    header: Option<&str>,
) -> String {
    let mut params = json!({"protocolVersion":"2025-11-25", "clientInfo":{"name":client,"version":"test"},"capabilities":{},"profile":profile});
    if let Some(project) = project {
        params["project"] = json!(project);
    }
    let (body, sid) = candidate_rpc(
        app,
        None,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":params}),
        header,
    )
    .await;
    assert!(
        body["result"]["capabilities"]["tools"].is_object(),
        "{body}"
    );
    sid.expect("initialize session")
}

async fn candidate_call(app: &axum::Router, sid: &str, name: &str, mut arguments: Value) -> Value {
    // Keep fixtures compact; the wire request always uses the canonical nested
    // selector, which avoids root-level unions in native host tool schemas.
    if matches!(name, "search" | "graph") && arguments.get("file").is_some() {
        let object = arguments.as_object_mut().unwrap();
        let mut target = serde_json::Map::new();
        for key in ["file", "name", "symbol_id", "start_byte", "end_byte"] {
            if let Some(value) = object.remove(key) {
                target.insert(key.to_owned(), value);
            }
        }
        object.insert("target".to_owned(), Value::Object(target));
    }
    candidate_rpc(app, Some(sid), json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":arguments}}), None).await.0
}

fn data(body: &Value) -> Value {
    if body["result"].get("structuredContent").is_some() {
        return body["result"]["structuredContent"].clone();
    }
    first_tool_payload(&body.to_string())["data"].clone()
}

#[tokio::test]
async fn analysis_lite_http_only_two_descriptors_and_actual_dispatch_rejects_modes_aliases_and_mutations()
 {
    let state = candidate_state();
    let project = state.project().as_path().display().to_string();
    let app = build_router(state.clone());
    let sid =
        candidate_initialize(&app, Some(&project), "CodexHarness", "analysis-lite", None).await;
    for params in [
        json!({}),
        json!({"full":true,"includeOutputSchema":true}),
        json!({"namespace":"session"}),
    ] {
        let (listed, _) = candidate_rpc(
            &app,
            Some(&sid),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":params}),
            None,
        )
        .await;
        let result = &listed["result"];
        // An explicit namespace filter may reduce the candidate surface but
        // cannot expand it into control or legacy tools.
        let names = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        if params.get("namespace").is_none() {
            assert_eq!(names, ["search", "graph"], "{listed}");
        }
        assert!(names.iter().all(|name| matches!(*name, "search" | "graph")));
        for tool in result["tools"].as_array().unwrap() {
            let mode = if tool["name"] == "search" {
                "refs"
            } else {
                "impact"
            };
            assert_eq!(
                tool["inputSchema"]["properties"]["mode"]["enum"],
                json!([mode])
            );
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert!(tool["inputSchema"].get("anyOf").is_none());
            assert_eq!(
                tool["inputSchema"]["properties"]["target"]["anyOf"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
        }
        if params.get("namespace").is_none() {
            let candidate = crate::tool_defs::analysis_lite::tools()
                .iter()
                .collect::<Vec<_>>();
            assert_eq!(
                result["surface_generation"]["tool_schema_fingerprint"],
                crate::tool_schema_generation::tool_schema_fingerprint(&candidate)
            );
        }
    }
    let original = std::fs::read_to_string(state.project().as_path().join("a.ts")).unwrap();
    for (name, args) in [
        (
            "search",
            json!({"mode":"symbol","name":"sink","file":"a.ts"}),
        ),
        (
            "graph",
            json!({"mode":"callers","function_name":"sink","file":"a.ts"}),
        ),
        (
            "find_referencing_symbols",
            json!({"file_path":"a.ts","symbol_name":"sink"}),
        ),
        ("impact_report", json!({"symbol":"sink"})),
        (
            "analysis_lite_refs",
            json!({"mode":"refs","target":{"name":"sink","file":"a.ts"}}),
        ),
        (
            "rename_symbol",
            json!({"symbol_name":"sink","new_name":"changed","relative_path":"a.ts"}),
        ),
        ("set_profile", json!({"profile":"builder"})),
        ("prepare_harness_session", json!({"project":project})),
        (
            "search",
            json!({"mode":"refs","name":"sink","file":"a.ts","max_results":0}),
        ),
        (
            "graph",
            json!({"mode":"impact","name":"sink","file":"a.ts","max_depth":9}),
        ),
        (
            "search",
            json!({"mode":"refs","name":"sink","file":"a.ts","max_results":"1"}),
        ),
        (
            "search",
            json!({"mode":"refs","name":"sink","file":"a.ts","use_lsp":true}),
        ),
        (
            "search",
            json!({"mode":"refs","name":"sink","file":"a.ts","start_byte":16}),
        ),
    ] {
        let body = candidate_call(&app, &sid, name, args).await;
        assert_eq!(
            body["result"]["isError"], true,
            "{name} must actually fail: {body}"
        );
        assert_eq!(data(&body)["status"], "error", "{body}");
        assert!(data(&body)["results"]["total"].is_null());
    }
    assert_eq!(
        std::fs::read_to_string(state.project().as_path().join("a.ts")).unwrap(),
        original
    );
    let readonly =
        candidate_initialize(&app, Some(&project), "CodexHarness", "readonly", None).await;
    let legacy = candidate_call(
        &app,
        &readonly,
        "graph",
        json!({"mode":"callers","function_name":"sink"}),
    )
    .await;
    assert_ne!(
        legacy["result"]["isError"], true,
        "other profile retains callers mode: {legacy}"
    );
    let direct = candidate_call(
        &app,
        &readonly,
        "analysis_lite_refs",
        json!({"mode":"refs","target":{"name":"sink","file":"a.ts"}}),
    )
    .await;
    assert!(
        direct["error"].is_object() || direct["result"]["isError"] == true,
        "private candidate target cannot add a direct legacy API: {direct}"
    );
}

#[tokio::test]
async fn analysis_lite_http_structured_evidence_errors_and_generation_fence() {
    let state = candidate_state();
    let project = state.project().as_path().display().to_string();
    let app = build_router(state.clone());
    let sid = candidate_initialize(&app, Some(&project), "ClaudeCode", "analysis-lite", None).await;
    let refs = candidate_call(
        &app,
        &sid,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink"}),
    )
    .await;
    assert_ne!(refs["result"]["isError"], true, "{refs}");
    let evidence = data(&refs);
    assert_eq!(evidence["schema_version"], "analysis-lite/v1");
    assert_eq!(evidence["project"], project);
    assert_eq!(evidence["status"], "partial");
    assert_eq!(
        evidence["results"]["references"].as_array().unwrap().len(),
        3,
        "{refs}"
    );
    assert_eq!(evidence["freshness"]["generation_is_disk_freshness"], false);
    assert!(evidence["snapshot"]["source_bundle_hash"].is_string());
    assert!(evidence["results"]["total"].is_null());
    assert!(
        first_tool_payload(&refs.to_string())
            .get("confidence")
            .is_none()
    );
    let generation = evidence["snapshot"]["generation"].as_u64().unwrap();
    let graph = candidate_call(
        &app,
        &sid,
        "graph",
        json!({"mode":"impact","file":"a.ts","name":"sink","max_edges":1}),
    )
    .await;
    assert_eq!(
        data(&graph)["results"]["edges"].as_array().unwrap().len(),
        1
    );
    assert_eq!(data(&graph)["truncation"]["edges"], true);
    let missing = candidate_call(
        &app,
        &sid,
        "graph",
        json!({"mode":"impact","file":"missing.ts","name":"sink"}),
    )
    .await;
    assert_eq!(missing["result"]["isError"], true);
    assert_eq!(data(&missing)["errors"][0]["code"], "source_read_failed");
    assert!(data(&missing)["results"]["total"].is_null());
    let conflict = candidate_call(
        &app,
        &sid,
        "graph",
        json!({"mode":"impact","file":"a.ts","name":"sink","generation":generation+999}),
    )
    .await;
    assert_eq!(conflict["result"]["isError"], true);
    assert_eq!(
        data(&conflict)["errors"][0]["code"],
        "index_generation_changed"
    );
    assert_eq!(data(&conflict)["errors"][0]["retryable"], true);
    assert!(data(&conflict)["target"].is_null());
    let after = candidate_call(
        &app,
        &sid,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink"}),
    )
    .await;
    assert_eq!(data(&after)["project"], project);
    assert_eq!(
        data(&after)["results"]["references"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for target in ["analysis_lite_refs", "analysis_lite_impact"] {
        assert!(crate::tool_defs::tool_symbol_generation_consistent(target));
        assert!(!crate::tool_defs::is_content_mutation_tool(target));
        assert_eq!(
            crate::principals::required_role_for(target),
            crate::principals::Role::ReadOnly
        );
    }
}

#[tokio::test]
async fn analysis_lite_http_requires_binding_and_shares_canonical_runtime_across_hosts() {
    let state = candidate_state();
    let root = state.project();
    let project = root.as_path().display().to_string();
    let app = build_router(state.clone());
    let unbound = candidate_initialize(&app, None, "CodexHarness", "analysis-lite", None).await;
    let denied = candidate_call(
        &app,
        &unbound,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink"}),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        data(&denied)["errors"][0]["code"],
        "project_binding_required"
    );
    assert!(data(&denied)["project"].is_null());
    let codex =
        candidate_initialize(&app, Some(&project), "CodexHarness", "analysis-lite", None).await;
    let claude =
        candidate_initialize(&app, None, "ClaudeCode", "analysis-lite", Some(&project)).await;
    let default_index = state.symbol_index();
    let bound_index = {
        let _guard = state.bind_request_project_scope(&project).unwrap();
        state.symbol_index()
    };
    assert!(
        Arc::ptr_eq(&default_index, &bound_index),
        "same canonical project reuses ProjectContext index"
    );
    default_index
        .index_files(&[root.as_path().join("a.ts"), root.as_path().join("b.ts")])
        .unwrap();
    let shared_generation = default_index.committed_generation();
    for sid in [&codex, &claude] {
        let body = candidate_call(
            &app,
            sid,
            "search",
            json!({"mode":"refs","file":"a.ts","name":"sink","generation":shared_generation}),
        )
        .await;
        assert_eq!(data(&body)["project"], project, "{body}");
        assert_eq!(
            data(&body)["snapshot"]["generation"],
            shared_generation,
            "{body}"
        );
    }
    let other_dir = temp_project_dir("analysis-lite-other-worktree");
    std::fs::write(other_dir.join("package.json"), "{}").unwrap();
    std::fs::write(
        other_dir.join("a.ts"),
        "export function isolated() {}\nisolated();\n",
    )
    .unwrap();
    let other = ProjectRoot::new_exact(&other_dir).unwrap();
    let other_project = other.as_path().display().to_string();
    let other_sid = candidate_initialize(
        &app,
        Some(&other_project),
        "ClaudeCode",
        "analysis-lite",
        None,
    )
    .await;
    let isolated = candidate_call(
        &app,
        &other_sid,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"isolated"}),
    )
    .await;
    assert_eq!(data(&isolated)["project"], other_project, "{isolated}");
    assert_ne!(data(&isolated)["project"], project);
    let other_index = {
        let _guard = state.bind_request_project_scope(&other_project).unwrap();
        state.symbol_index()
    };
    assert!(
        !Arc::ptr_eq(&default_index, &other_index),
        "different canonical worktree roots own separate indexes"
    );
    let still_bound = candidate_call(
        &app,
        &codex,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink"}),
    )
    .await;
    assert_eq!(
        data(&still_bound)["project"],
        project,
        "other session cannot rebind this host"
    );
}

#[tokio::test]
async fn analysis_lite_http_initialize_instructions_match_the_two_capabilities() {
    let state = candidate_state();
    let project = state.project().as_path().display().to_string();
    let app = build_router(state);
    let (body, _) = candidate_rpc(&app, None, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"profile":"analysis-lite","project":project,"clientInfo":{"name":"CodexHarness","version":"test"}}}), None).await;
    let instructions = body["result"]["instructions"].as_str().unwrap();
    let prefix = instructions.chars().take(512).collect::<String>();
    assert!(
        prefix.contains("search(mode=refs)") && prefix.contains("graph(mode=impact)"),
        "{instructions}"
    );
    assert!(prefix.contains("project") && prefix.contains("partial") && prefix.contains("native"));
    assert!(!instructions.contains("prepare_harness_session"));
    assert!(!instructions.contains("review_architecture"));
    assert!(instructions.len() < 900);
}

#[tokio::test]
async fn analysis_lite_http_invalid_header_cannot_fall_back_to_daemon_project() {
    let state = candidate_state();
    let app = build_router(state);
    let missing = "/not/a/real/analysis-lite-project";
    let sid =
        candidate_initialize(&app, None, "CodexHarness", "analysis-lite", Some(missing)).await;
    let body = candidate_call(
        &app,
        &sid,
        "search",
        json!({"mode":"refs","file":"a.ts","name":"sink"}),
    )
    .await;
    assert_eq!(
        body["result"]["isError"], true,
        "invalid explicit header must not read the daemon's sink declaration: {body}"
    );
    assert_eq!(data(&body)["status"], "error");
    assert!(data(&body)["target"].is_null());
}

#[tokio::test]
async fn analysis_lite_http_preserves_typed_locations_under_small_legacy_budget() {
    let state = candidate_state();
    std::fs::write(state.project().as_path().join("a.ts"), "export function sink() {}\nsink(); sink(); sink(); sink(); sink(); sink(); sink(); sink(); sink();\n").unwrap();
    let project = state.project().as_path().display().to_string();
    let app = build_router(state);
    let sid = candidate_initialize(&app, Some(&project), "ClaudeCode", "analysis-lite", None).await;
    let body = candidate_call(&app, &sid, "search", json!({"mode":"refs","file":"a.ts","name":"sink","path":"a.ts","_profile":"fast_local","_compact":true})).await;
    assert_ne!(body["result"]["isError"], true, "{body}");
    let evidence = data(&body);
    assert_eq!(
        evidence["results"]["references"].as_array().unwrap().len(),
        9,
        "generic summarization cannot clip typed evidence or its provenance: {body}"
    );
    assert_eq!(evidence["truncation"]["results"], false);
    assert!(evidence["results"]["references"][8]["location"]["span"]["start"].is_number());
    let text: Value =
        serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        text["data"]["results"]["references"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    // A row's file hash is hoisted once into the snapshot; the row keeps its
    // code line and drops the target it shares with the response.
    let row = &text["data"]["results"]["references"][8];
    let file = row["location"]["file"].as_str().unwrap();
    assert!(
        text["data"]["snapshot"]["source_hashes"][file].is_string(),
        "{text}"
    );
    assert!(row["provenance"][0].get("source_hash").is_none(), "{row}");
    assert!(row.get("target").is_none(), "{row}");
    assert!(text["data"]["target"]["symbol_id"].is_string());
    assert!(
        !row["location"]["line_text"].as_str().unwrap().is_empty(),
        "{row}"
    );
    assert_eq!(
        evidence["results"]["references"][8]["provenance"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
