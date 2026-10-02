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
   `prepare_harness_session(project=<absolute-root>, detail="compact")`.
   Confirm the effective project and reuse that binding for the session.
   Declare observed host capabilities and available tool names when known;
   model names alone do not establish host capabilities.
3. Select the smallest facade that answers the task:

   - `review`: architecture, changed-file, boundary, dead-code, or duplicate analysis.
   - `graph`: callers, callees, type hierarchy, request trace, or changed-file references.
   - `diagnose`: file, symbol, unresolved-reference, or issue diagnostics.
   - `get_capabilities`: index, LSP, semantic, and runtime readiness checks.
   - `search` / `overview`, when exposed: bounded symbol/context retrieval and file structure.

4. Call only tools exposed to this host; use native tool search for deferred tools.
   If a needed tool is unavailable, use native evidence instead of repeating discovery.
   Keep results bounded and reuse existing evidence; cache hits still add host rounds.
   Use native `rg` for exact text and native edit tools for mutations.
5. Cite file and line evidence, state index or diagnostic limitations, and run the
   narrowest relevant verification after a change.

If the `codelens` MCP dependency or a diagnostic capability is unavailable, continue
with native tools and report the limitation. Do not start a second server or invent
a fallback endpoint. Bootstrap again only after a binding or connection change.
