use super::*;

#[test]
fn ci_audit_reports_use_fixed_machine_schema() {
    let project = project_root();
    fs::write(
        project.as_path().join("audit.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));

    let payload = call_tool(&state, "impact_report", json!({"path": "audit.py"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["profile"], json!("ci-audit"));
    assert_eq!(
        payload["data"]["schema_version"],
        json!("codelens-ci-audit-v1")
    );
    assert_eq!(payload["data"]["report_kind"], json!("impact_report"));
    assert!(payload["data"]["machine_summary"]["finding_count"].is_number());
    assert!(payload["data"]["machine_summary"]["blocker_count"].is_number());
    assert!(payload["data"]["machine_summary"]["verifier_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["ready_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["blocked_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["quality_focus_count"].is_number());
    assert!(payload["data"]["machine_summary"]["recommended_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["performance_watchpoint_count"].is_number());
    assert!(payload["data"]["evidence_handles"].is_array());
    assert!(
        payload["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.ends_with("/summary"))
            .unwrap_or(false)
    );
    assert!(payload["data"]["section_handles"].is_array());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["readiness"].is_object());
    assert!(payload["data"]["verifier_checks"].is_array());
    assert!(payload["data"]["quality_focus"].is_array());
    assert!(payload["data"]["recommended_checks"].is_array());
    assert!(payload["data"]["performance_watchpoints"].is_array());
}

#[test]
fn start_analysis_job_returns_completed_handle() {
    let project = project_root();
    fs::write(
        project.as_path().join("impact.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let arguments =
        json!({"kind": "impact_report", "path": "impact.py", "profile_hint": "reviewer-graph"});
    // Store job without enqueuing to background worker — run synchronously to
    // eliminate timing dependency that causes flaky failures under parallel load.
    let job = state
        .store_analysis_job_for_current_scope(
            "impact_report",
            Some("reviewer-graph".to_owned()),
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    assert_eq!(job.status, crate::runtime_types::JobLifecycle::Queued);
    let job_id = job.id.clone();

    // Run synchronously on the test thread — same code path as the background worker.
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        job_id.clone(),
        "impact_report".to_owned(),
        arguments,
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let completed_job = state.get_analysis_job(&job_id).unwrap();
    assert_eq!(
        completed_job.status,
        crate::runtime_types::JobLifecycle::Completed
    );
    assert_eq!(completed_job.progress, 100);
    let analysis_id = completed_job.analysis_id.as_deref().unwrap();
    let poll = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    assert_eq!(poll["data"]["analysis_id"], json!(analysis_id));
    assert!(
        poll["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.ends_with("/summary"))
            .unwrap_or(false)
    );
    assert!(
        poll["data"]["section_handles"]
            .as_array()
            .map(|items| !items.is_empty())
            .unwrap_or(false)
    );

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "impact_rows"}),
    );
    assert_eq!(section["success"], json!(true));
}

#[cfg(feature = "http")]
#[test]
fn eval_session_audit_aggregates_across_tracked_sessions() {
    let project = project_root();
    fs::write(
        project.as_path().join("eval_builder_warn.py"),
        "print('old')\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("eval_planner_pass.py"),
        "print('ok')\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("eval_planner_warn.py"),
        "print('ok')\n",
    )
    .unwrap();
    let state = make_http_state(&project);
    let builder_session = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::BuilderMinimal,
    );
    let planner_pass_session = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );
    let planner_warn_session = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "eval_builder_warn.py"}),
        &builder_session,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update eval builder warn file",
            "changed_files": ["eval_builder_warn.py"]
        }),
        &builder_session,
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &planner_pass_session,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "eval_planner_pass.py"}),
        &planner_pass_session,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["eval_planner_pass.py"], "task": "review planner pass"}),
        &planner_pass_session,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "eval_planner_warn.py"}),
        &planner_warn_session,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["eval_planner_warn.py"], "task": "review planner warn"}),
        &planner_warn_session,
    );

    let arguments = json!({"kind": "eval_session_audit"});
    let job = state
        .store_analysis_job_for_current_scope(
            "eval_session_audit",
            None,
            vec!["audit_pass_rate".to_owned(), "session_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    let job_id = job.id.clone();
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        job_id.clone(),
        "eval_session_audit".to_owned(),
        arguments,
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let completed_job = state.get_analysis_job(&job_id).unwrap();
    let analysis_id = completed_job.analysis_id.as_deref().unwrap();

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "audit_pass_rate"}),
    );
    assert_eq!(section["success"], json!(true));
    let content = &section["data"]["content"];
    assert_eq!(content["tracked_session_count"], json!(3));
    assert_eq!(content["session_count"], json!(3));
    assert_eq!(content["builder_session_count"], json!(1));
    assert_eq!(content["planner_session_count"], json!(2));
    assert_eq!(content["builder_pass_rate"], json!(0.0));
    assert_eq!(content["planner_pass_rate"], json!(0.5));
    assert_eq!(
        content["top_failed_checks"][0]["code"],
        json!("bootstrap_order")
    );
    assert_eq!(content["top_failed_checks"][0]["count"], json!(2));

    let rows = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "session_rows"}),
    );
    assert_eq!(rows["success"], json!(true));
    assert_eq!(rows["data"]["content"]["count"], json!(3));
    assert!(
        rows["data"]["content"]["sessions"]
            .as_array()
            .map(|sessions| sessions.iter().any(|session| {
                session["role"] == json!("builder") && session["status"] == json!("warn")
            }))
            .unwrap_or(false)
    );
}

