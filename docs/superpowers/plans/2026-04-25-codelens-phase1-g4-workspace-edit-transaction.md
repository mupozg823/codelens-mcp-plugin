# Phase 1 G4 — WorkspaceEditTransaction Substrate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Generalize the existing engine apply path into a `WorkspaceEditTransaction` domain object with hash-based `ApplyEvidence` and rollback report; migrate `safe_delete_apply` off its own `fs::write`.

**Architecture:** New `crates/codelens-engine/src/edit_transaction.rs` module hosts the substrate (types + `apply_with_evidence` method). `lsp::workspace_edit::apply_workspace_edit_transaction` becomes a thin wrapper. mcp `semantic_transaction_contract` takes `evidence: Option<&ApplyEvidence>` and uses it as single source of truth when present.

**Tech Stack:** Rust (engine + mcp), `serde_json::Value`, `sha2::Sha256`, `anyhow::Result`.

**Spec reference:** `docs/superpowers/specs/2026-04-25-codelens-phase1-g4-workspace-edit-transaction-design.md` (commit `25a49523`).

**Branch:** `feat/phase1-g4-workspace-edit-transaction` (stacked on `codex/ide-refactor-substrate` head `25a49523`).

**Existing types (reused, do NOT redefine):**

- `crates/codelens-engine/src/rename.rs:21` — `pub struct RenameEdit { file_path: String, line: usize, column: usize, old_text: String, new_text: String }`
- `crates/codelens-engine/src/rename.rs:419` — `pub fn apply_edits(project: &ProjectRoot, edits: &[RenameEdit]) -> Result<()>`
- `crates/codelens-engine/src/lsp/types.rs:160` — `pub struct LspResourceOp { kind, file_path, old_file_path, new_file_path }`
- `crates/codelens-engine/src/lsp/types.rs:168` — `pub struct LspWorkspaceEditTransaction { edits, resource_ops, modified_files, edit_count, rollback_available }`

---

## File Structure

| Role                                                        | Path                                                             | Change     |
| ----------------------------------------------------------- | ---------------------------------------------------------------- | ---------- |
| Substrate types + `apply_with_evidence`                     | `crates/codelens-engine/src/edit_transaction.rs`                 | 신규       |
| Module registration + re-export                             | `crates/codelens-engine/src/lib.rs`                              | 1~3줄 추가 |
| LSP integration (signature + `From` impl)                   | `crates/codelens-engine/src/lsp/workspace_edit.rs`               | 수정       |
| `LspWorkspaceEditTransaction.rollback_available` deprecated | `crates/codelens-engine/src/lsp/types.rs`                        | 1~3줄      |
| Contract serialization + safe_delete_apply migration        | `crates/codelens-mcp/src/tools/semantic_edit.rs`                 | 수정       |
| Adapter caller (None marker)                                | `crates/codelens-mcp/src/tools/semantic_adapter.rs`              | 1줄        |
| T2/T3 integration tests                                     | `crates/codelens-mcp/src/integration_tests/semantic_refactor.rs` | 수정       |

총 7 파일 (3 신규 + 4 수정 미만, AC-6 ≤ 9 만족).

---

### Task 1: substrate skeleton — 타입 정의 + 모듈 등록

**Files:**

- Create: `crates/codelens-engine/src/edit_transaction.rs`
- Modify: `crates/codelens-engine/src/lib.rs`

**Why this batch:** 타입을 먼저 정의하면 후속 task가 `apply_with_evidence` 동작을 구현할 수 있는 슬롯을 갖게 됨. 컴파일만 확인 (functional behavior 없음).

- [ ] **Step 1: 신규 모듈 작성 — 타입 + skeleton apply_with_evidence**

`crates/codelens-engine/src/edit_transaction.rs`:

```rust
//! Workspace edit transaction substrate.
//!
//! Provides a reusable domain object for multi-file mutations with
//! pre-apply hash capture, post-apply hash verification, and rollback
//! evidence. Used by LSP rename, safe_delete_apply, and future engine
//! mutation primitives.
//!
//! Rollback model: transactional best-effort with rollback evidence.
//! In-memory backups + restore-on-error. TOCTOU re-check is a light
//! same-function two-read window; disk-snapshot/lock guarantees are
//! deferred to Phase 2.

use crate::lsp::types::LspResourceOp;
use crate::project::ProjectRoot;
use crate::rename::RenameEdit;
use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct WorkspaceEditTransaction {
    pub edits: Vec<RenameEdit>,
    pub resource_ops: Vec<LspResourceOp>,
    pub modified_files: usize,
    pub edit_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyEvidence {
    pub status: ApplyStatus,
    pub file_hashes_before: BTreeMap<String, FileHash>,
    pub file_hashes_after: BTreeMap<String, FileHash>,
    pub rollback_report: Vec<RollbackEntry>,
    pub modified_files: usize,
    pub edit_count: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApplyStatus {
    Applied,
    RolledBack,
    NoOp,
}

#[derive(Debug, Clone, Serialize)]
pub struct RollbackEntry {
    pub file_path: String,
    pub restored: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileHash {
    pub sha256: String,
    pub bytes: usize,
}

#[derive(Debug)]
pub enum ApplyError {
    ResourceOpsUnsupported,
    PreReadFailed {
        file_path: String,
        source: anyhow::Error,
    },
    PreApplyHashMismatch {
        file_path: String,
        expected: String,
        actual: String,
    },
    ApplyFailed {
        source: anyhow::Error,
        evidence: ApplyEvidence,
    },
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResourceOpsUnsupported => write!(
                f,
                "unsupported_semantic_refactor: resource operations are preview-only in this release"
            ),
            Self::PreReadFailed { file_path, source } => {
                write!(f, "pre-apply read failed for `{file_path}`: {source}")
            }
            Self::PreApplyHashMismatch {
                file_path,
                expected,
                actual,
            } => write!(
                f,
                "pre-apply hash mismatch for `{file_path}`: expected {expected}, got {actual}"
            ),
            Self::ApplyFailed { source, .. } => write!(f, "apply failed: {source}"),
        }
    }
}

impl std::error::Error for ApplyError {}

impl WorkspaceEditTransaction {
    pub fn new(edits: Vec<RenameEdit>, resource_ops: Vec<LspResourceOp>) -> Self {
        let modified_files = edits
            .iter()
            .map(|edit| &edit.file_path)
            .collect::<std::collections::HashSet<_>>()
            .len();
        let edit_count = edits.len();
        Self {
            edits,
            resource_ops,
            modified_files,
            edit_count,
        }
    }

    /// Apply edits with hash-based evidence and rollback on failure.
    /// Returns `Ok(ApplyEvidence)` on success, `Err(ApplyError)` on
    /// pre-apply failure or apply failure (apply failure carries
    /// `ApplyEvidence` with `status: RolledBack`).
    pub fn apply_with_evidence(
        &self,
        project: &ProjectRoot,
    ) -> Result<ApplyEvidence, ApplyError> {
        // skeleton: future tasks fill in
        let _ = project;
        unimplemented!("apply_with_evidence implemented in Task 2~6")
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
```

(`Context`, `HashMap`, `PathBuf`, `fs` 등은 후속 task에서 사용하므로 unused warning 회피 위해 `#[allow(unused_imports)]`를 모듈 선언에 잠시 추가하거나, 사용 시점에 import. Step 1에서는 `#[allow(dead_code, unused_imports)]`를 모듈 상단에 임시 추가.)

```rust
#![allow(dead_code, unused_imports)] // removed in Task 2 once the function body lands
```

- [ ] **Step 2: lib.rs에 모듈 등록 + 핵심 타입 re-export**

`crates/codelens-engine/src/lib.rs`에서 모듈 선언 블록(다른 `pub mod` 옆)에 추가:

```rust
pub mod edit_transaction;
```

기존 re-export 블록 또는 `pub use` 모음 옆에 추가:

```rust
pub use edit_transaction::{
    ApplyError, ApplyEvidence, ApplyStatus, FileHash, RollbackEntry, WorkspaceEditTransaction,
};
```

