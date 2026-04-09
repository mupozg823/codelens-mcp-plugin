# CodeLens Harness Evaluation

## Summary

| Metric | Value |
|---|---|
| Binary | /Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp |
| Synthetic entries | 12 |
| Real-session entries | 0 |
| Policy-input real sessions | 0 |
| Excluded non-qualifying real sessions | 0 |
| Representative repos | 1 |
| Task summaries | 4 |
| Baseline workflow savings | 63.8% |
| Baseline low-level chain | 8 -> 0 |
| Baseline avg bootstrap tokens | 8351 |
| Baseline direct-composite overhead | 1944.4% |
| Point lookup regression | Context retrieval remains worse than native point lookup |

## Task-by-Task Results

| Repo | Task Kind | Mode | Source | Success | Acceptance | Verify | Quality | Total Tokens | Bootstrap | Calls | Low-level Chain | Elapsed(ms) | Policy |
|---|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---|
| Rust MCP repo | impact/reviewer | baseline | synthetic | True | None | None | - | 2647 | 0 | 3 | 3 | 266 | prefer_naive_codelens |
| Rust MCP repo | impact/reviewer | naive-on | synthetic | True | None | None | - | 400 | 0 | 1 | 0 | 374 | prefer_naive_codelens |
| Rust MCP repo | impact/reviewer | routed-on | synthetic | unsupported | None | None | - | 0 | 0 | 0 | 0 | - | prefer_naive_codelens |
| Rust MCP repo | onboarding/planning | baseline | synthetic | True | None | None | - | 2452 | 0 | 2 | 2 | 172 | prefer_naive_codelens |
| Rust MCP repo | onboarding/planning | naive-on | synthetic | True | None | None | - | 482 | 0 | 1 | 0 | 554 | prefer_naive_codelens |
| Rust MCP repo | onboarding/planning | routed-on | synthetic | unsupported | None | None | - | 0 | 0 | 0 | 0 | - | prefer_naive_codelens |
| Rust MCP repo | refactor preflight | baseline | synthetic | True | None | None | - | 938 | 0 | 3 | 3 | 471 | prefer_naive_codelens |
| Rust MCP repo | refactor preflight | naive-on | synthetic | True | None | None | - | 477 | 0 | 1 | 0 | 2369 | prefer_naive_codelens |
| Rust MCP repo | refactor preflight | routed-on | synthetic | unsupported | None | None | - | 0 | 0 | 0 | 0 | - | prefer_naive_codelens |
| Rust MCP repo | simple local lookup/edit | baseline | synthetic | True | None | None | - | 13587 | 0 | 3 | 0 | 18 | native_or_naive_both_ok_but_default_native |
| Rust MCP repo | simple local lookup/edit | naive-on | synthetic | True | None | None | - | 8770 | 0 | 3 | 0 | 168 | native_or_naive_both_ok_but_default_native |
| Rust MCP repo | simple local lookup/edit | routed-on | synthetic | True | None | None | - | 13587 | 0 | 0 | 0 | 18 | native_or_naive_both_ok_but_default_native |

## Where CodeLens Helped

- Rust MCP repo / impact/reviewer: `prefer_naive_codelens` (baseline=2647, naive=400, routed=0, confidence=low)
- Rust MCP repo / onboarding/planning: `prefer_naive_codelens` (baseline=2452, naive=482, routed=0, confidence=low)
- Rust MCP repo / refactor preflight: `prefer_naive_codelens` (baseline=938, naive=477, routed=0, confidence=low)

## Where CodeLens Hurt

- no hurt segments recorded yet

## Needs More Data

- Rust MCP repo / impact/reviewer: confidence=low, unsupported=routed-on, failing=-
- Rust MCP repo / onboarding/planning: confidence=low, unsupported=routed-on, failing=-
- Rust MCP repo / refactor preflight: confidence=low, unsupported=routed-on, failing=-

## Recommended Routing Rules

- Rust MCP repo / impact/reviewer: `prefer_naive_codelens` — a direct composite call is already worthwhile; routed session overhead is not required (confidence=low, unsupported=routed-on)
- Rust MCP repo / onboarding/planning: `prefer_naive_codelens` — a direct composite call is already worthwhile; routed session overhead is not required (confidence=low, unsupported=routed-on)
- Rust MCP repo / refactor preflight: `prefer_naive_codelens` — a direct composite call is already worthwhile; routed session overhead is not required (confidence=low, unsupported=routed-on)
- Rust MCP repo / simple local lookup/edit: `native_or_naive_both_ok_but_default_native` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=medium, unsupported=-)