#[test]
fn start_analysis_job_reports_running_progress() {
    let project = project_root();
    fs::write(
        project.as_path().join("progress_job.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "progress_job.py",
            "debug_step_delay_ms": 30
        }),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    assert!(payload["data"]["summary_resource"].is_null());
    assert_eq!(payload["data"]["section_handles"], json!([]));
    let mut saw_running = false;
    let mut saw_mid_progress = false;
    let mut saw_step = false;
    for _ in 0..100 {
        let job = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
        let status = job["data"]["status"].as_str().unwrap_or_default();
        let progress = job["data"]["progress"].as_u64().unwrap_or_default();
        if status == "running" {
            saw_running = true;
        }
        if (1..100).contains(&progress) {
            saw_mid_progress = true;
        }
        if job["data"]["current_step"].is_string() {
            saw_step = true;
        }
        if status == "completed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_running);
    assert!(saw_mid_progress);
    assert!(saw_step);
}

#[test]
fn analysis_job_text_payload_preserves_job_handle_fields() {
    let project = project_root();
    fs::write(
        project.as_path().join("job_text.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let start_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(4101)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "start_analysis_job",
                "arguments": { "kind": "impact_report", "path": "job_text.py", "debug_step_delay_ms": 20 }
            })),
        },
    )
    .unwrap();
    let start_payload = parse_tool_payload(&extract_tool_text(&start_response));
    assert!(start_payload["data"]["job_id"].is_string());
    assert_eq!(start_payload["routing_hint"], json!("async"));
    assert_eq!(start_payload["data"]["summary_resource"], json!(null));
    assert_eq!(start_payload["data"]["section_handles"], json!([]));

    let sync_job = state
        .store_analysis_job_for_current_scope(
            "impact_report",
            None,
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    let sync_job_id = sync_job.id.clone();
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        sync_job_id.clone(),
        "impact_report".to_owned(),
        json!({"kind": "impact_report", "path": "job_text.py"}),
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let poll_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(4102)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "get_analysis_job",
                "arguments": { "job_id": sync_job_id }
            })),
        },
    )
    .unwrap();
    let completed = parse_tool_payload(&extract_tool_text(&poll_response));
    assert!(
        completed["data"]["summary_resource"]["uri"]
            .as_str()
            .map(|uri| uri.ends_with("/summary"))
            .unwrap_or(false)
    );
    assert!(
        completed["data"]["section_handles"]
            .as_array()
            .map(|items| !items.is_empty())
            .unwrap_or(false)
    );
}

