# CodeLens Harness Routing Policy

| Field | Value |
|---|---|
| Scope | agent |
| Agent | claude |
| Source report | /tmp/harness-eval-agent-split.json |
| Binary | /Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp |
| Generated at | 2026-04-04T17:36:28 |

## Global Rules

| Task Kind | Policy | Consensus | Explanation |
|---|---|---|---|
| impact/reviewer | prefer_codelens_after_bootstrap | majority (2/3) | Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy. |
| onboarding/planning | prefer_codelens_after_bootstrap | majority (2/3) | Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy. |
| refactor preflight | prefer_codelens_after_bootstrap | unanimous (3/3) | Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy. |
| simple local lookup/edit | avoid_codelens_for_simple_local_lookup | majority (2/3) | Do not bootstrap CodeLens for point lookups or already-local single-file edits. |

## Repo Overrides

| Repo | Task Kind | Policy | Confidence | Explanation |
|---|---|---|---|---|
| Next/Electron app | impact/reviewer | prefer_routed_codelens | medium | Start with deferred workflow tools and expand evidence or primitive tiers only if needed. |
| Next.js app with AGENTS | onboarding/planning | prefer_native_baseline | medium | Stay on native rg/read/test by default and escalate to CodeLens only if the task broadens. |
| Next/Electron app | simple local lookup/edit | native_or_naive_both_ok_but_default_native | medium | Native is the default, but an opportunistic direct CodeLens call is acceptable if it avoids extra manual search. |

## Suggested AGENTS Snippet

- `impact/reviewer`: `prefer_codelens_after_bootstrap`
  Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.
- `onboarding/planning`: `prefer_codelens_after_bootstrap`
  Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.
- `refactor preflight`: `prefer_codelens_after_bootstrap`
  Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.
- `simple local lookup/edit`: `avoid_codelens_for_simple_local_lookup`
  Do not bootstrap CodeLens for point lookups or already-local single-file edits.

Repo-specific exceptions:
- `Next/Electron app / impact/reviewer`: `prefer_routed_codelens`
  Start with deferred workflow tools and expand evidence or primitive tiers only if needed.
- `Next.js app with AGENTS / onboarding/planning`: `prefer_native_baseline`
  Stay on native rg/read/test by default and escalate to CodeLens only if the task broadens.
- `Next/Electron app / simple local lookup/edit`: `native_or_naive_both_ok_but_default_native`
  Native is the default, but an opportunistic direct CodeLens call is acceptable if it avoids extra manual search.
