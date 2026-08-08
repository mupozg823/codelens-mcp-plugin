# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime backend: `not_loaded`, preference=`cpu`, max_length=`256`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset size: 112
- Ranking cutoff: top-10
- Requested methods: `get_ranked_context`
- Workers: 1
- Method workers: 1
- Batch size: 1
- Query cache probe: enabled

## Timings

| Phase | Wall ms |
|---|---:|
| total | 102092.1 |
| dataset_load | 1.5 |
| get_capabilities | 169.1 |
| index_embeddings | 5280.9 |
| query_cache_probe | 2247.6 |

## Metrics

| Method | MRR@10 | Recall@10 | Acc@1 | Acc@3 | Acc@5 | Method wall ms | Calls | Avg ms | P95 ms | Avg batch ms | P95 batch ms | Avg bytes | P95 bytes | Avg tokens | P95 tokens |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| get_ranked_context | 0.745 | 88% | 65% | 83% | 86% | 94378.1 | 112 | 842.1 | 1447.3 | n/a | n/a | 32740 | 65896 | 8185 | 16474 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Recall | Acc@1 | Acc@3 | Acc@5 | Avg ms | P95 ms | Avg batch ms | P95 batch ms | Avg bytes | P95 bytes | Avg tokens | P95 tokens |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| get_ranked_context | identifier | 33 | 0.970 | 97% | 97% | 97% | 97% | 67.9 | 78.2 | n/a | n/a | 8411 | 37727 | 2102 | 9431 |
| get_ranked_context | issue_to_edit | 11 | 0.774 | 82% | 73% | 82% | 82% | 1163.8 | 1409.3 | n/a | n/a | 50112 | 71518 | 12528 | 17879 |
| get_ranked_context | natural_language | 54 | 0.620 | 80% | 50% | 72% | 76% | 1151.5 | 1447.3 | n/a | n/a | 43577 | 69942 | 10894 | 17485 |
| get_ranked_context | short_phrase | 14 | 0.673 | 100% | 43% | 93% | 100% | 1221.0 | 1642.9 | n/a | n/a | 34638 | 58684 | 8659 | 14671 |

## Query Cache Probe

| Query | First ms | Second ms | First tier | Second tier | Hit observed |
|---|---:|---:|---|---|---|
| rename a variable or function across the project | 993.4 | 1253.2 | exact | exact | True |

## Ranker Diagnostics

| Query type | Status | Count |
|---|---|---:|
| all | hybrid_candidate_missing | 8 |
| all | hybrid_hit | 104 |
| identifier | hybrid_candidate_missing | 1 |
| identifier | hybrid_hit | 32 |
| issue_to_edit | hybrid_candidate_missing | 1 |
| issue_to_edit | hybrid_hit | 10 |
| natural_language | hybrid_candidate_missing | 6 |
| natural_language | hybrid_hit | 48 |
| short_phrase | hybrid_hit | 14 |

## Ranker Diagnostic Details

| Query type | Status | Query | Expected | Semantic rank | Hybrid rank | Hybrid top candidate | Cause candidates |
|---|---|---|---|---:|---:|---|---|
| natural_language | hybrid_candidate_missing | parse source code into an AST | parse_symbols | miss | miss | parse_commits (crates/codelens-engine/src/coupling.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | truncate response when too large | bounded_result_payload | miss | miss | truncate_chars (crates/codelens-mcp/src/dispatch/response_support/truncation.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | resolve which file a called function belongs to | collect_candidate_files | miss | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | split camelCase or snake_case identifier into words | split_identifier | miss | miss | split_identifier (scripts/finetune/collect_camelcase_data.py) | expected_symbol_absent_from_hybrid_candidates |
| identifier | hybrid_candidate_missing | suggestion_reasons_for | suggestion_reasons_for | miss | miss | suggestion_reasons_for (crates/codelens-mcp/src/tools/suggestions/reasons.rs) | expected_symbol_absent_from_hybrid_candidates |
| issue_to_edit | hybrid_candidate_missing | switch the active tool profile for a session | set_profile | miss | miss | record_profile_switch_for_session (crates/codelens-mcp/src/telemetry/registry/events.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | summarize host skill and memory roots during bootstrap | HostEnvironmentSnapshot | miss | miss | summarize_skill_root (crates/codelens-mcp/src/skill_catalog/scan.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | report embedding index coverage and freshness | embedding_coverage_report_handler | miss | miss | index_info_from_coverage (crates/codelens-mcp/src/dispatch/embedding_coverage.rs) | expected_symbol_absent_from_hybrid_candidates |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| get_ranked_context | start an HTTP server with routes | 16 | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context | parse source code into an AST | miss | parse_commits (crates/codelens-engine/src/coupling.rs) |
| get_ranked_context | get project structure and key files on first load | 54 | get_project_structure (crates/codelens-mcp/src/tools/symbols/inventory.rs) |
| get_ranked_context | skip comments and string literals during search | 9 | strict_comments_enabled (crates/codelens-engine/src/embedding/prompt/nl_tokens.rs) |
| get_ranked_context | exclude directories from indexing | 4 | EXCLUDED_DIRS (crates/codelens-engine/src/project/exclusions.rs) |
| get_ranked_context | truncate response when too large | miss | truncate_chars (crates/codelens-mcp/src/dispatch/response_support/truncation.rs) |
| get_ranked_context | record which files were recently accessed | 47 | recent_file_paths (crates/codelens-mcp/src/server/session.rs) |
| get_ranked_context | resolve which file a called function belongs to | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) |
| get_ranked_context | store embedding vectors in sqlite database | 23 | get_embedding (crates/codelens-engine/src/embedding/vec_store.rs) |
| get_ranked_context | split camelCase or snake_case identifier into words | miss | split_identifier (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context | get preflight TTL timeout in milliseconds | 4 | preflight_ttl_seconds (crates/codelens-mcp/src/state/preflight.rs) |
| get_ranked_context | normalize file path relative to project root | 5 | normalize_path (crates/codelens-engine/src/project/paths.rs) |
| get_ranked_context | determine which language config to use for a file | 6 | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | suggestion_reasons_for | miss | suggestion_reasons_for (crates/codelens-mcp/src/tools/suggestions/reasons.rs) |
| get_ranked_context | preflight a safe rename before applying edits | 84 | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | switch the active tool profile for a session | miss | record_profile_switch_for_session (crates/codelens-mcp/src/telemetry/registry/events.rs) |
| get_ranked_context | summarize host skill and memory roots during bootstrap | miss | summarize_skill_root (crates/codelens-mcp/src/skill_catalog/scan.rs) |
| get_ranked_context | render remediation text for stale embedding coverage | 12 | remediation_payload (crates/codelens-mcp/src/dispatch/embedding_coverage/freshness.rs) |
| get_ranked_context | report embedding index coverage and freshness | miss | index_info_from_coverage (crates/codelens-mcp/src/dispatch/embedding_coverage.rs) |

