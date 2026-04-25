# Phase 0 — Mutation Surface Truthing (옵션 B)

- 작성일: 2026-04-25
- 브랜치: `codex/ide-refactor-substrate` (in-flight)
- 단계: Phase 0 (1차 PR)
- 다음 단계: Phase 1 (G4 WorkspaceEditTransaction, G5 runtime capability truth, G7 engine fs::write 통합)

## 0. 배경 — 왜 이 PR인가

`codex/ide-refactor-substrate` 브랜치는 시니어 리뷰가 지적한 영역(semantic refactor / LSP substrate)을 6커밋(+6,848 / -1,107)에 걸쳐 절반쯤 빌드한 in-flight 상태다. 정찰 결과:

**이미 해결된 것**

- `ProductCapabilityRegistry` (`crates/codelens-mcp/src/backend_operation_matrix.rs`)에 `authority` / `can_preview` / `can_apply` / `verified` / `blocker_reason` / `failure_policy: fail_closed` 일관 정의
- `unsupported_semantic_refactor` fail-closed prefix가 engine/mcp 양쪽 13곳에서 일관되게 사용
- `semantic_edit` 도구가 `transactional_best_effort_with_rollback_evidence` 모델로 `transaction_id` / `file_hashes_before` 명시
- `tools/semantic_edit.rs` / `tools/semantic_adapter.rs` / `tools/lsp.rs` 응답에 `authority` / `authority_backend` / `can_preview` / `can_apply` / `edit_authority` 일관 노출
- `scripts/surface-manifest.py` CI 게이트 (`.github/workflows/ci.yml:78`)

**남은 갭 (이 PR이 닫는 것)**

| ID  | 갭                                                                                                                               | 위치                                    |
| --- | -------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- |
| G1  | `tools/mutation.rs` 11개 raw primitive가 `authority` / `can_apply` / `edit_authority` / `transaction` 필드 없이 raw fs 결과 반환 | `mutation.rs` 전반                      |
| G2  | tree-sitter 분기 `rename_symbol`이 transaction contract를 거치지 않고 raw `rename::rename_symbol` 호출 후 `fs::write`            | `mutation.rs:43-52`, `engine/rename.rs` |
| G3  | 모든 raw mutation이 `success_meta(BackendKind::Filesystem, 1.0)` — confidence 1.0이라는 misleading 신호                          | `mutation.rs` 전반                      |
| G6  | `verified: false` operation(extract/inline/move/change_signature)이 도구로 노출되는지 surface-manifest가 게이트하지 않음         | `scripts/surface-manifest.py`           |

**이번 PR이 명시적으로 다루지 않는 갭 (Phase 1로 명시 위임)**

| ID  | 갭                                                                                              |
| --- | ----------------------------------------------------------------------------------------------- |
| G4  | `safe_delete_apply`(`semantic_edit.rs:416`)의 자체 `std::fs::write` → WorkspaceEdit 경로로 이전 |
| G5  | 정적 capability matrix → runtime LSP 가용성 / `prepareRename` 지원 여부 probe 결과 반영         |
| G7  | engine 쪽 `rename` / `auto_import` / `inline` / `move_symbol` / `memory` 11곳 `fs::write` 통합  |

---

## 1. Scope & Goals

### 사용자가 보는 변경

- raw mutation primitive 11개(`delete_lines`, `replace_lines`, `replace_content`, `replace_symbol_body`, `insert_at_line`, `insert_before_symbol`, `insert_after_symbol`, `insert_content_tool`, `replace_content_unified`, `add_import`, `create_text_file`)가 응답에 `authority` / `can_preview` / `can_apply` / `edit_authority` 4개 필드를 항상 포함. agent가 이 필드를 읽으면 "이건 syntax-level raw fs 편집이고 semantic authority 없음"임을 즉시 알 수 있음.
- tree-sitter 백엔드의 `rename_symbol`은 `can_apply: false`로 강등. apply하려면 `semantic_edit_backend=lsp` (또는 `jetbrains` / `roslyn`) 명시 선택 필요.
- surface-manifest CI가 정합성 위반(`verified: false && can_apply: true`, capability matrix↔manifest 불일치) 발견 시 즉시 fail.

### 비범위 (이 PR 안 함)

- engine 쪽 `fs::write` 11곳 통합 (G7) → Phase 1
- WorkspaceEditTransaction 도메인 객체 (G4) → Phase 1
- runtime capability probing (G5) → Phase 1
- 새 mutation primitive 추가 (어떤 형태든)
- 기능 확장 (LSP 백엔드 추가, 새 operation 등)

### 성공 조건