#[test]
fn analysis_jobs_queue_when_worker_busy() {
    let project = project_root();
    fs::write(
        project.as_path().join("queue_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("queue_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "queue_first.py",
            "debug_step_delay_ms": 60
        }),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "queue_second.py",
            "debug_step_delay_ms": 20
        }),
    );
    let second_job_id = second["data"]["job_id"].as_str().unwrap();
    let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
    assert_eq!(second_job["data"]["status"], json!("queued"));
    assert_eq!(second_job["data"]["current_step"], json!("queued"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["analysis_jobs_enqueued"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert!(
        metrics["data"]["session"]["analysis_jobs_started"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["analysis_queue_max_depth"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_worker_limit"],
        json!(1)
    );
}

#[test]
fn cancel_analysis_job_marks_job_cancelled() {
    let project = project_root();
    fs::write(
        project.as_path().join("cancel_job.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("cancel_blocker.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "cancel_blocker.py", "debug_step_delay_ms": 60}),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "cancel_job.py", "debug_step_delay_ms": 50}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    let queued = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    assert_eq!(queued["data"]["status"], json!("queued"));
    let cancelled = call_tool(&state, "cancel_analysis_job", json!({"job_id": job_id}));
    assert_eq!(cancelled["data"]["status"], json!("cancelled"));
    assert!(cancelled["data"]["summary_resource"].is_null());
    assert_eq!(cancelled["data"]["section_handles"], json!([]));
}

#[test]
fn analysis_lists_expose_resource_handles_and_counts() {
    let project = project_root();
    fs::write(
        project.as_path().join("analysis_list.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let project_path = project.as_path().to_string_lossy().to_string();
    let artifact = state
        .store_analysis_for_current_scope(
            "impact_report",
            None,
            "impact summary".to_owned(),
            vec!["top finding".to_owned()],
            "medium".to_owned(),
            0.94,
            vec!["next action".to_owned()],
            Vec::new(),
            crate::runtime_types::AnalysisReadiness::default(),
            Vec::new(),
            std::collections::BTreeMap::from([("impact_rows".to_owned(), json!([]))]),
        )
        .unwrap();
    let job = state
        .store_analysis_job_for_current_scope(
            "impact_report",
            None,
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Completed,
            100,
            Some("completed".to_owned()),
            Some(artifact.id.clone()),
            None,
        )
        .unwrap();
    let job_id = job.id.as_str();

    let completed = call_tool(
        &state,
        "get_analysis_job",
        json!({"job_id": job_id, "_session_project_path": project_path.clone()}),
    );
    assert_eq!(
        completed["data"]["status"],
        json!("completed"),
        "expected job to complete before list verification: {completed}"
    );

    let jobs = state.list_analysis_jobs_for_scope(&state.current_project_scope(), None);
    assert!(!jobs.is_empty());
    let listed_job = jobs
        .iter()
        .find(|job| job.id == job_id)
        .expect("completed job should be visible in direct job list");
    assert_eq!(
        listed_job.status,
        crate::runtime_types::JobLifecycle::Completed
    );
    assert!(listed_job.analysis_id.is_some());
    assert!(!listed_job.estimated_sections.is_empty());

    let artifacts = call_tool(
        &state,
        "list_analysis_artifacts",
        json!({"_session_project_path": project_path.clone()}),
    );
    assert!(artifacts["data"]["count"].as_u64().unwrap_or_default() >= 1);
    assert!(artifacts["data"]["latest_created_at_ms"].is_u64());
    assert!(
        artifacts["data"]["tool_counts"]["impact_report"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        artifacts["data"]["artifacts"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item["summary_resource"]["uri"].as_str())
            .map(|uri| uri.ends_with("/summary"))
            .unwrap_or(false)
    );
}

#[test]
fn resources_include_profile_guides_and_analysis_summaries() {
    let project = project_root();
    fs::write(
        project.as_path().join("module.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "dead_code_report",
        json!({"scope": ".", "max_results": 5}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(21)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_response).unwrap();
    assert!(encoded.contains("codelens://profile/planner-readonly/guide"));
    assert!(encoded.contains("codelens://profile/planner-readonly/guide/full"));
    assert!(encoded.contains("codelens://tools/list/full"));
    assert!(encoded.contains("codelens://surface/manifest"));
    assert!(encoded.contains("codelens://harness/modes"));
    assert!(encoded.contains("codelens://harness/spec"));
    assert!(encoded.contains("codelens://harness/host-adapters"));
    assert!(encoded.contains("codelens://harness/host"));
    assert!(encoded.contains("codelens://design/agent-experience"));
    assert!(encoded.contains("codelens://host-adapters/claude-code"));
    assert!(encoded.contains("codelens://host-adapters/codex"));
    assert!(encoded.contains("codelens://host-adapters/cursor"));
    assert!(encoded.contains("codelens://host-adapters/windsurf"));
    assert!(encoded.contains("codelens://schemas/handoff-artifact/v1"));
    assert!(encoded.contains("codelens://session/http"));
    assert!(encoded.contains("codelens://analysis/recent"));
    assert!(encoded.contains("codelens://analysis/jobs"));
    assert!(encoded.contains(&format!("codelens://analysis/{analysis_id}/summary")));
    assert!(encoded.contains("symbiote://profile/planner-readonly/guide"));
    assert!(encoded.contains("symbiote://tools/list/full"));
    assert!(encoded.contains("symbiote://harness/host"));
    assert!(encoded.contains("symbiote://host-adapters/codex"));
    assert!(encoded.contains("symbiote://schemas/handoff-artifact/v1"));
    assert!(encoded.contains("symbiote://analysis/jobs"));
    assert!(encoded.contains(&format!("symbiote://analysis/{analysis_id}/summary")));

    let read_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&read_response).unwrap();
    assert!(body.contains("available_sections"));

    let recent_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22_1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://analysis/recent"})),
        },
    )
    .unwrap();
    let recent_body = serde_json::to_string(&recent_response).unwrap();
    assert!(recent_body.contains("summary_resource"));
    assert!(recent_body.contains("tool_counts"));

    let jobs_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22_2)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://analysis/jobs"})),
        },
    )
    .unwrap();
    let jobs_body = serde_json::to_string(&jobs_response).unwrap();
    assert!(jobs_body.contains("status_counts"));
    assert!(jobs_body.contains("active_count"));

    let tools_summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list"})),
        },
    )
    .unwrap();
    let tools_summary_body = serde_json::to_string(&tools_summary).unwrap();
    assert!(tools_summary_body.contains("recommended_tools"));
    assert!(tools_summary_body.contains("visible_namespaces"));
    assert!(tools_summary_body.contains("visible_tiers"));
    assert!(tools_summary_body.contains("all_namespaces"));
    assert!(tools_summary_body.contains("all_tiers"));
    assert!(tools_summary_body.contains("loaded_namespaces"));
    assert!(tools_summary_body.contains("loaded_tiers"));
    assert!(tools_summary_body.contains("effective_namespaces"));
    assert!(tools_summary_body.contains("effective_tiers"));
    assert!(!tools_summary_body.contains("\"description\""));
    assert!(tools_summary_body.contains("reports"));

    let tools_full = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(24)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list/full"})),
        },
    )
    .unwrap();
    let tools_full_body = serde_json::to_string(&tools_full).unwrap();
    assert!(tools_full_body.contains("description"));
    assert!(tools_full_body.contains("namespace"));
    assert!(tools_full_body.contains("tier"));
    assert!(tools_full_body.contains("loaded_namespaces"));
    assert!(tools_full_body.contains("loaded_tiers"));
    assert!(tools_full_body.contains("full_tool_exposure"));

    let session_resource = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(241)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://session/http"})),
        },
    )
    .unwrap();
    let session_resource_body = serde_json::to_string(&session_resource).unwrap();
    assert!(session_resource_body.contains("resume_supported"));
    assert!(session_resource_body.contains("active_sessions"));
    assert!(session_resource_body.contains("deferred_loading_supported"));
    assert!(session_resource_body.contains("loaded_namespaces"));
    assert!(session_resource_body.contains("loaded_tiers"));
    assert!(session_resource_body.contains("full_tool_exposure"));
    assert!(session_resource_body.contains("preferred_namespaces"));
    assert!(session_resource_body.contains("preferred_tiers"));
    assert!(session_resource_body.contains("deferred_namespace_gate"));

    let surface_manifest = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://surface/manifest"})),
        },
    )
    .unwrap();
    let surface_manifest_body = serde_json::to_string(&surface_manifest).unwrap();
    assert!(surface_manifest_body.contains("schema_version"));
    assert!(surface_manifest_body.contains("tool_registry"));
    assert!(surface_manifest_body.contains("surfaces"));
    assert!(surface_manifest_body.contains("harness_modes"));
    assert!(surface_manifest_body.contains("harness_spec"));
    assert!(surface_manifest_body.contains("host_adapters"));
    assert!(surface_manifest_body.contains("agent_experience"));
    assert!(surface_manifest_body.contains("harness_artifacts"));
    assert!(surface_manifest_body.contains("languages"));

    let harness_modes = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/modes"})),
        },
    )
    .unwrap();
    let harness_modes_body = serde_json::to_string(&harness_modes).unwrap();
    assert!(harness_modes_body.contains("planner-builder"));
    assert!(harness_modes_body.contains("reviewer-gate"));
    assert!(harness_modes_body.contains("explicit-only"));
    assert!(harness_modes_body.contains("asymmetric-handoff"));

    let harness_spec = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_2)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/spec"})),
        },
    )
    .unwrap();
    let harness_spec_body = serde_json::to_string(&harness_spec).unwrap();
    assert!(harness_spec_body.contains("planner-builder-handoff"));
    assert!(harness_spec_body.contains("reviewer-signoff"));
    assert!(harness_spec_body.contains("batch-analysis-artifact"));
    assert!(harness_spec_body.contains("planner_builder_dispatch"));
    assert!(harness_spec_body.contains("expected_duration_x_1_5"));

    let host_adapters = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_21)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/host-adapters"})),
        },
    )
    .unwrap();
    let host_adapters_body = serde_json::to_string(&host_adapters).unwrap();
    assert!(host_adapters_body.contains("codelens-host-adapters-v1"));
    assert!(host_adapters_body.contains("memory_only_routing"));
    assert!(host_adapters_body.contains("claude-code"));
    assert!(host_adapters_body.contains("codex"));
    assert!(host_adapters_body.contains("cursor"));
    assert!(host_adapters_body.contains("windsurf"));
    assert!(host_adapters_body.contains("handoff_id"));
    assert!(host_adapters_body.contains("delegate_handoff_id"));
    assert!(host_adapters_body.contains("replay_rule"));
    assert!(host_adapters_body.contains("native_primitives"));
    assert!(host_adapters_body.contains("preferred_codelens_use"));
    assert!(host_adapters_body.contains("routing_defaults"));

    let harness_host = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_215)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://harness/host", "host": "claude-code"})),
        },
    )
    .unwrap();
    let harness_host_value = serde_json::to_value(&harness_host).unwrap();
    let harness_host_text = harness_host_value["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    let harness_host_payload: serde_json::Value =
        serde_json::from_str(harness_host_text).expect("valid harness host JSON");
    assert_eq!(
        harness_host_payload["schema_version"],
        json!("codelens-harness-host-v1")
    );
    assert_eq!(harness_host_payload["requested_host"], json!("claude-code"));
    assert_eq!(
        harness_host_payload["selection_source"],
        json!("request_param")
    );
    assert_eq!(
        harness_host_payload["adapter_resource"],
        json!("codelens://host-adapters/claude-code")
    );
    assert_eq!(
        harness_host_payload["default_profile"],
        json!("planner-readonly")
    );
    assert_eq!(
        harness_host_payload["default_task_overlay"],
        json!("planning")
    );
    assert_eq!(
        harness_host_payload["detected_host"]["host_id"],
        json!("claude-code")
    );
    assert!(
        harness_host_payload["detected_host"]["bootstrap_sequence"]
            .as_array()
            .map(|items| items.iter().any(|value| value == "analyze_change_request"))
            .unwrap_or(false)
    );

    let agent_experience = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_211)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://design/agent-experience"})),
        },
    )
    .unwrap();
    let agent_experience_body = serde_json::to_string(&agent_experience).unwrap();
    assert!(agent_experience_body.contains("codelens-agent-experience-v1"));
    assert!(agent_experience_body.contains("blocked_pending_trademark_clearance"));
    assert!(agent_experience_body.contains("delegate_to_codex_builder"));
    assert!(agent_experience_body.contains("under_60_seconds_to_first_compressed_answer"));

    let codex_host_adapter = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_22)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://host-adapters/codex"})),
        },
    )
    .unwrap();
    let codex_host_adapter_body = serde_json::to_string(&codex_host_adapter).unwrap();
    assert!(codex_host_adapter_body.contains("builder-minimal"));
    assert!(codex_host_adapter_body.contains("~/.codex/config.toml"));
    assert!(codex_host_adapter_body.contains("AGENTS.md"));
    assert!(codex_host_adapter_body.contains("delegate_to_codex_builder"));
    assert!(codex_host_adapter_body.contains("handoff_id"));
    assert!(codex_host_adapter_body.contains("overlay_previews"));
    assert!(codex_host_adapter_body.contains("primary_bootstrap_sequence"));
    assert!(codex_host_adapter_body.contains("default_task_overlay"));
    assert!(codex_host_adapter_body.contains("editing"));
    assert!(codex_host_adapter_body.contains("## Compiled Routing Overlays"));

    let cursor_host_adapter = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://host-adapters/cursor"})),
        },
    )
    .unwrap();
    let cursor_host_adapter_body = serde_json::to_string(&cursor_host_adapter).unwrap();
    assert!(cursor_host_adapter_body.contains(".cursor/rules/codelens-routing.mdc"));
    assert!(cursor_host_adapter_body.contains("background agents"));
    assert!(cursor_host_adapter_body.contains("handoff_id"));
    assert!(cursor_host_adapter_body.contains("overlay_previews"));
    assert!(cursor_host_adapter_body.contains("primary_bootstrap_sequence"));
    assert!(cursor_host_adapter_body.contains("## Compiled Routing Overlays"));

    let windsurf_host_adapter = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_24)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://host-adapters/windsurf"})),
        },
    )
    .unwrap();
    let windsurf_host_adapter_body = serde_json::to_string(&windsurf_host_adapter).unwrap();
    assert!(windsurf_host_adapter_body.contains("~/.codeium/windsurf/mcp_config.json"));
    assert!(windsurf_host_adapter_body.contains("100-tool cap"));

    let handoff_schema = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(242_3)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://schemas/handoff-artifact/v1"})),
        },
    )
    .unwrap();
    let handoff_schema_body = serde_json::to_string(&handoff_schema).unwrap();
    assert!(handoff_schema_body.contains("codelens-handoff-artifact-v1"));
    assert!(handoff_schema_body.contains("planner_brief"));
    assert!(handoff_schema_body.contains("builder_result"));
    assert!(handoff_schema_body.contains("reviewer_verdict"));

    assert!(session_resource_body.contains("deferred_tier_gate"));
    assert!(session_resource_body.contains("mutation_preflight_required"));
    assert!(session_resource_body.contains("preflight_ttl_seconds"));
    assert!(session_resource_body.contains("rename_requires_symbol_preflight"));
    assert!(session_resource_body.contains("requires_namespace_listing_before_tool_call"));
    assert!(session_resource_body.contains("requires_tier_listing_before_tool_call"));
    assert!(session_resource_body.contains("client_profile"));
    assert!(session_resource_body.contains("default_tools_list_contract_mode"));

    let profile_summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(25)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://profile/reviewer-graph/guide"})),
        },
    )
    .unwrap();
    let profile_summary_body = serde_json::to_string(&profile_summary).unwrap();
    assert!(profile_summary_body.contains("preferred_namespaces"));
    assert!(profile_summary_body.contains("preferred_tiers"));
    assert!(tools_summary_body.contains("preferred_namespaces"));
    assert!(tools_summary_body.contains("preferred_tiers"));
}