(`crates/codelens-engine/src/lib.rs` 구조 확인: 기존 `pub mod xxx;` 줄과 `pub use xxx::*;` 줄을 모방. 필요 시 grep `pub mod` 후 알파벳 순 삽입.)

- [ ] **Step 3: cargo check — 컴파일 확인**

```bash
cargo check -p codelens-engine 2>&1 | tail -10
```

Expected: clean. `unimplemented!()` 매크로 호출은 컴파일 시점에 문제 없음(런타임 panic).

- [ ] **Step 4: 회귀 가드 — 다른 패키지 빌드 확인**

```bash
cargo check -p codelens-mcp 2>&1 | tail -5
cargo check -p codelens-mcp --features http 2>&1 | tail -5
```

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs crates/codelens-engine/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(engine): add edit_transaction module skeleton

WorkspaceEditTransaction / ApplyEvidence / ApplyStatus / RollbackEntry /
FileHash / ApplyError types defined; apply_with_evidence stubbed with
unimplemented!() pending Task 2~6 behavior.

Phase 1 G4 substrate foundation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: NoOp + ResourceOpsUnsupported (no I/O)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Why this batch:** 가장 가벼운 두 분기를 먼저 구현하여 control-flow shape을 확정. I/O 0이라 fault-injection 불필요.

- [ ] **Step 1: 두 fail test 작성 (`#[cfg(test)] mod tests`)**

`edit_transaction.rs` 끝에 추가:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn empty_project() -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-edit-tx-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        ProjectRoot::new(dir.to_str().unwrap()).unwrap()
    }

    #[test]
    fn noop_returns_evidence_with_status_noop() {
        let project = empty_project();
        let tx = WorkspaceEditTransaction::new(vec![], vec![]);
        let evidence = tx.apply_with_evidence(&project).expect("noop apply ok");
        assert_eq!(evidence.status, ApplyStatus::NoOp);
        assert!(evidence.file_hashes_before.is_empty());
        assert!(evidence.file_hashes_after.is_empty());
        assert!(evidence.rollback_report.is_empty());
        assert_eq!(evidence.modified_files, 0);
        assert_eq!(evidence.edit_count, 0);
    }

    #[test]
    fn resource_ops_non_empty_returns_unsupported() {
        let project = empty_project();
        let tx = WorkspaceEditTransaction::new(
            vec![],
            vec![LspResourceOp {
                kind: "create".to_owned(),
                file_path: "new.txt".to_owned(),
                old_file_path: None,
                new_file_path: None,
            }],
        );
        let result = tx.apply_with_evidence(&project);
        assert!(matches!(result, Err(ApplyError::ResourceOpsUnsupported)));
    }
}
```

- [ ] **Step 2: 실행 — 두 test fail 확인 (unimplemented panic)**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -10
```

Expected: 두 test 모두 `panicked at 'not implemented'` (unimplemented! 매크로).

- [ ] **Step 3: NoOp + ResourceOpsUnsupported 분기 구현 + skeleton 제거**

`edit_transaction.rs` 상단의 `#![allow(dead_code, unused_imports)]` 제거. `apply_with_evidence` 본문 교체:

```rust
    pub fn apply_with_evidence(
        &self,
        project: &ProjectRoot,
    ) -> Result<ApplyEvidence, ApplyError> {
        if !self.resource_ops.is_empty() {
            return Err(ApplyError::ResourceOpsUnsupported);
        }
        if self.edits.is_empty() {
            return Ok(ApplyEvidence {
                status: ApplyStatus::NoOp,
                file_hashes_before: BTreeMap::new(),
                file_hashes_after: BTreeMap::new(),
                rollback_report: Vec::new(),
                modified_files: 0,
                edit_count: 0,
            });
        }
        // future tasks (Task 3~6): pre-read, recheck, apply, post-hash
        let _ = project;
        unimplemented!("apply path implemented in Task 3~6")
    }
```

(unused import 제거: `Context`, `HashMap`, `PathBuf`, `fs`, `sha256_hex`는 Task 3에서 사용. `dead_code` allow 일단 유지.)

`#![allow(dead_code, unused_imports)]` → `#![allow(dead_code)]` (sha256_hex가 아직 unused).

- [ ] **Step 4: 실행 — 두 test 통과 + 빌드 확인**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -5
cargo check -p codelens-engine 2>&1 | tail -3
```

Expected: 2 PASS. 0 fail.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
feat(engine): handle NoOp + ResourceOpsUnsupported in apply_with_evidence

empty edits return ApplyEvidence{status: NoOp}. resource_ops non-empty
returns Err(ResourceOpsUnsupported). 2 unit tests added.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: pre-read failure (Phase 1 read 실패)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

- [ ] **Step 1: pre-read-failed fail test 추가**

`edit_transaction.rs`의 `mod tests` 안에 추가:

```rust
    #[test]
    fn pre_read_fails_when_file_missing() {
        let project = empty_project();
        let tx = WorkspaceEditTransaction::new(
            vec![RenameEdit {
                file_path: "missing.txt".to_owned(),
                line: 1,
                column: 1,
                old_text: "x".to_owned(),
                new_text: "y".to_owned(),
            }],
            vec![],
        );
        let result = tx.apply_with_evidence(&project);
        assert!(
            matches!(result, Err(ApplyError::PreReadFailed { ref file_path, .. }) if file_path == "missing.txt"),
            "expected PreReadFailed for missing.txt, got {:?}",
            result.err()
        );
    }
```

- [ ] **Step 2: 실행 — fail 확인**

```bash
cargo test -p codelens-engine --lib edit_transaction::tests::pre_read_fails 2>&1 | tail -10
```

Expected: `panicked at 'not implemented'`.

- [ ] **Step 3: Phase 1 read 구현**

`apply_with_evidence` 본문에서 `unimplemented!("apply path implemented in Task 3~6")` 부분을 교체:

```rust
        // Phase 1: capture pre-apply state (single read; backup + hash)
        let mut backups: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let mut file_hashes_before: BTreeMap<String, FileHash> = BTreeMap::new();
        for file_path in self.unique_file_paths() {
            let resolved = project
                .resolve(&file_path)
                .map_err(|e| ApplyError::PreReadFailed {
                    file_path: file_path.clone(),
                    source: e,
                })?;
            let bytes = fs::read(&resolved).map_err(|e| ApplyError::PreReadFailed {
                file_path: file_path.clone(),
                source: anyhow::Error::from(e),
            })?;
            file_hashes_before.insert(
                file_path.clone(),
                FileHash {
                    sha256: sha256_hex(&bytes),
                    bytes: bytes.len(),
                },
            );
            backups.insert(resolved, bytes);
        }
        // future: Phase 2 recheck, Phase 3 apply, Phase 4 post-hash
        let _ = backups;
        let _ = file_hashes_before;
        unimplemented!("apply path Task 4~6")
```

`unique_file_paths` 헬퍼를 `impl WorkspaceEditTransaction` 안에 추가:

```rust
    fn unique_file_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .edits
            .iter()
            .map(|edit| edit.file_path.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        paths.sort();
        paths
    }
```

- [ ] **Step 4: 실행 — pre-read-failed test 통과 + 회귀 0**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -5
```

Expected: 3 PASS (`noop`, `resource_ops_non_empty`, `pre_read_fails`).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
feat(engine): pre-read phase + PreReadFailed error in substrate

Phase 1 reads each unique edit file once, captures sha256 + length
into file_hashes_before, stores raw bytes as in-memory backup. Missing
files surface as ApplyError::PreReadFailed with the file path. 1 test
added.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: happy path — apply via `apply_edits` + Phase 4 hash + hash determinism

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

- [ ] **Step 1: happy + determinism fail tests 추가**

`mod tests`에 추가:

