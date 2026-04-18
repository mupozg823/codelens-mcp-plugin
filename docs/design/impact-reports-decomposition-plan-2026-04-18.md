# tools/reports/impact_reports.rs 분해 계획 — 2026-04-18

P2. 1,164줄 파일을 `tools/reports/impact_reports/` 디렉토리로 재구성.
embedding/state/symbols/query_analysis/dispatch/metrics_config 6건 분해와 동일 기조 — 순수 이동, 외부 API 파괴 0.

## 범위 (엄수)

**허용**:

- `crates/codelens-mcp/src/tools/reports/impact_reports.rs` 삭제
- `crates/codelens-mcp/src/tools/reports/impact_reports/mod.rs` 신규 facade
- `crates/codelens-mcp/src/tools/reports/impact_reports/helpers.rs` 신규 (private helper 모음)
- `crates/codelens-mcp/src/tools/reports/impact_reports/mermaid.rs` 신규 (render_module_mermaid + mermaid_module_graph)
- `crates/codelens-mcp/src/tools/reports/impact_reports/boundary.rs` 신규 (module_boundary_report + dead_code_report)
- `crates/codelens-mcp/src/tools/reports/impact_reports/impact.rs` 신규 (impact_report + diff_aware_references)
- `crates/codelens-mcp/src/tools/reports/impact_reports/refactor.rs` 신규 (refactor_safety_report + semantic_code_review)

**금지**:

- `tools/reports/mod.rs`의 `mod impact_reports;` + `pub use impact_reports::{...}` 선언 변경
- 모든 public 함수 시그니처 변경
- 소비자 수정 (`report_jobs.rs`, `dispatch/*`, `tool_defs/*`, `integration_tests/*`)
- 로직/리네임/cfg gate 변경

## 외부 API (정확히 보존)

`tools/reports/mod.rs` 에서 현재 re-export:

```rust
pub use impact_reports::{
    dead_code_report, diff_aware_references, impact_report, mermaid_module_graph,
    module_boundary_report, refactor_safety_report, semantic_code_review,
};
```

모두 그대로 호출 가능해야 함.

## 파일 매핑 (예상)

| 파일          | LOC 예측 | 라인 범위 (v1.9.44) | 포함                                                                                                                                                                                                                          |
| ------------- | -------: | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `helpers.rs`  |     ~100 | 11-110              | semantic_status_is_ready, push_unique, semantic_degraded_note, insert_semantic_status, path_hint, build_module_semantic_query, build_dead_code_semantic_query, impact_entry_file, mermaid_escape_label, parent_dir, file_name |
| `mermaid.rs`  |     ~210 | 114-318             | render_module_mermaid, mermaid_module_graph                                                                                                                                                                                   |
| `boundary.rs` |     ~220 | 319-536             | module_boundary_report, dead_code_report                                                                                                                                                                                      |
| `impact.rs`   |     ~380 | 537-912             | impact_report, diff_aware_references                                                                                                                                                                                          |
| `refactor.rs` |     ~260 | 913-end             | refactor_safety_report, semantic_code_review                                                                                                                                                                                  |
| `mod.rs`      |      ≤40 | —                   | `mod helpers; mod mermaid; mod boundary; mod impact; mod refactor;` + `pub use` 재노출                                                                                                                                        |

합계 ≤ 1,210줄 (현재 1,164, ~4% facade 오버헤드 허용).

## 가시성 규칙

- 외부 공개 함수는 `mod.rs`의 `pub use`로 재노출
- 모듈 간 공유 helper는 `pub(super)`
- 파일 내부 private: `fn foo(...)`

## 실행 단계

1. `verify_change_readiness` with `tools/reports/impact_reports.rs` → ready 확인
2. baseline:
   - `cargo check -p codelens-mcp --features http` clean
   - `cargo test -p codelens-mcp` 263 pass
   - `cargo test -p codelens-mcp --features http` 316 pass
3. `mkdir crates/codelens-mcp/src/tools/reports/impact_reports/` + 6 파일 생성 + 라인 범위 통째 이동 (주석 포함)
4. `tools/reports/impact_reports.rs` 삭제
5. `impact_reports/mod.rs` 작성 (`mod` + `pub use` 재노출)
6. 검증:
   - `cargo check -p codelens-mcp --features http` + default + `--no-default-features` 전부 clean
   - `cargo test -p codelens-mcp` 263 pass 유지
   - `cargo test -p codelens-mcp --features http` 316 pass 유지
   - `cargo clippy --workspace --all-features` 경고 수 동일

## 수락 기준

- 모든 검증 명령 0 error, 0 new warning
- `mod.rs` ≤ 40, `helpers.rs` ≤ 130, `mermaid.rs` ≤ 240, `boundary.rs` ≤ 250, `impact.rs` ≤ 420, `refactor.rs` ≤ 300
- 외부 소비자 수정 0 (`git diff main -- crates/codelens-mcp/src/tools/report_jobs.rs crates/codelens-mcp/src/dispatch/ crates/codelens-mcp/src/integration_tests/ crates/codelens-mcp/src/tool_defs/` 빈 diff)
- mcp default 263 / mcp http 316 테스트 통과 유지
- `#[cfg(feature = "semantic")]` 가드 위치 변경 0

## 하지 말 것

- 로직 개선, 리네임, 추가 helper 추출, cfg gate 재배치
- semantic\_\*\_note 들의 본체 수정
- `impact_report`의 batch_semantic_enrichment 블록 재조직

## 롤백

FAIL 시 전체 롤백. 부분 반영 금지.