#[test]
fn ci_audit_analysis_summary_resource_matches_machine_schema() {
    let project = project_root();
    fs::write(
        project.as_path().join("ci_audit.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));
    let payload = call_tool(&state, "impact_report", json!({"path": "ci_audit.py"}));
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(26)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&summary).unwrap();
    assert!(body.contains("codelens-ci-audit-v1"));
    assert!(body.contains("machine_summary"));
    assert!(body.contains("evidence_handles"));
    assert!(body.contains("summary_resource"));
    assert!(body.contains("section_handles"));
    assert!(body.contains("blocker_count"));
    assert!(body.contains("verifier_check_count"));
    assert!(body.contains("ready_check_count"));
    assert!(body.contains("blocked_check_count"));
    assert!(body.contains("readiness"));
    assert!(body.contains("verifier_checks"));
    assert!(body.contains("quality_focus"));
    assert!(body.contains("recommended_checks"));
    assert!(body.contains("performance_watchpoints"));
}

#[test]
fn oversized_analysis_handle_keeps_structured_content_schema_shape() {
    let project = project_root();
    fs::write(project.as_path().join("preflight.py"), "print('hello')\n").unwrap();
    let state = make_state(&project);
    state.set_token_budget(1);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3104)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "verify_change_readiness",
                "arguments": {
                    "task": "update preflight.py",
                    "changed_files": ["preflight.py"]
                }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        parse_tool_payload(&extract_tool_text(&response))["truncated"],
        json!(true)
    );
    assert_eq!(value["result"]["structuredContent"].get("truncated"), None);
    assert!(
        value["result"]["structuredContent"]["analysis_id"]
            .as_str()
            .is_some()
    );
    assert!(
        value["result"]["structuredContent"]["readiness"]["mutation_ready"]
            .as_str()
            .is_some()
    );
}

