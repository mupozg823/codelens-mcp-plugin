# CodeLens MCP — Agent Flow Specification v1

Status: current public product-flow summary  
Date: 2026-04-21  
Runtime summary: `codelens://design/agent-experience`

This document describes the public agent-flow contract for **CodeLens MCP**.
The point is straightforward: keep the host-visible surface small, make
retrieval compositional instead of repetitive, and make mutation follow
explicit verifier evidence.

## Product naming

- Public product name: **CodeLens MCP**
- Public binary name: `codelens-mcp`
- Public workspace and crate family: `codelens-*`
- Compatibility aliases may exist at runtime, but public docs, install
  guidance, and release language stay CodeLens-first.

## Core user flow

1. Attach `codelens-mcp` to the host.
2. Bootstrap with `prepare_harness_session`.
3. Read the active profile, visible surface, and health summary.
4. Use workflow entrypoints instead of raw repeated file I/O.
5. Before mutation, require verifier evidence such as
   `verify_change_readiness`.
6. After mutation, validate with diagnostics, audit, and release artifacts.

## Host contract

CodeLens does not replace the host. It acts as the **control plane**
between the host and the repository.

- The host keeps its own UI and conversation model.
- CodeLens contributes bounded retrieval, graph analysis, mutation gates,
  audit evidence, and reusable analysis handles.
- The host should prefer profile-scoped surfaces over a flat full registry.

## Success criteria

- Under one minute to first attached session
- Under one call to establish active surface and health state
- Under one verifier round before risky mutation
- Durable handoff and audit artifacts that survive the current chat

## Related

- [Host-adaptive harness](../host-adaptive-harness.md)
- [Architecture overview](../architecture.md)
- [Multi-agent integration](../multi-agent-integration.md)
- [Release notes v1.9.54](../release-notes/v1.9.54.md)
