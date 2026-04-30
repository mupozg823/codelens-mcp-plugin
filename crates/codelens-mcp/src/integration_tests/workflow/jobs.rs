use super::*;

#[test]
fn reviewer_jobs_use_parallel_http_pool() {
    let project = project_root();
    fs::write(
        project.as_path().join("parallel_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("parallel_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.configure_transport_mode("http");

    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "parallel_first.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "parallel_second.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    assert_eq!(first["success"], json!(true), "first job failed: {first}");
    assert_eq!(
        second["success"],
        json!(true),
        "second job failed: {second}"
    );
    let first_job_id = first["data"]["job_id"]
        .as_str()
        .expect("first job_id should be present");
    let second_job_id = second["data"]["job_id"]
        .as_str()
        .expect("second job_id should be present");
    for _ in 0..100 {
        let metrics = call_tool(&state, "get_tool_metrics", json!({}));
        let peak_workers = metrics["data"]["session"]["peak_active_analysis_workers"]
            .as_u64()
            .unwrap_or_default();
        if peak_workers >= 2 {
            break;
        }
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
        if first_job["data"]["status"] == json!("completed")
            && second_job["data"]["status"] == json!("completed")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_worker_limit"],
        json!(2)
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_transport_mode"],
        json!("http")
    );
    assert!(
        metrics["data"]["session"]["peak_active_analysis_workers"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert_eq!(metrics["data"]["session"]["analysis_cost_budget"], json!(3));
}

#[test]
fn low_cost_jobs_bypass_heavy_jobs_in_http_queue() {
    let project = project_root();
    fs::write(
        project.as_path().join("priority_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("priority_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.configure_transport_mode("http");

    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "priority_first.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
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

    let heavy = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "dead_code_report",
            "scope": ".",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let heavy_job_id = heavy["data"]["job_id"].as_str().unwrap();

    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "priority_second.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let second_job_id = second["data"]["job_id"].as_str().unwrap();

    let mut saw_second_ahead_of_heavy = false;
    for _ in 0..100 {
        let heavy_job = call_tool(&state, "get_analysis_job", json!({"job_id": heavy_job_id}));
        let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
        if (second_job["data"]["status"] == json!("running")
            || second_job["data"]["status"] == json!("completed"))
            && heavy_job["data"]["status"] == json!("queued")
        {
            saw_second_ahead_of_heavy = true;
            break;
        }
        if heavy_job["data"]["status"] == json!("completed")
            && second_job["data"]["status"] == json!("completed")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(saw_second_ahead_of_heavy);
    assert!(
        metrics["data"]["session"]["analysis_queue_priority_promotions"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["analysis_queue_max_weighted_depth"]
            .as_u64()
            .unwrap_or_default()
            >= 4
    );
}

#[test]
fn foreign_project_scoped_job_file_is_ignored() {
    let project = project_root();
    let state = make_state(&project);
    let jobs_dir = state.analysis_dir().join("jobs");
    fs::create_dir_all(&jobs_dir).unwrap();
    let job_path = jobs_dir.join("job-foreign.json");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let job = json!({
        "id": "job-foreign",
        "kind": "impact_report",
        "project_scope": "/tmp/other-project",
        "status": "queued",
        "progress": 0,
        "current_step": "queued",
        "profile_hint": "reviewer-graph",
        "estimated_sections": ["impact"],
        "analysis_id": null,
        "error": null,
        "created_at_ms": now_ms,
        "updated_at_ms": now_ms,
    });
    fs::write(&job_path, serde_json::to_vec_pretty(&job).unwrap()).unwrap();

    assert!(state.get_analysis_job("job-foreign").is_none());
    assert!(!job_path.exists());
}