#[test]
fn analysis_handle_schema_exposes_resource_handles() {
    let schema = crate::tool_defs::tool_definition("impact_report")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("impact_report schema");

    let properties = schema["properties"].as_object().expect("schema properties");
    assert!(properties.contains_key("summary_resource"));
    assert!(properties.contains_key("section_handles"));
}

#[test]
fn analysis_job_schema_exposes_resource_handles() {
    let schema = crate::tool_defs::tool_definition("get_analysis_job")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("get_analysis_job schema");

    let properties = schema["properties"].as_object().expect("schema properties");
    assert!(properties.contains_key("summary_resource"));
    assert!(properties.contains_key("section_handles"));
}

#[test]
fn analysis_list_schemas_expose_machine_summary_fields() {
    let jobs_schema = crate::tool_defs::tool_definition("list_analysis_jobs")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("list_analysis_jobs schema");
    let job_properties = jobs_schema["properties"]
        .as_object()
        .expect("jobs schema properties");
    assert!(job_properties.contains_key("jobs"));
    assert!(job_properties.contains_key("active_count"));
    assert!(job_properties.contains_key("status_counts"));

    let artifacts_schema = crate::tool_defs::tool_definition("list_analysis_artifacts")
        .and_then(|tool| tool.output_schema.as_ref())
        .expect("list_analysis_artifacts schema");
    let artifact_properties = artifacts_schema["properties"]
        .as_object()
        .expect("artifacts schema properties");
    assert!(artifact_properties.contains_key("artifacts"));
    assert!(artifact_properties.contains_key("tool_counts"));
    assert!(artifact_properties.contains_key("latest_created_at_ms"));
}

