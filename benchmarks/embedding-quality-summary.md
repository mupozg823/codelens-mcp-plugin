# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime backend: `not_loaded`, preference=`coreml_preferred`, max_length=`256`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset size: 84
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.706 | 64% | 74% | 79% | 646.2 |
| get_ranked_context_no_semantic | 0.596 | 51% | 67% | 70% | 55.9 |
| get_ranked_context | 0.740 | 68% | 77% | 81% | 128.0 |
| bm25_symbol_search | 0.642 | 58% | 69% | 71% | 127.6 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | identifier | 24 | 1.000 | 100% | 100% | 100% | 190.0 |
| semantic_search | natural_language | 53 | 0.574 | 49% | 60% | 68% | 837.3 |
| semantic_search | short_phrase | 7 | 0.690 | 57% | 86% | 86% | 763.8 |
| get_ranked_context_no_semantic | identifier | 24 | 1.000 | 100% | 100% | 100% | 38.6 |
| get_ranked_context_no_semantic | natural_language | 53 | 0.432 | 32% | 53% | 58% | 63.2 |
| get_ranked_context_no_semantic | short_phrase | 7 | 0.452 | 29% | 57% | 57% | 60.5 |
| get_ranked_context | identifier | 24 | 1.000 | 100% | 100% | 100% | 37.9 |
| get_ranked_context | natural_language | 53 | 0.629 | 55% | 66% | 72% | 164.2 |
| get_ranked_context | short_phrase | 7 | 0.690 | 57% | 86% | 86% | 163.2 |
| bm25_symbol_search | identifier | 24 | 1.000 | 100% | 100% | 100% | 124.8 |
| bm25_symbol_search | natural_language | 53 | 0.485 | 40% | 57% | 58% | 129.0 |
| bm25_symbol_search | short_phrase | 7 | 0.600 | 57% | 57% | 71% | 126.7 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.144 |
| Acc@1 uplift | +17% |
| Acc@3 uplift | +11% |
| Acc@5 uplift | +11% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| identifier | +0.000 | +0% | +0% | +0% |
| natural_language | +0.197 | +23% | +13% | +13% |
| short_phrase | +0.239 | +29% | +29% | +29% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | rename a variable or function across the project | 6 | rename (crates/codelens-engine/src/lib.rs) |
| semantic_search | find where a symbol is defined in a file | 5 | find_symbol (crates/codelens-engine/src/symbols/api.rs) |
| semantic_search | search code by natural language query | miss | is_natural_language_query (crates/codelens-mcp/src/tools/query_analysis/intent.rs) |
| semantic_search | read input from stdin line by line | miss | read_line_window (crates/codelens-engine/src/file_ops/text_refs.rs) |
| semantic_search | parse source code into an AST | 8 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| semantic_search | build embedding vectors for all symbols | miss | all_with_embeddings (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | categorize a function by its purpose | miss | surface_compiler_input_builder_matches_free_function (crates/codelens-mcp/src/tool_defs/presets.rs) |
| semantic_search | get project structure and key files on first load | 6 | get_project_structure (crates/codelens-engine/src/symbols/index.rs) |
| semantic_search | skip comments and string literals during search | miss | cli_project_arg_skips_flag_values (crates/codelens-mcp/src/cli/mod.rs) |
| semantic_search | mutation gate preflight check before editing | 5 | gate (crates/codelens-mcp/src/mutation/mod.rs) |
| semantic_search | truncate response when too large | miss | truncate_body_preview (crates/codelens-mcp/src/tools/symbols/formatter.rs) |
| semantic_search | find all functions that call a given function | 4 | call_graph (crates/codelens-engine/src/lib.rs) |
| semantic_search | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| semantic_search | measure density of internal edges in a cluster | 8 | get_imports_of (crates/codelens-engine/src/db/ops.rs) |
| semantic_search | store embedding vectors in sqlite database | miss | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| semantic_search | split camelCase or snake_case identifier into words | 5 | split_identifier (scripts/finetune/collect_camelcase_data.py) |
| semantic_search | get overview of all symbols in a file | miss | get_symbols_overview (crates/codelens-engine/src/symbols/api.rs) |
| semantic_search | search for a symbol by name | miss | search_symbols (crates/codelens-tui/src/app.rs) |
| semantic_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/engine_impl.rs) |
| semantic_search | check if a tool requires preflight verification | miss | is_builder_preflight_tool (crates/codelens-mcp/src/tools/session/builder_audit.rs) |
| semantic_search | determine which language config to use for a file | miss | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| semantic_search | select the most relevant symbols for a query | 6 | semantic_query_for_embedding_search (crates/codelens-mcp/src/tools/query_analysis/bridge.rs) |
| get_ranked_context_no_semantic | search code by natural language query | miss | embedding_search_query_frames_natural_language_with_code_prefix (crates/codelens-mcp/src/tools/query_analysis/tests.rs) |
| get_ranked_context_no_semantic | move code to a different file | 10 | remove (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | build embedding vectors for all symbols | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | find near-duplicate code in the codebase | 4 | synthesize_next_actions_detailed (crates/codelens-mcp/src/tools/report_contract.rs) |
| get_ranked_context_no_semantic | categorize a function by its purpose | miss | exported_function_also_offers_references (crates/codelens-mcp/src/tools/symbols/support.rs) |
| get_ranked_context_no_semantic | get project structure and key files on first load | miss | get (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context_no_semantic | watch filesystem for file changes | 20 | for_file (crates/codelens-mcp/src/tools/session/metrics_config/capabilities/guidance.rs) |
| get_ranked_context_no_semantic | extract lines of code into a new function | 8 | inject_into (crates/codelens-mcp/src/limits.rs) |
| get_ranked_context_no_semantic | skip comments and string literals during search | miss | optional_string (crates/codelens-mcp/src/tool_runtime.rs) |
| get_ranked_context_no_semantic | mutation gate preflight check before editing | 6 | RecentPreflight (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | exclude directories from indexing | 16 | from_str (crates/codelens-mcp/src/runtime_types.rs) |
| get_ranked_context_no_semantic | truncate response when too large | miss | Tool (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | resolve which file a called function belongs to | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | store embedding vectors in sqlite database | 10 | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | split camelCase or snake_case identifier into words | 5 | _split_camel (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context_no_semantic | find all occurrences of a word in project files | miss | title_word (crates/codelens-mcp/src/tool_defs/build.rs) |
| get_ranked_context_no_semantic | get overview of all symbols in a file | miss | get_symbols_overview (crates/codelens-engine/src/symbols/api.rs) |
| get_ranked_context_no_semantic | search for a symbol by name | miss | flatten_symbols (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context_no_semantic | get project directory tree structure | miss | get_symbols_for_directory (crates/codelens-engine/src/db/ops.rs) |
| get_ranked_context_no_semantic | handle semantic search tool request | 23 | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | build embedding index for all project symbols | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context_no_semantic | normalize file path relative to project root | miss | relative_path (scripts/artifact_maintenance.py) |
| get_ranked_context_no_semantic | check if a tool requires preflight verification | miss | phase_tool_names_from_label (crates/codelens-mcp/src/tool_defs/mod.rs) |
| get_ranked_context_no_semantic | build a success response with suggested next tools | 4 | success (crates/codelens-mcp/src/protocol.rs) |
| get_ranked_context_no_semantic | determine which language config to use for a file | miss | FileRow (crates/codelens-engine/src/db/mod.rs) |
| get_ranked_context_no_semantic | select the most relevant symbols for a query | 8 | weights_for_query_type (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | classify what kind of symbol this is | 12 | symbol_kind_prior (crates/codelens-engine/src/symbols/ranking.rs) |
| get_ranked_context_no_semantic | parse incoming MCP tool call JSON | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context | rename a variable or function across the project | 4 | rename (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | search code by natural language query | miss | is_natural_language_query (crates/codelens-mcp/src/tools/query_analysis/intent.rs) |
| get_ranked_context | read input from stdin line by line | 8 | read_line_window (crates/codelens-engine/src/file_ops/text_refs.rs) |
| get_ranked_context | parse source code into an AST | 6 | slice_source (crates/codelens-engine/src/symbols/parser.rs) |
| get_ranked_context | build embedding vectors for all symbols | miss | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | categorize a function by its purpose | miss | classify_asset (scripts/generate-release-manifest.py) |
| get_ranked_context | get project structure and key files on first load | miss | get_project_structure (crates/codelens-engine/src/symbols/index.rs) |
| get_ranked_context | mutation gate preflight check before editing | 7 | gate (crates/codelens-mcp/src/mutation/mod.rs) |
| get_ranked_context | truncate response when too large | 10 | truncate_body_preview (crates/codelens-mcp/src/tools/symbols/formatter.rs) |
| get_ranked_context | find all functions that call a given function | 4 | call_graph (crates/codelens-engine/src/lib.rs) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | store embedding vectors in sqlite database | 16 | SqliteVecStore (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | 7 | split_identifier (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context | find all occurrences of a word in project files | 4 | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | get overview of all symbols in a file | miss | get_symbols_overview (crates/codelens-engine/src/symbols/api.rs) |
| get_ranked_context | search for a symbol by name | miss | search_symbols (crates/codelens-tui/src/app.rs) |
| get_ranked_context | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/engine_impl.rs) |
| get_ranked_context | check if a tool requires preflight verification | miss | is_builder_preflight_tool (crates/codelens-mcp/src/tools/session/builder_audit.rs) |
| get_ranked_context | determine which language config to use for a file | miss | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| bm25_symbol_search | rename a variable or function across the project | miss | project_scope_renames_across_files (crates/codelens-engine/src/rename.rs) |
| bm25_symbol_search | search code by natural language query | miss | embedding_search_query_frames_natural_language_with_code_prefix (crates/codelens-mcp/src/tools/query_analysis/tests.rs) |
| bm25_symbol_search | move code to a different file | 5 | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| bm25_symbol_search | start an HTTP server with routes | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| bm25_symbol_search | parse source code into an AST | miss | collect_type_candidates_ast (crates/codelens-engine/src/auto_import.rs) |
| bm25_symbol_search | build embedding vectors for all symbols | miss | all_symbols (benchmarks/lsp-boost-spotcheck.py) |
| bm25_symbol_search | find near-duplicate code in the codebase | miss | find_dead_code (crates/codelens-engine/src/import_graph/dead_code.rs) |
| bm25_symbol_search | categorize a function by its purpose | miss | inline_function (crates/codelens-engine/src/inline.rs) |
| bm25_symbol_search | get project structure and key files on first load | miss | get_project_structure (crates/codelens-mcp/src/tools/symbols/handlers.rs) |
| bm25_symbol_search | watch filesystem for file changes | miss | for_file (crates/codelens-mcp/src/tools/session/metrics_config/capabilities/guidance.rs) |
| bm25_symbol_search | skip comments and string literals during search | miss | extract_nl_tokens_collects_comments_and_string_literals (crates/codelens-engine/src/embedding/tests.rs) |
| bm25_symbol_search | mutation gate preflight check before editing | miss | record_mutation_preflight_gate_denied (crates/codelens-mcp/src/observability/telemetry/session_mutations.rs) |
| bm25_symbol_search | exclude directories from indexing | miss | excludes_agent_worktree_directories (crates/codelens-engine/src/project/tests.rs) |
| bm25_symbol_search | truncate response when too large | miss | truncate_body_preview (crates/codelens-mcp/src/tools/symbols/formatter.rs) |
| bm25_symbol_search | find all functions that call a given function | miss | find_all_word_matches (crates/codelens-engine/src/rename.rs) |
| bm25_symbol_search | resolve which file a called function belongs to | miss | refactor_move_to_file (crates/codelens-mcp/src/tools/composite.rs) |
| bm25_symbol_search | store embedding vectors in sqlite database | miss | register_sqlite_vec (crates/codelens-engine/src/embedding/ffi.rs) |
| bm25_symbol_search | split camelCase or snake_case identifier into words | 9 | words (scripts/finetune/build_nl_augmentation.py) |
| bm25_symbol_search | get overview of all symbols in a file | miss | all_symbols (benchmarks/lsp-boost-spotcheck.py) |
| bm25_symbol_search | search for a symbol by name | miss | symbol_name (scripts/finetune/verify_v6_external.py) |
| bm25_symbol_search | build embedding index for all project symbols | miss | index_from_project (crates/codelens-engine/src/embedding/engine_impl.rs) |
| bm25_symbol_search | check if a tool requires preflight verification | miss | trainable (scripts/finetune/train_lora.py) |
| bm25_symbol_search | build a success response with suggested next tools | 4 | build_suggested_next_calls (crates/codelens-mcp/src/dispatch/response/followups.rs) |
| bm25_symbol_search | determine which language config to use for a file | miss | determine_semantic_search_status (crates/codelens-mcp/src/tools/session/metrics_config/capabilities/snapshot.rs) |
| bm25_symbol_search | select the most relevant symbols for a query | 6 | semantic_scores_for_query (crates/codelens-mcp/src/tools/symbols/analyzer.rs) |
| bm25_symbol_search | parse incoming MCP tool call JSON | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |

