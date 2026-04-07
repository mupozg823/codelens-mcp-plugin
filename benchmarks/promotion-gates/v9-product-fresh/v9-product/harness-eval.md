# CodeLens Harness Evaluation

## Summary

| Metric | Value |
|---|---|
| Binary | /Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp |
| Synthetic entries | 12 |
| Real-session entries | 20 |
| Policy-input real sessions | 20 |
| Excluded non-qualifying real sessions | 0 |
| Representative repos | 1 |
| Task summaries | 11 |
| Baseline workflow savings | 63.8% |
| Baseline low-level chain | 8 -> 0 |
| Baseline avg bootstrap tokens | 8351 |
| Baseline direct-composite overhead | 1944.4% |
| Point lookup regression | Context retrieval remains worse than native point lookup |

## Task-by-Task Results

| Repo | Task Kind | Mode | Source | Success | Acceptance | Verify | Quality | Total Tokens | Bootstrap | Calls | Low-level Chain | Elapsed(ms) | Policy |
|---|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---|
| Next.js app with AGENTS | impact/reviewer | routed-on | real-session | True | None | None | 0.75 | 4149 | 3555 | 3 | 1 | 3972 | prefer_native_baseline |
| Rust MCP repo | impact/reviewer | baseline | synthetic | True | None | None | - | 3022 | 0 | 3 | 3 | 283 | prefer_codelens_after_bootstrap |
| Rust MCP repo | impact/reviewer | naive-on | synthetic | True | None | None | - | 400 | 0 | 1 | 0 | 402 | prefer_codelens_after_bootstrap |
| Rust MCP repo | impact/reviewer | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 0 | 0 | prefer_codelens_after_bootstrap |
| Rust MCP repo | impact/reviewer | routed-on | real-session | True | None | None | 0.793 | 24996 | 22428 | 7 | 0 | 869 | prefer_codelens_after_bootstrap |
| Rust MCP repo | impact/reviewer | routed-on | synthetic | unsupported | None | None | - | 0 | 0 | 0 | 0 | - | prefer_codelens_after_bootstrap |
| Rust MCP repo | onboarding/planning | baseline | synthetic | True | None | None | - | 2690 | 0 | 2 | 2 | 178 | prefer_codelens_after_bootstrap |
| Rust MCP repo | onboarding/planning | naive-on | synthetic | True | None | None | - | 482 | 0 | 1 | 0 | 662 | prefer_codelens_after_bootstrap |
| Rust MCP repo | onboarding/planning | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 0 | 0 | prefer_codelens_after_bootstrap |
| Rust MCP repo | onboarding/planning | routed-on | real-session | True | None | None | 0.075 | 11305 | 11214 | 4 | 0 | 210 | prefer_codelens_after_bootstrap |
| Rust MCP repo | onboarding/planning | routed-on | synthetic | unsupported | None | None | - | 0 | 0 | 0 | 0 | - | prefer_codelens_after_bootstrap |
| Rust MCP repo | refactor preflight | baseline | synthetic | True | None | None | - | 938 | 0 | 3 | 3 | 515 | prefer_codelens_after_bootstrap |
| Rust MCP repo | refactor preflight | naive-on | synthetic | True | None | None | - | 477 | 0 | 1 | 0 | 2531 | prefer_codelens_after_bootstrap |
| Rust MCP repo | refactor preflight | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 1 | 0 | prefer_codelens_after_bootstrap |
| Rust MCP repo | refactor preflight | routed-on | real-session | True | None | None | 0.075 | 7201 | 7110 | 6 | 1 | 252 | prefer_codelens_after_bootstrap |
| Rust MCP repo | refactor preflight | routed-on | synthetic | unsupported | None | None | - | 0 | 0 | 0 | 0 | - | prefer_codelens_after_bootstrap |
| Rust MCP repo | simple local lookup/edit | baseline | real-session | True | True | True | 1.0 | 0 | 0 | 0 | 0 | 0 | avoid_codelens_for_simple_local_lookup |
| Rust MCP repo | simple local lookup/edit | baseline | real-session | True | True | True | 1.0 | 11214 | 11214 | 1 | 0 | 0 | avoid_codelens_for_simple_local_lookup |
| Rust MCP repo | simple local lookup/edit | baseline | synthetic | True | None | None | - | 12068 | 0 | 3 | 0 | 20 | avoid_codelens_for_simple_local_lookup |
| Rust MCP repo | simple local lookup/edit | naive-on | synthetic | True | None | None | - | 8796 | 0 | 3 | 0 | 173 | avoid_codelens_for_simple_local_lookup |
| Rust MCP repo | simple local lookup/edit | routed-on | synthetic | True | None | None | - | 12068 | 0 | 0 | 0 | 20 | avoid_codelens_for_simple_local_lookup |
| Next.js app with AGENTS | impact/reviewer | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 0 | 0 | prefer_routed_codelens |
| Next.js app with AGENTS | onboarding/planning | baseline | real-session | True | None | None | - | 0 | 0 | 0 | 0 | 0 | prefer_native_baseline |
| Next.js app with AGENTS | onboarding/planning | baseline | real-session | True | None | None | 0.3 | 11214 | 11214 | 1 | 1 | 0 | prefer_native_baseline |
| Next.js app with AGENTS | refactor preflight | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 1 | 0 | prefer_native_baseline |
| Next.js app with AGENTS | refactor preflight | routed-on | real-session | True | None | None | 0.754 | 11855 | 11214 | 4 | 0 | 835 | prefer_native_baseline |
| Next/Electron app | impact/reviewer | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 1 | 0 | prefer_native_baseline |
| Next/Electron app | impact/reviewer | routed-on | real-session | True | None | None | 0.887 | 12140 | 11214 | 4 | 1 | 276 | prefer_native_baseline |
| Next/Electron app | onboarding/planning | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 1 | 0 | prefer_native_baseline |
| Next/Electron app | onboarding/planning | routed-on | real-session | True | None | None | 0.887 | 12100 | 11214 | 4 | 0 | 254 | prefer_native_baseline |
| Next/Electron app | refactor preflight | routed-on | real-session | True | None | None | - | 0 | 0 | 0 | 1 | 0 | prefer_native_baseline |
| Next/Electron app | refactor preflight | routed-on | real-session | True | None | None | 0.75 | 14249 | 11214 | 9 | 0 | 1529 | prefer_native_baseline |

## Where CodeLens Helped

- Rust MCP repo / impact/reviewer: `prefer_codelens_after_bootstrap` (baseline=3022, naive=400, routed=8332, confidence=high)
- Rust MCP repo / onboarding/planning: `prefer_codelens_after_bootstrap` (baseline=2690, naive=482, routed=3768, confidence=high)
- Rust MCP repo / refactor preflight: `prefer_codelens_after_bootstrap` (baseline=938, naive=477, routed=2400, confidence=high)
- Next.js app with AGENTS / impact/reviewer: `prefer_routed_codelens` (baseline=0, naive=0, routed=0, confidence=low)

## Where CodeLens Hurt

- Next.js app with AGENTS / impact/reviewer: `prefer_native_baseline` (baseline=0, naive=0, routed=4149, confidence=low)
- Rust MCP repo / simple local lookup/edit: `avoid_codelens_for_simple_local_lookup` (baseline=7761, naive=8796, routed=12068, confidence=high)
- Next.js app with AGENTS / onboarding/planning: `prefer_native_baseline` (baseline=5607, naive=0, routed=0, confidence=low)
- Next.js app with AGENTS / refactor preflight: `prefer_native_baseline` (baseline=0, naive=0, routed=5928, confidence=low)
- Next/Electron app / impact/reviewer: `prefer_native_baseline` (baseline=0, naive=0, routed=6070, confidence=low)
- Next/Electron app / onboarding/planning: `prefer_native_baseline` (baseline=0, naive=0, routed=6050, confidence=low)
- Next/Electron app / refactor preflight: `prefer_native_baseline` (baseline=0, naive=0, routed=7124, confidence=low)

## Needs More Data

- Next.js app with AGENTS / impact/reviewer: confidence=low, unsupported=baseline, naive-on, failing=-
- Next.js app with AGENTS / impact/reviewer: confidence=low, unsupported=baseline, naive-on, failing=-
- Next.js app with AGENTS / onboarding/planning: confidence=low, unsupported=naive-on, routed-on, failing=-
- Next.js app with AGENTS / refactor preflight: confidence=low, unsupported=baseline, naive-on, failing=-
- Next/Electron app / impact/reviewer: confidence=low, unsupported=baseline, naive-on, failing=-
- Next/Electron app / onboarding/planning: confidence=low, unsupported=baseline, naive-on, failing=-
- Next/Electron app / refactor preflight: confidence=low, unsupported=baseline, naive-on, failing=-

## Recommended Routing Rules

- Next.js app with AGENTS / impact/reviewer: `prefer_native_baseline` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=low, unsupported=baseline,naive-on)
- Rust MCP repo / impact/reviewer: `prefer_codelens_after_bootstrap` — use CodeLens for multi-file reasoning, but only after the session is already warm or the task spans several steps
- Rust MCP repo / onboarding/planning: `prefer_codelens_after_bootstrap` — use CodeLens for multi-file reasoning, but only after the session is already warm or the task spans several steps
- Rust MCP repo / refactor preflight: `prefer_codelens_after_bootstrap` — use CodeLens for multi-file reasoning, but only after the session is already warm or the task spans several steps
- Rust MCP repo / simple local lookup/edit: `avoid_codelens_for_simple_local_lookup` — stay native with rg/read/test; do not bootstrap CodeLens for point lookup or already-local edits
- Next.js app with AGENTS / impact/reviewer: `prefer_routed_codelens` — start with deferred workflow tools, then expand evidence/primitive tiers only as needed (confidence=low, unsupported=baseline,naive-on)
- Next.js app with AGENTS / onboarding/planning: `prefer_native_baseline` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=low, unsupported=naive-on,routed-on)
- Next.js app with AGENTS / refactor preflight: `prefer_native_baseline` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=low, unsupported=baseline,naive-on)
- Next/Electron app / impact/reviewer: `prefer_native_baseline` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=low, unsupported=baseline,naive-on)
- Next/Electron app / onboarding/planning: `prefer_native_baseline` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=low, unsupported=baseline,naive-on)
- Next/Electron app / refactor preflight: `prefer_native_baseline` — default to native rg/read/test and only escalate to CodeLens when the task becomes multi-file or reviewer-heavy (confidence=low, unsupported=baseline,naive-on)
