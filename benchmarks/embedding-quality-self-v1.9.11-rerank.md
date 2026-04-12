# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime backend: `not_loaded`, preference=`coreml_preferred`, max_length=`256`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset size: 104
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.734 | 65% | 81% | 84% | 484.7 |
| get_ranked_context_no_semantic | 0.614 | 53% | 66% | 74% | 39.9 |
| get_ranked_context | 0.765 | 70% | 80% | 85% | 111.5 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 31 | 1.000 | 100% | 100% | 100% | 181.6 |
| semantic_search | natural_language | 62 | 0.600 | 48% | 69% | 74% | 618.8 |
| semantic_search | short_phrase | 11 | 0.742 | 64% | 91% | 91% | 583.0 |
| get_ranked_context_no_semantic | identifier | 31 | 1.000 | 100% | 100% | 100% | 28.2 |
| get_ranked_context_no_semantic | natural_language | 62 | 0.455 | 35% | 52% | 60% | 45.2 |
| get_ranked_context_no_semantic | short_phrase | 11 | 0.424 | 18% | 55% | 82% | 42.6 |
| get_ranked_context | identifier | 31 | 1.000 | 100% | 100% | 100% | 28.3 |
| get_ranked_context | natural_language | 62 | 0.650 | 56% | 68% | 76% | 146.8 |
| get_ranked_context | short_phrase | 11 | 0.751 | 64% | 91% | 91% | 146.5 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.151 |
| Acc@1 uplift | +17% |
| Acc@3 uplift | +13% |
| Acc@5 uplift | +11% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.195 | +21% | +16% | +16% |
| short_phrase | +0.326 | +45% | +36% | +9% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | 6 | rename (crates/codelens-engine/src/lib.rs) |
| semantic_search | find where a symbol is defined in a file | 5 | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | search code by natural language query | 4 | is_natural_language_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| semantic_search | read input from stdin line by line | miss | read_file (crates/codelens-engine/src/file_ops/reader.rs) |
| semantic_search | parse source code into an AST | 6 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | categorize a function by its purpose | miss | refactor_extract_function (crates/codelens-mcp/src/tools/composite.rs) |
| semantic_search | get project structure and key files on first load | 6 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | skip comments and string literals during search | miss | search_workspace_symbols (crates/codelens-engine/src/lsp/session.rs) |
| semantic_search | truncate response when too large | miss | truncate_body_preview (crates/codelens-mcp/src/tools/symbols.rs) |
| semantic_search | find all functions that call a given function | 8 | refresh_all (crates/codelens-engine/src/symbols/writer.rs) |
| semantic_search | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| semantic_search | measure density of internal edges in a cluster | 9 | insert_at_line (crates/codelens-engine/src/file_ops/writer.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | get_embedding (crates/codelens-engine/src/embedding_store.rs) |
| semantic_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | determine which language config to use for a file | miss | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| semantic_search | get current timestamp in milliseconds | miss | get_directory_symbols (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | assess mutation readiness before code changes | 5 | is_symbol_aware_mutation_tool (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | WorkflowFirst profile for agents | miss | read_only_daemon_rejects_mutation_even_with_mutating_profile (crates/codelens-mcp/src/integration_tests/protocol.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 21 | embedding_search_query_frames_natural_language_with_code_prefix (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context_no_semantic | move code to a different file | 12 | different_bool_values_produce_different_hash (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | for_each_embedding_batch (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | miss | for_file (crates/codelens-mcp/src/tools/session/metrics_config.rs) |
| get_ranked_context_no_semantic | extract lines of code into a new function | 9 | new (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | push_unique_string (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | mutation gate preflight check before editing | 10 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context_no_semantic | detect if client is Claude Code or Codex | 8 | detect_frameworks (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | exclude directories from indexing | 4 | from_path (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | truncate response when too large | miss | ToolCallResponse (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | find all functions that call a given function | 6 | get_callers (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | 5 | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | dirs_fallback (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | search for a symbol by name | 4 | make_symbol_id (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | handle semantic search tool request | 10 | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | relative_path (scripts/artifact_maintenance.py) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | raw_visible_tool_entries (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 5 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 17 | SymbolKind (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | 14 | for_each_embedding_batch (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | current_project_scope (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | Relation type for symbol dependencies | miss | SymbolInfo (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | workflow delegation to lower level tools | miss | default_risk_level (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | diagnose file issues and unresolved references | 5 | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | strip empty null fields from JSON response | 4 | from_json (crates/codelens-mcp/src/session_context.rs) |
| get_ranked_context_no_semantic | format structured pretty print response | 4 | ToolCallResponse (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | WorkflowFirst profile for agents | miss | ToolProfile (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | rename a variable or function across the project | 4 | rename (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | search code by natural language query | 4 | is_natural_language_query (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context | read input from stdin line by line | 11 | read_line_at (crates/codelens-engine/src/file_ops/mod.rs) |
| get_ranked_context | parse source code into an AST | 5 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | categorize a function by its purpose | miss | parse_function_parts (crates/codelens-engine/src/inline.rs) |
| get_ranked_context | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | skip comments and string literals during search | miss | extract_comment_body (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | mutation gate preflight check before editing | 6 | mutation_gate (crates/codelens-mcp/src/main.rs) |
| get_ranked_context | truncate response when too large | 8 | truncate_body_preview (crates/codelens-mcp/src/tools/symbols.rs) |
| get_ranked_context | record which files were recently accessed | 4 | record_file_access (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | find all functions that call a given function | 7 | get_callers (crates/codelens-engine/src/call_graph.rs) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | store embedding vectors in sqlite database | 23 | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 7 | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_tool_in_preset (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | determine which language config to use for a file | 27 | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | get current timestamp in milliseconds | 11 | get_fresh_file_by_mtime (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context | diagnose file issues and unresolved references | 4 | unresolved_reference_check (crates/codelens-mcp/src/tools/reports/verifier_reports.rs) |
| get_ranked_context | WorkflowFirst profile for agents | miss | default_budget_for_profile (crates/codelens-mcp/src/tool_defs/presets.rs) |

