# Role Retrieval Summary

- Project: `/var/folders/z0/0tcbss795xsdp75jmr_t9_kh0000gn/T/codelens-role-retrieval-mcl4_xn7/codelens-mcp-plugin`
- Binary: `/Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp`
- Runtime model: `12L`, `32MB`, `sha256:ef1d1e9cfa72e492`
- Runtime model path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-core/models/codesearch/model.onnx`
- Dataset: `/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/role-retrieval-dataset.json`
- Dataset size: 70
- Ranking cutoff: top-10

## Metrics

| Method | MRR@10 | Acc@1 | Acc@3 | Acc@5 | Avg ms |
|---|---:|---:|---:|---:|---:|
| semantic_search | 0.741 | 69% | 80% | 81% | 507.0 |
| get_ranked_context | 0.819 | 76% | 87% | 89% | 139.9 |
| get_ranked_context_no_semantic | 0.693 | 67% | 70% | 70% | 36.6 |

