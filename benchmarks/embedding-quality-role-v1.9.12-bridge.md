# Embedding Quality Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Embedding model: `MiniLM-L12-CodeSearchNet-INT8`
- Runtime backend: `not_loaded`, preference=`coreml_preferred`, max_length=`256`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset size: 70
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.900 | 84% | 96% | 97% | 868.6 |
| get_ranked_context_no_semantic | 0.832 | 79% | 87% | 89% | 123.3 |
| get_ranked_context | 0.962 | 94% | 97% | 99% | 245.9 |

## Query Type Breakdown

| Method | Query type | Count | MRR | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---|---:|---:|---:|---:|---:|---:|
| semantic_search | natural_language | 35 | 0.881 | 83% | 94% | 94% | 900.3 |
| semantic_search | short_phrase | 35 | 0.920 | 86% | 97% | 100% | 836.8 |
| get_ranked_context_no_semantic | natural_language | 35 | 0.843 | 80% | 89% | 89% | 179.7 |
| get_ranked_context_no_semantic | short_phrase | 35 | 0.821 | 77% | 86% | 89% | 66.9 |
| get_ranked_context | natural_language | 35 | 0.960 | 94% | 97% | 97% | 283.7 |
| get_ranked_context | short_phrase | 35 | 0.964 | 94% | 97% | 100% | 208.1 |

## Hybrid Uplift

| KPI | Delta |
|---|---:|
| MRR uplift | +0.130 |
| Acc@1 uplift | +16% |
| Acc@3 uplift | +10% |
| Acc@5 uplift | +10% |

## Hybrid Uplift by Query Type

| Query type | MRR | Acc@1 | Acc@3 | Acc@5 |
|---|---:|---:|---:|---:|
| natural_language | +0.117 | +14% | +9% | +9% |
| short_phrase | +0.143 | +17% | +11% | +11% |

## Misses

| Method | Query | Rank | Top candidate |
|---|---|---:|---|
| semantic_search | search primary implementation | 5 | search (crates/codelens-engine/src/embedding/mod.rs) |
| semantic_search | which symbol is responsible for search | miss | search_symbols (crates/codelens-tui/src/app.rs) |
| semantic_search | which Rust struct type is EmbeddingEngine and holds the ONNX model | 6 | is_static_method_ident_accepts_pascal_and_rejects_snake (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context_no_semantic | search primary implementation | 4 | search_dual (crates/codelens-engine/src/embedding_store.rs) |
| get_ranked_context_no_semantic | which symbol is responsible for search | miss | select_solve_symbols (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context_no_semantic | primary index from project handler | miss | dispatch_tool (crates/codelens-mcp/src/dispatch/mod.rs) |
| get_ranked_context_no_semantic | which entrypoint handles index from project | miss | from_path (crates/codelens-engine/src/symbols/types.rs) |
| get_ranked_context_no_semantic | file watcher primary implementation | miss | index_files_with_retry (crates/codelens-engine/src/watcher.rs) |
| get_ranked_context_no_semantic | which symbol is responsible for file watcher | miss | for_file (crates/codelens-mcp/src/tools/session/metrics_config.rs) |
| get_ranked_context_no_semantic | collect candidate files internal helper | miss | collect_files (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | which helper implements collect candidate files | miss | collect_files (crates/codelens-engine/src/project.rs) |
| get_ranked_context_no_semantic | EmbeddingEngine struct type definition and fields | miss | find_symbol_range (crates/codelens-engine/src/symbols/mod.rs) |
| get_ranked_context | search primary implementation | 4 | search (crates/codelens-engine/src/embedding/mod.rs) |
| get_ranked_context | which symbol is responsible for search | 9 | search_symbols (crates/codelens-tui/src/app.rs) |

