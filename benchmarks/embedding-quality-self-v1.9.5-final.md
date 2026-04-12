# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-core/models/codesearch/model.onnx`
- Dataset size: 105
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.590 | 55% | 61% | 62% | 480.0 |
| get_ranked_context_no_semantic | 0.564 | 49% | 62% | 67% | 38.6 |
| get_ranked_context | 0.679 | 60% | 74% | 77% | 107.2 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 32 | 0.969 | 97% | 97% | 97% | 164.2 |
| semantic_search | natural_language | 62 | 0.411 | 35% | 44% | 45% | 626.3 |
| semantic_search | short_phrase | 11 | 0.500 | 45% | 55% | 55% | 574.5 |
| get_ranked_context_no_semantic | identifier | 32 | 0.969 | 97% | 97% | 97% | 27.8 |
| get_ranked_context_no_semantic | natural_language | 62 | 0.380 | 29% | 44% | 50% | 43.8 |
| get_ranked_context_no_semantic | short_phrase | 11 | 0.421 | 18% | 64% | 73% | 41.2 |
| get_ranked_context | identifier | 32 | 0.969 | 97% | 97% | 97% | 27.0 |
| get_ranked_context | natural_language | 62 | 0.529 | 44% | 58% | 63% | 142.9 |
| get_ranked_context | short_phrase | 11 | 0.682 | 45% | 100% | 100% | 139.1 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.115 |
| Acc@1 uplift | +11% |
| Acc@3 uplift | +12% |
| Acc@5 uplift | +10% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.148 | +15% | +15% | +13% |
| short_phrase | +0.260 | +27% | +36% | +27% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | find where a symbol is defined in a file | 7 | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | search code by natural language query | 6 | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| semantic_search | move code to a different file | miss | normalized_touched_files (crates/codelens-mcp/src/tools/report_verifier.rs) |
| semantic_search | change function parameters | miss | changed_files_output_schema (crates/codelens-mcp/src/tool_defs/output_schemas.rs) |
| semantic_search | read input from stdin line by line | miss | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | parse source code into an AST | miss | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | categorize a function by its purpose | miss | is_word_byte (crates/codelens-engine/src/symbols/scoring.rs) |
| semantic_search | get project structure and key files on first load | 5 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | skip comments and string literals during search | miss | search_workspace_symbols (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | how to build embedding text from a symbol | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | mutation gate preflight check before editing | 6 | MutationGateAllowance (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | exclude directories from indexing | miss | from (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | truncate response when too large | miss | extract_tool_text (crates/codelens-mcp/src/integration_tests/mod.rs) |
| semantic_search | record which files were recently accessed | miss | record_analysis_read (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | find all functions that call a given function | miss | inline_function (crates/codelens-engine/src/inline.rs) |
| semantic_search | resolve which file a called function belongs to | miss | summarize_file (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | calculate modularity score for graph partitioning | 6 | supports_import_graph (crates/codelens-engine/src/import_graph/mod.rs) |
| semantic_search | measure density of internal edges in a cluster | miss | insert_at_line (crates/codelens-engine/src/file_ops/writer.rs) |
| semantic_search | register sqlite vector extension for similarity search | miss | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | find all occurrences of a word in project files | miss | count_word_occurrences (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| semantic_search | handle semantic search tool request | miss | handle_key (crates/codelens-tui/src/app.rs) |
| semantic_search | build embedding index for all project symbols | miss | build_embedding_text (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/query_analysis.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | build a success response with suggested next tools | 10 | suggest_next (crates/codelens-mcp/src/tools/mod.rs) |
| semantic_search | how does the embedding engine initialize the model | miss | EmbeddingEngine (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | determine which language config to use for a file | miss | lang_config (crates/codelens-engine/src/lib.rs) |
| semantic_search | select the most relevant symbols for a query | 10 | for_each_file_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | find similar code snippets using embeddings | 6 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | upsert embedding vector for a symbol | 9 | embedding_model_assets_available (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | get current timestamp in milliseconds | miss | matches_scope (crates/codelens-mcp/src/artifact_store.rs) |
| semantic_search | ProjectOverride | miss | project (scripts/finetune/promotion_gate.py) |
| semantic_search | Relation type for symbol dependencies | miss | build_type_map (crates/codelens-engine/src/type_hierarchy.rs) |
| semantic_search | workflow delegation to lower level tools | miss | is_workflow_tool (crates/codelens-mcp/src/telemetry.rs) |
| semantic_search | assess mutation readiness before code changes | miss | is_symbol_aware_mutation_tool (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | diagnose file issues and unresolved references | miss | unresolved_reference_check (crates/codelens-mcp/src/tools/reports/verifier_reports.rs) |
| semantic_search | WorkflowFirst profile for agents | miss | workflow_first_surfaces_prefer_alias_bootstrap (crates/codelens-mcp/src/integration_tests/workflow.rs) |
| semantic_search | TUI file entry for dashboard | miss | for_each_file_symbols_with_bytes (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | rename a variable or function across the project | 6 | has_gradle_or_maven_dependency (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 16 | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | move code to a different file | 13 | different_bool_values_produce_different_hash (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | for_each_file_embeddings (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | for_file (crates/codelens-mcp/src/tools/session/metrics_config.rs) |
| get_ranked_context_no_semantic | extract lines of code into a new function | 9 | new (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | push_unique_string (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | mutation gate preflight check before editing | 9 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 8 | detect_frameworks (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | truncate response when too large | miss | ToolCallResponse (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | 6 | get_callers (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | filter out standard library noise from call graph | 9 | call_graph (crates/codelens-engine/src/lib.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | EmbeddingStore (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | 5 | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | dirs_fallback (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | search for a symbol by name | 7 | make_symbol_id (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context_no_semantic | handle semantic search tool request | 10 | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | reusable_embedding_key_for_chunk (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | rerank semantic search results by relevance | miss | search_symbols_hybrid_with_semantic (crates/codelens-engine/src/search.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | relative_path (scripts/artifact_maintenance.py) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | raw_visible_tool_entries (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 16 | SymbolKind (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | miss | for_each_embedding_batch (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | find all word matches in a single file | miss | is_word_byte (crates/codelens-engine/src/symbols/scoring.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | current_project_scope (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | ProjectOverride | miss |  |
| get_ranked_context_no_semantic | Relation type for symbol dependencies | miss | SymbolInfo (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | workflow delegation to lower level tools | miss | default_risk_level (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | diagnose file issues and unresolved references | 5 | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | strip empty null fields from JSON response | 4 | from_json (crates/codelens-mcp/src/session_context.rs) |
| get_ranked_context_no_semantic | format structured pretty print response | 4 | ToolCallResponse (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | WorkflowFirst profile for agents | miss | ToolProfile (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | find where a symbol is defined in a file | 4 | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | search code by natural language query | 4 | is_natural_language_query (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context | read input from stdin line by line | 10 | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| get_ranked_context | parse source code into an AST | 12 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | categorize a function by its purpose | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | skip comments and string literals during search | miss | search_symbols_fuzzy (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | how to build embedding text from a symbol | 10 | build_symbol_text (scripts/finetune/collect_training_data.py) |
| get_ranked_context | mutation gate preflight check before editing | 13 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | truncate response when too large | 6 | truncated (benchmarks/harness/harness_runner_common.py) |
| get_ranked_context | find all functions that call a given function | 8 | __call__ (scripts/finetune/train_v6_internet_only.py) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 4 | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | find all occurrences of a word in project files | 28 | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | check if a query is natural language for semantic search | miss | is_natural_language_semantic_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | symbol_index (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | rerank semantic search results by relevance | miss | rerank_semantic_matches (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | how does the embedding engine initialize the model | miss | embedding_engine (crates/codelens-mcp/src/state.rs) |
| get_ranked_context | determine which language config to use for a file | miss | lang_config (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | classify what kind of symbol this is | 6 | classify_symbol (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | ProjectOverride | miss |  |
| get_ranked_context | workflow delegation to lower level tools | 8 | is_workflow_tool (crates/codelens-mcp/src/telemetry.rs) |
| get_ranked_context | diagnose file issues and unresolved references | 18 | unresolved_reference_check (crates/codelens-mcp/src/tools/reports/verifier_reports.rs) |
| get_ranked_context | WorkflowFirst profile for agents | miss | WORKFLOW_FIRST_TOOLS (crates/codelens-mcp/src/tool_defs/presets.rs) |

