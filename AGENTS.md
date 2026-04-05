# Rust MCP repo

## Repo Contracts

This repo keeps project-specific agent policy inside versioned repo-local documents.
Global defaults still come from `~/.codex/AGENTS.md`.

- `PROJECT_AGENT_POLICY.md` — shared agent roles, routing, and non-goals
- `EVAL_CONTRACT.md` — verification and benchmark interpretation
- `HARNESS_MODES.md` — native vs CodeLens vs verifier vs async job paths
- `DEVELOPMENT_PIPELINE.md` — local, build, CI, and release flow
- `HARNESS_ARCHITECTURE.md` — harness optimization structure and architectural target

## Stack

- `rust`

## Local Guidance

- Keep diffs minimal and prefer existing modules over new wrappers.
- Use native `rg/read/test` for trivial point lookups.
- Escalate to CodeLens workflow tools only when the task becomes multi-file, reviewer-heavy, or refactor-sensitive.
- Follow `EVAL_CONTRACT.md` for what must be run before calling work done.

<!-- CODELENS_REPO_ROUTING_POLICY:BEGIN -->
## CodeLens Repo Routing Policy

_Generated from `/Users/bagjaeseog/.codex/harness/reports/refreshes/2026-04-04-231408-routing-policy-refresh-live.json` on 2026-04-04T23:14:08 for `codelens-mcp-plugin`_

_Derived from the authoritative Codex policy JSON. This repo section is non-authoritative._

Repo-specific routing rules:
- no repo-specific exceptions; follow the global CodeLens routing policy.

Operational guidance:
- prefer the global CodeLens routing policy unless a repo-specific rule above is more restrictive.
- keep simple point lookups on native rg/read/test when the repo rule says native is preferred.
- use verifier-first CodeLens workflow for refactor/impact tasks only when the routing threshold is crossed.
<!-- CODELENS_REPO_ROUTING_POLICY:END -->











