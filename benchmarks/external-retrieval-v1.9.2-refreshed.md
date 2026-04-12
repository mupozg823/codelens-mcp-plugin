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
| semantic_search | 0.463 | 33% | 54% | 67% | 155.2 |
| get_ranked_context | 0.753 | 62% | 83% | 92% | 124.7 |
| get_ranked_context_no_semantic | 0.597 | 46% | 75% | 75% | 18.8 |

## Repo Evidence

| Repo | Exists | Queries | Indexed symbols | Isolation | Ignored paths |
|---|---|---:|---:|---|---|
| claw-dev | True | 12 | 413 | /var/folders/z0/0tcbss795xsdp75jmr_t9_kh0000gn/T/codelens-external-retrieval-ee5ve8sz/claw-dev | Leonxlnx-claude-code |
| rg-family-clone | True | 12 | 4885 | no | — |