- 옵션 B의 G1+G2+G3+G6 갭이 모두 닫힘
- 회귀 0: 기존 `cargo test -p codelens-engine`, `cargo test -p codelens-mcp`(`--features http` 포함), `lsp-boost-regression-check.py`, surface-manifest 다 그대로 통과
- evaluator(opus) PR 머지 직전 채점 PASS

---

## 2. Components

### A. `crates/codelens-mcp/src/tools/mutation.rs` (수정)

- 새 helper:
  ```rust
  fn raw_fs_envelope(tool: &str, file_path: &str, validator_kind: Option<&str>) -> Value
  ```
  발급 필드: `authority: "syntax"` / `can_preview: true` / `can_apply: true` / `edit_authority: { kind: "raw_fs", operation: tool, validator: validator_kind }`.
- 11개 primitive의 결과 객체에 `raw_fs_envelope(...)` 머지.
- `success_meta(BackendKind::Filesystem, 1.0)` → `success_meta(BackendKind::Filesystem, 0.7)`.
- `rename_symbol`의 tree-sitter 분기(line 24~52): `dry_run=false`인 경우 `CodeLensError::Validation` 발생; `dry_run=true`인 경우 응답 envelope에 `can_apply: false`, `authority: "syntax"`, `support: "syntax_preview"`, `blocker_reason: "tree-sitter rename is preview-only; select semantic_edit_backend=lsp (or jetbrains/roslyn) to apply"` 강제.

### B. envelope 영향 범위 (코드 변경 없음, 검증 항목)

- `BackendKind::Filesystem` enum 자체는 그대로. `success_meta(kind, conf)` 호출 시점의 confidence만 변경 — `mutation.rs` 내부 호출 11개만 0.7. 다른 호출자(`session/project_ops.rs:343`, `composite.rs` 등) 영향 0. 회귀 가드는 grep + AC-2 assertion test로 잠금.

### C. `scripts/surface-manifest.py` (수정)

- 새 contract A: `verified: false && can_apply: true` 조합 reject (`sys.exit(1)`).
- 새 contract B: capability matrix(`backend_operation_matrix.rs` JSON 덤프) 안의 `(operation, backend, languages)` 모든 항목이 manifest의 도구 노출과 1:1 합치하는지 검증. 위반 시 `sys.exit(1)`.
- 위반 시 stderr에 위반 항목 enumerate.

### D. `crates/codelens-mcp/src/integration_tests/mutation_envelope.rs` (신규)

- 11개 primitive 각각 fixture 기반 envelope assertion (T1).
- tree-sitter `rename_symbol` 강등 3 case (T2).
- `BackendKind::Filesystem` confidence ≤ 0.7 assertion.

### E. `crates/codelens-mcp/src/integration_tests/mod.rs` (1줄 추가)

- 모듈 등록.

### F. `scripts/test/test-surface-manifest-contracts.py` (신규)

- 4 fixture (positive 1 + negative 3): contract A 위반 / contract B 도구 누락 / contract B operation 미반영.
- pytest 또는 직접 subprocess 실행.

### G. `.github/workflows/ci.yml` (1~2줄)

- T3 contract test step 추가.

### H. `crates/codelens-mcp/src/bin/dump-matrix.rs` (신규)

- `cargo run --bin dump-matrix > /tmp/matrix.json` 단일 진실 출처.
- 출력은 `backend_operation_matrix::semantic_edit_operation_matrix()` 직렬화.
- `surface-manifest.py`가 이 출력을 입력으로 사용.

**예상 footprint**: 5 파일 수정, 3 파일 신규, 약 +400줄 / -30줄. 단일 PR로 리뷰 가능.

---

## 3. Data Flow

### 경로 1 — Raw mutation (11개 primitive)

```
agent → MCP dispatch
      → tools/mutation.rs::<primitive>
      → engine helper (delete_lines/replace_lines/...)
      → engine fs::write           [Phase 1에서 차단, 이 PR은 광고만]
      ← 결과 content/replacements
      ← raw_fs_envelope() 머지:
          authority: "syntax"
          can_preview: true
          can_apply: true            (raw fs는 항상 즉시 apply)
          edit_authority: { kind: "raw_fs", operation, validator: null }
      ← success_meta(BackendKind::Filesystem, 0.7)
      → agent
```

### 경로 2 — Tree-sitter rename (강등)

```
agent → rename_symbol(backend=tree-sitter or unset)
      → tools/mutation.rs::rename_symbol (TreeSitter 분기)
      → if dry_run==false:
          return CodeLensError::Validation(
            "tree-sitter rename is preview-only; ..."
          )
      → engine rename::rename_symbol(dry_run=true)
      ← preview 결과
      ← envelope:
          authority: "syntax"
          can_preview: true
          can_apply: false
          support: "syntax_preview"
          blocker_reason: "tree-sitter rename is preview-only; ..."
      → agent
```

