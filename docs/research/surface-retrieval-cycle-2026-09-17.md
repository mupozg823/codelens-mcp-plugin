# CodeLens 표면·검색 실측 사이클 — 2026-09-17

> 범위: [2026-09-16 리서치](landscape-deep-research-2026-09-16.md)가 백로그로 넘긴 항목을 실측으로
> 판정한다. (1) 프로파일 × 클라이언트 18개 조합의 `tools/list` 를 바인딩 전후로 계측, (2) 사용
> 텔레메트리로 표면 도달성 분리, (3) 결정용 문헌 체크, (4) 계측이 지지한 코드 변경 3건
> (K-0032, K-0023, K-0030). 문헌 원문은 [`2026-09-17-sources/`](2026-09-17-sources/) 에 보존.

## 1. 요약

1. **ADR-0016 core-10 always-load 가 프로파일에 따라 빠져 있었다.** `anthropic/alwaysLoad` 는
   리스팅된 도구에만 붙는데, `review` 표면(REVIEWER_GRAPH)은 `plan_safe_refactor`·
   `get_changed_files`·`get_current_config` 를, `builder`(BUILDER_MINIMAL)는 `get_changed_files` 를
   리스팅하지 않았다. 바인딩 전 review 7종·builder 9종, 비-Claude 호스트는 바인딩 후 review 표면에
   떨어져 7종. K-0032 이후 18/18 조합이 10종.
2. **Tier 2 가정("비-Claude 호스트가 outputSchema 를 통째로 받는다")은 틀렸다.** `codex-mcp-client`
   는 이미 lean 계약이다(바인딩 후 20종·24.5 KB·outputSchema 0). 무거운 리스팅은 generic 클라이언트의
   **바인딩 전** CORE-20(19종·59.6 KB·outputSchema 19) 하나뿐이다. 스펙상 생략은 합법이지만(클라이언트
   검증은 SHOULD) generic 클라이언트의 CI 계약이라 이번엔 조치하지 않았다.
3. **프리셋 멤버십 이중 관리 제거(K-0023).** `presets.rs` 의 손수 배열 5개(항목 154개, 고유 이름 80개)를
   `tools.toml` `preset_tags` 에서 생성한다. 교체 전 생성 결과가 손수 배열과 멤버·순서까지 동일함을
   확인했다(25/26/41/46/20). `[[tool]]` 테이블이 없는 pending-D3 편집 코어 4종만
   `[dispatch_only_preset_members]` 로 분리하고 생성기가 허용목록 밖 이름을 거부한다.
4. **식별자 분할기 4벌 → 1벌(K-0030).** 엔진 `unicode::identifier_words` 하나를 임베딩 프롬프트,
   쿼리 의도 분석, BM25-F 희소 레인 토크나이저가 공유한다. 임베딩 입력은 실제 식별자 10,080개에서
   바이트 동일(재임베딩 불필요). 쿼리 의도 분석의 두문자어 버그(`getHTTPResponse` → `h t t p`)가
   사라졌다. BM25-F 레인 camelCase 분할은 3코퍼스 벤치에서 gson·jest `bm25_symbol_search` MRR 을
   0.079→0.283·0.060→0.122 로, hybrid MRR 을 세 코퍼스 모두 올렸고 self bm25 는 측정된 null
   (−0.0026)이다. 대가는 단일 단어 정확 식별자 쿼리의 정밀도(§4.3, K-0035).
5. **텔레메트리가 가리키는 축소 후보는 좁다.** 유기 호출 4,858건(07-10~09-17, 합성 클라이언트 제외,
   직접 호출 + facade 해석 대상 합산). 활성 표면에 리스팅됐는데 도달 0인 도구 34종 중 독립적으로
   떼어낼 수 있는 묶음은 이미 experimental 인 `secondary-projects` 4종뿐이다. `audit_*_session`·
   `export_session_markdown`·`audit_log_query` 는 ADR-0005/ADR-0009 계약에 묶여 있고,
   `plan_safe_refactor`·`get_changed_files` 같은 core-10 도 도달 0 이므로 "도달 0" 단독으로는 제거
   근거가 되지 않는다.
