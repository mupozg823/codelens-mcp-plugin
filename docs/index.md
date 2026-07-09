# CodeLens MCP 문서

CodeLens MCP는 multi-agent 코딩 하네스가 큰 코드베이스를 더 적은 token으로 이해하고, 안전하게 수정하며, 현재 index 상태를 검증할 수 있게 돕는 순수 Rust MCP 서버입니다. 이 문서 디렉터리에는 architecture, setup, release verification, ADR, 운영 가이드가 정리되어 있습니다.

<sub>English: CodeLens MCP is a pure Rust MCP server for multi-agent coding harnesses. These docs cover architecture, setup, release verification, ADRs, and operations.</sub>

## 릴리스

<!-- SURFACE_MANIFEST_INDEX_RELEASE:BEGIN -->

- [Latest GitHub Release](https://github.com/mupozg823/codelens-mcp-plugin/releases/latest)
- [All tagged releases](https://github.com/mupozg823/codelens-mcp-plugin/releases)
- [Repository README](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/README.md)
- [Current source tree](https://github.com/mupozg823/codelens-mcp-plugin)

<!-- SURFACE_MANIFEST_INDEX_RELEASE:END -->

## 먼저 볼 문서

- [README](../README.md) — 제품 소개, 설치, 기본 설정
- [Platform setup](platform-setup.md) — Claude Code, Cursor, VS Code, Codex, Windsurf 연결
- [Harness modes](harness-modes.md) — 역할 기반 profile과 preset
- [Multi-agent integration](multi-agent-integration.md) — HTTP daemon, coordination, delegation
- [Architecture overview](architecture.md) — 한글 아키텍처 요약, Mermaid 다이어그램, 요청 처리 흐름, 핵심 코드 지도
- [Release verification](release-verification.md) — 릴리스 검증 절차

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
