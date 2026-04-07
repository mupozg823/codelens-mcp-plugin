# Paper Benchmark Summary

- Harness report: `/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/promotion-gates/v9-product-fresh/v9-product/harness-eval.json`
- Retrieval report: `/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/promotion-gates/v9-product-fresh/v9-product/embedding-quality.json`
- Primary mode: `routed-on`
- Source kind: `real-session`

## Headline Metrics

| Metric | Value |
|---|---:|
| Task Success Rate | 100.0% |
| Tokens per Successful Task | 6124.7 |
| Latency per Successful Task (ms) | 512.3 |
| get_ranked_context MRR@10 | 0.683 |

## Harness Cohort

| Field | Value |
|---|---:|
| Entries after mode/filter | 20 |
| Selected entries | 16 |
| Source breakdown | `{"real-session": 16, "synthetic": 4}` |
| Successful tasks | 16 |
| Acceptance pass rate | n/a |
| Verify pass rate | n/a |
| Avg quality score | 0.621 |

## Retrieval Support

| Metric | Value |
|---|---:|
| Embedding model | `v9-product` |
| Dataset size | 89 |
| get_ranked_context MRR@10 | 0.683 |
| Lexical-only MRR@10 | 0.567 |
| Hybrid MRR delta | +0.116 |
| Hybrid Acc@1 delta | +9.0% |

## Protocol

- Main benchmark is harness task completion under `routed-on` mode.
- Real-session entries are preferred; synthetic entries are used only when real-session data is absent.
- Retrieval support metric is `get_ranked_context MRR@10` from the runtime benchmark.
- Token and latency metrics are reported per successful task, not per attempted task.