#[test]
fn suggested_next_calls_forward_task_and_analysis_id() {
    let project = project_root();
    fs::write(
        project.as_path().join("next_calls.py"),
        "def widget():\n    return 7\n",
    )
    .unwrap();
    let state = make_state(&project);

    let analyze = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "refactor widget safely", "changed_files": ["next_calls.py"]}),
    );
    let analyze_next_calls = analyze["suggested_next_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !analyze_next_calls.is_empty(),
        "analyze_change_request should populate suggested_next_calls: {analyze}"
    );
    let verify_entry = analyze_next_calls
        .iter()
        .find(|call| call.get("tool").and_then(|v| v.as_str()) == Some("verify_change_readiness"))
        .expect("verify_change_readiness should be forwarded with args");
    assert_eq!(
        verify_entry["arguments"]["task"].as_str(),
        Some("refactor widget safely")
    );
    assert!(
        verify_entry["arguments"]["changed_files"].is_array(),
        "changed_files should be forwarded as array: {verify_entry}"
    );

    let verify = call_tool(
        &state,
        "verify_change_readiness",
        json!({"task": "refactor widget safely", "changed_files": ["next_calls.py"]}),
    );
    let analysis_id = verify["data"]["analysis_id"]
        .as_str()
        .expect("verify_change_readiness should return analysis_id");
    let verify_next_calls = verify["suggested_next_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if let Some(expand) = verify_next_calls
        .iter()
        .find(|call| call.get("tool").and_then(|v| v.as_str()) == Some("get_analysis_section"))
    {
        assert_eq!(
            expand["arguments"]["analysis_id"].as_str(),
            Some(analysis_id),
            "get_analysis_section should forward the fresh analysis_id: {expand}"
        );
    }
}

#[test]
fn analysis_artifacts_evict_oldest_disk_payloads() {
    let project = project_root();
    fs::write(
        project.as_path().join("evict.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let mut first_analysis_id = None;

    for idx in 0..70 {
        let payload = call_tool(
            &state,
            "analyze_change_request",
            json!({"task": format!("update alpha flow {idx}")}),
        );
        let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
        if first_analysis_id.is_none() {
            first_analysis_id = Some(analysis_id);
        }
    }

    let first_analysis_id = first_analysis_id.expect("first analysis id");
    assert!(state.get_analysis(&first_analysis_id).is_none());
    assert!(!state.analysis_dir().join(&first_analysis_id).exists());
}

#[test]
fn foreign_project_scoped_analysis_is_ignored_for_reuse() {
    let project = project_root();
    fs::write(
        project.as_path().join("foreign.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let analysis_id = "analysis-foreign";
    let artifact_dir = state.analysis_dir().join(analysis_id);
    fs::create_dir_all(&artifact_dir).unwrap();
    let cache_key = json!({
        "tool": "analyze_change_request",
        "fields": {
            "task": "update alpha safely"
        }
    })
    .to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let artifact = json!({
        "id": analysis_id,
        "tool_name": "analyze_change_request",
        "surface": "preset:full",
        "project_scope": "/tmp/other-project",
        "cache_key": cache_key,
        "summary": "foreign",
        "top_findings": ["foreign"],
        "confidence": 0.5,
        "next_actions": ["ignore"],
        "available_sections": ["summary"],
        "created_at_ms": now_ms,
    });
    fs::write(
        artifact_dir.join("summary.json"),
        serde_json::to_vec_pretty(&artifact).unwrap(),
    )
    .unwrap();

    assert!(state.get_analysis(analysis_id).is_none());
    assert!(
        state
            .find_reusable_analysis_for_current_scope("analyze_change_request", &cache_key)
            .is_none()
    );
}

#[test]
fn mutation_tools_write_audit_log() {
    // Phase 2 close part 4: jsonl intent log retired. The sqlite
    // audit_sink absorbs the same per-call metadata via the new
    // session_metadata column.
    let project = project_root();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "audit.txt", "content": "hello"}),
    );
    assert_eq!(payload["success"], json!(true));

    let sink = state.audit_sink().expect("audit sink available");
    let rows = sink.query(None, None, 100).expect("query rows");
    let row = rows
        .iter()
        .find(|r| r.tool == "create_text_file")
        .expect("create_text_file row");
    let metadata = row
        .session_metadata
        .as_ref()
        .expect("session_metadata captured");
    assert_eq!(
        metadata["project_scope"],
        json!(state.current_project_scope()),
        "project_scope must round-trip through session_metadata, got {metadata}"
    );
}

#[test]
fn analysis_artifacts_expire_by_ttl() {
    let project = project_root();
    fs::write(
        project.as_path().join("ttl.py"),
        "def gamma():\n    return 3\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update gamma flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state
        .set_analysis_created_at_for_test(&analysis_id, 0)
        .unwrap();

    assert!(state.get_analysis(&analysis_id).is_none());
    assert!(!state.analysis_dir().join(&analysis_id).exists());
    assert!(
        state
            .list_analysis_summaries()
            .into_iter()
            .all(|summary| summary.id != analysis_id)
    );
}

#[test]
fn startup_cleanup_removes_expired_analysis_artifacts() {
    let project = project_root();
    fs::write(
        project.as_path().join("startup_ttl.py"),
        "def delta():\n    return 4\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update delta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state
        .set_analysis_created_at_for_test(&analysis_id, 0)
        .unwrap();

    // Must use full constructor — this test verifies startup cleanup behavior.
    let restarted = crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full);
    assert!(!restarted.analysis_dir().join(&analysis_id).exists());
}

#[test]
fn startup_cleanup_preserves_analysis_jobs_dir() {
    let project = project_root();
    fs::write(
        project.as_path().join("jobs_keep.py"),
        "def epsilon():\n    return 5\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "jobs_keep.py"}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap().to_owned();
    let job_path = project
        .as_path()
        .join(".codelens")
        .join("analysis-cache")
        .join("jobs")
        .join(format!("{job_id}.json"));
    assert!(job_path.exists());

    let restarted = make_state(&project);
    assert!(restarted.analysis_dir().join("jobs").exists());
    assert!(job_path.exists());
}

#[test]
fn analysis_reads_update_session_metrics() {
    let project = project_root();
    fs::write(
        project.as_path().join("metrics.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update beta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let _ = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );
    let _ = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_section_reads"],
        json!(1)
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_summary_reads"],
        json!(1)
    );
}

#[test]
fn repeated_composite_request_reuses_existing_analysis_handle() {
    let project = project_root();
    fs::write(
        project.as_path().join("reuse.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let first = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow", "profile_hint": "planner-readonly"}),
    );
    let second = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow", "profile_hint": "planner-readonly"}),
    );

    assert_eq!(first["data"]["reused"], json!(false));
    assert_eq!(second["data"]["reused"], json!(true));
    assert_eq!(first["data"]["analysis_id"], second["data"]["analysis_id"]);

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_cache_hit_count"],
        json!(1)
    );
}

#[test]
fn audit_builder_session_is_not_applicable_for_non_builder_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "reviewer-session",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "reviewer-session"}),
    );
    assert_eq!(audit["data"]["status"], json!("not_applicable"));
}

