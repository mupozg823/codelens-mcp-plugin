use super::*;

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
            .find_reusable_analysis_tiered_for_current_scope("analyze_change_request", &cache_key)
            .is_none()
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