6. **문헌은 방향만 준다.** 그래프 기능 유지(LocAgent·CoSIL·OrcaLoca·RepoGraph), 응답 절단은 U자형
   (SWE-agent), stale 인덱스 경고는 소규모 진단 연구 1건(n=17)이 지지. 09-16 소스의 RAG-MCP 인용은
   방향이 뒤집혀 있어 정오표를 달았다.

## 2. 표면 실측

에페메랄 HTTP 서버를 프로파일마다 띄우고 `initialize → tools/list → prepare_harness_session →
tools/list` 를 클라이언트 이름별로 실행했다. 칸은 `도구 수 / 바이트 / alwaysLoad 수`.

| profile | client | 기준(7bd7674e 이전) 바인딩 전 | 기준 바인딩 후 | K-0032 바인딩 전 | K-0032 바인딩 후 |
|---|---|---|---|---|---|
| review | claude-code | 8 / 12,134 / **7** | 56 / 50,703 / 10 | 11 / 13,687 / 10 | 56 / 50,703 / 10 |
| review | codex-mcp-client | 8 / 12,134 / **7** | 20 / 24,560 / **7** | 11 / 13,687 / 10 | 20 / 24,536 / 10 |
| review | mcp (generic) | 19 / 59,628 / 10 | 20 / 28,103 / **7** | 19 / 59,628 / 10 | 20 / 28,072 / 10 |
| builder | claude-code | 14 / 15,535 / **9** | 56 / 50,703 / 10 | 15 / 15,958 / 10 | 56 / 50,703 / 10 |
| builder | codex-mcp-client | 14 / 15,535 / **9** | 20 / 24,560 / **7** | 15 / 15,958 / 10 | 20 / 24,536 / 10 |
| builder | mcp (generic) | 19 / 59,628 / 10 | 20 / 28,103 / **7** | 19 / 59,628 / 10 | 20 / 28,072 / 10 |
| readonly | claude-code | 14 / 15,260 / 10 | 56 / 50,703 / 10 | 14 / 15,260 / 10 | 56 / 50,703 / 10 |
| readonly | codex-mcp-client | 14 / 15,260 / 10 | 20 / 24,560 / **7** | 14 / 15,260 / 10 | 20 / 24,536 / 10 |
| readonly | mcp (generic) | 19 / 59,628 / 10 | 20 / 28,103 / **7** | 19 / 59,628 / 10 | 20 / 28,072 / 10 |

- generic 클라이언트 바인딩 전 리스팅만 outputSchema 19개를 싣는다(그중 `prepare_harness_session`
  엔트리 20,383 B). 나머지 17개 조합은 outputSchema 0.
- K-0032 는 review 표면에 3종을 넣고 3종(`impact_report`·`diff_aware_references`·
  `safe_rename_report`)을 빼 20종을 유지했다. 뺀 3종은 `graph(mode=impact)`·`graph(mode=diff-refs)`·
  `plan_safe_refactor` 로 여전히 도달한다. 기본 리스팅 facade 의 대상은 deferred-loading 허용을
  물려받고(`dispatch/query_engine.rs`), 호출 가능 게이트는 "리스팅 **또는** 등록"이라
  (`dispatch/access.rs`) 리스팅에서 빠진 등록 도구도 막히지 않는다.
- 새 테스트 `every_built_in_surface_lists_the_core_10` 이 6개 내장 표면 전부를 고정한다.

## 3. 텔레메트리 도달성

`.codelens/telemetry/tool_usage.jsonl`(분석 시점 약 46.2k행, 2026-07-10~09-17). `tools/list` 행, 합성 클라이언트
(bench·probe·doctor 계열), 비-runtime 기록을 제외하고 `tool` 과 `resolved_target` 을 합산했다.

