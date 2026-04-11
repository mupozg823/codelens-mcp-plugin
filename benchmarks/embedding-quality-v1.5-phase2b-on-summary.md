# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset size: 89
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.513 | 46% | 54% | 58% | 251.0 |
| get_ranked_context_no_semantic | 0.493 | 42% | 54% | 60% | 32.6 |
| get_ranked_context | 0.580 | 49% | 67% | 67% | 104.1 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 25 | 0.800 | 80% | 80% | 80% | 132.1 |
| semantic_search | natural_language | 55 | 0.400 | 33% | 44% | 51% | 302.7 |
| semantic_search | short_phrase | 9 | 0.403 | 33% | 44% | 44% | 265.4 |
| get_ranked_context_no_semantic | identifier | 25 | 0.800 | 80% | 80% | 80% | 23.4 |
| get_ranked_context_no_semantic | natural_language | 55 | 0.364 | 27% | 42% | 49% | 36.6 |
| get_ranked_context_no_semantic | short_phrase | 9 | 0.423 | 22% | 56% | 67% | 33.8 |
| get_ranked_context | identifier | 25 | 0.800 | 80% | 80% | 80% | 23.0 |
| get_ranked_context | natural_language | 55 | 0.481 | 38% | 58% | 58% | 136.0 |
| get_ranked_context | short_phrase | 9 | 0.574 | 33% | 89% | 89% | 134.3 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.087 |
| Acc@1 uplift | +8% |
| Acc@3 uplift | +13% |
| Acc@5 uplift | +8% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.117 | +11% | +16% | +9% |
| short_phrase | +0.151 | +11% | +33% | +22% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | miss | rename (crates/codelens-engine/src/lib.rs) |
| semantic_search | find where a symbol is defined in a file | 6 | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | search code by natural language query | miss | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | move code to a different file | miss | dead_code_report (crates/codelens-mcp/src/tools/reports/impact_reports.rs) |
| semantic_search | change function parameters | miss | changed_files_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | read input from stdin line by line | miss | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | parse source code into an AST | 4 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | build embedding vectors for all symbols | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | find near-duplicate code in the codebase | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | categorize a function by its purpose | miss | embedding_to_bytes_roundtrip (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | get project structure and key files on first load | 8 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | skip comments and string literals during search | miss | engine_search_returns_results (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | compute similarity between two vectors | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | cosine_similarity | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | how to build embedding text from a symbol | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | exclude directories from indexing | miss | from (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | truncate response when too large | 4 | max_result_size_chars_for_tool (crates/codelens-mcp/src/dispatch_response_support.rs) |
| semantic_search | record which files were recently accessed | miss | recent_file_paths (crates/codelens-mcp/src/state.rs) |
| semantic_search | EmbeddingEngine | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | find all functions that call a given function | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | filter out standard library noise from call graph | 5 | empty_graph_returns_empty (crates/codelens-engine/src/community.rs) |
| semantic_search | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| semantic_search | calculate modularity score for graph partitioning | 10 | supports_import_graph (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | measure density of internal edges in a cluster | miss | insert_at_line_tool (crates/codelens-mcp/src/tools/mutation.rs) |
| semantic_search | register sqlite vector extension for similarity search | miss | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | split camelCase or snake_case identifier into words | miss | split_camel_case (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | SemanticMatch | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | SqliteVecStore | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | find all occurrences of a word in project files | miss | count_word_occurrences (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | handle semantic search tool request | miss | semantic_code_review (crates/codelens-mcp/src/tools/reports/impact_reports.rs) |
| semantic_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | how does the embedding engine initialize the model | miss | configured_embedding_model_name (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | determine which language config to use for a file | miss | LanguageConfig (crates/codelens-engine/src/lang_config.rs) |
| semantic_search | find similar code snippets using embeddings | 4 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | upsert embedding vector for a symbol | miss | upsert (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | get current timestamp in milliseconds | 8 | matches_scope (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | ProjectOverride | miss | project (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | search code by natural language query | miss | configured_embedding_model_name_defaults_to_codesearchnet (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | move code to a different file | 18 | remove (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find near-duplicate code in the codebase | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | change_signature (crates/codelens-engine/src/change_signature.rs) |
| get_ranked_context_no_semantic | compute similarity between two vectors | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | cosine_similarity | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | how to build embedding text from a symbol | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 7 | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | EmbeddingEngine | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | 10 | get_callees_finds_callees (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | filter out standard library noise from call graph | 9 | call_graph (crates/codelens-engine/src/lib.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | register sqlite vector extension for similarity search | miss | register_sqlite_vec (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | miss | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | SemanticMatch | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | SqliteVecStore | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | validate that a new name is a valid identifier | 5 | new (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | still_detects_project_root_before_home_directory (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | collect rename edits scoped to a single file | 5 | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | check if a query is natural language for semantic search | miss | for_each_embedding_batch (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | rerank semantic search results by relevance | miss | engine_search_returns_results (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | ProjectRoot (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | build an error response with failure kind | 4 | is_protocol_error (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | doom loop detection and prevention | 4 | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 20 | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 16 | SymbolKind (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | check if a tool is a symbol-aware mutation | 9 | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | collect rename edits across the entire project | 15 | rename_symbol (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | find all word matches in a single file | miss | finds_references_in_single_file (crates/codelens-engine/src/scope_analysis.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | set_recent_preflight_timestamp_for_test (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | parse_tier_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | ProjectOverride | miss |  |
| get_ranked_context | search code by natural language query | miss | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context | read input from stdin line by line | 9 | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | max_embed_symbols (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | find near-duplicate code in the codebase | miss | find_duplicates (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | categorize a function by its purpose | miss | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | get project structure and key files on first load | 11 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | skip comments and string literals during search | 11 | search (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | compute similarity between two vectors | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | cosine_similarity | miss | cosine_similarity (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | how to build embedding text from a symbol | miss | reusable_embedding_key_for_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | EmbeddingEngine | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | find all functions that call a given function | 8 | __call__ (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | register sqlite vector extension for similarity search | miss | sqlite_related_paths (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | miss | split_camel_case (crates/codelens-engine/src/symbols/scoring.rs) |
| get_ranked_context | SemanticMatch | miss | SemanticMatch (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | SqliteVecStore | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | find all occurrences of a word in project files | 10 | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | determine which language config to use for a file | miss | lang_config (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | find similar code snippets using embeddings | 9 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | classify what kind of symbol this is | 6 | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | upsert embedding vector for a symbol | miss | upsert (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | ProjectOverride | miss |  |

