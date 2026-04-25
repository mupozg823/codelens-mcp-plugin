# Phase 1 G4 — WorkspaceEditTransaction Substrate

- 작성일: 2026-04-25
- 브랜치: `feat/phase1-g4-workspace-edit-transaction` (stacked on `codex/ide-refactor-substrate`)
- 단계: Phase 1 첫 PR
- 다음 단계: G7(engine `fs::write` 11곳 통합), G5(runtime capability probing) — 별도 PR

## 0. 배경 — 왜 이 PR인가

Phase 0가 끝난 시점의 갭(시니어 리뷰 + Phase 0 final review에서 식별):

- `crates/codelens-mcp/src/tools/semantic_edit.rs:416` `safe_delete_apply`가 `transaction_contract` metadata는 첨부하지만 실제 apply는 자체 `std::fs::write` → engine의 백업/롤백 substrate 우회.
- 결과: `rollback_available: false`로 마킹된 contract가 실제로는 backup 0인 상태.

기존 자산:

- `crates/codelens-engine/src/lsp/workspace_edit.rs:67-90` `apply_workspace_edit_transaction` — 이미 in-memory 백업 + apply 실패 시 rollback 구현. LSP rename이 이걸 사용 중.
- `LspWorkspaceEditTransaction` (engine `lsp/types.rs`) — `edits` / `resource_ops` / `modified_files` / `edit_count` / `rollback_available: true` 필드 보유. 단 LSP-namespaced.
- `semantic_transaction_contract` (mcp `semantic_edit.rs:519-580`) — JSON 직렬화 contract. 4 callers (semantic_edit.rs:116/247/434, semantic_adapter.rs:86).

본 PR은 **이미 존재하는 substrate를 일반화하고 모든 mutation primitive가 거치도록 통합**한다. "도메인 객체 신규"가 아니라 "도메인 객체 격상 + safe_delete_apply 마이그레이션 + evidence 강화".

기존 substrate의 약점 두 가지를 함께 해소:

1. **TOCTOU 갭** — read와 apply 사이 외부 수정 감지 못함. 본 PR은 약한 형태(같은 함수 내 두 read)로 좁힘. 강한 검증(snapshot/lock)은 Phase 2.
2. **Silent rollback failure** — 기존 `let _ = fs::write(path, content)`는 rollback 실패 무시. 본 PR은 `RollbackEntry{restored, reason}`로 명시.

## 1. Scope & Goals

### 사용자가 보는 변경

- `safe_delete_apply` 응답이 `rollback_available: true` (기존 `false`)로 바뀌고, `rollback_plan.evidence`에 hash 기반 사실 기록(`file_hashes_after`, rollback 리포트)이 들어감.
- LSP rename 경로(`semantic_edit::rename_symbol_with_lsp_backend` 등 4 callers)도 같은 substrate를 거치므로 evidence 형태 통일 — 기존 `rollback_available: true`였지만 evidence 내용이 사실에 기반하게 됨(현재는 placeholder 문자열).
- 새 transaction 실패 시 응답에 `rollback_report: [{file, restored: bool, reason?: ...}]`가 노출돼 agent가 부분 적용 상태를 식별 가능.

### 비범위 (Phase 1 후속 PR로 위임)

- G7: engine `auto_import` / `inline` / `move_symbol` / `rename` (engine 측) / `memory` 등 다른 fs::write 호출 사이트의 substrate 마이그레이션.
- G5: capability matrix runtime probing.
- 디스크 기반 snapshot rollback (Phase 2 재료).
- Resource operation (create/rename/delete file) 지원 — 현재 LSP path도 preview-only이므로 그대로 유지.
- 강한 TOCTOU 검증 (apply 직전 lock 등) — Phase 2.

### 성공 조건

- `safe_delete_apply`가 substrate를 거쳐 apply 성공/실패 양쪽에서 evidence 정확.
- LSP rename path 회귀 0 (응답 schema 유지, evidence 값만 사실 기반으로 채워짐).
- 신규 fault-injection rollback test green.
- 기존 cargo test (engine + mcp default + http) 0 fail.

---

## 2. Components

### A. `crates/codelens-engine/src/edit_transaction.rs` (신규, ~250줄)