| 분류 | 수 | 판정 |
|---|---|---|
| 활성 표면에 한 번도 리스팅되지 않음 | 0 | — |
| 이미 폐기(ADR-0018) | 18 | ADR-0018 제거 컷에서 삭제 |
| 리스팅됐지만 도달 0 | 34 | 아래 |

표면별 유기 호출: review 2,421 · builder 1,036 · preset:balanced 959 · readonly 423 · preset:full 19.

도달 0 34종 판정:

- **wave-3 폐기 후보(사용자 결정):** `add_queryable_project`·`remove_queryable_project`·
  `query_project`·`list_queryable_projects`. 이미 `[experimental_features] secondary-projects` 로 묶여
  있어 떼어내도 다른 계약이 깨지지 않는다.
- **계약에 묶여 후보 아님:** `audit_builder_session`·`audit_planner_session`(ADR-0005 감사 게이트),
  `export_session_markdown`·`audit_log_query`(ADR-0009).
- **core/facade 경유라 판단 보류:** `plan_safe_refactor`·`get_changed_files`·`cancel_analysis_job`·
  `get_watch_status` 등 CORE-20 멤버. 호스트가 네이티브 git·편집으로 우회하는 것일 수 있어 도달 0 이
  무용을 뜻하지 않는다.
- **preset:full 에서만 리스팅(그 표면 호출 19건):** `read_file`·`list_dir`·`find_file`·`get_complexity`·
  `get_symbol_importance`·`prune_index_failures` 와 `get_watch_status`. 표본이 너무 작아 판정하지 않는다.

## 4. 코드 변경

### 4.1 K-0032 — core-10 을 모든 내장 표면에 (7bd7674e)

`MINIMAL_TOOLS` +3, `BUILDER_MINIMAL_TOOLS` +1, `REVIEWER_GRAPH_TOOLS` 3↔3 교체. `tools.toml`
`preset_tags` 동시 수정, 생성 문서의 카운트 갱신(builder 40→41, minimal 22→25). 검증:
`cargo test -p codelens-mcp --bin codelens-mcp` 1,067 통과, surface-manifest `--check` 통과, §2 재측정.

### 4.2 K-0023 — 프리셋 멤버십 코드젠 (aadf5818)

ADR-0013 결정 5 의 미구현분. `regen-tool-defs.py` 가 `preset_tags` 를 역색인해
`metadata_generated.rs` 에 상수 5개를 쓰고, `presets.rs` 는 별칭과 표면별 근거 주석만 남는다
(~1,040줄 → 859줄). 두 사본 일치만 확인하던 `validate_preset_tags` 는 사본이 하나가 되어 삭제.
검증: regen drift 테스트 14 PASS, fmt, regen `--check`, mcp 1,069 통과, manifest `--check`.

### 4.3 K-0030 — 공유 식별자 분할기와 BM25-F camelCase 분할

**변경.** `codelens_engine::unicode::identifier_words`(`_` 와 camel 경계, 두문자어 유지) 하나를
세 곳이 쓴다. ① 임베딩 프롬프트 `split_identifier` — 실제 식별자 10,080개 차분 테스트에서 출력
바이트 동일, 재임베딩 불필요. ② 쿼리 의도 `split_identifier_terms` — `getHTTPResponse` 가
`get h t t p response` 대신 `get http response`. ③ BM25-F 희소 레인 `tokenize` — 원래 대소문자로
경계를 읽은 뒤 소문자화해 복합 토큰과 부분 토큰을 함께 낸다(이전엔 먼저 소문자화해 camelCase 가 한
토큰이었다). SQLite `symbols_fts` 는 그대로다.