#[test]
fn audit_builder_session_warns_when_bootstrap_is_missing() {
    let project = project_root();
    fs::write(
        project.as_path().join("bootstrap_warn.py"),
        "print('old')\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "builder-minimal"}),
        "builder-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "bootstrap_warn.py"}),
        "builder-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update bootstrap warn file",
            "changed_files": ["bootstrap_warn.py"]
        }),
        "builder-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "replace",
        json!({
            "relative_path": "bootstrap_warn.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "builder-warn",
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "builder-warn"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("bootstrap_order")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_builder_session_fails_when_gate_failure_was_recorded() {
    let project = project_root();
    fs::write(project.as_path().join("audit_fail.py"), "print('old')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "refactor-full"}),
        "builder-fail",
    );
    let preflight = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update audit fail file",
            "changed_files": ["audit_fail.py"]
        }),
        "builder-fail",
    );
    assert_eq!(preflight["success"], json!(true));
    state.set_recent_preflight_timestamp_for_test("builder-fail", 0);

    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "audit_fail.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "builder-fail",
    );
    assert_eq!(payload["success"], json!(false));

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": "builder-fail"}),
    );
    assert_eq!(audit["data"]["status"], json!("fail"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("mutation_gate")))
            .unwrap_or(false)
    );
}

#[cfg(feature = "http")]
fn make_http_state(project: &codelens_engine::ProjectRoot) -> crate::AppState {
    crate::AppState::new_minimal(project.clone(), crate::tool_defs::ToolPreset::Full)
        .with_session_store()
}

#[cfg(feature = "http")]
fn create_http_profile_session(
    state: &crate::AppState,
    project: &codelens_engine::ProjectRoot,
    profile: crate::tool_defs::ToolProfile,
) -> String {
    let session = state
        .session_store
        .as_ref()
        .expect("session store")
        .create();
    let session_id = session.id.clone();
    session.set_client_metadata(crate::server::session::SessionClientMetadata {
        client_name: Some("BuilderHarness".to_owned()),
        requested_profile: Some(profile.as_str().to_owned()),
        project_path: Some(project.as_path().to_string_lossy().to_string()),
        ..Default::default()
    });
    state.set_session_surface_and_budget(
        &session_id,
        crate::tool_defs::ToolSurface::Profile(profile),
        crate::tool_defs::default_budget_for_profile(profile),
    );
    session_id
}

#[cfg(feature = "http")]
#[test]
fn audit_builder_session_passes_for_happy_path_http_builder() {
    let project = project_root();
    fs::write(project.as_path().join("http_builder.py"), "print('old')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "refactor-full", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "http_builder.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "http_builder.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "update http builder file", "changed_files": ["http_builder.py"]}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "builder-http",
            "branch": "audit/http-pass",
            "worktree": project.as_path().to_string_lossy().to_string(),
            "intent": "happy path builder audit"
        }),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "claim_files",
        json!({"paths": ["http_builder.py"], "reason": "happy path builder audit"}),
        &session_id,
    );
    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "http_builder.py",
            "old_text": "old",
            "new_text": "new"
        }),
        &session_id,
    );
    assert_eq!(payload["success"], json!(true));
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "http_builder.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "release_files",
        json!({"paths": ["http_builder.py"]}),
        &session_id,
    );

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("pass"));
}

