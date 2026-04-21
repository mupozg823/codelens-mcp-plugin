# CodeLens MCP Docs

CodeLens MCP is the bounded code-intelligence layer for agentic software
teams. It keeps the host-visible tool surface small, compresses retrieval
into workflow artifacts, and turns mutation into a preflighted,
auditable path.

## Why teams adopt it

- **6.1x fewer tokens** on structured retrieval tasks, with a **167x**
  best-case compression win
- **108 generated definitions in source**, but only **12-49 tools**
  visible per runtime profile instead of a single flat registry
- **Release-ready automated harness** in `v1.9.54` with usage-drift
  artifacts and independent signoff
- **Single Rust binary** that runs over stdio or shared HTTP

## Current Release

<!-- SURFACE_MANIFEST_INDEX_RELEASE:BEGIN -->
- [GitHub Release v1.9.54](https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.54)
- [Repository README](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/README.md)
- [Current source tree](https://github.com/mupozg823/codelens-mcp-plugin)
<!-- SURFACE_MANIFEST_INDEX_RELEASE:END -->

## Start By Goal

| Goal | Start here |
| ---- | ---------- |
| Install and attach the server | [Platform setup](platform-setup.md) |
| Understand runtime profiles and surfaces | [Harness modes](harness-modes.md) |
| Wire a portable multi-agent protocol | [Portable harness spec](harness-spec.md) |
| Attach the right host the right way | [Host-adaptive harness](host-adaptive-harness.md) |
| Run the multi-agent coordination pattern | [Multi-agent integration](multi-agent-integration.md) |
| See the product architecture visually | [Interactive D3 architecture map](architecture-d3.html) |
| Read the long-form system design | [Architecture overview](architecture.md) |
| Validate public performance claims | [Benchmarks](benchmarks.md) |
| Verify release bundles and gates | [Release verification](release-verification.md) |

## What Ships In v1.9.54

- `benchmarks/harness/release-harness-runner.py` for one-command
  orchestrator/evaluator/signoff execution
- opt-in `strict` coordination for trusted HTTP `refactor-full` mutation
- standardized `usage-drift.*` and `independent-signoff.*` artifacts
- capability output that publishes current `coordination_mode`
- second-pass registry reduction to `108` generated definitions with canonical workflow visibility

## Core Workflows

- `explore_codebase` for initial codebase orientation and targeted context retrieval
- `trace_request_path` for execution and request-flow tracing
- `review_architecture` for module boundaries and coupling
- `plan_safe_refactor` for gated refactor planning
- `review_changes` for diff-aware pre-merge review
- `diagnose_issues` for file, symbol, and directory diagnostics
- `cleanup_duplicate_logic` for duplicate logic and cleanup opportunities

## Architecture, Design, And Product Direction

- [Migration guide: CodeLens -> Symbiote](migrate-from-codelens.md)
- [Symbiote UX / Agent Flows](design/symbiote-ux-flows-v1.md)
- [Symbiote Phase 3 Rename Plan](design/symbiote-phase3-rename-plan.md)
- [BM25 sparse lane spec](design/bm25-sparse-lane-spec-2026-04-18.md)

## Decision Records

- [ADR-0001: Runtime boundaries and single-source registries](adr/ADR-0001-runtime-boundaries-and-single-source-registries.md)
- [ADR-0002: Enterprise productization and release gates](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)
- [ADR-0004: Multi-agent concurrency primitives](adr/ADR-0004-multi-agent-concurrency-primitives.md)
- [ADR-0005: Harness v2 — CodeLens as shared substrate](adr/ADR-0005-harness-v2.md)

## Additional References

- [SCIP precise navigation guide](scip-guide.md)
- [Serena comparison](serena-comparison.md)
- [Architecture audit snapshot](architecture-audit-2026-04-12.md)
