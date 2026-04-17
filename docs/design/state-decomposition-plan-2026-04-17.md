# state.rs 3-host 분해 계획 — 2026-04-17

> P1-7. 990줄 중 774줄이 `impl AppState { ... }` 한 블록. 외부 호출 시그니처 보존한 채로 **impl 블록만** 3개 책임 영역으로 이관.

## 범위 (엄수)

**허용 파일**:

- `crates/codelens-mcp/src/state.rs` (축소)
- `crates/codelens-mcp/src/state/session_host.rs` (신규)
- `crates/codelens-mcp/src/state/embedding_host.rs` (신규)
- `crates/codelens-mcp/src/state/metrics_host.rs` (신규)

**금지**:

- `AppState` 구조체 필드 변경
- 메서드 시그니처 변경 / 리네임
- 소비자 (`tools/*`, `dispatch/*`, `server/*`) 수정
- 새 trait 추가
- 기존 state/ 서브모듈 (`analysis.rs`, `coordination.rs`, `preflight.rs`, `project_runtime.rs`, `session_runtime.rs`, `watcher_health.rs`) 수정 금지

Rust는 동일 struct에 대한 `impl` 블록을 여러 파일에 나눌 수 있음 — 메서드를 이동해도 외부에서는 `app_state.method()` 호출이 그대로 동작.

## 3-host 매핑

### A. `embedding_host.rs` (~70줄)

`impl AppState`에서 이관할 메서드 (state.rs 라인 번호):

- `embedding_engine` (313)
- `embedding_ref` (338)
- `reset_embedding` (344)
- `scip` (352)

### B. `session_host.rs` (~250줄)

- `current_project_scope` (183)
- `project_scope_for_session` (187)
- `project_scope_for_arguments` (197)
- `default_project_scope` (202)
- `push_recent_tool_for_session` (245)
- `recent_tools_for_session` (253)
- `record_file_access_for_session` (260)
- `recent_file_paths_for_session` (268)
- `doom_loop_count_for_session` (278)
- `bind_project_to_session` (288)
- `ensure_session_project` (293, 301 — 두 cfg 변형)
- `with_session_store` (888)
- `active_session_count` (896, 917 — cfg 변형)
- `session_timeout_seconds` (904, 922)
- `session_resume_supported` (912, 927)

### C. `metrics_host.rs` (~90줄)

- `metrics` (613)
- `push_recent_tool` (618)
- `doom_loop_count` (628)
- `recent_tools` (652)
- `record_file_access` (657)
- `recent_file_paths` (662)
- `push_recent_analysis_id` (667)
- `recent_analysis_ids` (672)
- `token_budget` (677)
- `set_token_budget` (682)

### state.rs 유지 (~500줄)

- module-level utilities (`preflight_ttl_ms`, `push_unique_string`, `normalize_path_for_project`)
- re-exports (`ActiveAgentEntry`, `ClientProfile` 등)
- `AppState` struct 정의 + 필드
- `SecondaryProject` struct
- impl AppState에 남는 메서드: project lifecycle (new, new*minimal, build, clone_for_worker), switch_project, reset_project, is_default_project, lsp_pool, project, symbol_index, watcher*_, graph*cache, memories_dir, analysis_dir, artifact_store, audit_dir, surface/set_surface, configure_daemon_mode/transport_mode, transport_mode, daemon_mode, client_profile, effort_level/set_effort_level, mutation_allowed_in_runtime, analysis*_, secondary project ops, execution_surface/budget/set_session_surface_and_budget, daemon_started_at, now_ms, active_project_context/build_project_runtime_context/activate_project_context, extract_symbol_hint
- `#[cfg(test)] mod tests { ... }` 블록

목표: state.rs ≤ 700줄 (990 → 500대 이상이면 성공)

## 실행 단계

1. `verify_change_readiness` with `state.rs` → ready 확인
2. baseline: `cargo check -p codelens-mcp --features http` 통과 확인
3. baseline: `cargo test -p codelens-mcp` 통과 확인
4. `state/embedding_host.rs`, `state/session_host.rs`, `state/metrics_host.rs` 3개 신규 파일 생성
5. 각 파일 상단에 필요한 `use` 문 복사 (기존 state.rs top imports 참고)
6. `impl AppState { ... }` 블록을 각 파일에 두고 해당 메서드만 이동
7. `state.rs`에서 해당 메서드 삭제 + `mod session_host; mod embedding_host; mod metrics_host;` 선언
8. 검증: `cargo check -p codelens-mcp` (default features)
9. 검증: `cargo check -p codelens-mcp --features http`
10. 검증: `cargo check -p codelens-mcp --no-default-features`
11. 검증: `cargo test -p codelens-mcp` (248 tests 모두 통과)
12. 검증: `cargo clippy --workspace --all-features` (new warning 0)
13. LOC 확인: `wc -l crates/codelens-mcp/src/state.rs crates/codelens-mcp/src/state/*.rs`

## 수락 기준

- [ ] 모든 검증 명령 0 error, 0 new warning
- [ ] state.rs LOC ≤ 700
- [ ] 외부 호출자 수정 0 (tools/_, dispatch/_, server/\* 빌드 영향 없음)
- [ ] 기존 248 mcp 테스트 모두 통과
- [ ] cfg gate 변형 (http, semantic) 모두 빌드 성공

## 하지 말 것

- 메서드 로직 변경 / 리네임 / 시그니처 정리
- 필드 재조직
- 추가 trait 도입
- 기존 state/ 서브모듈 내용 변경
- 새 helper fn 추가

## 롤백

어떤 검증 단계라도 FAIL 시 전체 롤백.

## 후속 작업 (본 계획 범위 외)

- AppState 구조체 필드 세분화 (별 세션)
- secondary project ops 별도 host 분리 (별 세션)
