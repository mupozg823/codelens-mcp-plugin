# CodeLens MCP Docs

CodeLens MCP is a pure Rust MCP server for multi-agent coding harnesses. This site packages the repository's operational docs into a lighter public surface so architecture, setup, release verification, and ADRs are easier to browse than the raw repository tree.

## Start Here

- [Platform setup](platform-setup.md)
- [Harness modes](harness-modes.md)
- [Portable harness spec](harness-spec.md)
- [Host-adaptive harness](host-adaptive-harness.md)
- [Migration guide: CodeLens -> Symbiote](migrate-from-codelens.md)
- [Symbiote UX / Agent Flows](design/symbiote-ux-flows-v1.md)
- [Symbiote Phase 3 Rename Plan](design/symbiote-phase3-rename-plan.md)
- [Multi-agent integration](multi-agent-integration.md)
- [Architecture overview](architecture.md)
- [Release verification](release-verification.md)
- [Benchmarks](benchmarks.md)
- [BM25 sparse lane spec](design/bm25-sparse-lane-spec-2026-04-18.md)

## Current Release

<!-- SURFACE_MANIFEST_INDEX_RELEASE:BEGIN -->
- [Latest GitHub Release](https://github.com/mupozg823/codelens-mcp-plugin/releases/latest)
- [All tagged releases](https://github.com/mupozg823/codelens-mcp-plugin/releases)
- [Repository README](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/README.md)
- [Current source tree](https://github.com/mupozg823/codelens-mcp-plugin)
<!-- SURFACE_MANIFEST_INDEX_RELEASE:END -->

## Core Workflows

- `explore_codebase` for initial codebase orientation and targeted context retrieval
- `trace_request_path` for execution and request-flow tracing
- `review_architecture` for module boundaries and coupling
- `plan_safe_refactor` for gated refactor planning
- `review_changes` for diff-aware pre-merge review
- `diagnose_issues` for file, symbol, and directory diagnostics
- `cleanup_duplicate_logic` for duplicate logic and cleanup opportunities

## Decision Records

- [ADR-0001: Runtime boundaries and single-source registries](adr/ADR-0001-runtime-boundaries-and-single-source-registries.md)
- [ADR-0002: Enterprise productization and release gates](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)
- [ADR-0004: Multi-agent concurrency primitives](adr/ADR-0004-multi-agent-concurrency-primitives.md)
- [ADR-0005: Harness v2 — CodeLens as shared substrate](adr/ADR-0005-harness-v2.md)
- [ADR-0008: Serena upper-compatible absorption](adr/ADR-0008-serena-upper-compatible-absorption.md)

## Additional References

- [SCIP precise navigation guide](scip-guide.md)
- [Serena comparison](serena-comparison.md)
- [Architecture audit snapshot](architecture-audit-2026-04-24.md)
- [BM25 sparse lane spec](design/bm25-sparse-lane-spec-2026-04-18.md)
