use super::{call_tool, embedding_model_available_for_test, make_state, project_root};
use serde_json::{Value, json};

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
    let index = call_tool(&state, "index_embeddings", json!({}));
    assert!(
        tool_data(&index)
            .get("indexed_symbols")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 2
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
