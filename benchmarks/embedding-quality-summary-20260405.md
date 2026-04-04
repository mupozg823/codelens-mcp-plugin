# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Dataset size: 24

## Metrics

| Method | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.361 | 33% | 33% | 42% | 162.3 |
| get_ranked_context_no_semantic | 0.289 | 21% | 29% | 38% | 24.5 |
| get_ranked_context | 0.562 | 50% | 58% | 62% | 105.7 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 4 | 1.000 | 100% | 100% | 100% | 240.3 |
| semantic_search | natural_language | 15 | 0.103 | 7% | 7% | 20% | 156.7 |
| semantic_search | short_phrase | 5 | 0.622 | 60% | 60% | 60% | 116.7 |
| get_ranked_context_no_semantic | identifier | 4 | 1.000 | 100% | 100% | 100% | 23.4 |
| get_ranked_context_no_semantic | natural_language | 15 | 0.049 | 0% | 0% | 13% | 25.2 |
| get_ranked_context_no_semantic | short_phrase | 5 | 0.440 | 20% | 60% | 60% | 23.2 |
| get_ranked_context | identifier | 4 | 1.000 | 100% | 100% | 100% | 26.7 |
| get_ranked_context | natural_language | 15 | 0.450 | 40% | 47% | 53% | 121.7 |
| get_ranked_context | short_phrase | 5 | 0.547 | 40% | 60% | 60% | 121.0 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.273 |
| Acc@1 uplift | +29% |
| Acc@3 uplift | +29% |
| Acc@5 uplift | +25% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.401 | +40% | +47% | +40% |
| short_phrase | +0.106 | +20% | +0% | +0% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | miss | project_scope_renames_across_files (crates/codelens-core/src/rename.rs) |
| semantic_search | find where a symbol is defined in a file | miss | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| semantic_search | search code by natural language query | 10 | is_natural_language_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | move code to a different file | 9 | file_path_to_module (crates/codelens-core/src/move_symbol.rs) |
| semantic_search | change function parameters | miss | build_new_param_string (crates/codelens-core/src/change_signature.rs) |
| semantic_search | start an HTTP server with routes | miss | start_http_daemon (benchmarks/token-efficiency.py) |
| semantic_search | read input from stdin line by line | miss | read_file (benchmarks/benchmark_runtime_common.py) |
| semantic_search | parse source code into an AST | miss | parse_args (benchmarks/embedding-quality.py) |
| semantic_search | build embedding vectors for all symbols | miss | MAX_EMBED_SYMBOLS (crates/codelens-core/src/embedding.rs) |
| semantic_search | find near-duplicate code in the codebase | 4 | test_duplicate_detection (models/benchmark_full.py) |
| semantic_search | categorize a function by its purpose | 5 | classifies_definition_vs_read (crates/codelens-core/src/scope_analysis.rs) |
| semantic_search | get project structure and key files on first load | miss | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| semantic_search | watch filesystem for file changes | miss | watch_dir (benchmarks/harness/watch-routing-policy.py) |
| semantic_search | extract lines of code into a new function | miss | lines (benchmarks/render-summary.py) |
| semantic_search | skip comments and string literals during search | miss | excludes_comments_and_strings (crates/codelens-core/src/scope_analysis.rs) |
| semantic_search | route an incoming tool request to the right handler | miss | ToolHandler (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context_no_semantic | rename a variable or function across the project | 9 | still_detects_project_root_before_home_directory (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | find where a symbol is defined in a file | miss | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 12 | is_natural_language_query (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | inline a function and remove its definition | 5 | apply_edits (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | move code to a different file | 9 | get_file (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | change function parameters | 11 | natural_language_kind_prior_prefers_functions_over_types (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | start an HTTP server with routes | 11 | with_transaction (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | read input from stdin line by line | miss | reader (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | parse source code into an AST | miss | parse_symbols (crates/codelens-core/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | get_symbols_overview (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | find near-duplicate code in the codebase | 4 | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | apply_edits (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | get_file (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | extract lines of code into a new function | miss | new (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | search_symbols_fts (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | route an incoming tool request to the right handler | miss | tool_tier (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context | find where a symbol is defined in a file | miss | find_symbol_range (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context | move code to a different file | 7 | file_path_to_module (crates/codelens-core/src/move_symbol.rs) |
| get_ranked_context | change function parameters | 11 | natural_language_kind_prior_prefers_functions_over_types (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context | start an HTTP server with routes | 6 | start_http_daemon (benchmarks/harness/session-overhead-benchmark.py) |
| get_ranked_context | parse source code into an AST | miss | nest_symbols (crates/codelens-core/src/symbols/parser.rs) |
| get_ranked_context | get project structure and key files on first load | 4 | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context | watch filesystem for file changes | miss | watch_dir (benchmarks/harness/watch-routing-policy.py) |
| get_ranked_context | extract lines of code into a new function | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context | skip comments and string literals during search | miss | excludes_comments_and_strings (crates/codelens-core/src/scope_analysis.rs) |
| get_ranked_context | route an incoming tool request to the right handler | miss | ToolHandler (crates/codelens-mcp/src/tools/mod.rs) |

