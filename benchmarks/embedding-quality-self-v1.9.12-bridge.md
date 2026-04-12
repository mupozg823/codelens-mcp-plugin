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
| semantic_search | 0.798 | 71% | 88% | 91% | 507.5 |
| get_ranked_context_no_semantic | 0.614 | 53% | 66% | 74% | 39.4 |
| get_ranked_context | 0.841 | 76% | 92% | 95% | 135.4 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 31 | 1.000 | 100% | 100% | 100% | 169.8 |
| semantic_search | natural_language | 62 | 0.699 | 58% | 81% | 85% | 649.3 |
| semantic_search | short_phrase | 11 | 0.788 | 64% | 100% | 100% | 660.0 |
| get_ranked_context_no_semantic | identifier | 31 | 1.000 | 100% | 100% | 100% | 27.7 |
| get_ranked_context_no_semantic | natural_language | 62 | 0.455 | 35% | 52% | 60% | 44.8 |
| get_ranked_context_no_semantic | short_phrase | 11 | 0.424 | 18% | 55% | 82% | 41.9 |
| get_ranked_context | identifier | 31 | 1.000 | 100% | 100% | 100% | 28.0 |
| get_ranked_context | natural_language | 62 | 0.770 | 66% | 87% | 92% | 169.2 |
| get_ranked_context | short_phrase | 11 | 0.788 | 64% | 100% | 100% | 247.1 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.226 |
| Acc@1 uplift | +23% |
| Acc@3 uplift | +26% |
| Acc@5 uplift | +21% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.315 | +31% | +35% | +32% |
| short_phrase | +0.364 | +45% | +45% | +18% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | find where a symbol is defined in a file | 5 | find_symbol (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | categorize a function by its purpose | miss | parse_function_parts (crates/codelens-engine/src/inline.rs) |
| semantic_search | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| semantic_search | skip comments and string literals during search | miss | search_symbols_fts (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | resolve which file a called function belongs to | 10 | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| semantic_search | measure density of internal edges in a cluster | 9 | insert_at_line (crates/codelens-engine/src/file_ops/writer.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | get preflight TTL timeout in milliseconds | 4 | preflight_ttl_seconds (crates/codelens-mcp/src/state/preflight.rs) |
| semantic_search | assess mutation readiness before code changes | 5 | is_symbol_aware_mutation_tool (crates/codelens-mcp/src/mutation_gate.rs) |
| semantic_search | WorkflowFirst profile for agents | miss | read_only_daemon_rejects_mutation_even_with_mutating_profile (crates/codelens-mcp/src/integration_tests/protocol.rs) |
| get_ranked_context_no_semantic | search code by natural language query | 21 | embedding_search_query_frames_natural_language_with_code_prefix (crates/codelens-mcp/src/tools/query_analysis.rs) |
| get_ranked_context_no_semantic | move code to a different file | 12 | different_bool_values_produce_different_hash (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | embeddings_for_scored_chunks (crates/codelens-engine/src/embedding/vec_store.rs) |
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
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | is_deferred_control_tool (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | find similar code snippets using embeddings | 5 | find_similar_code (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 17 | SymbolKind (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | upsert embedding vector for a symbol | 14 | embeddings_for_scored_chunks (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | get current timestamp in milliseconds | miss | current_project_scope (crates/codelens-mcp/src/state.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | Relation type for symbol dependencies | miss | SymbolInfo (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | workflow delegation to lower level tools | miss | default_risk_level (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | diagnose file issues and unresolved references | 5 | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | strip empty null fields from JSON response | 4 | from_json (crates/codelens-mcp/src/session_context.rs) |
| get_ranked_context_no_semantic | format structured pretty print response | 4 | ToolCallResponse (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | WorkflowFirst profile for agents | miss | ToolProfile (crates/codelens-mcp/src/tool_defs/presets.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | categorize a function by its purpose | miss | functions (benchmarks/multi-repo-eval.py) |
| get_ranked_context | get project structure and key files on first load | 16 | get_project_structure (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | resolve which file a called function belongs to | 4 | collect_candidate_files (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | store embedding vectors in sqlite database | 4 | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 5 | split_identifier (scripts/finetune/convert_csn_codelens_v2.py) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | WorkflowFirst profile for agents | miss | default_budget_for_profile (crates/codelens-mcp/src/tool_defs/presets.rs) |