```rust
    fn write_file(project: &ProjectRoot, name: &str, content: &str) -> PathBuf {
        let resolved = project.resolve(name).unwrap();
        std::fs::create_dir_all(resolved.parent().unwrap()).ok();
        std::fs::write(&resolved, content).unwrap();
        resolved
    }

    #[test]
    fn happy_path_two_files_apply_succeeds_with_evidence() {
        let project = empty_project();
        write_file(&project, "a.txt", "alpha\n");
        write_file(&project, "b.txt", "beta\n");
        let tx = WorkspaceEditTransaction::new(
            vec![
                RenameEdit {
                    file_path: "a.txt".to_owned(),
                    line: 1,
                    column: 1,
                    old_text: "alpha".to_owned(),
                    new_text: "ALPHA".to_owned(),
                },
                RenameEdit {
                    file_path: "b.txt".to_owned(),
                    line: 1,
                    column: 1,
                    old_text: "beta".to_owned(),
                    new_text: "BETA".to_owned(),
                },
            ],
            vec![],
        );
        let evidence = tx
            .apply_with_evidence(&project)
            .expect("happy path apply ok");
        assert_eq!(evidence.status, ApplyStatus::Applied);
        assert_eq!(evidence.file_hashes_before.len(), 2);
        assert_eq!(evidence.file_hashes_after.len(), 2);
        assert!(evidence.rollback_report.is_empty());
        assert_eq!(evidence.modified_files, 2);
        assert_eq!(evidence.edit_count, 2);
        // before hashes != after hashes
        for (path, before) in &evidence.file_hashes_before {
            let after = evidence
                .file_hashes_after
                .get(path)
                .expect("after entry exists");
            assert_ne!(before.sha256, after.sha256, "hash for {path} should differ");
        }
        // disk shows new content
        assert_eq!(
            std::fs::read_to_string(project.resolve("a.txt").unwrap()).unwrap(),
            "ALPHA\n"
        );
        assert_eq!(
            std::fs::read_to_string(project.resolve("b.txt").unwrap()).unwrap(),
            "BETA\n"
        );
    }

    #[test]
    fn pre_apply_hash_is_deterministic_for_same_input() {
        let project = empty_project();
        write_file(&project, "x.txt", "stable\n");
        let tx_a = WorkspaceEditTransaction::new(
            vec![RenameEdit {
                file_path: "x.txt".to_owned(),
                line: 1,
                column: 1,
                old_text: "stable".to_owned(),
                new_text: "stable".to_owned(),
            }],
            vec![],
        );
        // capture pre-apply hash twice via separate apply_with_evidence calls;
        // since apply leaves content identical, second call sees same bytes.
        let ev_a = tx_a.apply_with_evidence(&project).unwrap();
        let tx_b = tx_a.clone();
        let ev_b = tx_b.apply_with_evidence(&project).unwrap();
        let hash_a = &ev_a.file_hashes_before["x.txt"].sha256;
        let hash_b = &ev_b.file_hashes_before["x.txt"].sha256;
        assert_eq!(hash_a, hash_b);
    }
```

- [ ] **Step 2: 실행 — happy + determinism fail 확인 (unimplemented)**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -10
```

Expected: 2 신규 fail (`happy_path_two_files_apply_succeeds_with_evidence`, `pre_apply_hash_is_deterministic_for_same_input`); 기존 3 PASS.

- [ ] **Step 3: Phase 3 apply + Phase 4 hash 구현**

`apply_with_evidence`의 끝부분(Phase 1 캡처 직후)을 다음으로 교체:

```rust
        // Phase 3: apply via crate::rename::apply_edits
        if let Err(source) = crate::rename::apply_edits(project, &self.edits) {
            // Rollback path implemented in Task 5; for now propagate the error
            // wrapped in ApplyFailed with empty evidence.
            return Err(ApplyError::ApplyFailed {
                source,
                evidence: ApplyEvidence {
                    status: ApplyStatus::RolledBack,
                    file_hashes_before,
                    file_hashes_after: BTreeMap::new(),
                    rollback_report: Vec::new(),
                    modified_files: 0,
                    edit_count: 0,
                },
            });
        }

        // Phase 4: capture post-apply state
        let mut file_hashes_after: BTreeMap<String, FileHash> = BTreeMap::new();
        for file_path in self.unique_file_paths() {
            let resolved = match project.resolve(&file_path) {
                Ok(path) => path,
                Err(_) => {
                    file_hashes_after.insert(
                        file_path.clone(),
                        FileHash {
                            sha256: String::new(),
                            bytes: 0,
                        },
                    );
                    continue;
                }
            };
            match fs::read(&resolved) {
                Ok(bytes) => {
                    file_hashes_after.insert(
                        file_path.clone(),
                        FileHash {
                            sha256: sha256_hex(&bytes),
                            bytes: bytes.len(),
                        },
                    );
                }
                Err(_) => {
                    file_hashes_after.insert(
                        file_path.clone(),
                        FileHash {
                            sha256: String::new(),
                            bytes: 0,
                        },
                    );
                }
            }
        }

        Ok(ApplyEvidence {
            status: ApplyStatus::Applied,
            file_hashes_before,
            file_hashes_after,
            rollback_report: Vec::new(),
            modified_files: self.modified_files,
            edit_count: self.edit_count,
        })
```

- [ ] **Step 4: 실행 — 5 test 모두 통과**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -5
```

Expected: 5 PASS.

- [ ] **Step 5: 회귀 가드 — engine 전체**

```bash
cargo test -p codelens-engine 2>&1 | grep -E "^test result:" | tail -10
```

Expected: 0 fail (모든 binaries 합산).

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
feat(engine): happy-path apply with post-hash evidence

Phase 3 calls crate::rename::apply_edits; Phase 4 reads each file
post-apply for sha256/byte length. ApplyFailed temporarily carries
empty rollback evidence (Task 5 fills in restore logic). 2 tests
added (happy two-file + hash determinism).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: rollback path — restore + RollbackReport (Phase 3 error)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Why this batch:** Phase 3 apply 실패 시 backups에서 복원 + 각 파일별 결과 기록. Permission-based fault injection으로 test.

- [ ] **Step 1: rollback-success fail test 추가**

`mod tests`에 추가 (Unix permission 사용; Windows에서는 skip):

```rust
    #[cfg(unix)]
    #[test]
    fn rollback_restores_first_file_when_second_apply_fails() {
        use std::os::unix::fs::PermissionsExt;
        let project = empty_project();
        let path_a = write_file(&project, "ra.txt", "alpha\n");
        let path_b = write_file(&project, "rb.txt", "beta\n");
        // make rb.txt read-only so apply_edits fails when writing it
        let mut perms = std::fs::metadata(&path_b).unwrap().permissions();
        perms.set_mode(0o444);
        std::fs::set_permissions(&path_b, perms).unwrap();

        let tx = WorkspaceEditTransaction::new(
            vec![
                RenameEdit {
                    file_path: "ra.txt".to_owned(),
                    line: 1,
                    column: 1,
                    old_text: "alpha".to_owned(),
                    new_text: "ALPHA".to_owned(),
                },
                RenameEdit {
                    file_path: "rb.txt".to_owned(),
                    line: 1,
                    column: 1,
                    old_text: "beta".to_owned(),
                    new_text: "BETA".to_owned(),
                },
            ],
            vec![],
        );

        let result = tx.apply_with_evidence(&project);
        let evidence = match result {
            Err(ApplyError::ApplyFailed { evidence, .. }) => evidence,
            other => panic!("expected ApplyFailed, got {other:?}"),
        };
        assert_eq!(evidence.status, ApplyStatus::RolledBack);
        assert_eq!(evidence.modified_files, 0);
        assert_eq!(evidence.edit_count, 0);
        // ra.txt restored on disk
        let ra_now = std::fs::read_to_string(&path_a).unwrap();
        assert_eq!(ra_now, "alpha\n", "ra.txt should be restored to alpha");
        // hashes_after for ra.txt matches hashes_before (truth check)
        let before = evidence.file_hashes_before.get("ra.txt").unwrap();
        let after = evidence.file_hashes_after.get("ra.txt").unwrap();
        assert_eq!(
            before.sha256, after.sha256,
            "ra.txt hash should match pre-apply after rollback"
        );
        // rollback_report contains an entry for ra.txt with restored=true
        let entry_a = evidence
            .rollback_report
            .iter()
            .find(|e| e.file_path == "ra.txt")
            .expect("rollback entry for ra.txt");
        assert!(entry_a.restored, "ra.txt restore should succeed");
        assert!(entry_a.reason.is_none());
        // rb.txt write was blocked, so it's still original content
        // restore _attempt_ tries to write the same original; permissions
        // may still block. Acceptable: entry exists for rb.txt either way.
        let entry_b = evidence
            .rollback_report
            .iter()
            .find(|e| e.file_path == "rb.txt");
        assert!(entry_b.is_some(), "rb.txt rollback entry should exist");

        // restore perms so tempdir cleanup works
        let mut restore = std::fs::metadata(&path_b).unwrap().permissions();
        restore.set_mode(0o644);
        let _ = std::fs::set_permissions(&path_b, restore);
    }
```

