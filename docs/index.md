# CodeLens MCP Docs

CodeLens MCP is a pure Rust MCP server for multi-agent coding harnesses. This directory contains operational docs for architecture, setup, release verification, and ADRs.

## Start Here

- [README](../README.md) — overview, install, setup
- [Platform setup](platform-setup.md) — Claude Code, Cursor, VS Code, Codex, Windsurf
- [Harness modes](harness-modes.md) — role-based profiles and presets
- [Multi-agent integration](multi-agent-integration.md) — HTTP daemon, coordination, delegation
- [Architecture overview](architecture.md) — system design and components
- [Release verification](release-verification.md) — how releases are validated

## Reference

- [Portable harness spec](harness-spec.md)
- [Host-adaptive harness](host-adaptive-harness.md)
- [SCIP precise navigation guide](scip-guide.md)
- [Serena comparison](serena-comparison.md)
- [Observability](observability.md)
- [Arg validation policy](design/arg-validation-policy.md)
- [Refactor backend honesty](design/refactor-backend-honesty.md)

## Benchmarks

- [Benchmarks overview](benchmarks.md)
- [BM25 sparse lane spec](design/bm25-sparse-lane-spec-2026-04-18.md)

## Decision Records

- [ADR-0001: Runtime boundaries and single-source registries](adr/ADR-0001-runtime-boundaries-and-single-source-registries.md)
- [ADR-0002: Enterprise productization and release gates](adr/ADR-0002-enterprise-productization-evaluation-and-release-gates.md)
- [ADR-0004: Multi-agent concurrency primitives](adr/ADR-0004-multi-agent-concurrency-primitives.md)
- [ADR-0005: Harness v2 — CodeLens as shared substrate](adr/ADR-0005-harness-v2.md)
- [ADR-0006: Agent routing enforcement](adr/ADR-0006-agent-routing-enforcement.md)
- [ADR-0007: Symbiote rebrand](adr/ADR-0007-symbiote-rebrand.md)
- [ADR-0008: Serena upper-compatible absorption](adr/ADR-0008-serena-upper-compatible-absorption.md)
- [ADR-0009: Mutation trust substrate](adr/ADR-0009-mutation-trust-substrate.md)

## Release Notes

Per-release notes in [release-notes/](release-notes/).
Changelog history in [../CHANGELOG.md](../CHANGELOG.md).

## Current Release

- [Latest GitHub Release](https://github.com/mupozg823/codelens-mcp-plugin/releases/latest)
- [All tagged releases](https://github.com/mupozg823/codelens-mcp-plugin/releases)

## Archived

Historical plans, superseded audits, and completed decomposition work live in [archive/](archive/).