```rust
pub struct WorkspaceEditTransaction {
    pub edits: Vec<RenameEdit>,
    pub resource_ops: Vec<LspResourceOp>,
    pub modified_files: usize,
    pub edit_count: usize,
}

pub struct ApplyEvidence {
    pub status: ApplyStatus,
    pub file_hashes_before: BTreeMap<String, FileHash>,
    pub file_hashes_after: BTreeMap<String, FileHash>,
    pub rollback_report: Vec<RollbackEntry>,
    pub modified_files: usize,
    pub edit_count: usize,
}

pub struct RollbackEntry {
    pub file_path: String,
    pub restored: bool,
    pub reason: Option<String>,
}

pub enum ApplyStatus { Applied, RolledBack, NoOp }
pub struct FileHash { pub sha256: String, pub bytes: usize }

impl WorkspaceEditTransaction {
    pub fn new(edits: Vec<RenameEdit>, resource_ops: Vec<LspResourceOp>) -> Self;
    pub fn apply_with_evidence(&self, project: &ProjectRoot) -> Result<ApplyEvidence, ApplyError>;
}

pub enum ApplyError {
    ResourceOpsUnsupported,
    PreReadFailed { file_path: String, source: anyhow::Error },
    PreApplyHashMismatch { file_path: String, expected: String, actual: String },
    ApplyFailed { source: anyhow::Error, evidence: ApplyEvidence },
}
```

`RenameEdit`은 `crates/codelens-engine/src/rename.rs`의 기존 구조 재사용. `LspResourceOp`은 `crates/codelens-engine/src/lsp/types.rs`에서 가져오되, edit_transaction 모듈에서도 use.

### B. `crates/codelens-engine/src/lsp/workspace_edit.rs` (수정, ~30줄 변경)

- `apply_workspace_edit_transaction(project, &LspWorkspaceEditTransaction)` → 내부에서 `WorkspaceEditTransaction::from(lsp_transaction).apply_with_evidence(project)`로 위임.
- 반환 타입 변경: `Result<()>` → `Result<ApplyEvidence, ApplyError>`. caller(LSP rename path)도 업데이트.
- `LspWorkspaceEditTransaction::into_workspace_edit_transaction()` 또는 `From` impl 추가.

### C. `crates/codelens-engine/src/lib.rs` (1~2줄)

- `pub mod edit_transaction;` 등록 + 핵심 타입 re-export(`WorkspaceEditTransaction`, `ApplyEvidence`, `ApplyStatus`, `RollbackEntry`, `ApplyError`).

### D. `crates/codelens-mcp/src/tools/semantic_edit.rs` (수정, ~80줄 변경)

- `safe_delete_apply`(line ~380-516):
  1. tree-sitter로 delete range 산출(기존 코드 유지).
  2. `RenameEdit` 단일-element vec 구성(`file_path`, `line`, `column`, `old_text=잘라낼 문자열`, `new_text=""`).
  3. `WorkspaceEditTransaction::new(edits, vec![]).apply_with_evidence(project)` 호출.
  4. 반환된 `ApplyEvidence`로 `semantic_transaction_contract` 채움.
  5. 자체 `fs::read_to_string` + `replace_range` + `fs::write` 블록 제거.
- `SemanticTransactionContractInput` / `semantic_transaction_contract`(line 519-580):
  - 새 필드 `evidence: Option<&ApplyEvidence>` 추가.
  - `evidence.is_some()`이면 그것이 single source of truth(file_hashes_before/after, rollback_report, modified_files, edit_count, apply_status, rollback_available).
  - `None`인 경우 기존 필드 사용(LSP rename 경로 호환 + dry_run/preview).
  - JSON 출력에 `file_hashes_after`, `rollback_report` 신규 필드 추가.
- 4 callers 업데이트:
  - L116, L247(LSP rename path): `apply_workspace_edit_transaction` 반환값으로 evidence 채움.
  - L434(safe_delete_apply): substrate apply 결과로 evidence 채움.
  - `semantic_adapter.rs:86`(JetBrains/Roslyn adapter): preview-only이므로 `evidence: None` 유지.

### E. `crates/codelens-engine/src/lsp/types.rs` (수정, ~10줄)

