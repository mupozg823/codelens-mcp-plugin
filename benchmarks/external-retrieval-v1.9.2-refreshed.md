# External Retrieval Summary

- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-core/models/codesearch/model.onnx`
- Dataset: `/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/external-retrieval-dataset.json`
- Available repos: 2 / 2
- Evidence sufficient: `True`
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.279 | 25% | 29% | 33% | 1194.0 |
| get_ranked_context | 0.504 | 42% | 54% | 67% | 253.2 |
| get_ranked_context_no_semantic | 0.434 | 33% | 54% | 54% | 166.1 |

## Repo Evidence

| Repo | Exists | Queries | Indexed symbols | Isolation |
|---|---|---:|---:|---|
| claw-dev | True | 12 | 97 | no |
| rg-family-clone | True | 12 | 391 | no |

