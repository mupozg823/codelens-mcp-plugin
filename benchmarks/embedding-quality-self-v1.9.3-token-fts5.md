# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-core/models/codesearch/model.onnx`
- Dataset size: 89
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.575 | 53% | 60% | 63% | 409.8 |
| get_ranked_context_no_semantic | 0.556 | 49% | 61% | 64% | 37.5 |
| get_ranked_context | 0.654 | 56% | 72% | 78% | 147.8 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 25 | 0.960 | 96% | 96% | 96% | 251.1 |
| semantic_search | natural_language | 55 | 0.403 | 33% | 44% | 49% | 483.0 |
| semantic_search | short_phrase | 9 | 0.556 | 56% | 56% | 56% | 403.3 |
| get_ranked_context_no_semantic | identifier | 25 | 0.960 | 96% | 96% | 96% | 28.6 |
| get_ranked_context_no_semantic | natural_language | 55 | 0.372 | 29% | 42% | 47% | 41.4 |
| get_ranked_context_no_semantic | short_phrase | 9 | 0.561 | 44% | 78% | 78% | 38.7 |
| get_ranked_context | identifier | 25 | 0.960 | 96% | 96% | 96% | 29.5 |
| get_ranked_context | natural_language | 55 | 0.510 | 40% | 56% | 65% | 201.6 |
| get_ranked_context | short_phrase | 9 | 0.685 | 44% | 100% | 100% | 147.9 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.098 |
| Acc@1 uplift | +7% |
| Acc@3 uplift | +11% |
| Acc@5 uplift | +13% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.138 | +11% | +15% | +18% |
| short_phrase | +0.124 | +0% | +22% | +22% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | find where a symbol is defined in a file | 7 | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | search code by natural language query | 5 | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | move code to a different file | miss | normalized_touched_files (crates/codelens-mcp/src/tools/report_verifier.rs) |
| semantic_search | change function parameters | miss | changed_files_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | read input from stdin line by line | miss | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | parse source code into an AST | miss | parse (crates/codelens-mcp/src/dispatch/mod.rs) |
| semantic_search | build embedding vectors for all symbols | miss | build_embedding_text (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | categorize a function by its purpose | miss | natural_language_kind_prior_prefers_functions_over_types (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | get project structure and key files on first load | 6 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | skip comments and string literals during search | miss | searches_text_pattern (crates/codelens-engine/src/file_ops/mod.rs) |
| semantic_search | how to build embedding text from a symbol | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | mutation gate preflight check before editing | 5 | MutationGateAllowance (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | exclude directories from indexing | miss | from (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | truncate response when too large | miss | workspace_symbols_from_response (crates/codelens-engine/src/lsp/parsers.rs) |
| semantic_search | record which files were recently accessed | miss | record_analysis_read (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | find all functions that call a given function | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | filter out standard library noise from call graph | 5 | empty_graph_returns_empty (crates/codelens-engine/src/community.rs) |
| semantic_search | resolve which file a called function belongs to | miss | summarize_file (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | calculate modularity score for graph partitioning | 9 | supports_import_graph (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | measure density of internal edges in a cluster | miss | add_import_inserts_at_correct_position (crates/codelens-engine/src/auto_import.rs) |
| semantic_search | register sqlite vector extension for similarity search | miss | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | find all occurrences of a word in project files | miss | count_word_occurrences (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| semantic_search | handle semantic search tool request | miss | handle_key (crates/codelens-tui/src/app.rs) |
| semantic_search | build embedding index for all project symbols | miss | build_embedding_text (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/query_analysis.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | build a success response with suggested next tools | miss | suggest_next (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | determine which language config to use for a file | miss | lang_config (crates/codelens-engine/src/lib.rs) |
| semantic_search | select the most relevant symbols for a query | 7 | for_each_file_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | find similar code snippets using embeddings | 6 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | upsert embedding vector for a symbol | miss | embedding_model_assets_available (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | get current timestamp in milliseconds | miss | matches_scope (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | ProjectOverride | miss | project (benchmarks/render-summary.py) |
| get_ranked_context_no_semantic | rename a variable or function across the project | 12 | compute_dominant_language_picks_python_for_python_heavy_project (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | search code by natural language query | miss | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | move code to a different file | 19 | remove (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | embeddings_for_scored_chunks (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | for_file (crates/codelens-mcp/src/tools/session/metrics_config.rs) |
| get_ranked_context_no_semantic | extract lines of code into a new function | 6 | new (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | extract_nl_tokens_collects_comments_and_string_literals (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | mutation gate preflight check before editing | 9 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 8 | detect_frameworks (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | truncate response when too large | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | 9 | extract_api_calls_rejects_module_prefixed_free_functions (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | filter out standard library noise from call graph | 9 | call_graph (crates/codelens-engine/src/lib.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | EmbeddingStore (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | 6 | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | compute_dominant_language_picks_rust_for_rust_heavy_project (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context_no_semantic | handle semantic search tool request | 15 | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | filters_direct_test_symbols_from_embedding_index (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | rerank semantic search results by relevance | miss | semantic_scores_boost_existing_results (crates/codelens-engine/src/search.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | ProjectRoot (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | tool_tier_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | build an error response with failure kind | 4 | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 5 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | miss | SymbolKind (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | reusable_embedding_key_for_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find all word matches in a single file | miss | finds_references_in_single_file (crates/codelens-engine/src/scope_analysis.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | current_project_scope (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | ProjectOverride | miss |  |
| get_ranked_context | find where a symbol is defined in a file | 4 | find_symbols_by_name (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | search code by natural language query | 4 | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context | read input from stdin line by line | 9 | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| get_ranked_context | parse source code into an AST | 9 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | categorize a function by its purpose | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | skip comments and string literals during search | miss | search_symbols_fuzzy (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | how to build embedding text from a symbol | 21 | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | mutation gate preflight check before editing | 12 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | truncate response when too large | 6 | truncated (benchmarks/harness/harness_runner_common.py) |
| get_ranked_context | find all functions that call a given function | 8 | __call__ (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | register sqlite vector extension for similarity search | 4 | sqlite_related_paths (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 4 | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | find all occurrences of a word in project files | 26 | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | determine which language config to use for a file | miss | lang_config (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | classify what kind of symbol this is | 5 | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | ProjectOverride | miss |  |