### 경로 3 — Semantic edit (LSP / JetBrains / Roslyn) — 변경 없음

```
agent → rename_symbol(semantic_edit_backend=lsp)
      → semantic_edit::rename_symbol_with_lsp_backend
        또는 semantic_adapter::rename_with_local_adapter
      → WorkspaceEdit transaction contract (기존)
      ← authority: "workspace_edit", can_apply: true, transaction:{...}
      → agent
```

### 경로 4 — CI surface manifest

```
ci.yml → cargo run --bin dump-matrix > /tmp/matrix.json
       → scripts/surface-manifest.py < /tmp/matrix.json
       → contract A: verified=false && can_apply=true → fail
       → contract B: matrix 항목 ↔ manifest 도구 노출 1:1 → 불일치 시 fail
ci.yml → scripts/test/test-surface-manifest-contracts.py
```

### 핵심 결정점

1. **경로 2 강등 방식**: `dry_run=false`면 `Validation error`. `dry_run=true` 자동 force는 silent behavior change라서 거부. fail-closed 원칙 정합.
2. **경로 4 matrix 덤프 방법**: 신규 바이너리 `dump-matrix`. fixture 수기 동기화 또는 `cargo test` 시 갱신은 drift 위험.

---

## 4. Error Handling

| ID  | 트리거                                                                                   | 동작                                                                                                                                    | agent 가이드                                                                                        |
| --- | ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| E1  | `rename_symbol(backend=tree-sitter, dry_run=false)` 또는 backend unset + `dry_run=false` | `CodeLensError::Validation`                                                                                                             | `suggested_next_tools: ["rename_symbol with semantic_edit_backend=lsp", "verify_change_readiness"]` |
| E2  | raw mutation 호출                                                                        | 별도 에러 없음. envelope의 `can_apply: true` / `edit_authority.kind: "raw_fs"`가 정직 광고                                              | —                                                                                                   |
| E3  | surface-manifest contract 위반 (A 또는 B)                                                | `surface-manifest.py` `sys.exit(1)`, stderr enumerate. CI step fail                                                                     | —                                                                                                   |
| E4  | `dump-matrix` 컴파일/직렬화 실패                                                         | cargo step fail. `surface-manifest.py`는 stdin empty면 명확한 에러로 종료                                                               | —                                                                                                   |
| E5  | 기존 semantic_edit 회귀                                                                  | regression 가드: `integration_tests/semantic_refactor.rs`(574), `integration_tests/lsp.rs` 그대로 통과 필수. 이 PR은 응답 schema 변경 0 | —                                                                                                   |

**명시적으로 다루지 않는 것**

- engine 쪽 `fs::write` 11곳 자체 차단 (Phase 1, G7)
- runtime LSP 가용성 probe 실패 시 fallback (Phase 1, G5)
- `composite.rs:210` `fs::write` 우회 가능성 (Phase 1, G7과 함께)
- `WorkspaceEditTransaction` rollback 실패 시 부분 적용 상태 (Phase 1, G4)

---

## 5. Testing

### T1 — Envelope contract test (신규)

파일: `crates/codelens-mcp/src/integration_tests/mutation_envelope.rs`

- 11개 raw primitive 각각: 임시 fixture project → 도구 호출 → 응답 JSON에서 4필드 + `edit_authority.{kind, operation, validator}` assertion.
- `BackendKind::Filesystem` 사용처 confidence ≤ 0.7 assertion.
- 도구당 1 case (총 11 case).

### T2 — Tree-sitter rename 강등 (신규, T1과 같은 파일)

- `rename_symbol(backend=tree-sitter, dry_run=false)` → `Validation` 매치
- `rename_symbol(backend=tree-sitter, dry_run=true)` → `can_apply == false`, `support == "syntax_preview"`, `blocker_reason` non-empty
- backend unset + `dry_run=false` → 동일 `Validation`
- 총 3 case

### T3 — Surface-manifest contract test (신규)

파일: `scripts/test/test-surface-manifest-contracts.py`

- Fixture A (positive): 정상 matrix + manifest → exit 0
- Fixture B (negative): `verified: false && can_apply: true` 항목 주입 → exit 1
- Fixture C (negative): manifest에서 도구 1개 제거 → exit 1
- Fixture D (negative): matrix에 새 operation 추가하고 manifest 미반영 → exit 1

### T4 — 기존 회귀 가드 (변경 없음, 통과만 확인)