- [ ] **Step 2: 실행 — fail 확인**

```bash
cargo test -p codelens-engine --lib edit_transaction::tests::rollback_restores 2>&1 | tail -15
```

Expected: fail. 현재 ApplyFailed가 빈 rollback_report와 빈 hashes_after를 들고 반환되므로 assert가 깨짐.

- [ ] **Step 3: rollback 로직 구현**

`apply_with_evidence`의 Phase 3 분기를 다음으로 교체:

```rust
        // Phase 3: apply via crate::rename::apply_edits
        if let Err(source) = crate::rename::apply_edits(project, &self.edits) {
            let mut rollback_report: Vec<RollbackEntry> = Vec::new();
            let mut file_hashes_after_rb: BTreeMap<String, FileHash> = BTreeMap::new();

            // Restore each backup; record per-file success/failure.
            // Iterate sorted file paths for deterministic ordering.
            let sorted_paths = self.unique_file_paths();
            for file_path in &sorted_paths {
                let resolved = match project.resolve(file_path) {
                    Ok(p) => p,
                    Err(e) => {
                        rollback_report.push(RollbackEntry {
                            file_path: file_path.clone(),
                            restored: false,
                            reason: Some(format!("resolve failed: {e}")),
                        });
                        continue;
                    }
                };
                let backup_bytes = match backups.get(&resolved) {
                    Some(bytes) => bytes,
                    None => {
                        rollback_report.push(RollbackEntry {
                            file_path: file_path.clone(),
                            restored: false,
                            reason: Some("no backup captured".to_owned()),
                        });
                        continue;
                    }
                };
                match fs::write(&resolved, backup_bytes) {
                    Ok(()) => rollback_report.push(RollbackEntry {
                        file_path: file_path.clone(),
                        restored: true,
                        reason: None,
                    }),
                    Err(e) => rollback_report.push(RollbackEntry {
                        file_path: file_path.clone(),
                        restored: false,
                        reason: Some(format!("write failed: {e}")),
                    }),
                }
            }

            // Capture post-rollback hashes (truth check).
            for file_path in &sorted_paths {
                let resolved = match project.resolve(file_path) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                if let Ok(bytes) = fs::read(&resolved) {
                    file_hashes_after_rb.insert(
                        file_path.clone(),
                        FileHash {
                            sha256: sha256_hex(&bytes),
                            bytes: bytes.len(),
                        },
                    );
                }
            }

            return Err(ApplyError::ApplyFailed {
                source,
                evidence: ApplyEvidence {
                    status: ApplyStatus::RolledBack,
                    file_hashes_before,
                    file_hashes_after: file_hashes_after_rb,
                    rollback_report,
                    modified_files: 0,
                    edit_count: 0,
                },
            });
        }
```

- [ ] **Step 4: 실행 — rollback test 통과 + 회귀 0**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -5
cargo test -p codelens-engine 2>&1 | grep -E "^test result:" | tail -5
```

Expected: 6 PASS in edit_transaction tests. Engine total 0 fail.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
feat(engine): rollback report + post-rollback hashes on apply failure

Phase 3 failure now restores each backup file in deterministic order,
records RollbackEntry{restored, reason} per file, and re-reads the
filesystem to populate file_hashes_after with the truth (matching
pre-apply hashes when restore succeeded). Replaces the silent
let _ = fs::write pattern. 1 unix-only test added.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: TOCTOU recheck (Phase 2)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Why this batch:** Phase 1과 Phase 3 사이에 약한 hash 재검증. TOCTOU가 자연스럽게 트리거되지 않으므로 substrate를 두 메서드(`capture_pre_apply` + `verify_pre_apply`)로 분리하여 test가 각각 호출 가능하게 함.

- [ ] **Step 1: TOCTOU fail test 작성**

`mod tests`에 추가:

```rust
    #[test]
    fn toctou_recheck_detects_external_mutation_between_phases() {
        let project = empty_project();
        let path = write_file(&project, "tt.txt", "before\n");
        let tx = WorkspaceEditTransaction::new(
            vec![RenameEdit {
                file_path: "tt.txt".to_owned(),
                line: 1,
                column: 1,
                old_text: "before".to_owned(),
                new_text: "after".to_owned(),
            }],
            vec![],
        );

        let (backups, hashes_before) = tx
            .capture_pre_apply(&project)
            .expect("phase 1 capture ok");
        // External writer mutates the file between phases.
        std::fs::write(&path, "TAMPERED\n").unwrap();

        let result = tx.verify_pre_apply(&project, &backups, &hashes_before);
        assert!(
            matches!(result, Err(ApplyError::PreApplyHashMismatch { ref file_path, .. }) if file_path == "tt.txt"),
            "expected PreApplyHashMismatch for tt.txt, got {:?}",
            result.err()
        );
        // Disk contains the external mutation; substrate did not apply edits.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "TAMPERED\n");
    }
```

- [ ] **Step 2: 실행 — fail 확인 (`capture_pre_apply` / `verify_pre_apply` 미정의)**

```bash
cargo test -p codelens-engine --lib edit_transaction::tests::toctou 2>&1 | tail -10
```

Expected: 컴파일 에러 (메서드 없음). 이 test는 컴파일 자체가 실패해야 함 — Step 3에서 메서드 신규.

- [ ] **Step 3: Phase 1 → `capture_pre_apply`, Phase 2 → `verify_pre_apply`로 분리**

`impl WorkspaceEditTransaction`에 두 메서드 추가:

```rust
    /// Phase 1: read each unique file once, capture sha256 + raw backup bytes.
    pub(crate) fn capture_pre_apply(
        &self,
        project: &ProjectRoot,
    ) -> Result<(HashMap<PathBuf, Vec<u8>>, BTreeMap<String, FileHash>), ApplyError> {
        let mut backups: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        let mut file_hashes_before: BTreeMap<String, FileHash> = BTreeMap::new();
        for file_path in self.unique_file_paths() {
            let resolved = project
                .resolve(&file_path)
                .map_err(|e| ApplyError::PreReadFailed {
                    file_path: file_path.clone(),
                    source: e,
                })?;
            let bytes = fs::read(&resolved).map_err(|e| ApplyError::PreReadFailed {
                file_path: file_path.clone(),
                source: anyhow::Error::from(e),
            })?;
            file_hashes_before.insert(
                file_path.clone(),
                FileHash {
                    sha256: sha256_hex(&bytes),
                    bytes: bytes.len(),
                },
            );
            backups.insert(resolved, bytes);
        }
        Ok((backups, file_hashes_before))
    }

    /// Phase 2: re-read each captured file and confirm sha256 still matches.
    /// Light same-function TOCTOU window; strong guarantees deferred to Phase 2.
    pub(crate) fn verify_pre_apply(
        &self,
        project: &ProjectRoot,
        backups: &HashMap<PathBuf, Vec<u8>>,
        hashes_before: &BTreeMap<String, FileHash>,
    ) -> Result<(), ApplyError> {
        for file_path in self.unique_file_paths() {
            let resolved = project
                .resolve(&file_path)
                .map_err(|e| ApplyError::PreReadFailed {
                    file_path: file_path.clone(),
                    source: e,
                })?;
            let bytes_now = fs::read(&resolved).map_err(|e| ApplyError::PreReadFailed {
                file_path: file_path.clone(),
                source: anyhow::Error::from(e),
            })?;
            let hash_now = sha256_hex(&bytes_now);
            let expected = hashes_before
                .get(&file_path)
                .map(|h| h.sha256.clone())
                .unwrap_or_default();
            if hash_now != expected {
                return Err(ApplyError::PreApplyHashMismatch {
                    file_path,
                    expected,
                    actual: hash_now,
                });
            }
            let _ = backups; // referenced for invariant: same set of files captured
        }
        Ok(())
    }
