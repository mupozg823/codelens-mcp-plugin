# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Dataset size: 89
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.730 | 63% | 82% | 90% | 112.5 |
| get_ranked_context_no_semantic | 0.561 | 48% | 60% | 66% | 22.3 |
| get_ranked_context | 0.671 | 57% | 73% | 82% | 90.4 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 25 | 0.930 | 92% | 92% | 96% | 108.5 |
| semantic_search | natural_language | 55 | 0.635 | 51% | 75% | 85% | 113.8 |
| semantic_search | short_phrase | 9 | 0.759 | 56% | 100% | 100% | 115.4 |
| get_ranked_context_no_semantic | identifier | 25 | 1.000 | 100% | 100% | 100% | 19.4 |
| get_ranked_context_no_semantic | natural_language | 55 | 0.374 | 27% | 42% | 49% | 23.6 |
| get_ranked_context_no_semantic | short_phrase | 9 | 0.484 | 33% | 56% | 78% | 21.7 |
| get_ranked_context | identifier | 25 | 1.000 | 100% | 100% | 100% | 19.2 |
| get_ranked_context | natural_language | 55 | 0.521 | 40% | 58% | 71% | 118.3 |
| get_ranked_context | short_phrase | 9 | 0.676 | 44% | 89% | 100% | 116.9 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.110 |
| Acc@1 uplift | +9% |
| Acc@3 uplift | +13% |
| Acc@5 uplift | +16% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.146 | +13% | +16% | +22% |
| short_phrase | +0.191 | +11% | +33% | +22% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | parse source code into an AST | 4 | parse (crates/codelens-mcp/src/dispatch.rs) |
| semantic_search | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-core/src/embedding_store.rs) |
| semantic_search | watch filesystem for file changes | 4 | start (crates/codelens-core/src/watcher.rs) |
| semantic_search | how to build embedding text from a symbol | miss | get_embedding (crates/codelens-core/src/embedding.rs) |
| semantic_search | find all functions that call a given function | miss | all_file_paths (crates/codelens-core/src/db/ops.rs) |
| semantic_search | resolve which file a called function belongs to | miss | resolve (crates/codelens-core/src/project.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-core/src/embedding.rs) |
| semantic_search | SymbolIndex | miss | indexed (scripts/finetune/bench_external.py) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | build embedding index for all project symbols | 5 | index_from_project (crates/codelens-core/src/embedding.rs) |
| semantic_search | rerank semantic search results by relevance | miss | search (crates/codelens-core/src/embedding.rs) |
| semantic_search | check if a tool requires preflight verification | 4 | is_verifier_source_tool (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | how does the embedding engine initialize the model | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| semantic_search | collect rename edits across the entire project | 4 | rename_symbol (crates/codelens-core/src/rename.rs) |
| semantic_search | parse incoming MCP tool call JSON | 5 | parse_lsp_args (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | MutationGateFailure | 4 | MutationGateAllowance (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | rename a variable or function across the project | 15 | still_detects_project_root_before_home_directory (crates/codelens-core/src/project.rs) |
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
| get_ranked_context_no_semantic | build an error response with failure kind | 5 | error (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 4 | find_similar_code (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 15 | SymbolKind (crates/codelens-core/src/symbols/types.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | reusable_embedding_key_for_symbol (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | current_project_scope (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | 23 | parse_tier_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context | move code to a different file | 4 | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| get_ranked_context | start an HTTP server with routes | 4 | start_http_daemon (benchmarks/benchmark_runtime_common.py) |
| get_ranked_context | read input from stdin line by line | miss | read_line_at (crates/codelens-core/src/file_ops/mod.rs) |
| get_ranked_context | parse source code into an AST | 7 | parser (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-core/src/embedding_store.rs) |
| get_ranked_context | get project structure and key files on first load | 4 | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context | extract lines of code into a new function | 5 | query_lines (scripts/finetune/extract_major_langs.py) |
| get_ranked_context | how to build embedding text from a symbol | miss | build_symbol_text (scripts/finetune/collect_training_data.py) |
| get_ranked_context | mutation gate preflight check before editing | 7 | record_mutation_preflight_checked (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context | record which files were recently accessed | 4 | recent_file_paths (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | find all functions that call a given function | 11 | __call__ (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve (crates/codelens-core/src/project.rs) |
| get_ranked_context | register sqlite vector extension for similarity search | 4 | find_duplicates (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | SqliteVecStore (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 4 | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | rerank semantic search results by relevance | miss | search (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | build a success response with suggested next tools | 7 | suggest_next (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | how does the embedding engine initialize the model | miss | EmbeddingIndexInfo (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | determine which language config to use for a file | miss | LanguageConfig (crates/codelens-core/src/lang_config.rs) |
| get_ranked_context | collect rename edits across the entire project | 5 | rename_symbol (crates/codelens-core/src/rename.rs) |
| get_ranked_context | parse incoming MCP tool call JSON | 8 | mcp_http_tool_call (benchmarks/harness/harness_runner_common.py) |

