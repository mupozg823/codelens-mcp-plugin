---
name: codelens
description: Use the existing CodeLens MCP server for multi-file architecture analysis, change review, call-path tracing, impact checks, and language diagnostics. Use when Codex needs indexed structural evidence across files or modules. Do not use for a single exact text lookup or an already-local one-file edit.
---

# CodeLens

Use CodeLens inside Codex's native loop. The plugin supplies this workflow only; the
host-level `codelens` MCP registration remains the source of truth.

## Workflow

1. Resolve the absolute repository root. If CodeLens tools are deferred, discover
   `codelens prepare_harness_session` with native tool search.
2. Make the first CodeLens call
   `prepare_harness_session(project=<absolute-root>, agent_role=<main|subagent>)`.
   Continue only after the response is bound to the intended project.
3. Select the smallest facade that answers the task:

   - `review`: architecture, changed-file, boundary, dead-code, or duplicate analysis.
   - `graph`: callers, callees, type hierarchy, request trace, or changed-file references.
   - `diagnose`: file, symbol, unresolved-reference, or issue diagnostics.
   - `get_capabilities`: index, LSP, semantic, and runtime readiness checks.

4. Keep returned context bounded. Use native `rg` for exact text and native edit tools
   for mutations; CodeLens supplies evidence and review.
5. Cite file and line evidence, state index or diagnostic limitations, and run the
   narrowest relevant verification after a change.

If the `codelens` MCP dependency is unavailable, report the missing host registration
instead of starting a second server or inventing a fallback endpoint.
