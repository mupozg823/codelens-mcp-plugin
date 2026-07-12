# Tool Surface Diet — 데이터 기반 축소 제안 (2026-07-12)

> 상태: **제안(결정 대기)**. 근거 텔레메트리는 단일 사용자(운영자 본인) 14일 트랜스크립트 1,686개
> + codelens-first 훅 라이브 메트릭 1,224건. 표면 플립/삭제는 운영자 확정 후 실행.

## 문제

tools.toml에 도구 100개가 정의돼 있고 review 프로필이 ~48개를 노출하지만, 14일 실사용은
극단적으로 집중돼 있다:

| 도구 | 호출 수 |
|---|---|
| prepare_harness_session | 96 |
| find_symbol | 83 |
| get_file_diagnostics | 17 |
| find_referencing_symbols | 16 |
| get_current_config | 11 |
| search (verb façade) | 8 |
| review/overview/graph 계열 합산 | ~20 |
| **나머지 ~85개 도구** | **사실상 0** |

노출 도구 수는 그 자체로 비용이다 — 스키마 토큰(alwaysLoad ≤10 정책이 이미 인정한 문제),
호스트의 도구 선택 혼란, 그리고 "무엇이 이 제품의 핵심인가"라는 포지셔닝 흐림.

## 제안: 2단계

### 1단계 (가역, 코드 삭제 없음): 기본 노출 표면 48 → 15

surface 게이트(presets.rs)는 이미 존재한다. 기본 프로필 노출을 아래 core-15로 축소하고,
나머지는 "정의는 유지하되 미노출"(tools/call 직접 호출은 계속 가능 — 기존 아키텍처 특성).

**core-15 (사용 데이터 + 전략 축 '변경 안전성'):**

정밀도 사다리 조회(6): `prepare_harness_session` `find_symbol` `find_referencing_symbols`
`get_symbols_overview` `get_ranked_context` `get_file_diagnostics`

변경 안전성 — 경쟁자·하네스가 못 하는 축(6): `verify_change_readiness` `impact_report`
`diff_aware_references` `safe_rename_report` `review_changes` `review_architecture`

운영(3): `explore_codebase` `refresh_symbol_index` `get_capabilities`

### 2단계 (파괴적, v2.0): 하네스 중복 서브시스템 제거

memory 도구군(write/read/list/archive/restore/rename/delete_memory 등), agent
coordination(claim/release_files, register_agent_work, list_active_agents), operator
대시보드, RBAC principals — **호스트 하네스가 이미 소유한 기능의 중복**이며 86.6K LOC
MCP 레이어 비대의 주요인. 죽은 코드는 아니지만(배선 확인됨) 수요 데이터가 0이다.

## 리스크 / 확인 필요

- suggested_next_tools 체인이 미노출 도구를 추천하는 조합 존재 여부 (체인 스캔 필요)
- codelens-first.py deny 메시지와 rules/harness.md의 canonical verb(search/graph) 정합
- Codex/Cursor 쪽 attach 블록(CODELENS_HOST_ROUTING)의 도구 언급 재생성 필요
- 2단계는 semver major + 마이그레이션 노트 필수

## 결정 요청

1. core-15 구성 승인 여부 (특히 semantic_search를 사다리 조회에 포함할지 — 14일 사용 1회)
2. 2단계 제거 대상 4패밀리 중 실제 삭제 vs 영구 미노출
3. 실행 시점 (1단계는 반나절, 2단계는 v2.0 릴리스 트레인)