#[cfg(feature = "http")]
#[test]
fn audit_builder_session_warns_when_http_coordination_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("http_warn.py"), "print('old')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::RefactorFull,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "refactor-full", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "http_warn.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "update http warn file", "changed_files": ["http_warn.py"]}),
        &session_id,
    );
    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "http_warn.py",
            "old_text": "old",
            "new_text": "new"
        }),
        &session_id,
    );
    assert_eq!(payload["success"], json!(true));

    let audit = call_tool(
        &state,
        "audit_builder_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| {
                findings
                    .iter()
                    .any(|finding| finding["code"] == json!("coordination_registration"))
                    && findings
                        .iter()
                        .any(|finding| finding["code"] == json!("coordination_claim"))
            })
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_is_not_applicable_for_non_planner_session() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "builder-minimal"}),
        "builder-session",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "builder-session"}),
    );
    assert_eq!(audit["data"]["status"], json!("not_applicable"));
}

#[cfg(feature = "http")]
#[test]
fn audit_planner_session_passes_for_happy_path_reviewer() {
    let project = project_root();
    fs::write(project.as_path().join("planner_pass.py"), "print('ok')\n").unwrap();
    let state = make_http_state(&project);
    let session_id = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_pass.py"}),
        &session_id,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_pass.py"], "task": "review planner path"}),
        &session_id,
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": session_id}),
    );
    assert_eq!(audit["data"]["status"], json!("pass"));
}

#[test]
fn audit_planner_session_warns_when_bootstrap_is_missing() {
    let project = project_root();
    fs::write(project.as_path().join("planner_warn.py"), "print('ok')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_warn.py"}),
        "planner-warn",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_warn.py"], "task": "review planner path"}),
        "planner-warn",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-warn"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("bootstrap_order")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_warns_when_change_evidence_is_missing() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-change-evidence",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "planner-change-evidence",
    );
    let _ = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({"task": "review change evidence"}),
        "planner-change-evidence",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-change-evidence"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("change_evidence")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_warns_when_workflow_first_guidance_is_missed() {
    let project = project_root();
    fs::write(project.as_path().join("planner_chain.py"), "print('ok')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "planner-readonly", "detail": "compact"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_chain.py"}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "find_symbol",
        json!({"name": "missing_symbol", "include_body": true}),
        "planner-chain",
    );
    let _ = call_tool_with_session(
        &state,
        "get_file_diagnostics",
        json!({"file_path": "planner_chain.py"}),
        "planner-chain",
    );

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-chain"}),
    );
    assert_eq!(audit["data"]["status"], json!("warn"));
    assert!(
        audit["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("workflow_first")))
            .unwrap_or(false)
    );
}

#[test]
fn audit_planner_session_fails_on_mutation_attempt() {
    let project = project_root();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-fail",
    );
    let payload = call_tool_with_session(
        &state,
        "create_text_file",
        json!({"relative_path": "planner_fail.py", "content": "print('fail')\n"}),
        "planner-fail",
    );
    assert_eq!(payload["success"], json!(false));

    let audit = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": "planner-fail"}),
    );
    assert_eq!(audit["data"]["status"], json!("fail"));
}

#[cfg(feature = "http")]
#[test]
fn audit_planner_session_isolated_by_session_id() {
    let project = project_root();
    fs::write(project.as_path().join("planner_a.py"), "print('a')\n").unwrap();
    fs::write(project.as_path().join("planner_b.py"), "print('b')\n").unwrap();
    let state = make_http_state(&project);
    let session_a = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );
    let session_b = create_http_profile_session(
        &state,
        &project,
        crate::tool_defs::ToolProfile::ReviewerGraph,
    );

    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_a.py"], "task": "review a"}),
        &session_a,
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        &session_b,
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_b.py"}),
        &session_b,
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_b.py"], "task": "review b"}),
        &session_b,
    );

    let metrics_a = call_tool(
        &state,
        "get_tool_metrics",
        json!({"session_id": session_a.as_str()}),
    );
    assert_eq!(metrics_a["data"]["session_id"], json!(session_a.as_str()));
    let audit_a = call_tool(
        &state,
        "audit_planner_session",
        json!({"session_id": session_a.as_str()}),
    );
    assert_eq!(audit_a["data"]["status"], json!("warn"));
    assert!(
        audit_a["data"]["findings"]
            .as_array()
            .map(|findings| findings
                .iter()
                .any(|finding| finding["code"] == json!("bootstrap_order")))
            .unwrap_or(false)
    );
}

#[test]
fn export_session_markdown_appends_planner_audit_summary() {
    let project = project_root();
    fs::write(project.as_path().join("planner_md.py"), "print('md')\n").unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "planner_md.py"}),
        "planner-md",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["planner_md.py"], "task": "review markdown"}),
        "planner-md",
    );

    let markdown = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "planner-md", "name": "planner-md"}),
    );
    let body = markdown["data"]["markdown"].as_str().unwrap_or("");
    assert!(body.contains("## Planner Audit"));
}
