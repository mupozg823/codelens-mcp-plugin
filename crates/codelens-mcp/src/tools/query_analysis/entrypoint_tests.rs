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
