# CodeLens vs Serena vs grep+Read — 도구 비교 매트릭스

> 새 사용자가 _어느 자리에서 CodeLens가 grep보다 우월한지_ 빠르게 판단할 수 있는 카테고리별 비교.
> 출처: [`tools.toml`](../crates/codelens-mcp/tools.toml) 121 entries · [oraios/serena `src/serena/tools/`](https://github.com/oraios/serena/tree/main/src/serena/tools).

## 1. Symbol Navigation

| Task                 | CodeLens                                                 | Serena                       | grep+Read                             | CodeLens 우위 시점                                           |
| -------------------- | -------------------------------------------------------- | ---------------------------- | ------------------------------------- | ------------------------------------------------------------ |
| 함수/타입 정의 찾기  | `find_symbol` (include_body=true)                        | `FindSymbolTool`             | partially (정의/호출 혼재)            | 정확한 file/line/column + signature + `suggested_next_tools` |
| 파일 구조 한 눈에    | `get_symbols_overview`                                   | `GetSymbolsOverviewTool`     | ❌                                    | tree-sitter 정확 — private symbol까지 포함                   |
| 누가 호출/상속하는가 | `find_referencing_symbols`, `get_callers`, `get_callees` | `FindReferencingSymbolsTool` | partially (import/string 노이즈 폭발) | call graph 노이즈 거름 + `use_lsp=true` 정밀 모드            |
| 타입 계층            | `get_type_hierarchy`                                     | `FindImplementationsTool`    | ❌                                    | LSP textDocument/typeHierarchy 호출                          |
| 워크스페이스 fuzzy   | `search_workspace_symbols`, `search_symbols_fuzzy`       | LSP workspace/symbol         | partially (rg)                        | LSP-aware + BM25 fallback                                    |

## 2. Search & Context Retrieval

| Task                  | CodeLens                        | Serena                        | grep+Read    | CodeLens 우위 시점                              |
| --------------------- | ------------------------------- | ----------------------------- | ------------ | ----------------------------------------------- |
| 단순 텍스트 매칭      | `search_for_pattern`            | `SearchForPatternTool`        | ✅ (rg/grep) | 1-2 file 작은 repo면 grep이 더 빠름             |
| 부분 이름 / NL 토큰   | `bm25_symbol_search`            | ❌                            | ❌           | "register…" 같은 partial 토큰 허용              |
| 임베딩 시맨틱 검색    | `semantic_search` (ONNX MiniLM) | ❌                            | ❌           | 의미 기반 — 자연어 쿼리 매칭                    |
| 작업 컨텍스트 한 번에 | `get_ranked_context`            | ❌                            | ❌           | hybrid BM25+semantic+structural — 우선순위 순서 |
| 파일/디렉토리 찾기    | `find_file`, `list_dir`         | `FindFileTool`, `ListDirTool` | ✅ (find/ls) | 동등                                            |

## 3. Refactor / Mutation

| Task                | CodeLens                                                | Serena                                            | grep+Read            | CodeLens 우위 시점                                  |
| ------------------- | ------------------------------------------------------- | ------------------------------------------------- | -------------------- | --------------------------------------------------- |
| 함수 본문 통째 교체 | `replace_symbol_body`                                   | `ReplaceSymbolBodyTool`                           | partially (sed/Edit) | 함수 boundary 자동 검출 — line-stale 회피           |
| 심볼 앞/뒤 삽입     | `insert_after_symbol`, `insert_before_symbol`           | `InsertAfterSymbolTool`, `InsertBeforeSymbolTool` | partially            | 심볼 단위로 정확                                    |
| Cross-file rename   | `rename_symbol` (verifier-gated)                        | `RenameSymbolTool`                                | partially (grep+sed) | `safe_rename_report` 사전 검증 — broken rename 거부 |
| 함수 추출 / 인라인  | `refactor_extract_function`, `refactor_inline_function` | ❌                                                | ❌                   | scope-aware — closure 변수 자동 처리                |
| 시그니처 변경       | `refactor_change_signature`                             | ❌                                                | ❌                   | callsite 자동 갱신                                  |
| import 추가         | `add_import`                                            | ❌                                                | partially            | duplicate 감지 + 정렬 유지                          |

## 4. Workflow (CodeLens 전용)

CodeLens의 가장 큰 차별 — Serena·grep 모두 부재.

| Task                 | CodeLens                                                                      | Serena                  | grep+Read |
| -------------------- | ----------------------------------------------------------------------------- | ----------------------- | --------- |
| 첫 onboarding        | `onboard_project` → `explore_codebase`                                        | `OnboardingTool` (단순) | ❌        |
| 변경 사전 검증       | `verify_change_readiness` (4-verifier)                                        | ❌                      | ❌        |
| Mutation gate        | `safe_rename_report`, `unresolved_reference_check`                            | ❌                      | ❌        |
| Pre-merge review     | `review_changes` → `impact_report` → `diff_aware_references`                  | ❌                      | partially |
| 아키텍처 audit       | `review_architecture`, `module_boundary_report`, `find_circular_dependencies` | ❌                      | ❌        |
| 안전한 refactor 계획 | `plan_safe_refactor`, `analyze_change_request`                                | ❌                      | ❌        |
| 중복 로직 정리       | `cleanup_duplicate_logic`, `find_similar_code`                                | ❌                      | ❌        |
| 비동기 무거운 분석   | `start_analysis_job` → `get_analysis_job`                                     | ❌                      | ❌        |

## 5. Diagnostics & Code Health

| Task                   | CodeLens                                                                      | Serena                                 | grep+Read |
| ---------------------- | ----------------------------------------------------------------------------- | -------------------------------------- | --------- |
| 파일 진단              | `get_file_diagnostics`, `diagnose_issues`                                     | `GetDiagnosticsForFileTool/SymbolTool` | ❌        |
| Dead code              | `dead_code_report`, `find_orphan_handlers`, `find_phantom_modules`            | ❌                                     | partially |
| 복잡도 / 변경 coupling | `get_complexity`, `get_change_coupling`                                       | ❌                                     | ❌        |
| 누락 import            | `analyze_missing_imports`                                                     | ❌                                     | partially |
| 잘못된 위치의 코드     | `find_misplaced_code`, `find_over_visible_apis`, `find_redundant_definitions` | ❌                                     | ❌        |
| 테스트 위치            | `find_tests`                                                                  | ❌                                     | partially |

## 6. Memory / Audit / Telemetry

| Task                | CodeLens                                         | Serena                              | grep+Read |
| ------------------- | ------------------------------------------------ | ----------------------------------- | --------- |
| 프로젝트 메모리     | `write_memory`, `read_memory`, `list_memories`   | `WriteMemoryTool`, `ReadMemoryTool` | ❌        |
| 세션 audit          | `audit_builder_session`, `audit_planner_session` | ❌                                  | ❌        |
| Tool surface 일관성 | `audit_tool_surface_consistency`                 | ❌                                  | ❌        |
| 메트릭 export       | `get_tool_metrics`, `audit_log_query`            | ❌                                  | ❌        |

## 7. Multi-agent Coordination (CodeLens 전용)

`claim_files`, `release_files`, `register_agent_work`, `list_active_agents` — Serena·grep 모두 없음. builder/evaluator 멀티 에이전트 mutation 충돌 회피용.

---

## 결론 — 어떤 자리에 무엇을 쓰나

**🎯 CodeLens 단독 우위** — 다른 어디서도 같은 답을 못 받는 영역:

1. **`get_ranked_context`** — hybrid BM25+semantic+structural 단일 호출로 task 컨텍스트 (Serena·grep 모두 X)
2. **`impact_report` + `diff_aware_references`** — blast radius 사전 산출, pre-merge gate
3. **`verify_change_readiness`** — 4-verifier (diagnostics/reference/test/mutation) 한 번에
4. **`review_architecture` + `module_boundary_report`** — cycle/coupling 정량 audit
5. **5-stage adaptive token compression** — 200K/100K/50K tier per tool, prompt-cache hygiene 보장
6. **Multi-agent claim/release** — builder/evaluator/codex 멀티 dispatch 시 mutation 충돌 회피

**🔍 grep + Read가 더 빠른 자리**:

- 1-2 file에서 단순 string 1-2번 매칭
- import/comment/docstring 같은 **non-code mention** 감사 — CodeLens는 의도적으로 거름
- 30 LOC 미만 single-file 편집 (CodeLens warm-up 비용 회피)
- "이 단어가 어디든 언급된 곳" 같은 recall 우선 audit

**🧰 Serena가 더 적합한 자리**:

- multi-LSP 동시 사용 (Serena `solidlsp`는 deadlock-free 동시 LSP)
- JetBrains 네이티브 통합 — CodeLens는 LSP/stdio/HTTP만
- Python-stack agent에 직접 내장 (같은 언어 스택, lighter binary)

**작업 shape별 한 줄 가이드**:

- "정확한 정의 위치 + signature" → CodeLens `find_symbol`
- "이 함수 어디 호출되는지" → CodeLens `find_referencing_symbols` (use_lsp=true)
- "이 변경이 뭘 깨뜨릴까" → CodeLens `impact_report`
- "이 단어 어디든" → grep
- "이 파일 빠르게 보기" → Read (Read는 30 LOC 이하면 CodeLens보다 항상 빠름)
- "여러 LSP 동시 + IDE 안에서" → Serena
