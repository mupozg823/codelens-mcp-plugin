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
| total | 104782.2 |
| dataset_load | 1.7 |
| get_capabilities | 530.2 |
| index_embeddings | 7224.2 |
| query_cache_probe | 2239.8 |

## Metrics

| Method | MRR@10 | Recall@10 | Acc@1 | Acc@3 | Acc@5 | Method wall ms | Calls | Avg ms | P95 ms | Avg batch ms | P95 batch ms | Avg bytes | P95 bytes | Avg tokens | P95 tokens |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| get_ranked_context | 0.745 | 88% | 65% | 83% | 86% | 94767.3 | 112 | 845.6 | 1489.0 | n/a | n/a | 35347 | 65934 | 8836 | 16483 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Recall | Acc@1 | Acc@3 | Acc@5 | Avg ms | P95 ms | Avg batch ms | P95 batch ms | Avg bytes | P95 bytes | Avg tokens | P95 tokens |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| get_ranked_context | identifier | 33 | 0.970 | 97% | 97% | 97% | 97% | 66.5 | 77.3 | n/a | n/a | 8568 | 37727 | 2142 | 9431 |
| get_ranked_context | issue_to_edit | 11 | 0.774 | 82% | 73% | 82% | 82% | 1189.2 | 1780.7 | n/a | n/a | 50112 | 71518 | 12528 | 17879 |
| get_ranked_context | natural_language | 54 | 0.621 | 80% | 50% | 72% | 76% | 1113.6 | 1449.7 | n/a | n/a | 47491 | 69942 | 11872 | 17485 |
| get_ranked_context | short_phrase | 14 | 0.673 | 100% | 43% | 93% | 100% | 1378.0 | 2502.7 | n/a | n/a | 40032 | 58684 | 10008 | 14671 |

## Query Cache Probe

| Query | First ms | Second ms | First tier | Second tier | Hit observed |
|---|---:|---:|---|---|---|
| rename a variable or function across the project | 1004.2 | 1234.2 | exact | exact | True |

## Ranker Diagnostics

| Query type | Status | Count |
|---|---|---:|
| all | hybrid_candidate_missing | 6 |
| all | hybrid_hit | 106 |
| identifier | hybrid_candidate_missing | 1 |
| identifier | hybrid_hit | 32 |
| issue_to_edit | hybrid_candidate_missing | 1 |
| issue_to_edit | hybrid_hit | 10 |
| natural_language | hybrid_candidate_missing | 4 |
| natural_language | hybrid_hit | 50 |
| short_phrase | hybrid_hit | 14 |

## Ranker Diagnostic Details

| Query type | Status | Query | Expected | Semantic rank | Hybrid rank | Hybrid top candidate | Cause candidates |
|---|---|---|---|---:|---:|---|---|
| natural_language | hybrid_candidate_missing | parse source code into an AST | parse_symbols | miss | miss | parse_commits (crates/codelens-engine/src/coupling.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | truncate response when too large | bounded_result_payload | miss | miss | truncate_chars (crates/codelens-mcp/src/dispatch/response_support/truncation.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | resolve which file a called function belongs to | collect_candidate_files | miss | miss | resolve_module_for_file (crates/codelens-engine/src/import_graph/resolvers.rs) | expected_symbol_absent_from_hybrid_candidates |
| identifier | hybrid_candidate_missing | suggestion_reasons_for | suggestion_reasons_for | miss | miss | suggestion_reasons_for (crates/codelens-mcp/src/tools/suggestions/reasons.rs) | expected_symbol_absent_from_hybrid_candidates |
| issue_to_edit | hybrid_candidate_missing | switch the active tool profile for a session | set_profile | miss | miss | record_profile_switch_for_session (crates/codelens-mcp/src/telemetry/registry/events.rs) | expected_symbol_absent_from_hybrid_candidates |
| natural_language | hybrid_candidate_missing | summarize host skill and memory roots during bootstrap | HostEnvironmentSnapshot | miss | miss | summarize_skill_root (crates/codelens-mcp/src/skill_catalog/scan.rs) | expected_symbol_absent_from_hybrid_candidates |

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
| get_ranked_context | split camelCase or snake_case identifier into words | 26 | split_identifier (scripts/finetune/collect_camelcase_data.py) |
| get_ranked_context | get preflight TTL timeout in milliseconds | 4 | preflight_ttl_seconds (crates/codelens-mcp/src/state/preflight.rs) |
| get_ranked_context | normalize file path relative to project root | 5 | normalize_path (crates/codelens-engine/src/project/paths.rs) |
| get_ranked_context | determine which language config to use for a file | 6 | language_for_path (crates/codelens-engine/src/lang_config.rs) |
| get_ranked_context | suggestion_reasons_for | miss | suggestion_reasons_for (crates/codelens-mcp/src/tools/suggestions/reasons.rs) |
| get_ranked_context | preflight a safe rename before applying edits | 84 | apply_edits (crates/codelens-engine/src/rename.rs) |
| get_ranked_context | switch the active tool profile for a session | miss | record_profile_switch_for_session (crates/codelens-mcp/src/telemetry/registry/events.rs) |
| get_ranked_context | summarize host skill and memory roots during bootstrap | miss | summarize_skill_root (crates/codelens-mcp/src/skill_catalog/scan.rs) |
| get_ranked_context | render remediation text for stale embedding coverage | 12 | remediation_payload (crates/codelens-mcp/src/dispatch/embedding_coverage/freshness.rs) |
| get_ranked_context | report embedding index coverage and freshness | 33 | index_info_from_coverage (crates/codelens-mcp/src/dispatch/embedding_coverage.rs) |

