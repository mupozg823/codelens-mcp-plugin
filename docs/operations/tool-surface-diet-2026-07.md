# Tool Surface Diet — 데이터 기반 축소 제안 (2026-07-12)

> 상태: **1단계 적용됨 (2026-07-13)**. 2단계는 여전히 제안(v2.0 릴리스 트레인). 근거
> 텔레메트리는 단일 사용자(운영자 본인) 14일 트랜스크립트 1,686개 + codelens-first 훅
> 라이브 메트릭 1,224건. 1단계는 가역(코드/도구 삭제 없음) — 미노출 도구도 tools/call로 계속 호출 가능.

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

### 1단계 (가역, 코드 삭제 없음): 기본 노출 표면 49 → 20 — **적용됨 (2026-07-13)**

surface 게이트(presets.rs)는 이미 존재한다. 기본 `review` 프로필(:7839)이 참조하는 상수
`REVIEWER_GRAPH_TOOLS`(49개)를 아래 core-20으로 축소했고, 나머지 33개는 "정의는 유지하되
미노출"(tools/call 직접 호출은 계속 가능 — 기존 아키텍처 특성). 상수와 짝을 이루는 tools.toml
`preset_tags["reviewer-graph"]`도 lockstep 동기화(37 라인: 33 제거 + 4 추가)해
`regen-tool-defs.py::validate_preset_tags` 게이트를 유지했다.

문서 core-15를 **alwaysLoad 9종 + canonical verb façade + 상한 20** 제약과 화해시켜 확정한
최종 **core-20**:

- **canonical verb façade (5)**: `search` `graph` `overview` `diagnose` `review`
  — codelens-first 훅 deny 메시지 + rules/harness.md가 지시하는 mode-라우팅 진입점. 숨기면 안내가 깨짐.
  (`analyze`도 verb façade지만 상한 20 초과라 미노출 — 호출은 계속 가능.)
- **alwaysLoad 9종 (v1.13.34 CHANGELOG)**: `prepare_harness_session` `explore_codebase`
  `review_changes` `review_architecture` `verify_change_readiness` `find_symbol`
  `find_referencing_symbols` `get_symbols_overview` `get_ranked_context`
- **변경 안전성 + 진단 코어 (6)**: `get_file_diagnostics` `impact_report`
  `diff_aware_references` `safe_rename_report` `refresh_symbol_index` `get_capabilities`
  — 문서 core-15의 `safe_rename_report`는 실제 등록 도구로 유지, 기존 reviewer-graph에 있던
  별개 도구 `refactor_safety_report`는 core에서 제외(미노출).

구성은 `presets.rs`의 `reviewer_graph_core_surface_contains_alwaysload_and_verb_facades`
테스트가 고정한다(alwaysLoad 9 + verb 5 포함 + 상한 20 + 중복 없음). `#350` fallback-hint 불변식은
planner/builder 표면으로 한정(reviewer-graph 코어는 recovery 타깃을 dispatch-only로 둠).

체인/표면 스캔 결과: 제거된 도구를 참조하는 지점 — reviewer-graph bootstrap slice의
`cleanup_duplicate_logic`, ci-audit bootstrap slice의 `cleanup_duplicate_logic`,
그리고 `refresh_index` remediation·`semantic` capability 안내가 표면 축소에 맞춰 갱신됨.
전부 하드브레이크 아님(미노출 도구는 tools/call로 계속 호출 가능).

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
