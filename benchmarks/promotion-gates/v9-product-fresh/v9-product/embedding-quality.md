# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `v9-product`
- Dataset size: 89
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.691 | 60% | 76% | 81% | 122.8 |
| get_ranked_context_no_semantic | 0.567 | 49% | 60% | 66% | 26.4 |
| get_ranked_context | 0.683 | 58% | 76% | 83% | 99.4 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 25 | 0.784 | 76% | 80% | 80% | 120.1 |
| semantic_search | natural_language | 55 | 0.634 | 51% | 73% | 80% | 123.9 |
| semantic_search | short_phrase | 9 | 0.778 | 67% | 89% | 89% | 124.2 |
| get_ranked_context_no_semantic | identifier | 25 | 1.000 | 100% | 100% | 100% | 23.1 |
| get_ranked_context_no_semantic | natural_language | 55 | 0.384 | 29% | 42% | 49% | 28.0 |
| get_ranked_context_no_semantic | short_phrase | 9 | 0.484 | 33% | 56% | 78% | 26.1 |
| get_ranked_context | identifier | 25 | 1.000 | 100% | 100% | 100% | 22.6 |
| get_ranked_context | natural_language | 55 | 0.536 | 40% | 64% | 75% | 129.7 |
| get_ranked_context | short_phrase | 9 | 0.704 | 56% | 89% | 89% | 128.2 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.116 |
| Acc@1 uplift | +9% |
| Acc@3 uplift | +17% |
| Acc@5 uplift | +17% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.152 | +11% | +22% | +25% |
| short_phrase | +0.219 | +22% | +33% | +11% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | search code by natural language query | 4 | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-core/src/embedding.rs) |
| semantic_search | watch filesystem for file changes | miss | index_files_with_retry (crates/codelens-core/src/watcher.rs) |
| semantic_search | skip comments and string literals during search | 4 | search (crates/codelens-core/src/embedding.rs) |
| semantic_search | how to build embedding text from a symbol | miss | get_embedding (crates/codelens-core/src/embedding.rs) |
| semantic_search | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-core/src/import_graph/resolvers.rs) |
| semantic_search | CallEdge | miss | get_callees (crates/codelens-core/src/call_graph.rs) |
| semantic_search | measure density of internal edges in a cluster | 9 | resolve_call_edges (crates/codelens-core/src/call_graph.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-core/src/embedding_store.rs) |
| semantic_search | SymbolIndex | miss | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | build embedding index for all project symbols | miss | inspect_existing_index (crates/codelens-core/src/embedding.rs) |
| semantic_search | rerank semantic search results by relevance | miss | semantic_query_for_retrieval (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a tool requires preflight verification | 4 | is_symbol_aware_mutation_tool (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | MutationFailureKind | miss | record_mutation_audit (crates/codelens-mcp/src/state.rs) |
| semantic_search | MutationGateAllowance | 10 | record_mutation_audit (crates/codelens-mcp/src/state.rs) |
| semantic_search | how does the embedding engine initialize the model | 9 | inspect_existing_index (crates/codelens-core/src/embedding.rs) |
| semantic_search | select the most relevant symbols for a query | miss | search_symbols_hybrid_with_semantic (crates/codelens-core/src/search.rs) |
| semantic_search | find similar code snippets using embeddings | 4 | find_similar_code (crates/codelens-core/src/embedding.rs) |
| semantic_search | get current timestamp in milliseconds | miss | get_current_config (crates/codelens-mcp/src/tools/filesystem.rs) |
| semantic_search | MutationGateFailure | miss | record_mutation_audit (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | rename a variable or function across the project | 14 | still_detects_project_root_before_home_directory (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 11 | is_natural_language_query (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | move code to a different file | 13 | remove_files (crates/codelens-core/src/symbols/writer.rs) |
| get_ranked_context_no_semantic | read input from stdin line by line | miss | reader (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | get_file_symbols (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | apply_edits (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | index_files_with_retry (crates/codelens-core/src/watcher.rs) |
| get_ranked_context_no_semantic | extract lines of code into a new function | 6 | new (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | search_symbols_fts (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | compute similarity between two vectors | 4 | compute_pagerank (crates/codelens-core/src/import_graph/mod.rs) |
| get_ranked_context_no_semantic | route an incoming tool request to the right handler | miss | visible_tools (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | mutation gate preflight check before editing | 7 | mutation_gate (benchmarks/token-efficiency.py) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 4 | detect_frameworks (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | truncate response when too large | miss | ToolResponseMeta (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | 8 | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | filter out standard library noise from call graph | 11 | from_str_label (crates/codelens-core/src/symbols/types.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | EmbeddingStore (crates/codelens-core/src/embedding_store.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | 4 | validate_identifier (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | find_all_word_matches (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | search for a symbol by name | 5 | search_symbols_fts (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context_no_semantic | handle semantic search tool request | miss | is_content_mutation_tool (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | filters_direct_test_symbols_from_embedding_index (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | rerank semantic search results by relevance | miss | search_symbols_fts (crates/codelens-core/src/db/ops.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | still_detects_project_root_before_home_directory (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | is_content_mutation_tool (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build an error response with failure kind | 4 | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 4 | find_similar_code (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 15 | SymbolKind (crates/codelens-core/src/symbols/types.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | reusable_embedding_key_for_symbol (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | current_project_scope (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | 23 | parse_tier_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context | search code by natural language query | 4 | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | start an HTTP server with routes | 5 | start_http_daemon (benchmarks/benchmark_runtime_common.py) |
| get_ranked_context | read input from stdin line by line | 5 | read_line_at (crates/codelens-core/src/file_ops/mod.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | all_symbols (scripts/finetune/collect_training_data.py) |
| get_ranked_context | extract lines of code into a new function | 6 | lines (benchmarks/harness/refresh-routing-policy.py) |
| get_ranked_context | skip comments and string literals during search | miss | search (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | how to build embedding text from a symbol | 21 | build_symbol_text (scripts/finetune/collect_training_data.py) |
| get_ranked_context | mutation gate preflight check before editing | 4 | record_mutation_preflight_checked (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-core/src/import_graph/resolvers.rs) |
| get_ranked_context | measure density of internal edges in a cluster | 7 | resolve_call_edges (crates/codelens-core/src/call_graph.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-core/src/embedding_store.rs) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_project (scripts/finetune/verify_v6_external.py) |
| get_ranked_context | rerank semantic search results by relevance | miss | semantic_query_for_retrieval (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | build a success response with suggested next tools | 7 | suggest_next (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | how does the embedding engine initialize the model | 20 | index_project (scripts/finetune/verify_v6_external.py) |
| get_ranked_context | determine which language config to use for a file | 4 | LanguageConfig (crates/codelens-core/src/lang_config.rs) |
| get_ranked_context | select the most relevant symbols for a query | 23 | search_symbols_hybrid_with_semantic (crates/codelens-core/src/search.rs) |
| get_ranked_context | find similar code snippets using embeddings | 4 | find_similar_code (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | get current timestamp in milliseconds | miss | get_current_config (crates/codelens-mcp/src/tools/filesystem.rs) |

