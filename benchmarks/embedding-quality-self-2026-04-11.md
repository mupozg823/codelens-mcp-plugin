# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/debug/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-core/models/codesearch/model.onnx`
- Dataset size: 89
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.598 | 54% | 63% | 66% | 574.3 |
| get_ranked_context_no_semantic | 0.604 | 53% | 64% | 70% | 168.5 |
| get_ranked_context | 0.664 | 58% | 70% | 78% | 265.4 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 25 | 0.960 | 96% | 96% | 96% | 302.8 |
| semantic_search | natural_language | 55 | 0.444 | 36% | 49% | 55% | 692.0 |
| semantic_search | short_phrase | 9 | 0.532 | 44% | 56% | 56% | 608.6 |
| get_ranked_context_no_semantic | identifier | 25 | 0.960 | 96% | 96% | 96% | 139.2 |
| get_ranked_context_no_semantic | natural_language | 55 | 0.467 | 36% | 53% | 60% | 173.5 |
| get_ranked_context_no_semantic | short_phrase | 9 | 0.457 | 33% | 44% | 56% | 219.4 |
| get_ranked_context | identifier | 25 | 0.960 | 96% | 96% | 96% | 132.5 |
| get_ranked_context | natural_language | 55 | 0.528 | 44% | 55% | 65% | 318.9 |
| get_ranked_context | short_phrase | 9 | 0.676 | 44% | 89% | 100% | 307.0 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.060 |
| Acc@1 uplift | +6% |
| Acc@3 uplift | +6% |
| Acc@5 uplift | +8% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.061 | +7% | +2% | +5% |
| short_phrase | +0.219 | +11% | +44% | +44% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | miss | rename (crates/codelens-core/src/lib.rs) |
| semantic_search | find where a symbol is defined in a file | 7 | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| semantic_search | search code by natural language query | 7 | is_natural_language_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | move code to a different file | 6 | dead_code_report (crates/codelens-mcp/src/tools/reports/impact_reports.rs) |
| semantic_search | change function parameters | miss | changed_files_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | read input from stdin line by line | miss | read_file (crates/codelens-core/src/file_ops/reader.rs) |
| semantic_search | parse source code into an AST | miss | slice_source (crates/codelens-core/src/symbols/parser.rs) |
| semantic_search | build embedding vectors for all symbols | miss | reusable_embedding_key_for_symbol (crates/codelens-core/src/embedding.rs) |
| semantic_search | categorize a function by its purpose | miss | embedding_to_bytes (crates/codelens-core/src/embedding.rs) |
| semantic_search | get project structure and key files on first load | 9 | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| semantic_search | extract lines of code into a new function | 4 | extract_leading_doc_returns_none_for_no_doc (crates/codelens-core/src/embedding.rs) |
| semantic_search | skip comments and string literals during search | miss | engine_new_and_index (crates/codelens-core/src/embedding.rs) |
| semantic_search | how to build embedding text from a symbol | miss | reusable_embedding_key_for_symbol (crates/codelens-core/src/embedding.rs) |
| semantic_search | mutation gate preflight check before editing | 4 | MutationFailureKind (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | exclude directories from indexing | miss | chunk_from_row (crates/codelens-core/src/embedding.rs) |
| semantic_search | record which files were recently accessed | miss | recent_file_paths (crates/codelens-mcp/src/state.rs) |
| semantic_search | find all functions that call a given function | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-core/src/import_graph/resolvers.rs) |
| semantic_search | calculate modularity score for graph partitioning | 8 | empty_graph_returns_empty (crates/codelens-core/src/community.rs) |
| semantic_search | measure density of internal edges in a cluster | miss | insert_at_line_tool (crates/codelens-mcp/src/tools/mutation.rs) |
| semantic_search | register sqlite vector extension for similarity search | miss | search_dual (crates/codelens-core/src/embedding_store.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-core/src/embedding.rs) |
| semantic_search | find all occurrences of a word in project files | miss | count_word_occurrences (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| semantic_search | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | how does the embedding engine initialize the model | miss | configured_embedding_model_name (crates/codelens-core/src/embedding.rs) |
| semantic_search | determine which language config to use for a file | miss | LanguageConfig (crates/codelens-core/src/lang_config.rs) |
| semantic_search | find similar code snippets using embeddings | miss | find_similar_code (crates/codelens-core/src/embedding.rs) |
| semantic_search | classify what kind of symbol this is | 4 | classify_symbol (crates/codelens-core/src/embedding.rs) |
| semantic_search | get current timestamp in milliseconds | 8 | matches_scope (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | ProjectOverride | miss | project (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 7 | configured_embedding_model_name_defaults_to_codesearchnet (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | move code to a different file | 18 | remove (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | change_signature (crates/codelens-core/src/change_signature.rs) |
| get_ranked_context_no_semantic | compute similarity between two vectors | 6 | index_from_project (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 7 | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | 10 | get_callees_finds_callees (crates/codelens-core/src/call_graph.rs) |
| get_ranked_context_no_semantic | filter out standard library noise from call graph | 10 | call_graph (crates/codelens-core/src/lib.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | 5 | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | still_detects_project_root_before_home_directory (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | collect rename edits scoped to a single file | 5 | rename_symbol (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | search for a symbol by name | 7 | parse_symbol_id (crates/codelens-core/src/symbols/types.rs) |
| get_ranked_context_no_semantic | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | rerank semantic search results by relevance | miss | engine_search_returns_results (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | ProjectRoot (crates/codelens-core/src/project.rs) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | build an error response with failure kind | 4 | is_protocol_error (crates/codelens-mcp/src/error.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-core/src/db/mod.rs) |
| get_ranked_context_no_semantic | doom loop detection and prevention | 4 | client_profile (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 17 | find_duplicates (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 16 | SymbolKind (crates/codelens-core/src/symbols/types.rs) |
| get_ranked_context_no_semantic | check if a tool is a symbol-aware mutation | 9 | evaluate_mutation_gate (crates/codelens-mcp/src/mutation_gate.rs) |
| get_ranked_context_no_semantic | collect rename edits across the entire project | 15 | rename_symbol (crates/codelens-core/src/rename.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| get_ranked_context_no_semantic | find all word matches in a single file | miss | finds_references_in_single_file (crates/codelens-core/src/scope_analysis.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | set_recent_preflight_timestamp_for_test (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | parse_tier_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | ProjectOverride | miss |  |
| get_ranked_context | find where a symbol is defined in a file | 4 | find_symbol (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context | search code by natural language query | 5 | is_natural_language_query (crates/codelens-core/src/symbols/ranking.rs) |
| get_ranked_context | change function parameters | 4 | ParamSpec (crates/codelens-core/src/change_signature.rs) |
| get_ranked_context | read input from stdin line by line | 9 | read_file_text (crates/codelens-core/src/project.rs) |
| get_ranked_context | parse source code into an AST | 7 | slice_source (crates/codelens-core/src/symbols/parser.rs) |
| get_ranked_context | build embedding vectors for all symbols | 19 | reusable_embedding_key_for_symbol (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | get project structure and key files on first load | 10 | get_project_structure (crates/codelens-core/src/symbols/mod.rs) |
| get_ranked_context | skip comments and string literals during search | 9 | search (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | how to build embedding text from a symbol | 18 | reusable_embedding_key_for_symbol (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | mutation gate preflight check before editing | 5 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | find all functions that call a given function | 9 | __call__ (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-core/src/import_graph/resolvers.rs) |
| get_ranked_context | register sqlite vector extension for similarity search | 4 | find_similar_code (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 8 | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | find all occurrences of a word in project files | 11 | find_all_word_matches (crates/codelens-core/src/rename.rs) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | build a success response with suggested next tools | 4 | suggest_next (crates/codelens-mcp/src/tools/mod.rs) |
| get_ranked_context | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | determine which language config to use for a file | 15 | lang_config (crates/codelens-core/src/lib.rs) |
| get_ranked_context | find similar code snippets using embeddings | 15 | find_similar_code (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | classify what kind of symbol this is | 5 | classify_symbol (crates/codelens-core/src/embedding.rs) |
| get_ranked_context | parse incoming MCP tool call JSON | miss | write_mcp_json (install.sh) |
| get_ranked_context | ProjectOverride | miss |  |