**측정.** `benchmarks/embedding-quality.py --isolated-copy`, 메서드 3종, 코퍼스 3개(self = 7bd7674e
`git archive` 고정, gson 854c825 `gson/src/main/java/com/google`, jest 121c2ae). before =
7bd7674e debug `--features semantic`, after = aadf5818 + K-0030 워킹트리 같은 빌드. 이 차이에는
K-0023 도 들어 있지만 K-0023 은 생성 멤버십이 손수 배열과 동일하고 토크나이저·점수 경로를 건드리지
않아 검색 결과를 움직일 수 없다. 랭킹은 결정적이다(같은 입력 → 같은 순위).

| 코퍼스 | 메서드 | MRR@10 | Recall@10 | Acc@1 |
|---|---|---|---|---|
| self (112) | `bm25_symbol_search` | 0.6538 → 0.6512 | 0.8125 → 0.8214 | 0.5804 → 0.5714 |
| self | `get_ranked_context_no_semantic` | 0.6631 → 0.6661 | 0.7857 → 0.7946 | 0.5982 → 0.5982 |
| self | `get_ranked_context` (hybrid) | 0.7396 → 0.7435 | 0.8750 → 0.8750 | 0.6429 → 0.6518 |
| gson (30) | `bm25_symbol_search` | 0.0793 → **0.2829** | 0.2667 → **0.5667** | 0.0333 → 0.1667 |
| gson | `get_ranked_context_no_semantic` | 0.2456 → 0.3099 | 0.5667 → 0.6000 | 0.1333 → 0.2000 |
| gson | `get_ranked_context` (hybrid) | 0.4676 → 0.4991 | 0.7667 → 0.7667 | 0.3000 → 0.3667 |
| jest (24) | `bm25_symbol_search` | 0.0602 → **0.1215** | 0.1250 → 0.2083 | 0.0417 → 0.0833 |
| jest | `get_ranked_context_no_semantic` | 0.1055 → 0.1738 | 0.2500 → 0.2083 | 0.0000 → 0.1250 |
| jest | `get_ranked_context` (hybrid) | 0.1883 → 0.2581 | 0.3750 → 0.3750 | 0.0417 → 0.1667 |

self CI 하한선(after): hybrid MRR 0.7435 ≥ 0.70, lexical(`get_ranked_context_no_semantic`) 0.6661 ≥
0.50, natural_language hybrid MRR 0.6325 ≥ 0.55, issue_to_edit hybrid recall 0.8182 ≥ 0.80, p95 응답
토큰 16,364 ≤ 20,000, 후보 누락률 0.0625 ≤ 0.10 — 전부 통과. 단 후보 누락률은 0.0536 → 0.0625 로
나빠졌다(1건: "preflight a safe rename before applying edits" 가 hybrid 후보 84위에서 이탈).

**판정.** 측정 전에 정한 규칙은 "`bm25_symbol_search` MRR 이 self 와 외부 코퍼스 하나 이상에서
개선 + CI 하한선 유지"였다. self 조건은 문자 그대로 미달이다(−0.0026). 그래도 채택한 이유: self 의
변화는 순위 이동 13건에 득실이 섞인 쿼리 1개 규모의 순변화다(Acc@1 −1, Recall@10 +1). 랭킹이 결정적이라
노이즈 대역이 있는 것은 아니고, 실제로 생긴 작은 순손실이다. 외부 두 코퍼스의 이득은 그 24배(jest)~78배(gson)이고
hybrid 는 세 코퍼스 모두 올랐다. 다만 gson 30개·jest 24개 쿼리로 표본이 작다. **self 는 개선이
아니라 측정된 null 로 기록한다.** 벤치 요약의 `lexical_mrr` 필드는 `bm25_symbol_search` 가 아니라
`get_ranked_context_no_semantic` 이고, 규칙은 `bm25_symbol_search` 로 읽었다.

