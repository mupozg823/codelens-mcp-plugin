# Role Retrieval Summary

- Project: `/Users/bagjaeseog/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-engine/models/codesearch/model.onnx`
- Dataset: `/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/role-retrieval-dataset.json`
- Dataset size: 70
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.813 | 74% | 89% | 91% | 594.0 |
| get_ranked_context | 0.831 | 76% | 90% | 91% | 147.8 |
| get_ranked_context_no_semantic | 0.586 | 53% | 60% | 61% | 42.0 |

