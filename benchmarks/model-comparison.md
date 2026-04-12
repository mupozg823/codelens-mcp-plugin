# Embedding vNext: Policy Freeze + Model Bake-off + Real Rerank

## Phase Order

1. **Policy freeze** — current query/document shaping rules are baseline. No new heuristics.
2. **Model bake-off** — 2-3 candidates against fixed shaping, 4-axis comparison.
3. **Real rerank** — bi-encoder top-K → cross-encoder rerank → final top-N.
4. **Fine-tune decision** — only if generic semantic gap remains after 1-3.

## Current Baseline (policy frozen at 2f934e0)

| Dataset     | Method   | MRR   | Acc@1 | Notes                         |
| ----------- | -------- | ----- | ----- | ----------------------------- |
| Role (70q)  | hybrid   | 0.952 | 91%   | NL framing + structural boost |
| Role (70q)  | semantic | 0.925 | 87%   | NL framing active             |
| Self (105q) | hybrid   | 0.679 | 60%   | ceiling limited by model      |
| Self (105q) | semantic | 0.590 | 55%   | model ceiling                 |

## Model Bake-off Candidates

| Model                                       | Dim | Size  | Max Tokens | Case     | Source             |
| ------------------------------------------- | --- | ----- | ---------- | -------- | ------------------ |
| **MiniLM-L12-CodeSearchNet INT8** (current) | 384 | 32MB  | 128        | lower    | bundled ONNX       |
| **nomic-embed-code-v1**                     | 768 | 135MB | 8192       | preserve | HuggingFace ONNX   |
| **all-MiniLM-L6-v2**                        | 384 | 23MB  | 256        | lower    | fastembed built-in |
| **UniXcoder-base**                          | 768 | 440MB | 512        | preserve | HuggingFace        |

## Bake-off Axes (4)

| Axis          | Metric                             | Acceptable range    |
| ------------- | ---------------------------------- | ------------------- |
| Quality       | MRR@10, Acc@1 (role + self)        | >= current baseline |
| Index time    | seconds for full project reindex   | < 60s for 250 files |
| Query latency | avg ms per search call             | < 500ms warm        |
| Memory        | peak RSS during index + model size | < 500MB peak        |

## Rerank Architecture (Phase 3)

```
Query → bi-encoder → top-3K candidates (cosine)
                         ↓
                  cross-encoder rerank (top-K pairs scored)
                         ↓
                  final top-N results
```

Slot already reserved at search_scored() (embedding/mod.rs:1043).
Cross-encoder candidates: ms-marco-MiniLM-L-6-v2, bge-reranker-base.
