use super::{analyze_retrieval_query, rerank_semantic_matches};
use codelens_engine::SemanticMatch;

fn semantic_match(file_path: &str, symbol_name: &str, kind: &str, score: f64) -> SemanticMatch {
    SemanticMatch {
        file_path: file_path.to_owned(),
        symbol_name: symbol_name.to_owned(),
        kind: kind.to_owned(),
        line: 1,
        signature: String::new(),
        name_path: symbol_name.to_owned(),
        score,
    }
}

#[test]
fn prefers_extract_entrypoint_over_script_variables() {
    let reranked = rerank_semantic_matches(
        "extract lines of code into a new function",
        vec![
            semantic_match(
                "scripts/finetune/build_codex_dataset.py",
                "line",
                "variable",
                0.233,
            ),
            semantic_match(
                "benchmarks/harness/task-bootstrap.py",
                "lines",
                "variable",
                0.219,
            ),
            semantic_match(
                "crates/codelens-mcp/src/tools/composite.rs",
                "refactor_extract_function",
                "function",
                0.184,
            ),
        ],
        3,
    );

    assert_eq!(reranked[0].symbol_name, "refactor_extract_function");
}

#[test]
fn prefers_dispatch_entrypoint_over_handler_types() {
    let reranked = rerank_semantic_matches(
        "route an incoming tool request to the right handler",
        vec![
            semantic_match(
                "crates/codelens-mcp/src/tools/mod.rs",
                "ToolHandler",
                "unknown",
                0.313,
            ),
            semantic_match(
                "benchmarks/harness/harness_runner_common.py",
                "tool_list",
                "variable",
                0.266,
            ),
            semantic_match(
                "crates/codelens-mcp/src/dispatch.rs",
                "dispatch_tool",
                "function",
                0.224,
            ),
        ],
        3,
    );

    assert_eq!(reranked[0].symbol_name, "dispatch_tool");
}

#[test]
fn prefers_stdio_entrypoint_over_generic_read_helpers() {
    let reranked = rerank_semantic_matches(
        "read input from stdin line by line run_stdio stdio stdin",
        vec![
            semantic_match(
                "crates/codelens-core/src/file_ops/mod.rs",
                "read_line_at",
                "function",
                0.261,
            ),
            semantic_match(
                "crates/codelens-core/src/file_ops/reader.rs",
                "read_file",
                "function",
                0.258,
            ),
            semantic_match(
                "crates/codelens-mcp/src/server/transport_stdio.rs",
                "run_stdio",
                "function",
                0.148,
            ),
        ],
        3,
    );

    assert_eq!(reranked[0].symbol_name, "run_stdio");
}

#[test]
fn prefers_mutation_gate_entrypoint_over_telemetry_helpers() {
    let reranked = rerank_semantic_matches(
        "mutation gate preflight check before editing evaluate_mutation_gate mutation_gate preflight",
        vec![
            semantic_match(
                "crates/codelens-mcp/src/telemetry.rs",
                "record_mutation_preflight_checked",
                "function",
                0.402,
            ),
            semantic_match(
                "crates/codelens-mcp/src/telemetry.rs",
                "record_mutation_preflight_gate_denied",
                "function",
                0.314,
            ),
            semantic_match(
                "crates/codelens-mcp/src/mutation_gate.rs",
                "evaluate_mutation_gate",
                "function",
                0.280,
            ),
        ],
        3,
    );

    assert_eq!(reranked[0].symbol_name, "evaluate_mutation_gate");
}

#[test]
fn prefers_rename_entrypoint_for_project_wide_rename_request() {
    let reranked = rerank_semantic_matches(
        "rename a variable or function across the project",
        vec![
            semantic_match(
                "crates/codelens-engine/src/rename.rs",
                "project_scope_renames_across_files",
                "function",
                0.291,
            ),
            semantic_match(
                "crates/codelens-engine/src/rename.rs",
                "rename_symbol",
                "function",
                0.245,
            ),
        ],
        2,
    );

    assert_eq!(reranked[0].symbol_name, "rename_symbol");
}

#[test]
fn expands_stdio_alias_terms() {
    let expanded = analyze_retrieval_query("read input from stdin line by line").expanded_query;
    assert!(expanded.contains("run_stdio"));
    assert!(expanded.contains("stdio"));
}

#[test]
fn expands_product_retrieval_alias_terms() {
    let embedding_index =
        analyze_retrieval_query("build embedding vectors for all symbols").expanded_query;
    assert!(embedding_index.contains("index_from_project"));

    let classifier = analyze_retrieval_query("categorize a function by its purpose").expanded_query;
    assert!(classifier.contains("classify_symbol"));

    let preflight =
        analyze_retrieval_query("check if a tool requires preflight verification").expanded_query;
    assert!(preflight.contains("is_refactor_gated_mutation_tool"));

    let timestamp = analyze_retrieval_query("get current timestamp in milliseconds").expanded_query;
    assert!(timestamp.contains("now_ms"));
}

#[test]
fn prefers_embedding_index_entrypoint_over_embedding_helpers() {
    let reranked = rerank_semantic_matches(
        "build embedding vectors for all symbols",
        vec![
            semantic_match(
                "crates/codelens-engine/src/embedding/prompt.rs",
                "build_embedding_text",
                "function",
                0.292,
            ),
            semantic_match(
                "crates/codelens-engine/src/embedding/engine_impl/index.rs",
                "index_from_project",
                "function",
                0.272,
            ),
        ],
        2,
    );

    assert_eq!(reranked[0].symbol_name, "index_from_project");
}

#[test]
fn prefers_symbol_classifier_over_generic_function_matches() {
    let reranked = rerank_semantic_matches(
        "categorize a function by its purpose",
        vec![
            semantic_match(
                "crates/codelens-engine/src/inline.rs",
                "inline_function",
                "function",
                0.281,
            ),
            semantic_match(
                "crates/codelens-engine/src/embedding/engine_impl/analysis.rs",
                "classify_symbol",
                "function",
                0.181,
            ),
        ],
        2,
    );

    assert_eq!(reranked[0].symbol_name, "classify_symbol");
}

#[test]
fn prefers_timestamp_helper_over_config_queries() {
    let reranked = rerank_semantic_matches(
        "get current timestamp in milliseconds",
        vec![
            semantic_match(
                "crates/codelens-mcp/src/tools/filesystem.rs",
                "get_current_config",
                "function",
                0.241,
            ),
            semantic_match(
                "crates/codelens-mcp/src/util.rs",
                "now_ms",
                "function",
                0.128,
            ),
        ],
        2,
    );

    assert_eq!(reranked[0].symbol_name, "now_ms");
}