- `LspWorkspaceEditTransaction`에 `From<LspWorkspaceEditTransaction> for WorkspaceEditTransaction` impl.
- `rollback_available` 필드: `#[deprecated(note = "use ApplyEvidence::status from substrate apply_with_evidence")]` 마킹. 제거는 G7 머지 후 별도 정리 PR(공개 API 안정성).

### F. `crates/codelens-engine/src/edit_transaction_tests.rs` 또는 `edit_transaction.rs` 안의 `#[cfg(test)] mod tests` (신규, ~250줄)

T1 케이스 7~9개(섹션 5).

### G. `crates/codelens-mcp/src/integration_tests/semantic_refactor.rs` (수정)

T2(safe_delete_apply 마이그레이션 회귀) + T3(LSP rename evidence 회귀) 케이스.

**예상 footprint**: 3 신규 + 4~5 수정 = 7~8 파일, +650/-100줄. 단일 PR.

---

## 3. Data Flow

### 경로 1 — `apply_with_evidence()` 내부 (substrate 핵심)

```
WorkspaceEditTransaction::apply_with_evidence(project)
  ├─ if !resource_ops.empty() → return Err(ResourceOpsUnsupported)
  ├─ if edits.empty() → return Ok(ApplyEvidence{status: NoOp, ...})
  │
  ├─ Phase 1: capture pre-apply state
  │   for each unique file_path in edits:
  │     resolved = project.resolve(file_path)
  │     bytes = fs::read(resolved)   ← single read; backup + hash
  │     hash_before = sha256(bytes)
  │     backups.insert(resolved, bytes.clone())
  │     hashes_before.insert(file_path, FileHash{sha256, bytes: len})
  │
  ├─ Phase 2: TOCTOU re-check (light)
  │   for each (resolved, _) in backups:
  │     bytes_now = fs::read(resolved)
  │     if sha256(bytes_now) != hash_before:
  │       discard backups; return Err(PreApplyHashMismatch{...})
  │
  ├─ Phase 3: apply via crate::rename::apply_edits(project, &edits)
  │   if Err:
  │     ── rollback ──
  │     for each (resolved, original_bytes) in backups (sorted):
  │       match fs::write(resolved, &original_bytes):
  │         Ok  → push RollbackEntry{file, restored: true, reason: None}
  │         Err(e) → push RollbackEntry{file, restored: false, reason: Some(e)}
  │     hashes_after = read each path post-rollback (truth)
  │     return Err(ApplyFailed{source, evidence: ApplyEvidence{
  │       status: RolledBack, hashes_before, hashes_after, rollback_report,
  │       modified_files: 0, edit_count: 0
  │     }})
  │
  ├─ Phase 4: capture post-apply state
  │   for each file_path:
  │     bytes_after = fs::read(resolved)
  │     hashes_after.insert(file_path, FileHash{sha256, bytes: len})
  │     (read 실패 시 file_hashes_after[file] = {error: "..."})
  │
  └─ return Ok(ApplyEvidence{
       status: Applied, hashes_before, hashes_after, rollback_report: [],
       modified_files, edit_count
     })
```

### 경로 2 — `safe_delete_apply` (mcp)

```
safe_delete_apply(state, args)
  ├─ tree-sitter로 (start_byte, end_byte) 산출
  ├─ source = fs::read_to_string(resolved)  ← preview용 (substrate가 별도 read)
  ├─ delete_text = source[start_byte..delete_end]
  ├─ edits = vec![RenameEdit{file_path, line, column, old_text, new_text: ""}]
  ├─ tx = WorkspaceEditTransaction::new(edits, vec![])
  │
  ├─ if dry_run:
  │   evidence = None
  │   apply_status = "preview_only"
  │
  ├─ else:
  │   match tx.apply_with_evidence(state.project()):
  │     Ok(evidence) → apply_status = "applied"
  │     Err(ApplyFailed{evidence}) → apply_status = "rolled_back"
  │       (도구는 Err로 던지지 않고 Ok 응답에 evidence 포함; agent가 apply_status로 판정)
  │     Err(other) → 도구는 Err 반환
  │
  └─ build response with semantic_transaction_contract(contract_input)
```

### 경로 3 — LSP rename 통합 (semantic_edit.rs:116, 247)

