# Rust MCP repo

## Codex Harness Bootstrap

This AGENTS.md was bootstrap-generated to attach minimal verification defaults and repo-local CodeLens routing guidance.
Global defaults still come from `~/.codex/AGENTS.md`.

## Verification

Before finishing, run:
- `cargo fmt --check`
- `cargo check`
- `cargo test`

## Stack

- `rust`

## Local Guidance

- Keep diffs minimal and prefer existing modules over new wrappers.
- Use native `rg/read/test` for trivial point lookups.
- Escalate to CodeLens workflow tools only when the task becomes multi-file, reviewer-heavy, or refactor-sensitive.

<!-- CODELENS_REPO_ROUTING_POLICY:BEGIN -->
## CodeLens Repo Routing Policy

_Generated from `/Users/bagjaeseog/.codex/harness/reports/refreshes/2026-04-04-043122-routing-policy-refresh.json` on 2026-04-04T04:31:22 for `codelens-mcp-plugin`_

Repo-specific routing rules:
- no repo-specific exceptions; follow the global CodeLens routing policy.

Operational guidance:
- prefer the global CodeLens routing policy unless a repo-specific rule above is more restrictive.
- keep simple point lookups on native rg/read/test when the repo rule says native is preferred.
- use verifier-first CodeLens workflow for refactor/impact tasks only when the routing threshold is crossed.
<!-- CODELENS_REPO_ROUTING_POLICY:END -->