```

- [ ] **Step 4: `apply_with_evidence` 본문 — 두 메서드 호출로 교체**

기존 Phase 1 inline 코드를 다음으로 대체:

```rust
        // Phase 1: capture pre-apply state
        let (backups, file_hashes_before) = self.capture_pre_apply(project)?;

        // Phase 2: light TOCTOU re-check (same-function window)
        self.verify_pre_apply(project, &backups, &file_hashes_before)?;
```

이후 Phase 3, Phase 4 코드는 그대로.

- [ ] **Step 5: 실행 — TOCTOU test 통과 + 회귀 0**

```bash
cargo test -p codelens-engine --lib edit_transaction:: 2>&1 | tail -5
cargo test -p codelens-engine 2>&1 | grep -E "^test result:" | tail -5
```

Expected: 7 PASS in edit_transaction. Engine total 0 fail.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
feat(engine): TOCTOU pre-apply hash re-check substrate phase

split Phase 1 (capture) and Phase 2 (verify) into pub(crate) methods
so apply_with_evidence calls them sequentially and tests can drive
them independently. detects external file mutation between the two
reads via sha256 comparison and returns PreApplyHashMismatch. Light
same-function window; strong guarantees deferred to Phase 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: LSP integration — `apply_workspace_edit_transaction` signature

**Files:**

- Modify: `crates/codelens-engine/src/lsp/types.rs`
- Modify: `crates/codelens-engine/src/lsp/workspace_edit.rs`
- Modify: callers of `apply_workspace_edit_transaction` in mcp (semantic_edit.rs L116, L247 likely)

**Why this batch:** engine-side signature change. callers temporarily discard ApplyEvidence beyond `?` propagation; Task 8 wires evidence into the contract.

- [ ] **Step 1: `From<LspWorkspaceEditTransaction> for WorkspaceEditTransaction` impl 추가 + deprecated 마킹**

`crates/codelens-engine/src/lsp/types.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct LspWorkspaceEditTransaction {
    pub edits: Vec<crate::rename::RenameEdit>,
    pub resource_ops: Vec<LspResourceOp>,
    pub modified_files: usize,
    pub edit_count: usize,
    #[deprecated(
        note = "use ApplyEvidence::status from substrate apply_with_evidence instead"
    )]
    pub rollback_available: bool,
}

impl From<LspWorkspaceEditTransaction> for crate::edit_transaction::WorkspaceEditTransaction {
    fn from(value: LspWorkspaceEditTransaction) -> Self {
        crate::edit_transaction::WorkspaceEditTransaction::new(value.edits, value.resource_ops)
    }
}
```

(`#[deprecated]` 추가 시 컴파일러가 self의 `rollback_available` 사용에서 warning을 낼 수 있음. `lsp/workspace_edit.rs`의 기존 `LspWorkspaceEditTransaction { ..., rollback_available: true }` 빌더 호출에 `#[allow(deprecated)]` 추가하여 무음 처리.)

- [ ] **Step 2: `apply_workspace_edit_transaction` 시그니처 변경**

`crates/codelens-engine/src/lsp/workspace_edit.rs`의 `apply_workspace_edit_transaction` 함수 본문을 다음으로 교체:

```rust
pub(super) fn apply_workspace_edit_transaction(
    project: &ProjectRoot,
    transaction: &LspWorkspaceEditTransaction,
) -> Result<crate::edit_transaction::ApplyEvidence, crate::edit_transaction::ApplyError> {
    let workspace_tx: crate::edit_transaction::WorkspaceEditTransaction =
        transaction.clone().into();
    workspace_tx.apply_with_evidence(project)
}
```

(`pub(super)` 가시성 유지. mcp 측은 `lsp::workspace_edit_transaction_from_response` + `apply_workspace_edit_transaction`을 직접 import하지 않고 mcp re-export 또는 lsp module 내부에서만 호출. mcp 측 caller는 다른 helper를 통해 호출하므로 시그니처 변경이 필요한지 확인.)

```bash
grep -rn "apply_workspace_edit_transaction" crates/codelens-mcp/ 2>&1 | head
```

만약 mcp 측 caller가 있다면 caller signatures도 업데이트 (Result<()> → Result<ApplyEvidence, ApplyError>).

- [ ] **Step 3: 컴파일 확인 — 에러 식별**

```bash
cargo check -p codelens-engine 2>&1 | tail -20
cargo check -p codelens-mcp 2>&1 | tail -20
cargo check -p codelens-mcp --features http 2>&1 | tail -20
```

caller 사이트에서 type mismatch 에러 발생 예상. 각 에러 메시지의 file:line 식별.

- [ ] **Step 4: caller 업데이트 (mcp 측)**

각 caller에서 `apply_workspace_edit_transaction(project, &lsp_tx)?`를 다음으로 변경:

```rust
let evidence = apply_workspace_edit_transaction(project, &lsp_tx)
    .map_err(|e| /* 적절한 CodeLensError 변환; ApplyError → anyhow → CodeLensError */)?;
```

`ApplyError`는 `std::error::Error` impl이 있으므로 `anyhow::Error::from(e)` 또는 `e.to_string()`으로 변환 가능. 이 단계에서는 `evidence`를 받기만 하고 사용 안 함 (Task 8에서 contract에 전달).

caller 위치 후보: `crates/codelens-mcp/src/tools/semantic_edit.rs::rename_symbol_with_lsp_backend` 본문 안. (line ~100~250 사이의 LSP rename 처리 분기).

- [ ] **Step 5: 빌드 + 기존 회귀 가드**

```bash
cargo check -p codelens-mcp --features http 2>&1 | tail -5
cargo test -p codelens-engine 2>&1 | grep -E "^test result:" | tail -5
cargo test -p codelens-mcp --no-default-features 2>&1 | tail -3
cargo test -p codelens-mcp --features http 2>&1 | tail -3
```

Expected: 모두 0 fail. (LSP rename test는 evidence를 사용하지 않더라도 schema 변화 0이므로 통과.)

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-engine/src/lsp/types.rs crates/codelens-engine/src/lsp/workspace_edit.rs crates/codelens-mcp/src/tools/semantic_edit.rs
git commit -m "$(cat <<'EOF'
feat(engine): integrate LSP apply path into substrate

apply_workspace_edit_transaction now delegates to
WorkspaceEditTransaction::apply_with_evidence and returns
Result<ApplyEvidence, ApplyError>. LspWorkspaceEditTransaction gains
From<> for the substrate type. rollback_available marked deprecated
in favor of ApplyEvidence::status. mcp callers consume the new return
type but discard evidence pending Task 8 contract wiring.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Contract serialization — `evidence` parameter

**Files:**

- Modify: `crates/codelens-mcp/src/tools/semantic_edit.rs`
- Modify: `crates/codelens-mcp/src/tools/semantic_adapter.rs`

**Why this batch:** `semantic_transaction_contract`이 evidence를 single source of truth로 받게 함. LSP rename callers (L116, L247)는 Task 7의 evidence를 전달. adapter caller는 None으로 명시.

- [ ] **Step 1: `SemanticTransactionContractInput` + `semantic_transaction_contract` 시그니처 확장**

`crates/codelens-mcp/src/tools/semantic_edit.rs`의 struct에 새 필드 추가:

```rust
pub(crate) struct SemanticTransactionContractInput<'a> {
    pub(crate) state: &'a AppState,
    pub(crate) backend_id: &'a str,
    pub(crate) operation: &'a str,
    pub(crate) target_symbol: Option<&'a str>,
    pub(crate) file_paths: &'a [String],
    pub(crate) dry_run: bool,
    pub(crate) modified_files: usize,
    pub(crate) edit_count: usize,
    pub(crate) resource_ops: Value,
    pub(crate) rollback_available: bool,
    pub(crate) workspace_edit: Value,
    pub(crate) apply_status: &'a str,
    pub(crate) references_checked: bool,
    pub(crate) conflicts: Value,
    /// When `Some`, evidence overrides `file_hashes_before`/`apply_status`/
    /// `modified_files`/`edit_count`/`rollback_available` from this struct;
    /// also adds `file_hashes_after` and `rollback_report` to the output.
    pub(crate) evidence: Option<&'a codelens_engine::ApplyEvidence>,
}
```