```
... 기존 LSP rename 흐름 ...
lsp_tx: LspWorkspaceEditTransaction = workspace_edit_transaction_from_response(...)
let workspace_tx: WorkspaceEditTransaction = lsp_tx.into();
match workspace_tx.apply_with_evidence(project):
  Ok(evidence) → contract_input.evidence = Some(&evidence)
  Err(ApplyFailed{evidence}) → 동일하게 evidence 포함
```

### 경로 4 — Contract serialization

```
semantic_transaction_contract(input)
  ├─ if input.evidence.is_some():
  │   contract 필드 모두 evidence에서 (single source of truth)
  │   rollback_available = matches!(evidence.status, RolledBack | Applied)
  │   apply_status = match evidence.status {
  │     Applied → "applied", RolledBack → "rolled_back", NoOp → "no_op"
  │   }
  │
  ├─ else (preview/dry_run):
  │   file_hashes_before = file_hashes_before_helper(state, input.file_paths) [기존]
  │   file_hashes_after = []
  │   rollback_report = []
  │   apply_status = input.apply_status (호출자 명시)
  │
  └─ return JSON contract
```

### 핵심 결정점

1. **TOCTOU 재read 갭 인정** — Phase 2 hash 재확인은 같은 함수 안의 약한 형태. 외부 writer가 두 read 사이에 끼어들 갭은 잔존. design에 명시.
2. **Apply 실패 응답 형식** — 도구는 `Err`이 아니라 `Ok` 응답에 `apply_status: "rolled_back"`과 evidence 포함. agent가 evidence를 잃지 않게 함.

---

## 4. Error Handling

| ID     | 트리거                        | substrate 동작                                                                                  | 도구 응답 형식                                                                                                                    |
| ------ | ----------------------------- | ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| **E1** | `resource_ops` non-empty      | `Err(ResourceOpsUnsupported)`. 백업 진입 안 함.                                                 | tool returns `Err(Validation("unsupported_semantic_refactor: resource operations are preview-only..."))`                          |
| **E2** | `edits` empty                 | `Ok(ApplyEvidence{status: NoOp, ...})`                                                          | response: `apply_status: "no_op"`, modified=0, edit=0                                                                             |
| **E3** | Phase 1 read 실패             | `Err(PreReadFailed{file_path, source})`. 부분 백업 폐기.                                        | tool returns `Err(...)`. evidence 없음.                                                                                           |
| **E4** | Phase 2 hash mismatch         | `Err(PreApplyHashMismatch{...})`. 백업 폐기. 디스크 변경 0.                                     | tool returns `Err(...)`. evidence 없음.                                                                                           |
| **E5** | Phase 3 `apply_edits` 실패    | rollback 시도 → `Err(ApplyFailed{source, evidence: status=RolledBack, rollback_report 채워짐})` | **tool returns Ok((response, meta))** with `apply_status: "rolled_back"`, `error_message`, `rollback_report`, `file_hashes_after` |
| **E6** | Phase 4 hash 재계산 실패      | `Ok(ApplyEvidence{status: Applied, file_hashes_after[file] = error})`                           | response: `apply_status: "applied"`, `file_hashes_after[file]: {error}`                                                           |
| **E7** | rollback 중 일부 restore 실패 | `RollbackEntry{restored: false, reason: Some(e)}` 기록. 나머지 파일 계속 시도.                  | response의 `rollback_report`에 노출. **위험 신호** — agent는 사용자 개입 권유.                                                    |

### 핵심 설계 결정

1. **E5는 `Err`이 아니라 `Ok`로 반환** — agent가 evidence를 잃지 않도록. `apply_status` 필드가 fail-closed signal.
2. **E1/E3/E4는 `Err`** — 디스크 변경 없음이 보장된 케이스.
3. **E7(부분 rollback)은 `Ok`** — 사용자가 진단할 수 있는 정보가 더 가치 있음.
4. **rollback report ordered** — `BTreeMap` 또는 sorted Vec로 deterministic.
5. **TOCTOU 재read 갭 인정** — design doc에 명시. Phase 2(snapshot 기반)에서 좁힘.

### 명시적으로 다루지 않는 것

- 파일 핸들 lock / advisory lock (Phase 2)
- 디스크 기반 snapshot (Phase 2)
- 프로세스 크래시 중 apply 부분 적용 → 재시작 시 자동 rollback (Phase 2)
- 동시 transaction 상호 lock (Phase 2)
- inotify/fsevents 기반 외부 변경 감지 (Phase 2+)