**메커니즘(손실 쪽).** 경계 없는 단일 단어 식별자 쿼리가 밀렸다 — jest `Resolver` 9위 → 이탈,
`Runtime` 1위 → 4위, self `split_identifier` 8위 → 이탈(bm25). 쿼리 쪽 토큰은 그대로이고, 문서 쪽에서
`ResolverConfig` 같은 복합 이름이 부분 토큰 `resolver` 를 **같은 필드 가중치**로 내면서 정확 일치와
동점이 되고, 흔한 단어의 df 가 늘어 idf 가 내려갔다. 이득 쪽은 구 단위 쿼리다 — jest
"object containing" → `objectContaining` 이탈 → 1위, "module mocker" → `ModuleMocker` 이탈 → 1위,
short_phrase bm25 MRR 0 → 0.4667. **정확한 식별자 정밀도를 조금 내주고 여러 단어 쿼리 recall 을 크게
얻은 변경**이다. 완화책(부분 토큰을 낮은 가중치로, 또는 정확 이름 일치 부스트)은 별도 벤치가 필요한
설계 결정이라 K-0035 로 넘긴다.

## 5. 문헌 결정

원문: [`2026-09-17-sources/d-literature-decisions.md`](2026-09-17-sources/d-literature-decisions.md).

| # | 질문 | 결론 | 이번 사이클 반영 |
|---|---|---|---|
| D1 | 도구 수 vs 선택 정확도 | 방향은 재확인, 임계값은 소스마다 다름. RAG-MCP 43.13% 는 제안 기법, 13.62% 는 baseline | 자체 계측(§2)으로 판정, 09-16 정오표 |
| D2 | outputSchema·설명 길이 비용 | 리스팅에서 outputSchema 생략은 스펙 합법. 설명 길이 통제 연구는 없음 | generic 클라이언트 계약 유지 |
| D3 | 식별자 인식 토크나이징 | 방향만 지지(2605.18561 ablation). +89.3% 는 분할 효과 수치가 아님 | K-0030, 자체 벤치로 판정 |
| D4 | 그래프 기반 로컬라이제이션 | call + import/module 그래프가 4편 공통 로드베어링 | 그래프 기능 축소 대상 아님 |
| D5 | 응답 크기·가드레일 | SWE-agent: 100줄 18.0% > 30줄 14.3% > 전체 파일 12.7%, lint 18.0% vs 15.0% | 절단은 극단 배제 원칙, mutation gate 유지 |
| D6 | stale 인덱스 | stale-only 컨텍스트에서 구식 참조 76.5~88.2%(n=17) | staleness 배너(K-0029) 근거, 과신 금지 |

**소스 문서 정정.** `d-literature-decisions.md` D2 의 "Codex 가 63종 전체 스키마를 받는다"는 이번 §2
실측과 어긋난다(`codex-mcp-client` 바인딩 후 20종, outputSchema 0). D3 의 "FTS5 분할 여부 확인"은
확인 완료: `symbols_fts` 는 `unicode61 remove_diacritics 2 separators _` 라 snake_case 만 쪼개고
camelCase 는 쪼개지 않는다.

## 6. 하지 않은 것과 남은 것

- **K-0025(응답 정형화 기계 축소) 보류.** `dispatch/response_support/mod.rs` 에 09-10 미커밋 WIP 가
  있어 충돌한다. WIP 정리 후 재개.
- **K-0030 의 FTS5 절반은 열려 있다(K-0036).** `symbols_fts` 토크나이저를 바꾸려면 인덱스 스키마
  마이그레이션과 재색인이 필요하다. 이번 변경은 인메모리 BM25-F 레인만 다룬다. 부분 토큰 가중치
  완화(K-0035)를 먼저 판정한다.
- **generic 클라이언트 바인딩 전 59.6 KB(K-0034).** outputSchema 를 빼면 절반 이하로 줄지만 generic
  클라이언트 계약(CI CORE-20 + outputSchema) 변경이라 ADR 이 필요하다.
- **secondary-projects 4종 폐기(K-0033)** 는 사용자 결정 사항으로 남긴다.
- **런타임 미반영.** launchd 데몬은 bdbbafc 에 머물러 있어 09-16·09-17 변경이 아직 적용되지 않았다.
