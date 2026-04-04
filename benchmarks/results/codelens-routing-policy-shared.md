# CodeLens Harness Routing Policy

| Field | Value |
|---|---|
| Scope | shared |
| Agent | shared |
| Source report | /tmp/harness-eval-agent-split.json |
| Binary | /Users/bagjaeseog/codelens-mcp-plugin/target/release/codelens-mcp |
| Generated at | 2026-04-04T17:36:28 |

## Global Rules

| Task Kind | Policy | Consensus | Explanation |
|---|---|---|---|
| impact/reviewer | prefer_codelens_after_bootstrap | unanimous (3/3) | Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy. |
| onboarding/planning | prefer_codelens_after_bootstrap | unanimous (3/3) | Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy. |
| refactor preflight | prefer_codelens_after_bootstrap | unanimous (3/3) | Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy. |
| simple local lookup/edit | native_or_naive_both_ok_but_default_native | majority (2/3) | Native is the default, but an opportunistic direct CodeLens call is acceptable if it avoids extra manual search. |

## Repo Overrides

| Repo | Task Kind | Policy | Confidence | Explanation |
|---|---|---|---|---|
| Next.js app with AGENTS | simple local lookup/edit | avoid_codelens_for_simple_local_lookup | medium | Do not bootstrap CodeLens for point lookups or already-local single-file edits. |

## Suggested AGENTS Snippet

- `impact/reviewer`: `prefer_codelens_after_bootstrap`
  Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.
- `onboarding/planning`: `prefer_codelens_after_bootstrap`
  Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.
- `refactor preflight`: `prefer_codelens_after_bootstrap`
  Use native exploration for the very first local step, then switch to CodeLens once the task becomes multi-file or reviewer-heavy.
- `simple local lookup/edit`: `native_or_naive_both_ok_but_default_native`
  Native is the default, but an opportunistic direct CodeLens call is acceptable if it avoids extra manual search.

Repo-specific exceptions:
- `Next.js app with AGENTS / simple local lookup/edit`: `avoid_codelens_for_simple_local_lookup`
  Do not bootstrap CodeLens for point lookups or already-local single-file edits.