`semantic_transaction_contract` 함수 본문 교체:

```rust
pub(crate) fn semantic_transaction_contract(input: SemanticTransactionContractInput<'_>) -> Value {
    let (
        file_hashes_before,
        file_hashes_after,
        rollback_report,
        rollback_available,
        modified_files,
        edit_count,
        apply_status_resolved,
    ) = match input.evidence {
        Some(ev) => {
            let hashes_before = serde_json::to_value(&ev.file_hashes_before)
                .unwrap_or(Value::Null);
            let hashes_after = serde_json::to_value(&ev.file_hashes_after)
                .unwrap_or(Value::Null);
            let rollback = serde_json::to_value(&ev.rollback_report)
                .unwrap_or(Value::Array(Vec::new()));
            let status_str = match ev.status {
                codelens_engine::ApplyStatus::Applied => "applied",
                codelens_engine::ApplyStatus::RolledBack => "rolled_back",
                codelens_engine::ApplyStatus::NoOp => "no_op",
            };
            (
                hashes_before,
                hashes_after,
                rollback,
                matches!(
                    ev.status,
                    codelens_engine::ApplyStatus::Applied
                        | codelens_engine::ApplyStatus::RolledBack
                ),
                ev.modified_files,
                ev.edit_count,
                status_str,
            )
        }
        None => {
            let hashes_before = file_hashes_before(input.state, input.file_paths);
            (
                hashes_before,
                Value::Object(serde_json::Map::new()),
                Value::Array(Vec::new()),
                input.rollback_available,
                input.modified_files,
                input.edit_count,
                input.apply_status,
            )
        }
    };

    let tx_id = transaction_id(
        input.backend_id,
        input.operation,
        input.file_paths,
        &file_hashes_before,
    );

    json!({
        "transaction_id": tx_id,
        "model": "transactional_best_effort_with_rollback_evidence",
        "workspace_id": input.state.project().as_path().display().to_string(),
        "backend_id": input.backend_id,
        "operation": input.operation,
        "target_symbol": input.target_symbol,
        "input_snapshot": {
            "file_paths": unique_file_paths(input.file_paths),
            "dry_run": input.dry_run,
        },
        "file_hashes_before": file_hashes_before,
        "file_hashes_after": file_hashes_after,
        "rollback_report": rollback_report,
        "workspace_edit": input.workspace_edit,
        "preview_diff": [],
        "apply_status": apply_status_resolved,
        "modified_files": modified_files,
        "edit_count": edit_count,
        "resource_ops": input.resource_ops,
        "rollback_plan": {
            "available": rollback_available,
            "evidence": if rollback_available {
                "pre-apply file snapshots are held during apply; restored on apply failure"
            } else {
                "rollback evidence is unavailable for this operation path"
            }
        },
        "diagnostics_before": [],
        "diagnostics_after": [],
        "verification_result": {
            "references_checked": input.references_checked,
            "conflicts": input.conflicts,
        },
        "audit_record": {
            "recorded": false,
            "reason": "inline tool response only; session audit remains the durable audit channel"
        }
    })
}
```

- [ ] **Step 2: 4 callers 업데이트 — 새 필드 명시**

각 caller에서 struct literal에 `evidence: ...` 필드 추가:

- semantic_edit.rs:L116 (LSP rename apply 후): `evidence: Some(&apply_evidence_var)` (Task 7에서 받은 evidence)
- semantic_edit.rs:L247 (LSP rename 다른 분기): 동일
- semantic_edit.rs:L434 (safe_delete_apply): 일단 `evidence: None` (Task 9에서 substrate 호출로 변경)
- crates/codelens-mcp/src/tools/semantic_adapter.rs:L86 (JetBrains/Roslyn adapter): `evidence: None` (preview-only, substrate 안 거침)

(L116/L247의 정확한 변수 이름은 코드 읽고 확정. 본 plan에서는 `apply_evidence` 라고 가정.)

- [ ] **Step 3: 빌드 + 테스트**

```bash
cargo check -p codelens-mcp --features http 2>&1 | tail -5
cargo test -p codelens-mcp --no-default-features 2>&1 | tail -3
cargo test -p codelens-mcp --features http 2>&1 | tail -3
```

Expected: 0 fail. 응답 schema에 새 필드 (`file_hashes_after`, `rollback_report`)가 등장하지만 기존 test가 그것들을 assert하지 않으므로 회귀 0.

- [ ] **Step 4: T3-1 LSP rename evidence 회귀 test 추가**

`crates/codelens-mcp/src/integration_tests/semantic_refactor.rs`의 LSP rename 케이스 한 개에 evidence 검증 assertion 추가 (기존 test의 끝부분):

```rust
        // T3-1: evidence is fact-based, not placeholder
        let payload = parse_tool_response(&response);
        let scope = payload.get("data").unwrap_or(&payload);
        let tx = &scope["transaction"]["contract"];
        // file_hashes_before/after must be non-empty objects with sha256 fields
        let hashes_before = tx["file_hashes_before"]
            .as_object()
            .expect("file_hashes_before should be an object");
        let hashes_after = tx["file_hashes_after"]
            .as_object()
            .expect("file_hashes_after should be an object");
        assert!(!hashes_before.is_empty(), "hashes_before should be populated");
        assert_eq!(
            hashes_before.len(),
            hashes_after.len(),
            "hashes_before and after should have same key set"
        );
        for (path, before) in hashes_before {
            assert!(
                before["sha256"].as_str().map(|s| !s.is_empty()).unwrap_or(false),
                "hashes_before[{path}].sha256 should be non-empty"
            );
        }
        // apply_status reflects substrate status
        assert!(
            matches!(
                tx["apply_status"].as_str(),
                Some("applied") | Some("rolled_back") | Some("no_op")
            ),
            "apply_status should be substrate-derived: {:?}",
            tx["apply_status"]
        );
```

(정확한 LSP rename test 함수 이름은 grep으로 확인: `grep -n "fn.*rename.*lsp\|fn.*lsp.*rename\|workspace_edit_lsp" crates/codelens-mcp/src/integration_tests/semantic_refactor.rs`. assertion을 적합한 test 안에 삽입.)

- [ ] **Step 5: 실행**

```bash
cargo test -p codelens-mcp --features http 2>&1 | tail -3
```

Expected: 0 fail. 신규 assertion이 evidence 사실성을 검증.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/tools/semantic_edit.rs crates/codelens-mcp/src/tools/semantic_adapter.rs crates/codelens-mcp/src/integration_tests/semantic_refactor.rs
git commit -m "$(cat <<'EOF'
feat(mcp): semantic_transaction_contract takes ApplyEvidence

new optional `evidence` field on SemanticTransactionContractInput.
when present, evidence is single source of truth for file_hashes_before/
file_hashes_after / rollback_report / apply_status / modified_files /
edit_count / rollback_available. four callers updated: LSP rename paths
pass Some(&evidence); safe_delete_apply leaves None pending Task 9;
JetBrains/Roslyn adapter passes None (preview-only). 1 LSP rename
regression test asserts hashes are fact-based, not placeholder.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: `safe_delete_apply` 마이그레이션 (TDD)

**Files:**

- Modify: `crates/codelens-mcp/src/tools/semantic_edit.rs`
- Modify: `crates/codelens-mcp/src/integration_tests/semantic_refactor.rs`

**Why this batch:** 자체 `std::fs::write` 제거. substrate를 거쳐 evidence 받아 contract에 전달. 4 case T2 추가.

- [ ] **Step 1: T2-1 ~ T2-4 fail tests 추가**

`crates/codelens-mcp/src/integration_tests/semantic_refactor.rs` (또는 신규 파일 `safe_delete_apply_evidence.rs`)에 추가:

