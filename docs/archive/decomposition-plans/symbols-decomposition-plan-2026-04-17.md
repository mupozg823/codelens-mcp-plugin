# tools/symbols.rs 3-split 계획 — 2026-04-17

> P1-8. 987줄 단일 파일을 analyzer/handlers/formatter로 분해. 외부 호출 경로 `crate::tools::symbols::*` 정확히 보존.

## 범위 (엄수)

**허용 파일** (신규 디렉토리 구조):

- `crates/codelens-mcp/src/tools/symbols.rs` **삭제**
- `crates/codelens-mcp/src/tools/symbols/mod.rs` (신규, facade + re-exports)
- `crates/codelens-mcp/src/tools/symbols/analyzer.rs` (신규)
- `crates/codelens-mcp/src/tools/symbols/handlers.rs` (신규)
- `crates/codelens-mcp/src/tools/symbols/formatter.rs` (신규)

**금지**:

- `tools/mod.rs`의 `pub mod symbols;` 선언 수정 금지
- 소비자 (`tools/graph.rs`, `tools/workflows.rs`, `tools/reports/*`, `dispatch/table.rs`) 수정 금지
- 모든 public 함수의 시그니처 변경 금지
- 로직 변경, 리네임, trait 추가 금지

## 외부 API (정확히 보존)

호출자가 사용하는 경로 (소비자 grep 결과):

- `crate::tools::symbols::flatten_symbols`
- `crate::tools::symbols::get_ranked_context`
- `crate::tools::symbols::find_symbol`
- `crate::tools::symbols::get_symbols_overview`
- `crate::tools::symbols::semantic_results_for_query`
- `crate::tools::symbols::semantic_status`

모두 `symbols/mod.rs`에서 `pub use X::*`로 재노출하여 소비자 수정 0.

## 3-split 매핑

### `analyzer.rs` (~260줄)

Semantic enrichment 헬퍼 (cfg-gated 두 변형 모두):

- `semantic_status` (17–75, 76–99 두 cfg)
- `semantic_results_for_query` (100–140, 141–149 두 cfg)
- `semantic_scores_for_query` (150–165)
- `merge_semantic_ranked_entries` (166–244)
- `compact_semantic_evidence` (245–271)
- `annotate_ranked_context_provenance` (272–320)

### `formatter.rs` (~110줄)

Preview / body shaping + complexity 카운터:

- `truncate_body_preview` (321–341)
- `compact_symbol_bodies` (342–365)
- `count_branches` (755–758)
- `count_branches_in_line` (759–771)
- `count_word_occurrences` (772–end)

### `handlers.rs` (~400줄)

Tool handler fn (각자 `ToolResult` 반환):

- `get_symbols_overview` (366–410)
- `find_symbol` (411–490)
- `get_ranked_context` (491–623)
- `refresh_symbol_index` (624–629)
- `get_complexity` (630–688)
- `get_project_structure` (689–703)
- `search_symbols_fuzzy` (704–743)
- `flatten_symbols` (744–754) — public helper

### `mod.rs` (≤ 30줄)

- `mod analyzer; mod formatter; mod handlers;`
- `pub use analyzer::{semantic_status, semantic_results_for_query};`
- `pub use handlers::{flatten_symbols, get_ranked_context, find_symbol, get_symbols_overview, refresh_symbol_index, get_complexity, get_project_structure, search_symbols_fuzzy};`
- 내부 cross-module 공유 items는 `pub(super) use`

## 가시성 규칙

- 외부 공개: 소비자가 쓰는 6개만 `pub` (위 리스트)
- 내부 cross-module (예: analyzer가 handlers의 helper 호출): `pub(super)`
- 파일 내부 static: private

## 실행 단계

1. `verify_change_readiness` with symbols.rs → ready 확인
2. baseline: `cargo check -p codelens-mcp --features http` 통과 확인
3. baseline: `cargo test -p codelens-mcp` (250 tests 통과 확인)
4. `tools/symbols/` 디렉토리 생성
5. analyzer.rs/handlers.rs/formatter.rs에 해당 내용 복사 (함수 경계 기준, 주석 포함)
6. mod.rs에 `mod` 선언 + `pub use` 재노출
7. 기존 `tools/symbols.rs` 삭제
8. 검증: `cargo check -p codelens-mcp` (default features)
9. 검증: `cargo check -p codelens-mcp --features http`
10. 검증: `cargo check -p codelens-mcp --no-default-features`
11. 검증: `cargo test -p codelens-mcp` (250 tests)
12. 검증: `cargo clippy --workspace --all-features` (new warning 0)
13. LOC 확인: `wc -l crates/codelens-mcp/src/tools/symbols/*.rs`

## 수락 기준

- [ ] 모든 검증 명령 0 error, 0 new warning
- [ ] `symbols/mod.rs` ≤ 30줄
- [ ] `analyzer.rs` ≤ 300줄
- [ ] `handlers.rs` ≤ 450줄
- [ ] `formatter.rs` ≤ 150줄
- [ ] 외부 소비자 (`tools/graph.rs`, `tools/workflows.rs`, `tools/reports/*`, `dispatch/table.rs`, `tools/mod.rs`) 수정 0
- [ ] 기존 250 mcp 테스트 모두 통과
- [ ] cfg gate 변형 (http, semantic, no-default) 모두 빌드 성공

## 하지 말 것

- 함수 로직 개선 / 리네임 / 시그니처 정리
- `get_ranked_context`(132줄) 자체 추가 분해 — 본 계획 범위 외
- analyzer 내부 helper를 engine crate로 이동
- 새 helper fn 추출

## 롤백

어떤 검증 단계라도 FAIL 시 전체 롤백.

## 후속 작업 (본 계획 범위 외)

- `get_ranked_context` 132줄 내부 분해 (별 세션)
- semantic cfg 변형 통합 (별 세션)
- complexity 카운터를 별도 engine crate fn으로 이관 (별 세션)