---

## 5. Testing

### T1 — substrate unit/integration test (신규)

파일: `crates/codelens-engine/src/edit_transaction_tests.rs` 또는 `edit_transaction.rs` 안의 `#[cfg(test)] mod tests`.

- **T1-happy** (2-file): 두 파일 edit 성공. status=Applied. hashes_before/after 정확. rollback_report=[]. modified_files=2, edit_count=2.
- **T1-noop**: empty edits. status=NoOp.
- **T1-resource-ops**: resource_ops non-empty. `Err(ResourceOpsUnsupported)`. 디스크 변경 0.
- **T1-toctou**: test-only hook(`#[cfg(test)] inject_pre_apply_corruption`)으로 두 read 사이 외부 mutate 시뮬레이션. `Err(PreApplyHashMismatch)`.
- **T1-rollback-success**: 두 번째 파일 read-only 권한 → `Err(ApplyFailed{evidence})`. 첫 파일 restore 성공 verify. status=RolledBack.
- **T1-partial-rollback**: T1-rollback-success 변형. 첫 파일 restore도 실패 시뮬 → `RollbackEntry{restored: false, reason: Some(...)}`.
- **T1-hash-determinism**: 동일 입력 두 번 호출 시 hashes_before 동일.
- **T1-pre-read-failed**: 존재하지 않는 file_path → `Err(PreReadFailed)`.

총 8 case.

### T2 — `safe_delete_apply` 마이그레이션 회귀

파일: `crates/codelens-mcp/src/integration_tests/semantic_refactor.rs` (기존 case 강화).

- **T2-1 dry_run**: 응답에 `apply_status: "preview_only"`, evidence 없는 contract.
- **T2-2 apply success**: `dry_run=false` → `apply_status: "applied"`, `rollback_available: true` (기존 false에서 변경), `file_hashes_after` 존재, `rollback_report: []`.
- **T2-3 apply rollback**: read-only 파일에 대해 → `apply_status: "rolled_back"`, `rollback_report` non-empty. **에러로 던지지 않고 Ok 응답**.
- **T2-4 unchanged on success boundary**: 기존 dry_run preview 응답의 다른 필드 변화 0.

총 4 case.

### T3 — LSP rename evidence 회귀

파일: 같은 파일(LSP rename 케이스 강화).

- **T3-1**: 기존 LSP rename apply 성공 — 응답 schema 변경 0. `file_hashes_before/after`/`rollback_report`/`modified_files`/`edit_count` 값이 사실 기반(placeholder 아님) verify.

### T4 — 기존 회귀 가드 (변경 없음, 통과만 확인)

- `cargo test -p codelens-engine` (T1 추가분 포함)
- `cargo test -p codelens-mcp --no-default-features` (T2/T3 추가분 포함)
- `cargo test -p codelens-mcp --features http`
- `python3 scripts/surface-manifest.py`
- `python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json`
- `python3 scripts/test/test-surface-manifest-contracts.py`
- `python3 benchmarks/lint-datasets.py --project .`
- `cargo clippy -- -W clippy::all`

### TOCTOU test 가능성

`#[cfg(test)] pub fn` hook 권장: substrate에 test-only 함수(`inject_pre_apply_corruption(&self, file_path: &str)`)로 두 read 사이 mutate 시뮬레이션. production code 영향 0.

### Pre-merge 게이트 순서

1. cargo check
2. cargo test (T1 + T2 + T3 + 기존)
3. cargo clippy
4. surface-manifest.py + contract A+B + fixtures
5. evaluator(opus) 채점 — §6 acceptance criteria 대비

---

## 6. Acceptance Criteria

### AC-1 (substrate 도메인 객체 + apply_with_evidence 동작)

- `crates/codelens-engine/src/edit_transaction.rs` 신규 파일에 정의: `WorkspaceEditTransaction`, `ApplyEvidence`, `ApplyStatus { Applied, RolledBack, NoOp }`, `RollbackEntry`, `FileHash`, `ApplyError { ResourceOpsUnsupported, PreReadFailed, PreApplyHashMismatch, ApplyFailed{source, evidence} }`.
- `WorkspaceEditTransaction::apply_with_evidence(&self, project) -> Result<ApplyEvidence, ApplyError>` 메서드 존재.
- `crates/codelens-engine/src/lib.rs`에 `pub mod edit_transaction;` 등록 + 핵심 타입 re-export.
- 증거: T1 8 case green.