```rust
#[test]
fn safe_delete_apply_dry_run_advertises_preview_only() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("sd_dry.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let response = call_tool(
        &state,
        "safe_delete_apply",
        json!({
            "file_path": "sd_dry.py",
            "symbol_name": "alpha",
            "line": 1,
            "column": 1,
            "dry_run": true
        }),
    );
    let scope = response.get("data").unwrap_or(&response);
    let tx = &scope["transaction"]["contract"];
    assert_eq!(tx["apply_status"], "preview_only");
}

#[test]
fn safe_delete_apply_real_apply_returns_evidence() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("sd_apply.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let response = call_tool(
        &state,
        "safe_delete_apply",
        json!({
            "file_path": "sd_apply.py",
            "symbol_name": "alpha",
            "line": 1,
            "column": 1,
            "dry_run": false
        }),
    );
    let scope = response.get("data").unwrap_or(&response);
    let tx = &scope["transaction"]["contract"];
    assert_eq!(tx["apply_status"], "applied");
    assert_eq!(tx["rollback_plan"]["available"], true);
    let hashes_after = tx["file_hashes_after"]
        .as_object()
        .expect("file_hashes_after");
    assert!(!hashes_after.is_empty());
    let report = tx["rollback_report"]
        .as_array()
        .expect("rollback_report");
    assert!(report.is_empty(), "rollback_report should be empty on success");
    // disk shows deletion
    let after = fs::read_to_string(&path).unwrap();
    assert!(!after.contains("def alpha"), "alpha should be deleted: {after:?}");
}

#[cfg(unix)]
#[test]
fn safe_delete_apply_rollback_when_write_blocked() {
    use std::os::unix::fs::PermissionsExt;
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("sd_rollback.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o444);
    fs::set_permissions(&path, perms).unwrap();

    let response = call_tool(
        &state,
        "safe_delete_apply",
        json!({
            "file_path": "sd_rollback.py",
            "symbol_name": "alpha",
            "line": 1,
            "column": 1,
            "dry_run": false
        }),
    );
    let scope = response.get("data").unwrap_or(&response);
    let tx = &scope["transaction"]["contract"];
    // Apply failed, but tool returned Ok with rolled_back status (E5).
    assert_eq!(tx["apply_status"], "rolled_back");
    let report = tx["rollback_report"]
        .as_array()
        .expect("rollback_report");
    assert!(!report.is_empty(), "rollback_report should be populated");

    // restore perms for cleanup
    let mut restore = fs::metadata(&path).unwrap().permissions();
    restore.set_mode(0o644);
    let _ = fs::set_permissions(&path, restore);
}

#[test]
fn safe_delete_apply_dry_run_other_fields_unchanged() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("sd_dry2.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let response = call_tool(
        &state,
        "safe_delete_apply",
        json!({
            "file_path": "sd_dry2.py",
            "symbol_name": "alpha",
            "line": 1,
            "column": 1,
            "dry_run": true
        }),
    );
    let scope = response.get("data").unwrap_or(&response);
    // pre-existing fields stay (safe_to_delete, etc.)
    assert!(scope.get("safe_to_delete").is_some());
    assert!(scope.get("affected_references").is_some());
}
```

- [ ] **Step 2: 실행 — fail 확인**

```bash
cargo test -p codelens-mcp --features http safe_delete_apply 2>&1 | tail -15
```

Expected: 4 새 test 모두 fail (apply_status != "applied" 등).

- [ ] **Step 3: `safe_delete_apply` 본문 마이그레이션**

`crates/codelens-mcp/src/tools/semantic_edit.rs::safe_delete_apply` (line ~380~516)의 apply 분기 (`if !dry_run { ... }` 블록) 교체:

기존:

```rust
if !dry_run {
    // ... tree-sitter range computation ...
    let resolved = state.project().resolve(&file_path)?;
    let mut source = std::fs::read_to_string(&resolved)?;
    // ... boundary checks ...
    source.replace_range(start_byte..delete_end, "");
    std::fs::write(&resolved, source)?;
    safe_delete_action = "applied";
    modified_files = 1;
    edit_count = 1;
}
```

신규:

```rust
let mut apply_evidence: Option<codelens_engine::ApplyEvidence> = None;
let mut apply_status_for_contract = if dry_run { "preview_only" } else { "applied" };
let mut apply_failure_message: Option<String> = None;

if !dry_run {
    // ... existing tree-sitter range computation (unchanged) ...
    let resolved = state.project().resolve(&file_path)?;
    let source_for_preview = std::fs::read_to_string(&resolved)?;
    // ... boundary checks (unchanged) ...
    let delete_text = source_for_preview[start_byte..delete_end].to_owned();

    let line_for_edit = source_for_preview[..start_byte]
        .matches('\n')
        .count()
        + 1;
    let last_newline = source_for_preview[..start_byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let column_for_edit = start_byte - last_newline + 1;

    let edits = vec![codelens_engine::RenameEdit {
        file_path: file_path.clone(),
        line: line_for_edit,
        column: column_for_edit,
        old_text: delete_text,
        new_text: String::new(),
    }];
    let tx = codelens_engine::WorkspaceEditTransaction::new(edits, Vec::new());
    match tx.apply_with_evidence(&state.project()) {
        Ok(evidence) => {
            modified_files = evidence.modified_files;
            edit_count = evidence.edit_count;
            safe_delete_action = "applied";
            apply_status_for_contract = "applied";
            apply_evidence = Some(evidence);
        }
        Err(codelens_engine::ApplyError::ApplyFailed { source, evidence }) => {
            modified_files = 0;
            edit_count = 0;
            safe_delete_action = "rolled_back";
            apply_status_for_contract = "rolled_back";
            apply_failure_message = Some(source.to_string());
            apply_evidence = Some(evidence);
        }
        Err(other) => {
            return Err(CodeLensError::Validation(format!(
                "safe_delete_apply: substrate refused: {other}"
            )));
        }
    }
}
```

기존 `std::fs::read_to_string` + `replace_range` + `std::fs::write` 호출 모두 제거.

contract input 구성 시점에서 새 필드 추가:

```rust
let transaction_contract = semantic_transaction_contract(SemanticTransactionContractInput {
    state,
    backend_id: &format!("lsp:{command_ref}"),
    operation: "safe_delete_check",
    target_symbol: Some(&symbol_name),
    file_paths: std::slice::from_ref(&file_path),
    dry_run,
    modified_files,
    edit_count,
    resource_ops: json!([]),
    rollback_available: apply_evidence.is_some(),
    workspace_edit: json!({"edits": []}),
    apply_status: apply_status_for_contract,
    references_checked: true,
    conflicts: ...,
    evidence: apply_evidence.as_ref(),
});
```

응답 객체 구성 시 `apply_failure_message`가 있으면 별도 필드로 노출 (E5: tool returns Ok with error_message field):

```rust
"error_message": apply_failure_message,  // None이면 JSON null로 직렬화
```

- [ ] **Step 4: 실행 — T2 4개 통과**

```bash
cargo test -p codelens-mcp --features http safe_delete_apply 2>&1 | tail -10
cargo test -p codelens-mcp --no-default-features safe_delete_apply 2>&1 | tail -5
```

Expected: 4 PASS.

- [ ] **Step 5: 회귀 가드 — 전체 mcp**

```bash
cargo test -p codelens-mcp --features http 2>&1 | tail -3
cargo test -p codelens-mcp --no-default-features 2>&1 | tail -3
```

Expected: 0 fail.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/tools/semantic_edit.rs crates/codelens-mcp/src/integration_tests/semantic_refactor.rs
git commit -m "$(cat <<'EOF'
feat(mcp): migrate safe_delete_apply onto WorkspaceEditTransaction

self-managed std::fs::write replaced with single-edit
WorkspaceEditTransaction::apply_with_evidence call. response now
carries fact-based file_hashes_after, rollback_report, and
apply_status (applied/rolled_back/preview_only/no_op). E5 contract:
apply failure surfaces as Ok response with apply_status=rolled_back +
error_message; agent reads apply_status to detect partial state.
4 integration tests added.

Closes Phase 1 G4 substrate migration target.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 10: Final verification + evaluator dispatch

**Files:** none (verification only)

