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
- **Metadata and package copy cleanup** in `v1.9.56`, so GitHub and crates.io
  describe each crate by its actual role
- **Single Rust binary** that runs over stdio or shared HTTP

## Current Release

<!-- SURFACE_MANIFEST_INDEX_RELEASE:BEGIN -->
- [GitHub Release v1.9.56](https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.56)
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

## What Ships In v1.9.56

- crate metadata now differentiates `codelens-engine`, `codelens-mcp`, and
  `codelens-tui` instead of publishing one workspace description everywhere
- crate README top sections and release links are aligned with the `v1.9.56`
  line
- GitHub repository description now matches the MCP server role instead of the
  older compressed-context marketing copy

## Core Workflows

- `explore_codebase` for initial codebase orientation and targeted context retrieval
- `trace_request_path` for execution and request-flow tracing
- `review_architecture` for module boundaries and coupling
- `plan_safe_refactor` for gated refactor planning
- `review_changes` for diff-aware pre-merge review
- `diagnose_issues` for file, symbol, and directory diagnostics
- `cleanup_duplicate_logic` for duplicate logic and cleanup opportunities

## Architecture, Design, And Product Direction

- [Host-adaptive harness](host-adaptive-harness.md)
- [Multi-agent integration](multi-agent-integration.md)
- [Release notes v1.9.56](release-notes/v1.9.56.md)
- [Interactive architecture map](architecture-d3.html)
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
