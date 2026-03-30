---
date: 2026-03-30
phase: session-telemetry (실제 Claude Code 세션)
project: codelens-mcp-plugin (self, 29K LOC)
binary: codelens-mcp (arm64-darwin)
commit: 6da48d2
session_type: Claude Opus 4.6 (1M context) — real development session
---

# Session Telemetry: 2026-03-30

> 이 데이터는 벤치마크가 아니라 **실제 개발 세션에서 Claude Code가 CodeLens 도구를 사용한 기록**입니다.

## 세션 요약

| 항목           | 값                                      |
| -------------- | --------------------------------------- |
| 총 도구 호출   | 72회                                    |
| 총 소요 시간   | 5,409ms                                 |
| 평균 호출 시간 | 75ms                                    |
| 총 토큰 사용   | 102,260                                 |
| 에러           | 1회 (get_file_diagnostics — LSP 미설치) |
| 고유 도구 사용 | 11종                                    |

## 도구별 사용 빈도 + 성능

| 도구                  | 호출   | 총 시간(ms) | 평균(ms) | 최대(ms) | 에러 |
| --------------------- | ------ | ----------- | -------- | -------- | ---- |
| find_symbol           | **35** | 33          | 0.9      | 3        | 0    |
| get_ranked_context    | **14** | 343         | 24.5     | 73       | 0    |
| delete_lines          | 6      | 1           | 0.2      | 1        | 0    |
| onboard_project       | 4      | 4,963       | 1,241    | 4,953    | 0    |
| get_symbols_overview  | 4      | 4           | 1.0      | 1        | 0    |
| semantic_search       | 3      | 51          | 17.0     | 31       | 0    |
| replace_symbol_body   | 2      | 10          | 5.0      | 5        | 0    |
| get_file_diagnostics  | 1      | 2           | 2.0      | 2        | 1    |
| get_project_structure | 1      | 1           | 1.0      | 1        | 0    |
| get_impact_analysis   | 1      | 1           | 1.0      | 1        | 0    |
| get_tool_metrics      | 1      | 0           | 0.0      | 0        | 0    |

## 사용 패턴 분석

### Claude Code가 가장 많이 호출한 도구 (자연 사용)

```
find_symbol:         35회 (48.6%) ← 코드 탐색의 기본 도구
get_ranked_context:  14회 (19.4%) ← 맥락 파악
delete_lines:         6회 (8.3%)  ← 코드 정리
onboard_project:      4회 (5.6%)  ← 프로젝트 파악
나머지 7종:          13회 (18.1%)
```

### 호출되지 않은 도구 (BALANCED 기준 39개 중 28개 미사용)

이 세션에서 사용되지 않은 도구:

- find_referencing_symbols, find_scoped_references, find_annotations, find_tests
- rename_symbol, replace_content, insert_content, replace
- get_changed_files, get_callers, get_callees
- list_memories, read_memory, write_memory, delete_memory, rename_memory
- activate_project, set_preset, get_capabilities, get_watch_status
- add_queryable_project, remove_queryable_project, query_project
- onboarding, prepare_for_new_conversation, summarize_changes
- refresh_symbol_index, search_workspace_symbols

### 시사점

1. **find_symbol이 압도적 1위 (49%)** — 도구 설명의 "Use this first"가 작동함
2. **get_ranked_context가 2위 (19%)** — 넓은 질문에 자동 선택됨
3. **사용 도구 집중도: 상위 2개가 68%** — 62개 중 대부분은 사용 안 됨
4. **onboard_project 4회 = 세션 전체 시간의 91.7%** — 시맨틱 임베딩이 지배적 병목
5. **에러 1건 (LSP)** — tree-sitter-first 전략이 에러를 1건으로 억제

## 토큰 효율 (추정)

| 방식              | 토큰/호출 (추정) | 72회 총 토큰    |
| ----------------- | ---------------- | --------------- |
| **CodeLens 도구** | ~1,420           | 102,260         |
| Read (전체 파일)  | ~3,000-8,000     | 216,000-576,000 |
| Grep (패턴 매칭)  | ~2,000-5,000     | 144,000-360,000 |

CodeLens 도구는 랭킹된 심볼만 반환하므로 Read/Grep 대비 **토큰 2-5x 절약** 추정.
(정확한 비교는 동일 작업의 A/B 테스트 필요)
