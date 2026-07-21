# CodeLens MCP 문서

CodeLens MCP는 multi-agent 코딩 하네스가 큰 코드베이스를 더 적은 token으로 이해하고, 안전하게 수정하며, 현재 index 상태를 검증할 수 있게 돕는 순수 Rust MCP 서버입니다. tree-sitter + BM25 + semantic 하이브리드 검색, mode-routed verb 표면(`search`/`graph`/`review`/`overview`/`diagnose`/`analyze`), mutation gate, 적응형 token 압축, index 신선도 신호를 하나의 정적 링크 바이너리로 제공합니다. 이 문서 디렉터리에는 architecture, setup, release verification, ADR, 운영 가이드가 정리되어 있습니다.

<sub>English: CodeLens MCP is a pure Rust MCP server for multi-agent coding harnesses — hybrid retrieval, mode-routed verb surface, mutation gates, adaptive token compression, and index-health signals in one statically linked binary. These docs cover architecture, setup, release verification, ADRs, and operations.</sub>

## 최근 하이라이트 (v1.13.34 이후 main)

- **Verb facade 표면** — 읽기 계열 도구를 6개 mode-routed verb(`search`/`graph`/`review`/`overview`/`diagnose`/`analyze`) 뒤로 통합, 부트스트랩 노출을 ~9개로 축소. 기존 도구 ID는 전부 유지·호출 가능 (#377).
- **순환 의존 검출 정밀화** — `#[cfg(test)]` 전용 import가 유령 순환을 만들던 오탐을 3값 cfg 평가 + 소비자단 정제 패스로 봉합. dead-code/blast-radius/PageRank는 전체 그래프를 그대로 유지.
- **Stage-5 압축이 오류 대신 요약으로 강등** — output_schema 없는 도구(verb facade 포함)도 `data_preview` + 보정된 `recovery_hint`를 text 채널로 수신. 호스트 25K-char truncated 상한과 프리뷰 상한을 공유 상수로 조정. 상세: [Response envelope](operations/response-envelope.md).
- **데몬 드리프트 신호 정밀화** — hooks/docs만 바뀐 binary-equivalent lag에는 재시작 경고를 억제(fail-open 유지). 상세: [HTTP daemon operations](operations/http-daemon.md).
- **`refresh_symbol_index` background 모드** — 대량 리인덱스를 job-handle(`get_analysis_job` 폴링)로 수행해 MCP 타임아웃 회피.
- **Claude 클라이언트 패리티** — 기본 budget 6000 + lean tool contract로 `tools/list` 토큰 대폭 절감 (#377).

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

## Operations

- [Response envelope & compression](operations/response-envelope.md) — effort level, 5-stage 적응 압축(stage-5 요약 강등), lean contract, doom-loop 보호, index 신선도 신호
- [HTTP daemon operations](operations/http-daemon.md) — launchd 데몬 배포/재배포, 드리프트 신호 의미론, exit-78 웨지 복구, codesigning
- [Runtime knobs](operations/runtime-knobs.md) — semantic edit backend, 분석 캐시, 백업 회전
- [Tool routing matrix](operations/tool-routing-matrix.md) — CodeLens vs grep 시나리오 매트릭스, 규모 의존성 측정

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
