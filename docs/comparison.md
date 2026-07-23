# CodeLens vs Serena vs grep+Read — 도구 비교 매트릭스

> 새 사용자가 _어느 자리에서 CodeLens가 grep보다 우월한지_ 빠르게 판단할 수 있는 카테고리별 비교.
> 출처: [`tools.toml`](../crates/codelens-mcp/tools.toml) manifest-visible 87 entries / source registry 93 `[[tool]]` blocks (v1.13.32) · [oraios/serena](https://github.com/oraios/serena) **v1.5.3** 소스 감사 (2026-06-10 갱신, CodeLens live re-check 2026-06-15).
> 심층 분석: [serena-comparison.md](serena-comparison.md) · 채택 설계: [2026-06-10 spec](superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md)
> 2026-06-15 local Serena check: `1.5.4.dev0` / `codex` context exposes a read/navigation active surface; edit tools are available but inactive in that context.

## 1. Symbol Navigation

| Task                 | CodeLens                                                 | Serena                                            | grep+Read                             | 우위 판정                                                     |
| -------------------- | -------------------------------------------------------- | ------------------------------------------------- | ------------------------------------- | ------------------------------------------------------------ |
| 함수/타입 정의 찾기  | `find_symbol` (include_body=true)                        | `find_symbol` (name_path 계층 + substring)        | partially (정의/호출 혼재)            | 동등 — CodeLens는 `symbol_id` 정준화 + `suggested_next_tools` |
| 파일 구조 한 눈에    | `get_symbols_overview`                                   | `get_symbols_overview`                            | ❌                                    | 동등 (CodeLens=tree-sitter, Serena=LSP)                      |
| 누가 호출/상속하는가 | `find_referencing_symbols`, `get_callers`, `get_callees` | `find_referencing_symbols`                        | partially (import/string 노이즈 폭발) | CodeLens — call graph 노이즈 거름 + `use_lsp=true` union 모드 |
| 선언/구현 찾기       | `find_declaration`, `find_implementations` (D1 landed; LSP unavailable 시 graceful degradation + fallback hint) | `find_declaration`, `find_implementations` (v1.3) | ❌                                    | 동등에 가까움 — Serena는 IDE/LSP 폭이 넓고 CodeLens는 harness envelope와 fallback hint가 강함 |
| 타입 계층            | `get_type_hierarchy`                                     | JetBrains `type_hierarchy` (optional)             | ❌                                    | CodeLens — 기본 표면에 노출                                  |
| 워크스페이스 fuzzy   | `search_workspace_symbols`, `search_symbols_fuzzy`       | LSP workspace/symbol                              | partially (rg)                        | CodeLens — LSP-aware + BM25 fallback                         |

## 2. Search & Context Retrieval

| Task                  | CodeLens                        | Serena                            | grep+Read    | 우위 판정                                        |
| --------------------- | ------------------------------- | --------------------------------- | ------------ | ------------------------------------------------ |
| 단순 텍스트 매칭      | (워크플로 경유)                 | `search_for_pattern` (+multiline) | ✅ (rg/grep) | 1-2 file 작은 repo면 grep이 가장 빠름            |
| 부분 이름 / NL 토큰   | `bm25_symbol_search`            | `find_symbol substring_matching`  | ❌           | CodeLens — BM25 랭킹 + NL 토큰 shape 허용        |
| 임베딩 시맨틱 검색    | `semantic_search` (ONNX MiniLM) | ❌                                | ❌           | **CodeLens 단독** — Serena는 임베딩 기반 전무    |
| 작업 컨텍스트 한 번에 | `get_ranked_context`            | ❌                                | ❌           | **CodeLens 단독** — hybrid BM25+semantic+구조     |
| 파일/디렉토리 찾기    | `find_file`, `list_dir`         | `find_file`, `list_dir`           | ✅ (find/ls) | 동등 (Serena는 claude-code 컨텍스트에서 둘 다 제외) |

## 3. Refactor / Mutation — ⚠️ 현재 정직 상태

**2026-06-15 재확인**: line-edit 계열(`create_text_file`, `replace_lines`, `add_import` 등)은
명시 tombstone 처리됐다. Symbolic edit core(`replace_symbol_body`, `insert_before/after_symbol`,
`rename_symbol`)와 refactor substrate(`refactor_*`, `propagate_deletions`)는 **dispatch-only
pending-D3 allowlist**로 남아 있으며, `tools.toml` schema / `tools/list` 노출은 아직 없다.
에이전트가 일반 tool discovery로 발견·호출할 수 없으므로 비교표에서 "가용"으로 표기하지 않는다.
재공개 설계는 [2026-06-10 spec D3](superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md) 참조.

| Task                | CodeLens (현재 노출 기준)                  | Serena v1.5.3                                     | grep+Read            | 우위 판정                                         |
| ------------------- | ------------------------------------------ | ------------------------------------------------- | -------------------- | ------------------------------------------------- |
| 함수 본문 통째 교체 | 👻 ghost (D3 재공개 후보)                   | `replace_symbol_body` ✅ **주력 경로**             | partially (sed/Edit) | **Serena**                                        |
| 심볼 앞/뒤 삽입     | 👻 ghost (D3 재공개 후보)                   | `insert_after_symbol`, `insert_before_symbol` ✅   | partially            | **Serena**                                        |
| Cross-file rename   | 👻 ghost — preflight `safe_rename_report`만 노출 | `rename_symbol` (LSP refactoring) ✅           | partially (grep+sed) | **Serena** — 단 CodeLens preflight 검증은 더 강함 |
| 안전 삭제           | ❌ (`propagate_deletions` ghost)            | `safe_delete_symbol` (참조 검사 후 거부/실행) ✅   | ❌                   | **Serena**                                        |
| 함수 추출/인라인/이동 | 👻 ghost (`refactor_*`)                    | JetBrains `move`/`inline` (optional, beta)        | ❌                   | 둘 다 조건부 — 어느 쪽도 product-green 아님       |
| regex 파일 내 치환  | ❌                                          | `replace_content` (regex/literal)                 | ✅ (sed/Edit)        | 호스트 네이티브 Edit로 충분                       |

**설계 노트**: mutation 게이트(`verify_change_readiness` → 신선한 preflight 증거 요구)는 CodeLens의
차별점으로 유지. Serena는 반대로 "편집 도구는 신뢰하라, 재검증 말라"는 confidence 프롬프트 전략.

## 4. Workflow (CodeLens 전용 우위)

Serena·grep 모두 부재 — CodeLens의 가장 큰 차별 축.

| Task                 | CodeLens                                                     | Serena                                  | grep+Read |
| -------------------- | ------------------------------------------------------------ | --------------------------------------- | --------- |
| 첫 onboarding        | `prepare_harness_session` → `explore_codebase`               | `onboarding` (memory_maintenance 시드)  | ❌        |
| 변경 사전 검증       | `verify_change_readiness` (4-verifier)                       | ❌                                      | ❌        |
| Mutation preflight   | `safe_rename_report`, `unresolved_reference_check`           | ❌ (사후 진단만)                        | ❌        |
| Pre-merge review     | `review_changes` → `impact_report` → `diff_aware_references` | ❌                                      | partially |
| 아키텍처 audit       | `review_architecture`, `module_boundary_report` (cycle 감지 포함) | ❌                                  | ❌        |
| 안전한 refactor 계획 | `plan_safe_refactor`, `plan_symbol_rename`                   | ❌                                      | ❌        |
| 중복 로직 정리       | `cleanup_duplicate_logic`, `find_similar_code`               | ❌                                      | ❌        |
| 비동기 무거운 분석   | `start_analysis_job` → `get_analysis_job` → section 핸들     | ❌                                      | ❌        |

> 👻 주의: `analyze_change_request`, `orchestrate_change`는 backward-compat dispatch arm이지만
> workflow-first entrypoint는 `explore_codebase`, `trace_request_path`, `review_architecture`,
> `plan_safe_refactor`, `cleanup_duplicate_logic`, `review_changes`, `diagnose_issues`를 우선한다.

## 5. Diagnostics & Code Health

| Task                   | CodeLens                                       | Serena                                            | grep+Read |
| ---------------------- | ---------------------------------------------- | ------------------------------------------------- | --------- |
| 파일 진단              | `get_file_diagnostics`, `diagnose_issues`      | `get_diagnostics_for_file` (v1.3)                 | ❌        |
| 심볼 단위 진단         | `get_diagnostics_for_symbol` (D1 landed; LSP unavailable 시 empty + degraded_reason) | `get_diagnostics_for_symbol` (v1.3)               | ❌        |
| Dead code              | `dead_code_report`                             | ❌                                                | partially |
| 복잡도                 | `get_complexity`                               | ❌                                                | ❌        |
| 잘못된 위치의 코드     | `find_misplaced_code` (G5 role-aware)          | ❌                                                | ❌        |
| 중복 감지              | `find_code_duplicates` (G6 filetype-aware)     | ❌                                                | ❌        |
| 테스트 위치            | `find_tests`                                   | ❌                                                | partially |
| IDE 인스펙션           | ❌                                              | JetBrains `run_inspections` (v1.3, optional)      | ❌        |

## 6. Memory / Audit / Telemetry

| Task                | CodeLens                                                      | Serena v1.5.3                                           | grep+Read |
| ------------------- | ------------------------------------------------------------- | ------------------------------------------------------- | --------- |
| 프로젝트 메모리     | `write/read/list/delete/rename/archive/restore_memory`        | `write/read/list/delete/rename/edit_memory`             | ❌        |
| 메모리 상호참조     | ❌ (P3 설계: D6)                                               | `mem:` 참조 + rename 자동 전파 + CLI 무결성 체크 (v1.5) | ❌        |
| 메모리 일관성 audit | `audit_memory_consistency`                                     | `serena memories check` (CLI)                           | ❌        |
| 세션 audit          | `audit_builder_session`, `audit_planner_session`               | ❌                                                      | ❌        |
| Tool surface 일관성 | `audit_tool_surface_consistency` (P1-4 Sprint A에서 부활)      | ❌                                                      | ❌        |
| 메트릭 export       | `get_tool_metrics`, `audit_log_query`                          | 사용 보고(analytics, opt-out)                           | ❌        |

## 7. Multi-agent Coordination (CodeLens 전용)

`claim_files`, `release_files`, `register_agent_work`, `list_active_agents` — Serena·grep 모두 없음.
builder/evaluator 멀티 에이전트 mutation 충돌 회피용. Serena는 프로젝트당 단일 에이전트 모델
(`single_project: true`로 표면 축소하는 방향).

## 8. 하네스 강제 레이어 (신규 축, 2026-06-10)

Serena v1.3→v1.5가 새로 연 비교 축 — 도구가 아니라 **라우팅 강제** 메커니즘:

| 메커니즘             | CodeLens                                       | Serena v1.5.3                                                          |
| -------------------- | ---------------------------------------------- | ---------------------------------------------------------------------- |
| 도구 체이닝 유도     | `suggested_next_tools` (응답 내 advisory)      | 프롬프트 + 도구 매핑 테이블                                            |
| 반복 호출 감지       | doom-loop 감지 (3+ 동일 호출 → hint)           | **PreToolUse hook: grep×3/read×3 연속 → deny** + 심볼릭 대안 안내      |
| 호스트별 표면 적응   | `HostContext`/`TaskOverlay` overlay 컴파일     | 14개 context yml (claude-code/codex/ide/…) — 도구 제외 + 전용 프롬프트 |
| 시스템 프롬프트 개입 | ❌ (MCP 계약 준수)                              | `cc_system_prompt_override` — Claude Code 시스템 프롬프트 전체 대체    |
| 자기 도구 마찰 제거  | ❌                                              | auto-approve hook (acceptEdits/auto 모드)                              |
| 응답 바운딩          | 5-stage adaptive compression + tier별 `_meta`  | 단일 char limit (초과 시 에러로 좁히라고 요구)                         |

채택 권고: deny hook의 **온건판**(soft `additionalContext` 리마인더, strict는 opt-in)을 플러그인에
번들 — [spec D4](superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md) 참조.

---

## 9. 5-렌즈 적대 아키텍처 평가 (2026-07-03)

양쪽 소스를 독립 판정자 5명이 직접 읽고(**양방향 반박 강제** — "CodeLens 우위" 가설과
"Serena 우위" 가설을 각자 공격 후 채점) 실시한 구조 평가. Serena 1.5.4.dev0
(코어 18K + solidlsp 38K LOC) vs CodeLens (105.6K LOC Rust).

| 렌즈 | Serena | CodeLens | 판정 |
|---|---:|---:|---|
| 의미 정밀도·정확성 | **8.5** | 6.5 | Serena — 단 대부분 구현 성숙도 갭(구조 상한 아님). LSP 프로토콜 패리티(P1.1a)·quiescence 보정(P1.1b)·pre-warm(P1.3)으로 수렴 중 |
| 에이전트 소비 경제학 | 5.0 | **8.5** | CodeLens 구조 우위 — 토큰 예산이 파싱→캡→압축→lean contract까지 1급 관심사 |
| 운영 강건성·스케일 | 5.5 | **8.5** | CodeLens 구조 우위 — 데몬+영속 WAL 인덱스+워처 vs per-session LS 워밍업 |
| 안전·거버넌스 | 4.0 | **8.5** | CodeLens 구조 우위 — identity RBAC fail-closed·verifier-first·감사로그 vs 정적 deny 레이어 |
| 확장성·진화 경제학 | **8.0** | 6.5 | Serena — LS 어댑터 외주화의 언어 한계비용. 단 품질 회귀 인프라(MRR floor·perf 게이트)는 CodeLens 압도 |

정직 노트: 판정자들은 Serena 소스에서도 결함을 실측했다(자인한 Python 전용
hack 주석, dry-run·old-text 검증 없는 rename 적용) — 변이 안전성만 보면
CodeLens 트랜잭션 계층이 더 방어적이다. 종합: **에이전트-소비 백엔드
(멀티세션·토큰·거버넌스) 정체성으로는 구조 우위, 범용 의미론 레이어로는
Serena가 오늘 우위**이며 정밀도 갭은 로드맵
([PLAN_precision-parity-gap-closure-2026-07](plans/PLAN_precision-parity-gap-closure-2026-07.md))으로 수렴 중.

## 결론 — 어떤 자리에 무엇을 쓰나

**🎯 CodeLens 단독 우위** — 다른 어디서도 같은 답을 못 받는 영역:

1. **`get_ranked_context`** — hybrid BM25+semantic+structural 단일 호출로 task 컨텍스트 (Serena·grep 모두 X)
2. **`impact_report` + `diff_aware_references`** — blast radius 사전 산출, pre-merge gate
3. **`verify_change_readiness`** — 4-verifier (diagnostics/reference/test/mutation) 한 번에
4. **`review_architecture` + `module_boundary_report`** — cycle/coupling 정량 audit
5. **5-stage adaptive token compression** — 200K/100K/50K tier per tool, prompt-cache hygiene 보장
6. **Multi-agent claim/release** — 여러 builder/evaluator agent dispatch 시 mutation 충돌 회피
7. **자기감사 detector 가족** — `audit_tool_surface_consistency`, `find_misplaced_code`, `find_code_duplicates` 등

**🔍 grep + Read가 더 빠른 자리**:

- 1-2 file에서 단순 string 1-2번 매칭
- import/comment/docstring 같은 **non-code mention** 감사 — CodeLens는 의도적으로 거름
- 30 LOC 미만 single-file 편집 (CodeLens warm-up 비용 회피)
- "이 단어가 어디든 언급된 곳" 같은 recall 우선 audit

**🧰 Serena가 더 적합한 자리**:

- **심볼 단위 편집이 주력인 워크플로** — replace_symbol_body/insert/rename/safe_delete가 1군 표면 (CodeLens는 유령화)
- LSP 정밀 navigation 완결 루프 (declaration/implementations 포함)
- multi-LSP 동시 사용 (`solidlsp` deadlock-free 동시 LSP, ~60 언어)
- JetBrains 네이티브 통합 (inspections/debug/move/inline)
- 메모리 상호참조·rename 전파가 필요한 장기 프로젝트 지식 관리

**작업 shape별 한 줄 가이드**:

- "정확한 정의 위치 + signature" → CodeLens `find_symbol`
- "이 함수 어디 호출되는지" → CodeLens `find_referencing_symbols` (use_lsp=true)
- "이 변경이 뭘 깨뜨릴까" → CodeLens `impact_report`
- "인터페이스 구현체 다 찾기" → CodeLens 또는 Serena `find_implementations` (Serena는 LSP breadth, CodeLens는 harness fallback hint)
- "심볼 통째 교체/삽입" → Serena 심볼릭 편집 (CodeLens ghost 해소 전까지)
- "자연어로 코드 찾기" → CodeLens `semantic_search` (Serena 불가)
- "이 단어 어디든" → grep
- "이 파일 빠르게 보기" → Read (30 LOC 이하면 CodeLens보다 항상 빠름)

---

### Note 1 — Detector 가족 이력 (v1.13.27 diet → P1-4 부활)

v1.13.27 diet에서 제거됐던 detector 5종 중 4종이 admin 계열로 부활 완료 (P1-4 Sprint A~,
2026-05-21~27 커밋군): `audit_tool_surface_consistency`(`ae8c6f2f`), `find_phantom_modules` +
`find_redundant_definitions`(`ad73dacb`), `find_over_visible_apis`(`be6b7dab`). `find_orphan_handlers`는
`find_misplaced_code`/`dead_code_report`가 흡수. 현재 9-detector 가족 전부 ✅
(tools.toml 등재 — 단 preset 미포함 시 직접 `tools/call`만 가능).

### Note 2 — Ghost 도구 전수 (2026-06-10 실측, 해소 대상)

dispatch에는 있으나 일반 `tools/list` schema 표면에 없는 도구는 현재 두 클래스로 분리된다:
line-edit tombstone 8종(`create_text_file`, `delete_lines`, `insert_at_line`, `replace_lines`,
`replace_content`, `insert_content`, `replace`, `add_import`)과 pending-D3 symbolic/refactor allowlist
9종(`rename_symbol`, `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`,
`refactor_*` ×4, `propagate_deletions`). Runtime/script audit now splits that 9-tool allowlist into
`pending_d3_symbolic_edit_core` 4종과 `pending_d3_refactor_substrate` 5종 so the D3 re-list/delete
decision can move one class without hiding the other. 해소 설계:
[2026-06-10 spec D2/D3](superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md).