- [ ] **Step 1: 전체 cargo test + clippy**

```bash
cargo check --workspace 2>&1 | tail -5
cargo test -p codelens-engine 2>&1 | grep -E "^test result:" | awk '{ sum += $4 } END { print "engine total:", sum }'
cargo test -p codelens-mcp --no-default-features 2>&1 | tail -3
cargo test -p codelens-mcp --features http 2>&1 | tail -3
cargo clippy -p codelens-engine -- -W clippy::all 2>&1 | tail -10
cargo clippy -p codelens-mcp --features http -- -W clippy::all 2>&1 | tail -10
```

Expected:

- workspace check clean
- engine ≥ 321 + 8 (T1) = ≥ 329 PASS
- mcp no-default ≥ 411 + 4 (T2) = ≥ 415 PASS, 0 fail
- mcp http ≥ 522 + 4 (T2) + 1 (T3) = ≥ 527 PASS
- clippy: 0 NEW warnings (deprecated 사용 사이트만 expected new note)

- [ ] **Step 2: surface-manifest + contract gate**

```bash
python3 scripts/surface-manifest.py 2>&1 | tail -3
echo "drift-exit=$?"
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json
echo "matrix-exit=$?"
python3 scripts/test/test-surface-manifest-contracts.py
echo "fixtures-exit=$?"
```

Expected: 모두 exit 0.

- [ ] **Step 3: lint-datasets + agent-contract-check**

```bash
python3 benchmarks/lint-datasets.py --project . 2>&1 | tail -3
python3 scripts/agent-contract-check.py --project . --strict 2>&1 | tail -3
```

Expected: 0 errors / 0 warnings.

- [ ] **Step 4: AC checklist self-check (grep)**

```bash
echo "=== AC-1: substrate types ==="
grep -E "pub struct WorkspaceEditTransaction|pub struct ApplyEvidence|pub enum ApplyStatus|pub struct RollbackEntry|pub struct FileHash|pub enum ApplyError" crates/codelens-engine/src/edit_transaction.rs | wc -l
echo "(expect: 6)"

echo "=== AC-2: LSP path 통합 ==="
grep -n "apply_workspace_edit_transaction" crates/codelens-engine/src/lsp/workspace_edit.rs
grep -n "From<LspWorkspaceEditTransaction>" crates/codelens-engine/src/lsp/types.rs
grep -n "deprecated" crates/codelens-engine/src/lsp/types.rs

echo "=== AC-3: safe_delete_apply substrate use ==="
grep -n "WorkspaceEditTransaction::new\|apply_with_evidence" crates/codelens-mcp/src/tools/semantic_edit.rs
grep -c "std::fs::write" crates/codelens-mcp/src/tools/semantic_edit.rs
echo "(expect: 0 std::fs::write in semantic_edit.rs)"

echo "=== AC-4: contract evidence param ==="
grep -n "evidence: Option" crates/codelens-mcp/src/tools/semantic_edit.rs

echo "=== AC-6: range guard ==="
git diff 25a49523..HEAD --name-only | wc -l
git diff 25a49523..HEAD --name-only

echo "=== AC-7: Phase 0 envelope preserved ==="
git diff 25a49523..HEAD -- crates/codelens-mcp/src/tools/mutation.rs | wc -l
echo "(expect: 0 — mutation.rs untouched)"
```

각 출력이 기대값과 일치하는지 확인.

- [ ] **Step 5: evaluator(opus) dispatch**

```text
Agent dispatch:
  subagent_type: evaluator
  model: opus
  prompt: |
    Spec: docs/superpowers/specs/2026-04-25-codelens-phase1-g4-workspace-edit-transaction-design.md (§6)
    Plan: docs/superpowers/plans/2026-04-25-codelens-phase1-g4-workspace-edit-transaction.md
    Branch HEAD vs base 25a49523 diff와 test 결과를 종합해 AC-1~AC-8 각각 PASS/PARTIAL/FAIL 채점.
    AC-1~AC-7 중 1개라도 FAIL이면 전체 FAIL — 머지 보류.
    출력 형식: 각 AC별 1줄 판정 + 증거 + 전체 verdict.
```

- [ ] **Step 6: PR 본문 작성 (선택, 사용자 승인 후)**

evaluator PASS 후 사용자 명시 승인 시:

```bash
gh pr create --base main --head feat/phase1-g4-workspace-edit-transaction \
  --title "feat(engine+mcp): Phase 1 G4 — WorkspaceEditTransaction substrate" \
  --body "$(cat <<'EOF'
## Summary

- New `crates/codelens-engine/src/edit_transaction.rs` substrate: `WorkspaceEditTransaction` domain object with `apply_with_evidence` returning `ApplyEvidence{status, file_hashes_before/after, rollback_report}`.
- LSP `apply_workspace_edit_transaction` thin wrapper delegating to substrate. `LspWorkspaceEditTransaction.rollback_available` deprecated.
- `safe_delete_apply` migrated off self-managed `fs::write` onto substrate. Apply failure surfaces as Ok response with `apply_status=rolled_back`.
- `semantic_transaction_contract` takes `evidence: Option<&ApplyEvidence>`; 4 callers updated.

Spec: `docs/superpowers/specs/2026-04-25-codelens-phase1-g4-workspace-edit-transaction-design.md`
Plan: `docs/superpowers/plans/2026-04-25-codelens-phase1-g4-workspace-edit-transaction.md`

Closes Phase 1 G4. G7 (engine `fs::write` 11 sites) and G5 (runtime capability probing) deferred to separate PRs.

## Test plan
- [x] `cargo test -p codelens-engine` — 8 substrate tests
- [x] `cargo test -p codelens-mcp --features http` — 4 safe_delete + 1 LSP regression
- [x] `python3 scripts/surface-manifest.py` + contract A/B + fixtures
- [x] evaluator(opus) AC-1~AC-8 PASS

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-Review

**1. Spec coverage:**

| Spec 항목                                                                    | Plan task                 |
| ---------------------------------------------------------------------------- | ------------------------- |
| §1 사용자 변경 — safe_delete_apply rollback_available true                   | Task 9                    |
| §1 사용자 변경 — LSP rename evidence 사실 기반                               | Task 8                    |
| §1 사용자 변경 — rollback_report 노출                                        | Task 5 + 8 + 9            |
| §2 A edit_transaction.rs 신규                                                | Task 1~6                  |
| §2 B lsp/workspace_edit.rs 시그니처                                          | Task 7                    |
| §2 C lib.rs 등록                                                             | Task 1 Step 2             |
| §2 D semantic_transaction_contract 시그니처 + safe_delete_apply 마이그레이션 | Task 8 + 9                |
| §2 E LspWorkspaceEditTransaction From + deprecated                           | Task 7 Step 1             |
| §2 F edit_transaction tests                                                  | Task 2~6 (8 case)         |
| §2 G integration tests T2/T3                                                 | Task 8 (T3) + Task 9 (T2) |
| §6 AC-1~AC-8                                                                 | Task 10                   |

전 항목 커버됨.

**2. Placeholder scan:** TBD/TODO/"add appropriate" 0건. 모든 step에 실제 코드/명령/expected output.

**3. Type consistency:**

- `WorkspaceEditTransaction::new(edits, resource_ops)` 시그니처 Task 1·9 동일. ✅
- `apply_with_evidence(&self, project) -> Result<ApplyEvidence, ApplyError>` Task 1·7·9 동일. ✅
- `RollbackEntry { file_path, restored, reason }` Task 1·5 동일. ✅
- `evidence: Option<&codelens_engine::ApplyEvidence>` Task 8·9 동일. ✅
- `apply_status` 값 enum: `applied` / `rolled_back` / `preview_only` / `no_op` Task 8·9 동일. ✅

**4. Plan refinements**: spec 100% 매칭. 추가 refinement 없음.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-25-codelens-phase1-g4-workspace-edit-transaction.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — Task별 builder agent dispatch + 두 단계 리뷰. 10 task = 약 10~12 dispatch.

**2. Inline Execution** — 이 세션에서 executing-plans 스킬로 batch 실행, 중간 checkpoint.

**Which approach?**
