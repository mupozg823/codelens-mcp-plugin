# CodeLens MCP 문서

![CodeLens turns a noisy dependency graph into focused, verifiable code context](assets/codelens-social-preview.jpg)

CodeLens MCP는 코딩 에이전트를 위한 살아 있는 코드 인덱스입니다. MCP host가 대화를 소유하는 동안 CodeLens는 저장소 구조를 인덱싱하고, 필요한 맥락을 제한된 크기로 제공하며, 안전한 변경을 위한 session·mutation·검증 경계를 관리합니다.

<sub>English: CodeLens is a live code index for coding agents. It provides bounded retrieval, verifiable structure, host-neutral workflows, single-writer sessions, and mutation gates without taking over the host conversation.</sub>

| 목적 | 기본 경로 |
| --- | --- |
| 저장소 파악 | `prepare_harness_session` → `overview` / `search` |
| 호출·의존 흐름 추적 | `graph(mode="callers")` / `graph(mode="callees")` / `graph(mode="trace")` |
| 아키텍처·변경 검토 | `review(mode="architecture")` / `review(mode="changes")` |
| 진단·검증 | `diagnose` → `verify_change_readiness` |

## 최근 하이라이트 (v1.13.34 이후 main)

- **Host-neutral 실행 계약** — Codex, Claude Code, Cursor 등 host가 대화를 소유하고 CodeLens는 공통 MCP code-intelligence·검증 계층으로 동작합니다. 상세: [ADR-0015](adr/ADR-0015-host-neutral-execution-contract.md).
- **Single-writer project runtime** — shared consumption daemon은 `:7838`, repo-local development daemon은 `:7736`을 사용하며 readonly/review/builder는 endpoint가 아니라 session profile로 분리됩니다. 상세: [HTTP daemon operations](operations/http-daemon.md), [ADR-0017](adr/ADR-0017-single-writer-project-runtime.md).
- **Session identity hardening** — session binding과 coordination 경계를 명시해 동시 작업에서 잘못된 project context와 stale claim을 줄입니다. 상세: [ADR-0018](adr/ADR-0018-session-identity-and-coordination-hardening.md).
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
- [ADR-0005: Shared Harness Substrate for Host-Selected Roles](adr/ADR-0005-harness-v2.md)
- [ADR-0006: Agent routing enforcement](adr/ADR-0006-agent-routing-enforcement.md)
- [ADR-0007: Historical candidate rename (not active)](adr/ADR-0007-symbiote-rebrand.md)
- [ADR-0008: Serena upper-compatible absorption](adr/ADR-0008-serena-upper-compatible-absorption.md)
- [ADR-0009: Mutation trust substrate](adr/ADR-0009-mutation-trust-substrate.md)
- [ADR-0015: Host-Neutral Execution Contract](adr/ADR-0015-host-neutral-execution-contract.md)
- [ADR-0016: Default Tool Surface ≤ 20](adr/ADR-0016-default-surface-twenty.md)
- [ADR-0017: Single-Writer Project Runtime and Session-Safe Context Cache](adr/ADR-0017-single-writer-project-runtime.md)
- [ADR-0018: Session Identity and Coordination Hardening](adr/ADR-0018-session-identity-and-coordination-hardening.md)

## Release Notes

Per-release notes remain available in the [GitHub release-notes directory](https://github.com/mupozg823/codelens-mcp-plugin/tree/main/docs/release-notes).
Changelog history in [../CHANGELOG.md](../CHANGELOG.md).

## Current Release

- [Latest GitHub Release](https://github.com/mupozg823/codelens-mcp-plugin/releases/latest)
- [All tagged releases](https://github.com/mupozg823/codelens-mcp-plugin/releases)

## Archived

Historical plans, superseded audits, and completed decomposition work remain available in the [GitHub archive directory](https://github.com/mupozg823/codelens-mcp-plugin/tree/main/docs/archive).