- `cargo test -p codelens-engine` (현재 295)
- `cargo test -p codelens-mcp` (현재 501)
- `cargo test -p codelens-mcp --features http`
- `cargo test -p codelens-mcp --no-default-features`
- `python3 benchmarks/lint-datasets.py --project .`
- `python3 scripts/surface-manifest.py` (실 capability matrix 입력)
- `python3 benchmarks/lsp-boost-regression-check.py` (있으면)
- `cargo clippy -- -W clippy::all`

### Pre-merge 게이트 순서

1. `cargo check`
2. `cargo test` (T1 + T2 + 기존 전부)
3. `cargo clippy`
4. `surface-manifest.py` (production matrix)
5. `test-surface-manifest-contracts.py` (T3)
6. evaluator(opus) 채점 — §6 acceptance criteria 대비

---

## 6. Acceptance Criteria

evaluator(opus)가 PASS/FAIL 판정할 binary, 측정 가능한 기준.

### AC-1 (필드 존재)

- 11개 raw primitive 응답에 4필드 모두 존재.
- 값: `authority == "syntax"` / `can_preview == true` / `can_apply == true` / `edit_authority.kind == "raw_fs"` / `edit_authority.operation == <tool_name>` / `edit_authority.validator == null`.
- 증거: `mutation_envelope.rs` 11 case green.

### AC-2 (confidence 강등)

- `mutation.rs`의 모든 `success_meta(BackendKind::Filesystem, ...)` confidence ≤ 0.7.
- 다른 호출자(예: `session/project_ops.rs:343` `backend_id: "rust-core"`) 영향 0.
- 증거: grep + assertion test green.

### AC-3 (Tree-sitter rename 강등)

- `rename_symbol(semantic_edit_backend=tree-sitter, dry_run=false)` → `Validation`.
- `rename_symbol(semantic_edit_backend=tree-sitter, dry_run=true)` → `can_apply == false` / `support == "syntax_preview"` / `blocker_reason` non-empty.
- 증거: T2 3 case green.

### AC-4 (Surface manifest contract)

- `scripts/surface-manifest.py`에 contract A + B 추가됨.
- production matrix에 대해 exit 0.
- T3 4 fixture 모두 expected exit code.
- CI `ci.yml`에 contract test step 추가됨.

### AC-5 (Matrix 단일 출처)

- `cargo run --bin dump-matrix > /tmp/matrix.json` 정상, JSON valid.
- 출력이 `backend_operation_matrix::semantic_edit_operation_matrix()`와 동등.
- `surface-manifest.py`가 이 출력을 입력으로 사용.

### AC-6 (회귀 0)

- `cargo test -p codelens-engine`: 통과 수 ≥ 295, 신규 fail 0.
- `cargo test -p codelens-mcp` (default): ≥ 501, 신규 fail 0.
- `cargo test -p codelens-mcp --features http`: 통과.
- `cargo test -p codelens-mcp --no-default-features`: 통과.
- `cargo clippy -- -W clippy::all`: 신규 warning 0.
- `integration_tests/semantic_refactor.rs`, `integration_tests/lsp.rs` 응답 schema 변경 0.

### AC-7 (범위 준수)

- 변경 파일 수 ≤ 8: `mutation.rs` / semantic-edit dispatch (필요 시) / `surface-manifest.py` / `mutation_envelope.rs` / `integration_tests/mod.rs` / `test-surface-manifest-contracts.py` / `ci.yml` / `dump-matrix.rs`.
- 새 LSP backend, 새 operation, 새 mutation primitive 0개.
- engine 쪽 `fs::write` 호출 사이트 변경 0 (Phase 1로 명시 위임).

### AC-8 (문서 정합)

- README, `CLAUDE.md`, `docs/architecture.md`에 옵션 B 광고 들어가지 않음 (substrate). 후속 PR.
- 이 design doc commit됨.

### Evaluator 판정 규칙

- AC-1~AC-7 중 1개라도 FAIL → 전체 FAIL, 머지 보류.
- AC-8 FAIL → 경고만, 머지 가능.

---

## 7. 다음 단계 (이 PR 머지 후)

Phase 1 첫 PR 후보 (이 spec 범위 외, 별도 brainstorming):

- G4: `safe_delete_apply`의 `fs::write`를 WorkspaceEdit 경로로 이전 + `WorkspaceEditTransaction` 도메인 객체 신규
- G5: capability matrix runtime probing (LSP 가용성 / `prepareRename` 지원 여부)
- G7: engine 쪽 `fs::write` 11곳을 transaction layer 단일 경로로 통합

각 항목은 별도 PR + 별도 design doc.
