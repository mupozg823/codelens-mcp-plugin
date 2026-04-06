# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Dataset size: 32

## Metrics

| Method | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.499 | 44% | 53% | 62% | 115.5 |
| get_ranked_context_no_semantic | 0.417 | 28% | 50% | 53% | 23.0 |
| get_ranked_context | 0.639 | 50% | 69% | 81% | 104.3 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 6 | 1.000 | 100% | 100% | 100% | 115.9 |
| semantic_search | natural_language | 20 | 0.332 | 25% | 35% | 50% | 115.4 |
| semantic_search | short_phrase | 6 | 0.556 | 50% | 67% | 67% | 115.6 |
| get_ranked_context_no_semantic | identifier | 6 | 1.000 | 100% | 100% | 100% | 20.4 |
| get_ranked_context_no_semantic | natural_language | 20 | 0.232 | 10% | 30% | 35% | 24.0 |
| get_ranked_context_no_semantic | short_phrase | 6 | 0.450 | 17% | 67% | 67% | 22.1 |
| get_ranked_context | identifier | 6 | 1.000 | 100% | 100% | 100% | 20.0 |
| get_ranked_context | natural_language | 20 | 0.560 | 40% | 60% | 80% | 124.6 |
| get_ranked_context | short_phrase | 6 | 0.543 | 33% | 67% | 67% | 120.9 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.222 |
| Acc@1 uplift | +22% |
| Acc@3 uplift | +19% |
| Acc@5 uplift | +28% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.327 | +30% | +30% | +45% |
| short_phrase | +0.093 | +17% | +0% | +0% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | miss | project_scope_renames_across_files (crates/codelens-core/src/rename.rs) |
| semantic_search | find where a symbol is defined in a file | miss | find_symbols_by_name (crates/codelens-core/src/db/ops.rs) |
| semantic_search | move code to a different file | miss | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | change function parameters | miss | parse_param_names (crates/codelens-core/src/change_signature.rs) |
| semantic_search | start an HTTP server with routes | 5 | start_http_daemon (benchmarks/benchmark_runtime_common.py) |
| semantic_search | read input from stdin line by line | miss | read_file (benchmarks/benchmark_runtime_common.py) |
| semantic_search | parse source code into an AST | miss | as_path (crates/codelens-core/src/project.rs) |
| semantic_search | build embedding vectors for all symbols | miss | MAX_EMBED_SYMBOLS (crates/codelens-core/src/embedding.rs) |
| semantic_search | categorize a function by its purpose | 4 | categories (models/benchmark_full.py) |
| semantic_search | get project structure and key files on first load | miss | get_project_structure (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | extract lines of code into a new function | 9 | line (scripts/finetune/build_codex_dataset.py) |
| semantic_search | skip comments and string literals during search | miss | excludes_comments_and_strings (crates/codelens-core/src/scope_analysis.rs) |
| semantic_search | route an incoming tool request to the right handler | miss | ToolHandler (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | mutation gate preflight check before editing | miss | record_mutation_preflight_checked (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | truncate response when too large | 4 | truncated (benchmarks/harness/harness_runner_common.py) |
| get_ranked_context_no_semantic | rename a variable or function across the project | 8 | still_detects_project_root_before_home_directory (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 12 | is_natural_language_query (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | inline a function and remove its definition | 5 | apply_edits (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | move code to a different file | 10 | get_file (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | change function parameters | 10 | natural_language_kind_prior_prefers_functions_over_types (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | start an HTTP server with routes | 7 | with_transaction (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | read input from stdin line by line | miss | read_only (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | get_file_symbols (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | apply_edits (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | get_file (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | search_symbols_fts (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | route an incoming tool request to the right handler | miss | visible_tools (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | mutation gate preflight check before editing | 6 | is_refactor_gated_mutation_tool (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 11 | detect_frameworks (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | truncate response when too large | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context | rename a variable or function across the project | 4 | project_scope_renames_across_files (crates/codelens-core/src/rename.rs) |
| get_ranked_context | find where a symbol is defined in a file | 6 | find_symbols_by_name (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context | move code to a different file | 6 | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context | change function parameters | 11 | parse_param_names (crates/codelens-core/src/change_signature.rs) |
| get_ranked_context | start an HTTP server with routes | 4 | start_http_daemon (benchmarks/benchmark_runtime_common.py) |
| get_ranked_context | read input from stdin line by line | miss | read_line_at (crates/codelens-core/src/file_ops/mod.rs) |
| get_ranked_context | build embedding vectors for all symbols | 8 | build_embedding_text (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | get project structure and key files on first load | 5 | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context | skip comments and string literals during search | 5 | excludes_comments_and_strings (crates/codelens-core/src/scope_analysis.rs) |
| get_ranked_context | mutation gate preflight check before editing | 6 | record_mutation_preflight_checked (crates/codelens-mcp/src/telemetry.rs) |

