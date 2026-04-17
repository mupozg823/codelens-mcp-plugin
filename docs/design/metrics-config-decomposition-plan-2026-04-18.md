# tools/session/metrics_config.rs 분해 계획 — 2026-04-18

P2. 1,252줄 god module을 `tools/session/metrics_config/` 디렉토리로 재구성.
embedding/state/symbols/query_analysis 4건 분해와 동일 기조 — 순수 이동, 외부 API 파괴 0.

## 범위 (엄수)

**허용**:

- `crates/codelens-mcp/src/tools/session/metrics_config.rs` 삭제
- `crates/codelens-mcp/src/tools/session/metrics_config/mod.rs` 신규 facade
- `crates/codelens-mcp/src/tools/session/metrics_config/capabilities.rs` 신규
- `crates/codelens-mcp/src/tools/session/metrics_config/preset_profile.rs` 신규
- `crates/codelens-mcp/src/tools/session/metrics_config/metrics.rs` 신규
- `crates/codelens-mcp/src/tools/session/metrics_config/watch_prune.rs` 신규
- `crates/codelens-mcp/src/tools/session/metrics_config/tests.rs` 신규 (필요 시)

**금지**:

- `tools/session/mod.rs`의 `pub(crate) mod metrics_config;` 선언 변경
- 모든 public 함수 시그니처 변경 (아래 외부 API 표 참고)
- 소비자 수정 (`dispatch/*`, `integration_tests/*`, `tool_defs/*`)
- 로직/리네임/cfg gate 변경

## 외부 API (정확히 보존)

`tools/session/mod.rs` 에서 현재 re-export되는 것:

```rust
pub use metrics_config::{
    get_capabilities, get_current_config, get_tool_metrics,
    set_preset, set_profile, set_daemon_mode,
    export_session_markdown,
    get_watch_status, prune_index_failures,
    DiagnosticsStatus, DiagnosticsGuidance,
    SemanticSearchStatus, SemanticSearchGuidance,
};
```

모두 그대로 호출 가능해야 함.

## 파일 매핑 (예상)

| 파일                | LOC 예측 | 포함                                                                                                                                   |
| ------------------- | -------: | -------------------------------------------------------------------------------------------------------------------------------------- |
| `capabilities.rs`   |     ~380 | `DiagnosticsStatus`, `DiagnosticsGuidance`, `SemanticSearchStatus`, `SemanticSearchGuidance`, `get_capabilities`, `get_current_config` |
| `preset_profile.rs` |     ~280 | `set_preset`, `set_profile`, `set_daemon_mode`, visible-surface side-effect handling                                                   |
| `metrics.rs`        |     ~330 | `get_tool_metrics`, `export_session_markdown`, latency histogram helpers, coordination scope lookup                                    |
| `watch_prune.rs`    |     ~180 | `get_watch_status`, `prune_index_failures`                                                                                             |
| `tests.rs`          |    ≤ 150 | 기존 `#[cfg(test)] mod tests` 있으면 통째 이동                                                                                         |
| `mod.rs`            |     ≤ 60 | `mod capabilities; mod preset_profile; mod metrics; mod watch_prune;` + `pub(crate) use` 재노출                                        |

합계 ≤ 1,380줄 (현재 1,252에서 ~10% facade 오버헤드).

## 가시성 규칙

- 외부 공개 함수는 `mod.rs`의 `pub(crate) use`로 재노출
- 모듈 간 공유 helper는 `pub(super)`
- 파일 내부 private은 `fn foo(...)`

## 실행 단계

1. `verify_change_readiness` with `tools/session/metrics_config.rs` → ready 확인
2. baseline: `cargo check -p codelens-mcp --features http`, `cargo test -p codelens-mcp` (263) + `-p codelens-mcp --features http` (315)
3. `mkdir crates/codelens-mcp/src/tools/session/metrics_config/` + 파일 5개 생성 + 주석 포함 통째 이동
4. `tools/session/metrics_config.rs` 삭제
5. `tools/session/metrics_config/mod.rs` 작성 (`mod` + `pub(crate) use` 재노출)
6. 검증: `cargo check` (default + http + no-default-features), `cargo test -p codelens-mcp` (263) + `-p codelens-mcp --features http` (315), `cargo clippy --workspace --all-features`

## 수락 기준

- 모든 검증 명령 0 error, 0 new warning
- `mod.rs` ≤ 60, `capabilities.rs` ≤ 420, `preset_profile.rs` ≤ 320, `metrics.rs` ≤ 380, `watch_prune.rs` ≤ 220
- 외부 소비자 수정 0 (`git diff main -- crates/codelens-mcp/src/dispatch/ crates/codelens-mcp/src/integration_tests/ crates/codelens-mcp/src/tool_defs/` 빈 diff)
- mcp default 263 / mcp http 315 테스트 통과
- `#[cfg(feature = "semantic")]` 가드 위치 변경 0 (feature off 시 동일 심볼 집합 제외)

## 하지 말 것

- 로직 개선, 리네임, 추가 helper 추출, cfg gate 재배치
- `Diagnostics*` / `SemanticSearch*` enum/struct 필드 수정
- `export_session_markdown` / `get_tool_metrics` 본체 단축/통합

## 롤백

검증 단계라도 FAIL 시 전체 롤백.

## 후속 작업 (범위 외)

- `metrics.rs` 내부의 `build_session_metrics_payload` 관련 branch 추가 세분화 — 별 세션
- `tools/reports/impact_reports.rs` (1,156줄) 분해 — 별 세션, 같은 레시피 재사용
