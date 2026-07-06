use super::{call_tool, embedding_model_available_for_test, make_state, project_root};
use serde_json::{Value, json};
use std::process::Command;

fn tool_data(response: &Value) -> &Value {
    response.get("data").unwrap_or(response)
}

fn result_signatures(response: &Value) -> Vec<String> {
    tool_data(response)
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("signature").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn git_stdout(project: &codelens_engine::ProjectRoot, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(project.as_path())
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout should be utf8")
        .trim()
        .to_owned()
}

#[test]
fn refresh_symbol_index_reconciles_embedding_freshness() {
    if !embedding_model_available_for_test() {
        return;
    }

    let project = project_root();
    std::fs::write(
        project.as_path().join("main.py"),
        "def hello():\n    print('hi')\n\ndef world():\n    return 42\n",
    )
    .unwrap();

    let state = make_state(&project);
    let _ = call_tool(&state, "refresh_symbol_index", json!({}));
    let index = call_tool(
        &state,
        "index_embeddings",
        json!({"prewarm_queries": ["hello function"], "prewarm_limit": 4}),
    );
    assert!(
        tool_data(&index)
            .get("indexed_symbols")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 2
    );
    assert_eq!(
        tool_data(&index)["query_cache"]["prewarmed"].as_u64(),
        Some(1)
    );

    std::fs::write(
        project.as_path().join("main.py"),
        "def hello(name):\n    print(name)\n\ndef world():\n    return 42\n",
    )
    .unwrap();

    let refresh = call_tool(&state, "refresh_symbol_index", json!({}));
    let freshness = &tool_data(&refresh)["embedding_freshness"];
    assert_eq!(freshness["checked_files"].as_u64(), Some(1));
    assert_eq!(freshness["refreshed_files"].as_u64(), Some(1));
    assert_eq!(freshness["removed_files"].as_u64(), Some(0));

    let search = call_tool(
        &state,
        "semantic_search",
        json!({"query": "hello function", "max_results": 5}),
    );
    assert!(
        result_signatures(&search)
            .iter()
            .any(|signature| signature == "def hello(name):"),
        "semantic_search should expose the refreshed signature: {search:#?}"
    );
}

#[test]
fn embedding_coverage_report_infers_sha_for_clean_legacy_index() {
    if !embedding_model_available_for_test() {
        return;
    }

    let project = project_root();
    std::fs::write(
        project.as_path().join("main.py"),
        "def hello():\n    print('hi')\n\ndef world():\n    return 42\n",
    )
    .unwrap();
    git_stdout(&project, &["init"]);
    git_stdout(&project, &["config", "user.email", "codelens@example.test"]);
    git_stdout(&project, &["config", "user.name", "CodeLens Test"]);
    git_stdout(&project, &["add", "main.py"]);
    git_stdout(&project, &["commit", "-m", "init"]);
    let head_sha = git_stdout(&project, &["rev-parse", "HEAD"]);

    let state = make_state(&project);
    let _ = call_tool(&state, "refresh_symbol_index", json!({}));
    let _ = call_tool(&state, "index_embeddings", json!({}));

    let embeddings_db = project.as_path().join(".codelens/index/embeddings.db");
    let conn = rusqlite::Connection::open(embeddings_db).unwrap();
    conn.execute("DELETE FROM meta WHERE key = 'last_index_sha'", [])
        .unwrap();

    let report = call_tool(&state, "embedding_coverage_report", json!({}));
    let data = tool_data(&report);
    let index = &data["index"];
    assert_eq!(data["status"].as_str(), Some("ready"));
    assert_eq!(data["compiled"].as_bool(), Some(true));
    assert_eq!(
        data["model_assets"]["available"].as_bool(),
        Some(true),
        "semantic test helper only runs when model assets are available"
    );
    assert_eq!(index["model_mismatch"].as_bool(), Some(false));
    assert!(index["indexed_symbols"].as_u64().unwrap_or(0) >= 2);
    assert_eq!(index["stale_files"].as_u64(), Some(0));
    assert_eq!(index["current_git_sha"].as_str(), Some(head_sha.as_str()));
    assert_eq!(index["last_index_sha"].as_str(), Some(head_sha.as_str()));
    assert_eq!(
        index["last_index_sha_source"].as_str(),
        Some("inferred_current_clean_index")
    );
    assert!(data["query_cache"]["enabled"].is_boolean());
    assert!(data["query_cache"]["entries"].is_u64());
    assert_eq!(data["recommended_action"].as_str(), Some("none"));
}

#[test]
fn ranked_context_reports_exact_query_cache_tier_after_repeat_query() {
    if !embedding_model_available_for_test() {
        return;
    }

    let project = project_root();
    std::fs::write(
        project.as_path().join("main.py"),
        "def hello():\n    print('hi')\n\ndef world():\n    return 42\n",
    )
    .unwrap();

    let state = make_state(&project);
    let _ = call_tool(&state, "refresh_symbol_index", json!({}));
    let index = call_tool(&state, "index_embeddings", json!({}));
    assert!(
        tool_data(&index)
            .get("indexed_symbols")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 2
    );

    let first = call_tool(
        &state,
        "get_ranked_context",
        json!({"query": "hello function", "max_tokens": 4096, "include_body": false}),
    );
    assert!(
        tool_data(&first)["retrieval"]["query_cache"]["cache_hit_tier"].is_string(),
        "first call should expose query cache tier: {first:#?}"
    );

    let second = call_tool(
        &state,
        "get_ranked_context",
        json!({"query": "hello function", "max_tokens": 4096, "include_body": false}),
    );
    let retrieval = &tool_data(&second)["retrieval"];
    assert_eq!(tool_data(&second)["cache_hit_tier"], json!("exact"));
    assert_eq!(retrieval["cache_hit_tier"], json!("exact"));
    assert_eq!(retrieval["query_cache"]["cache_hit_tier"], json!("exact"));
    assert_eq!(retrieval["query_cache"]["enabled"], json!(true));
    assert_eq!(retrieval["query_cache"]["used"], json!(true));
}
