# tools/query_analysis.rs 분해 계획 — 2026-04-17

P2-1. 1,191줄 단일 파일을 `tools/query_analysis/` 디렉토리로 재구성.
embedding/state/symbols 3건 분해와 동일 기조 — 순수 이동, 외부 API 파괴 0.

## 범위 (엄수)

**허용**:

- `crates/codelens-mcp/src/tools/query_analysis.rs` 삭제
- `crates/codelens-mcp/src/tools/query_analysis/mod.rs` 신규 facade
- `crates/codelens-mcp/src/tools/query_analysis/intent.rs` 신규
- `crates/codelens-mcp/src/tools/query_analysis/bridge.rs` 신규
- `crates/codelens-mcp/src/tools/query_analysis/rerank.rs` 신규
- `crates/codelens-mcp/src/tools/query_analysis/expansion.rs` 신규
- `crates/codelens-mcp/src/tools/query_analysis/tests.rs` 신규

**금지**:

- `tools/mod.rs`의 `pub mod query_analysis;` 선언 변경
- 모든 public 함수 시그니처 변경 (`analyze_retrieval_query`,
  `semantic_query_for_retrieval`, `semantic_query_for_embedding_search`,
  `rerank_semantic_matches`, `semantic_adjusted_score_parts`)
- 소비자(`tools/symbols/*`, `dispatch/table.rs`) 수정
- 로직/리네임/cfg gate 변경

## 외부 API (정확히 보존)

모두 `pub(crate)`이며, `crate::tools::query_analysis::X` 경로에서 동일 작동.

- `RetrievalQueryAnalysis` (struct)
- `analyze_retrieval_query(&str) -> RetrievalQueryAnalysis`
- `semantic_query_for_retrieval(&str) -> String`
- `semantic_query_for_embedding_search(&RetrievalQueryAnalysis, Option<&Path>) -> String` (semantic cfg only)
- `rerank_semantic_matches(query, matches, limit, hybrid_path) -> Vec<SemanticMatch>` (semantic cfg only)
- `semantic_adjusted_score_parts(&str, &SemanticMatch) -> (f64, f64)` (semantic cfg only)

## 파일 매핑

| 파일           | LOC 예측 | 라인 범위 (v1.9.38) | 포함                                                                                                                                                                                                                                                       |
| -------------- | -------: | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `intent.rs`    |     ~130 | 17-195              | query_prefers_lexical_only, is_natural_language_query, has_entrypoint_cue/helper_cue/builder_cue, exact_retrieval_aliases, specific_find_aliases, split_identifier_terms, semantic_identifier_query, analyze_retrieval_query, semantic_query_for_retrieval |
| `bridge.rs`    |     ~120 | 200-309             | `#[cfg(feature = "semantic")]` semantic_query_for_embedding_search, load_project_bridges, bridge_nl_to_code_vocabulary                                                                                                                                     |
| `rerank.rs`    |     ~230 | 311-530             | `#[cfg(feature = "semantic")]` prefers_semantic_entrypoint_prior, is_natural_language_semantic_query, semantic_result_prior, semantic_adjusted_score_with_lower, semantic_adjusted_score_parts, rerank_semantic_matches                                    |
| `expansion.rs` |     ~180 | 532-709             | expand_retrieval_query (+ 관련 helper가 있다면)                                                                                                                                                                                                            |
| `tests.rs`     |     ~480 | 710-end             | 기존 `#[cfg(test)] mod tests { ... }` 통째                                                                                                                                                                                                                 |
| `mod.rs`       |     ≤ 40 | —                   | `mod intent; mod bridge; mod rerank; mod expansion;` + `pub(crate) use` 재노출 + `RetrievalQueryAnalysis` struct 본체 (또는 intent.rs에 두고 re-export) + `#[cfg(test)] mod tests;`                                                                        |

`RetrievalQueryAnalysis` struct는 intent 모듈에 정의 + mod.rs에서 `pub(crate) use intent::RetrievalQueryAnalysis;` 재노출하는 편이 깔끔.

## 가시성 규칙

- 외부 공개(`pub(crate)`) 5개는 mod.rs의 `pub(crate) use`로 재노출.
- 모듈 간 공유 helper는 `pub(super)`.
- 파일 내부 private: `fn foo(...)`.

## 실행 단계

1. `verify_change_readiness` with `tools/query_analysis.rs` → ready 확인.
2. baseline: `cargo check -p codelens-mcp --features http`, `cargo test -p codelens-mcp` (250 pass), `cargo test -p codelens-mcp --features http` (298 pass).
3. `mkdir crates/codelens-mcp/src/tools/query_analysis/` + 5 파일 생성 + 라인 범위 이동 (주석 포함 통째 복사).
4. `tools/query_analysis.rs` 삭제.
5. `tools/query_analysis/mod.rs` 작성 (`mod` + `pub(crate) use` 재노출 + cfg-test tests mod).
6. 검증: `cargo check` (default + http + no-default-features), `cargo test -p codelens-mcp` (250) + `-p codelens-mcp --features http` (298), `cargo test -p codelens-engine --lib --features semantic` (278), `cargo clippy --workspace --all-features` (경고 수 동일).

## 수락 기준

- 모든 검증 명령 0 error, 0 new warning.
- `mod.rs` ≤ 40, `intent.rs` ≤ 180, `bridge.rs` ≤ 150, `rerank.rs` ≤ 280, `expansion.rs` ≤ 220, `tests.rs` ≤ 550.
- 외부 소비자 수정 0 (`git diff main -- crates/codelens-mcp/src/tools/symbols/ crates/codelens-mcp/src/dispatch/` 빈 diff).
- engine 278 / mcp default 250 / mcp http 298 테스트 통과.
- `#[cfg(feature = "semantic")]` 가드 위치 변경 0 (feature off 시 동일 심볼 집합 제외).

## 하지 말 것

- 로직 개선, 리네임, 추가 helper 추출, cfg gate 재배치.
- `RetrievalQueryAnalysis` 필드 수정.
- `analyze_retrieval_query` 본체 단축/통합.
- `expand_retrieval_query`의 대형 분기 재작성.

## 롤백

검증 단계라도 FAIL 시 전체 롤백.

## 후속 작업 (범위 외)

- `rerank.rs` 내 `semantic_result_prior` (약 170줄) 추가 세분화 — 별 세션.
- `expand_retrieval_query` 본체 분기 재조직 — 별 세션.