### AC-2 (LSP path 통합)

- `apply_workspace_edit_transaction` 시그니처 `Result<()> → Result<ApplyEvidence, ApplyError>`.
- `LspWorkspaceEditTransaction::into_workspace_edit_transaction()` 또는 `From` impl 존재.
- `LspWorkspaceEditTransaction.rollback_available`에 `#[deprecated]` 마킹.
- LSP rename 호출 사이트 응답 schema 변경 0.
- 증거: T3-1 green; 기존 `integration_tests/semantic_refactor.rs` LSP rename 0 fail.

### AC-3 (`safe_delete_apply` 마이그레이션)

- `semantic_edit.rs::safe_delete_apply`에서 `std::fs::write(&resolved, source)` 호출 0개.
- 자체 read+modify+write 블록이 substrate 호출로 대체.
- 응답에 `rollback_available: true`, `file_hashes_after`, `rollback_report` 신규 등장.
- `apply_status` 값이 `applied` / `rolled_back` / `preview_only` / `no_op`.
- 증거: T2-1, T2-2, T2-3, T2-4 green.

### AC-4 (Contract serialization 일반화)

- `semantic_transaction_contract` 시그니처에 `evidence: Option<&ApplyEvidence>` 추가.
- `evidence.is_some()`이면 single source of truth.
- 4 callers 업데이트(L116/247: `Some(&evidence)`, L434: `Some(&evidence)`, semantic_adapter.rs:86: `None`).
- JSON 출력에 `file_hashes_after`, `rollback_report` 신규 필드.

### AC-5 (회귀 0)

- `cargo test -p codelens-engine`: T1 추가 ≥ 7, 신규 fail 0.
- `cargo test -p codelens-mcp --no-default-features`: T2 추가 ≥ 3, 신규 fail 0.
- `cargo test -p codelens-mcp --features http`: 통과.
- `cargo clippy -- -W clippy::all`: 신규 warning 0.
- `python3 scripts/surface-manifest.py` + contract A+B + fixtures: exit 0.
- `python3 benchmarks/lint-datasets.py --project .`: 0/0.

### AC-6 (범위 준수)

- 변경 파일 수 ≤ 9: edit_transaction.rs(신규) / lsp/workspace_edit.rs / lsp/types.rs / lib.rs(engine) / semantic_edit.rs / semantic_adapter.rs / semantic_refactor.rs(test) / edit_transaction_tests.rs(신규 또는 inline) / +1.
- engine 측 다른 `fs::write` 호출 사이트(`auto_import`/`inline`/`move_symbol`/`rename`/`memory`) 변경 0 (G7 위임).
- capability matrix 변경 0.
- 새 LSP backend / 새 operation / 새 mutation primitive 0.

### AC-7 (Phase 0 evidence 보존)

- `tools/mutation.rs` 변경 0 (Phase 0 envelope 유지).
- `tools/semantic_edit.rs::rename_symbol_with_lsp_backend` 응답에 Phase 0 5필드(`authority`/`authority_backend`/`can_preview`/`can_apply`/`edit_authority`) 모두 그대로.
- tree-sitter rename downgrade 동작 변화 0.

### AC-8 (문서 정합)

- design doc commit됨.
- README, CLAUDE.md, architecture.md 변경 0 (substrate, 후속 PR).

### Evaluator 판정 규칙

- AC-1 ~ AC-7 중 1개라도 FAIL → 전체 FAIL.
- AC-8 FAIL → 경고만, 머지 가능.

---

## 7. 다음 단계 (이 PR 머지 후)

Phase 1 후속 PR 후보:

- **G7**: engine `auto_import`/`inline`/`move_symbol`/`rename`/`memory` 등 fs::write 호출 사이트를 substrate로 마이그레이션. 별도 brainstorming.
- **G5**: capability matrix runtime probing. 별도 brainstorming.
- **Phase 2**: 디스크 기반 snapshot rollback / 파일 lock / 프로세스 크래시 회복. 본 PR이 깐 substrate 위에 빌드.

각 항목 별도 PR + 별도 design doc.
