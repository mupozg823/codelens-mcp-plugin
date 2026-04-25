# Phase 1 G7 — Single-File Mutation Substrate Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate 9 single-file mutation primitives (writer.rs 8 + auto_import.rs `add_import`) onto the G4 `edit_transaction` substrate via a new free function `apply_full_write_with_evidence`, exposing `ApplyEvidence` in their return types and merging 6 evidence keys into MCP tool responses.

**Architecture:** A new free function in `crates/codelens-engine/src/edit_transaction.rs` performs the same 4-phase sequence as G4 `WorkspaceEditTransaction::apply_with_evidence` (capture → verify → write → post-hash) but for a single path with full-content rewrite semantics. The 9 primitive callsites replace their final `fs::write` line with one call to this helper. MCP `tools/mutation.rs` handlers unpack `(content, evidence)` tuples and merge 6 evidence keys into the existing Phase 0 envelope. `WorkspaceEditTransaction` domain object is untouched.

**Tech Stack:** Rust (edition 2021), `sha2` for hashing, `serde` for evidence serialization, `anyhow::Result` at engine boundaries, MCP JSON envelope.

**Branch:** `feat/phase1-g7-fullfile-substrate` (already created, stacked on `feat/phase1-g4-workspace-edit-transaction`; rebases onto `main` once PR #83 merges)

**Spec:** `docs/superpowers/specs/2026-04-25-codelens-phase1-g7-fullfile-substrate-design.md` (commit 5873b8f5)

---

## File Structure

| File                                                                   | Responsibility                      | Change                                                                      |
| ---------------------------------------------------------------------- | ----------------------------------- | --------------------------------------------------------------------------- |
| `crates/codelens-engine/src/edit_transaction.rs`                       | G4 substrate + new G7 free function | Add `apply_full_write_with_evidence` + 6 unit tests in existing `mod tests` |
| `crates/codelens-engine/src/file_ops/writer.rs`                        | 8 single-file mutation primitives   | Change return types (8 funcs); replace `fs::write` with substrate call      |
| `crates/codelens-engine/src/auto_import.rs`                            | `add_import` primitive (1 fn)       | Same pattern as writer.rs                                                   |
| `crates/codelens-mcp/src/tools/mutation.rs`                            | 9 MCP tool handlers                 | Unpack `(content, evidence)` and merge 6 evidence keys into envelope        |
| `crates/codelens-mcp/src/integration_tests/mutation_evidence.rs` (NEW) | M1-M5 integration tests             | Cover happy/E2/E4 contracts                                                 |
| `crates/codelens-mcp/src/integration_tests/mod.rs`                     | Test module registration            | Add `pub mod mutation_evidence;`                                            |

**No new types** — every type used in this plan (`ApplyEvidence`, `ApplyStatus`, `RollbackEntry`, `FileHash`, `ApplyError`) is already defined in `edit_transaction.rs` from G4.

**Out of scope** (G7b follow-up): `crates/codelens-engine/src/move_symbol.rs` lines 172, 194 (2-file atomic). `crates/codelens-engine/src/rename.rs:455` (already covered by G4 LSP path).

---

## Existing G4 substrate types (do not redefine)

```rust
// crates/codelens-engine/src/edit_transaction.rs (already exists)
pub struct ApplyEvidence {
    pub status: ApplyStatus,
    pub file_hashes_before: BTreeMap<String, FileHash>,
    pub file_hashes_after: BTreeMap<String, FileHash>,
    pub rollback_report: Vec<RollbackEntry>,
    pub modified_files: usize,
    pub edit_count: usize,
}

pub enum ApplyStatus { Applied, RolledBack, NoOp }

pub struct RollbackEntry {
    pub file_path: String,
    pub restored: bool,
    pub reason: Option<String>,
}

pub struct FileHash {
    pub sha256: String,
    pub bytes: usize,
}

pub enum ApplyError {
    ResourceOpsUnsupported,
    PreReadFailed { file_path: String, source: anyhow::Error },
    PreApplyHashMismatch { file_path: String, expected: String, actual: String },
    ApplyFailed { source: anyhow::Error, evidence: ApplyEvidence },
}

fn sha256_hex(bytes: &[u8]) -> String { ... }  // module-private free fn
```

---

## Task 1: Substrate skeleton + happy path (T1)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Goal:** Add the new free function with full happy-path implementation (capture → verify → write → post-hash) and one unit test that exercises the full success flow.

- [ ] **Step 1: Add the failing test for happy path**

In `crates/codelens-engine/src/edit_transaction.rs`, inside `#[cfg(test)] mod tests`, append:

```rust
#[test]
fn apply_full_write_happy_returns_evidence() {
    let project = empty_project();
    write_file(&project, "doc.txt", "old content\n");
    let evidence =
        apply_full_write_with_evidence(&project, "doc.txt", "new content\n").expect("apply ok");
    assert_eq!(evidence.status, ApplyStatus::Applied);
    assert_eq!(evidence.modified_files, 1);
    assert_eq!(evidence.edit_count, 1);
    assert!(evidence.rollback_report.is_empty());
    let before = evidence
        .file_hashes_before
        .get("doc.txt")
        .expect("before entry");
    let after = evidence
        .file_hashes_after
        .get("doc.txt")
        .expect("after entry");
    assert_ne!(before.sha256, after.sha256);
    assert_eq!(after.bytes, "new content\n".len());
    assert_eq!(
        std::fs::read_to_string(project.resolve("doc.txt").unwrap()).unwrap(),
        "new content\n"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p codelens-engine apply_full_write_happy_returns_evidence
```

Expected: `error[E0425]: cannot find function 'apply_full_write_with_evidence' in this scope` (compile error).

- [ ] **Step 3: Add the function above the `#[cfg(test)] mod tests` block**

In `crates/codelens-engine/src/edit_transaction.rs`, immediately above `#[cfg(test)]`:

```rust
/// Apply a full-content rewrite to a single file with hash-based evidence
/// and rollback on write failure. Used by single-file mutation primitives
/// (`create_text_file`, `delete_lines`, `replace_lines`, etc.) that already
/// performed an in-memory transform and need to commit the result with the
/// same TOCTOU + rollback guarantees as `WorkspaceEditTransaction`.
///
/// Phases:
/// 1. capture: read existing file (if any), sha256 + raw backup
/// 2. verify: re-read + sha256 compare (light TOCTOU window)
/// 3. write: fs::write — on failure, restore backup + populate rollback_report
/// 4. post-hash: read written file + sha256 → file_hashes_after
///
/// For files that do not exist (e.g., `create_text_file` against a new path),
/// Phase 1 captures no entry and Phase 2 is a no-op for that path.
pub fn apply_full_write_with_evidence(
    project: &ProjectRoot,
    relative_path: &str,
    new_content: &str,
) -> Result<ApplyEvidence, ApplyError> {
    let resolved = project
        .resolve(relative_path)
        .map_err(|e| ApplyError::PreReadFailed {
            file_path: relative_path.to_owned(),
            source: e,
        })?;

    // Phase 1: capture (only if file exists)
    let (backup_bytes, file_hashes_before) = match fs::read(&resolved) {
        Ok(bytes) => {
            let mut before = BTreeMap::new();
            before.insert(
                relative_path.to_owned(),
                FileHash {
                    sha256: sha256_hex(&bytes),
                    bytes: bytes.len(),
                },
            );
            (Some(bytes), before)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (None, BTreeMap::new()),
        Err(err) => {
            return Err(ApplyError::PreReadFailed {
                file_path: relative_path.to_owned(),
                source: anyhow::Error::from(err),
            });
        }
    };

    // Phase 2: verify (TOCTOU re-check) — only if file existed
    if let Some(expected_hash) = file_hashes_before
        .get(relative_path)
        .map(|h| h.sha256.clone())
    {
        let bytes_now = fs::read(&resolved).map_err(|e| ApplyError::PreReadFailed {
            file_path: relative_path.to_owned(),
            source: anyhow::Error::from(e),
        })?;
        let hash_now = sha256_hex(&bytes_now);
        if hash_now != expected_hash {
            return Err(ApplyError::PreApplyHashMismatch {
                file_path: relative_path.to_owned(),
                expected: expected_hash,
                actual: hash_now,
            });
        }
    }

    // Phase 3: write — on failure, restore backup + record rollback
    if let Err(write_err) = fs::write(&resolved, new_content) {
        let mut rollback_report: Vec<RollbackEntry> = Vec::new();
        if let Some(bytes) = backup_bytes.as_ref() {
            match fs::write(&resolved, bytes) {
                Ok(()) => rollback_report.push(RollbackEntry {
                    file_path: relative_path.to_owned(),
                    restored: true,
                    reason: None,
                }),
                Err(e) => rollback_report.push(RollbackEntry {
                    file_path: relative_path.to_owned(),
                    restored: false,
                    reason: Some(format!("write failed: {e}")),
                }),
            }
        } else {
            rollback_report.push(RollbackEntry {
                file_path: relative_path.to_owned(),
                restored: false,
                reason: Some("no backup captured (file did not exist before apply)".to_owned()),
            });
        }
        let mut file_hashes_after_rb: BTreeMap<String, FileHash> = BTreeMap::new();
        if let Ok(bytes) = fs::read(&resolved) {
            file_hashes_after_rb.insert(
                relative_path.to_owned(),
                FileHash {
                    sha256: sha256_hex(&bytes),
                    bytes: bytes.len(),
                },
            );
        }
        return Err(ApplyError::ApplyFailed {
            source: anyhow::Error::from(write_err),
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

    // Phase 4: post-hash
    let mut file_hashes_after: BTreeMap<String, FileHash> = BTreeMap::new();
    match fs::read(&resolved) {
        Ok(bytes) => {
            file_hashes_after.insert(
                relative_path.to_owned(),
                FileHash {
                    sha256: sha256_hex(&bytes),
                    bytes: bytes.len(),
                },
            );
        }
        Err(_) => {
            file_hashes_after.insert(
                relative_path.to_owned(),
                FileHash {
                    sha256: String::new(),
                    bytes: 0,
                },
            );
        }
    }

    Ok(ApplyEvidence {
        status: ApplyStatus::Applied,
        file_hashes_before,
        file_hashes_after,
        rollback_report: Vec::new(),
        modified_files: 1,
        edit_count: 1,
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p codelens-engine apply_full_write_happy_returns_evidence
```

Expected: `1 passed`. Also run `cargo check -p codelens-engine` and confirm 0 errors / 0 new warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
feat(engine): add apply_full_write_with_evidence substrate

new free function reusing G4 ApplyEvidence/ApplyStatus/RollbackEntry/
FileHash/ApplyError types. 4-phase sequence: capture (if file exists) →
verify (TOCTOU light window) → fs::write → post-hash. on Phase 3 failure
restores backup and surfaces ApplyFailed{evidence{status: RolledBack}}.
files that do not exist before apply (create_text_file new path) skip
Phase 1/2 and have no entry in file_hashes_before.

T1 happy-path test added.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: PreReadFailed test (T2)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Goal:** Cover the case where the parent directory itself does not exist (true unreadable case, distinct from `create_text_file` against a non-existent file in an existing dir).

- [ ] **Step 1: Add the failing test**

In `mod tests`, append:

```rust
#[test]
fn apply_full_write_pre_read_failed_on_unresolvable_path() {
    let project = empty_project();
    // Path with absolute escape — project.resolve will error.
    let result = apply_full_write_with_evidence(&project, "../escape.txt", "x");
    assert!(
        matches!(result, Err(ApplyError::PreReadFailed { ref file_path, .. }) if file_path == "../escape.txt"),
        "expected PreReadFailed for ../escape.txt, got {:?}",
        result.err()
    );
}
```

- [ ] **Step 2: Run test to verify it passes immediately**

```bash
cargo test -p codelens-engine apply_full_write_pre_read_failed_on_unresolvable_path
```

Expected: `1 passed` (the resolve error path is already implemented in Task 1).

If it fails because `project.resolve` succeeds for `../escape.txt` (depends on `ProjectRoot` policy), instead replace the test body with one that uses a deeply nonexistent absolute path or removes the parent dir mid-test. Verify the resulting Err arm is `PreReadFailed`.

- [ ] **Step 3: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
test(engine): cover PreReadFailed for unresolvable path in full-write substrate

T2 ensures path resolution failures (escape outside project root) surface
as ApplyError::PreReadFailed before any IO.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: TOCTOU mismatch test (T3)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Goal:** Verify that an external mutation between Phase 1 capture and Phase 2 verify is detected. Since the new helper is a single function (no separate `capture` / `verify` entry points like G4 method), the test simulates by patching the file and relying on a thread-local helper or by inspecting the function's behaviour with a wrapper.

Given that `apply_full_write_with_evidence` does capture and verify back-to-back internally, the only way to inject mutation between them in production code is via concurrent fs writes — flaky for tests. Instead, add a `#[cfg(test)]` injection point.

- [ ] **Step 1: Add `#[cfg(test)]` injection hook to the substrate**

Above the `apply_full_write_with_evidence` function, add:

```rust
#[cfg(test)]
thread_local! {
    /// Test-only hook: when set, called once between Phase 1 capture and
    /// Phase 2 verify with the resolved path so a test can mutate the file
    /// to simulate TOCTOU drift. Cleared after one call.
    pub(crate) static FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY:
        std::cell::RefCell<Option<Box<dyn FnOnce(&std::path::Path)>>> =
        std::cell::RefCell::new(None);
}
```

Then inside `apply_full_write_with_evidence`, after Phase 1 capture and immediately before the `if let Some(expected_hash)` Phase 2 block, insert:

```rust
    #[cfg(test)]
    FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY.with(|cell| {
        if let Some(hook) = cell.borrow_mut().take() {
            hook(&resolved);
        }
    });
```

- [ ] **Step 2: Add the failing test**

In `mod tests`, append:

```rust
#[test]
fn apply_full_write_toctou_mismatch_via_inject_hook() {
    let project = empty_project();
    let path = write_file(&project, "drift.txt", "before\n");
    FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY.with(|cell| {
        let hook: Box<dyn FnOnce(&std::path::Path)> = Box::new(|p: &std::path::Path| {
            std::fs::write(p, "TAMPERED\n").unwrap();
        });
        *cell.borrow_mut() = Some(hook);
    });
    let result = apply_full_write_with_evidence(&project, "drift.txt", "after\n");
    assert!(
        matches!(result, Err(ApplyError::PreApplyHashMismatch { ref file_path, .. }) if file_path == "drift.txt"),
        "expected PreApplyHashMismatch, got {:?}",
        result.err()
    );
    // Disk has the external mutation; substrate did not write "after\n".
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "TAMPERED\n");
}
```

- [ ] **Step 3: Run test to verify it passes**

```bash
cargo test -p codelens-engine apply_full_write_toctou_mismatch_via_inject_hook
```

Expected: `1 passed`. Also confirm `cargo check -p codelens-engine` is 0 errors.

- [ ] **Step 4: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
test(engine): TOCTOU mismatch test for full-write substrate via cfg(test) hook

T3 injects an external write between Phase 1 capture and Phase 2 verify
through a thread_local FnOnce hook (test-only). substrate detects the
sha256 drift and returns PreApplyHashMismatch without applying the
intended write.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Rollback test (T4)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Goal:** Verify that when `fs::write` fails, the substrate restores the backup and surfaces an `ApplyFailed{evidence}` with `rollback_report[0].restored=true`.

- [ ] **Step 1: Add the failing test (`#[cfg(unix)]` gated)**

In `mod tests`, append:

```rust
#[cfg(unix)]
#[test]
fn apply_full_write_rollback_on_write_failure() {
    use std::os::unix::fs::PermissionsExt;
    let project = empty_project();
    let path = write_file(&project, "ro.txt", "original\n");
    // Make parent dir read-only so fs::write to ro.txt fails.
    // (chmod the file alone is not enough — fs::write may still truncate.)
    let parent = path.parent().unwrap().to_path_buf();
    let mut parent_perms = std::fs::metadata(&parent).unwrap().permissions();
    parent_perms.set_mode(0o555);
    std::fs::set_permissions(&parent, parent_perms).unwrap();

    let result = apply_full_write_with_evidence(&project, "ro.txt", "new\n");

    // Restore parent perms before assertions so tempdir cleanup works.
    let mut restore = std::fs::metadata(&parent).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(&parent, restore).unwrap();

    let evidence = match result {
        Err(ApplyError::ApplyFailed { evidence, .. }) => evidence,
        other => panic!("expected ApplyFailed, got {other:?}"),
    };
    assert_eq!(evidence.status, ApplyStatus::RolledBack);
    assert_eq!(evidence.modified_files, 0);
    assert_eq!(evidence.edit_count, 0);
    assert_eq!(evidence.rollback_report.len(), 1);
    let entry = &evidence.rollback_report[0];
    assert_eq!(entry.file_path, "ro.txt");
    assert!(
        entry.restored,
        "expected restore success, got reason: {:?}",
        entry.reason
    );
    // Disk is back to original content.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
    // Hashes match between before and after (rollback succeeded).
    let before = evidence.file_hashes_before.get("ro.txt").unwrap();
    let after = evidence.file_hashes_after.get("ro.txt").unwrap();
    assert_eq!(before.sha256, after.sha256);
}
```

- [ ] **Step 2: Run test to verify it passes**

```bash
cargo test -p codelens-engine apply_full_write_rollback_on_write_failure
```

Expected: `1 passed` on unix. If chmod 0o555 on parent does not block `fs::write` on this OS (e.g., running as root), the test will need to use a different injection (e.g., chmod the file itself to 0o444 may suffice on some platforms). Confirm `before.sha256 == after.sha256` invariant holds after rollback.

- [ ] **Step 3: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
test(engine): rollback evidence on full-write substrate Phase 3 failure

T4 (#[cfg(unix)]) chmods the parent directory to 0o555 to force fs::write
to fail. substrate restores the backup and surfaces ApplyFailed{evidence}
with rollback_report[0].restored=true; post-rollback sha256 matches
pre-apply sha256.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Hash determinism + no-op tests (T5 + T6)

**Files:**

- Modify: `crates/codelens-engine/src/edit_transaction.rs`

**Goal:** Two small tests covering deterministic hashing and the no-op (new == old) case.

- [ ] **Step 1: Add both tests**

In `mod tests`, append:

```rust
#[test]
fn apply_full_write_hash_determinism() {
    let project = empty_project();
    write_file(&project, "stable.txt", "stable content\n");
    let ev1 =
        apply_full_write_with_evidence(&project, "stable.txt", "new1\n").expect("first apply");
    write_file(&project, "stable.txt", "stable content\n"); // reset disk
    let ev2 =
        apply_full_write_with_evidence(&project, "stable.txt", "new2\n").expect("second apply");
    let h1 = &ev1.file_hashes_before["stable.txt"].sha256;
    let h2 = &ev2.file_hashes_before["stable.txt"].sha256;
    assert_eq!(h1, h2, "same input bytes should yield identical sha256");
}

#[test]
fn apply_full_write_no_op_same_content() {
    let project = empty_project();
    write_file(&project, "noop.txt", "same\n");
    let evidence = apply_full_write_with_evidence(&project, "noop.txt", "same\n").expect("noop ok");
    assert_eq!(evidence.status, ApplyStatus::Applied);
    let before = &evidence.file_hashes_before["noop.txt"].sha256;
    let after = &evidence.file_hashes_after["noop.txt"].sha256;
    assert_eq!(before, after, "no-op should leave hash unchanged");
    assert_eq!(evidence.modified_files, 1);
    assert_eq!(evidence.edit_count, 1);
}
```

- [ ] **Step 2: Run both tests**

```bash
cargo test -p codelens-engine apply_full_write_hash_determinism apply_full_write_no_op_same_content
```

Expected: `2 passed`.

- [ ] **Step 3: Run the full edit_transaction test module**

```bash
cargo test -p codelens-engine edit_transaction
```

Expected: all G4 tests still PASS plus 6 new G7 tests (T1-T6) PASS. Total module count = G4 baseline (8) + G7 (6) = 14 passing, 0 failed.

- [ ] **Step 4: Commit**

```bash
git add crates/codelens-engine/src/edit_transaction.rs
git commit -m "$(cat <<'EOF'
test(engine): hash determinism + no-op cases for full-write substrate

T5 confirms identical input bytes yield identical sha256 across runs.
T6 confirms new_content == existing produces Applied status with
unchanged before/after hash (substrate still calls fs::write, mtime
touched, but content hash is invariant).

Closes the 6-case substrate test matrix for apply_full_write_with_evidence.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Migrate `writer.rs` 8 functions

**Files:**

- Modify: `crates/codelens-engine/src/file_ops/writer.rs`

**Goal:** Change return types of all 8 functions to expose `ApplyEvidence`, replace each final `fs::write(&resolved, &result)` line with a call to `apply_full_write_with_evidence`. Update existing unit tests in this file to destructure the new tuple return shape.

- [ ] **Step 1: Inspect existing tests in writer.rs**

```bash
rg -n '#\[test\]|fn.*test|create_text_file|delete_lines|insert_at_line|replace_lines|replace_content|replace_symbol_body|insert_before_symbol|insert_after_symbol' crates/codelens-engine/src/file_ops/writer.rs
```

Note which tests exist and what they assert. They will need destructuring updates after this task.

- [ ] **Step 2: Replace the entire content of `crates/codelens-engine/src/file_ops/writer.rs`**

```rust
use crate::edit_transaction::{apply_full_write_with_evidence, ApplyEvidence};
use crate::project::ProjectRoot;
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::fs;

pub fn create_text_file(
    project: &ProjectRoot,
    relative_path: &str,
    content: &str,
    overwrite: bool,
) -> Result<ApplyEvidence> {
    let resolved = project.resolve(relative_path)?;
    if !overwrite && resolved.exists() {
        bail!("file already exists: {}", resolved.display());
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directories for {}", resolved.display()))?;
    }
    let evidence = apply_full_write_with_evidence(project, relative_path, content)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok(evidence)
}

pub fn delete_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
) -> Result<(String, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start_line < 1 || start_line > total + 1 {
        bail!(
            "start_line {} out of range (file has {} lines)",
            start_line,
            total
        );
    }
    if end_line < start_line || end_line > total + 1 {
        bail!("end_line {} out of range", end_line);
    }
    let from = start_line - 1;
    let to = (end_line - 1).min(lines.len());
    lines.drain(from..to);
    let result = lines.join("\n");
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, evidence))
}

pub fn insert_at_line(
    project: &ProjectRoot,
    relative_path: &str,
    line: usize,
    content_to_insert: &str,
) -> Result<(String, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if line < 1 || line > total + 1 {
        bail!("line {} out of range (file has {} lines)", line, total);
    }
    let insert_pos = line - 1;
    let new_lines: Vec<&str> = content_to_insert.lines().collect();
    for (i, new_line) in new_lines.iter().enumerate() {
        lines.insert(insert_pos + i, new_line);
    }
    let result = lines.join("\n");
    let result = if content.ends_with('\n') || content_to_insert.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, evidence))
}

pub fn replace_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
    new_content: &str,
) -> Result<(String, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start_line < 1 || start_line > total + 1 {
        bail!(
            "start_line {} out of range (file has {} lines)",
            start_line,
            total
        );
    }
    if end_line < start_line || end_line > total + 1 {
        bail!("end_line {} out of range", end_line);
    }
    let from = start_line - 1;
    let to = (end_line - 1).min(lines.len());
    lines.drain(from..to);
    let replacement: Vec<&str> = new_content.lines().collect();
    for (i, rep_line) in replacement.iter().enumerate() {
        lines.insert(from + i, rep_line);
    }
    let result = lines.join("\n");
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, evidence))
}

pub fn replace_content(
    project: &ProjectRoot,
    relative_path: &str,
    old_text: &str,
    new_text: &str,
    regex_mode: bool,
) -> Result<(String, usize, ApplyEvidence)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let (result, count) = if regex_mode {
        let re = Regex::new(old_text).with_context(|| format!("invalid regex: {old_text}"))?;
        let mut count = 0usize;
        let replaced = re
            .replace_all(&content, |_caps: &regex::Captures| {
                count += 1;
                new_text
            })
            .into_owned();
        (replaced, count)
    } else {
        let count = content.matches(old_text).count();
        let replaced = content.replace(old_text, new_text);
        (replaced, count)
    };
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, count, evidence))
}

pub fn replace_symbol_body(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    new_body: &str,
) -> Result<(String, ApplyEvidence)> {
    let (start_byte, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len());
    buffer.extend_from_slice(&bytes[..start_byte]);
    buffer.extend_from_slice(new_body.as_bytes());
    buffer.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(buffer).with_context(|| "result is not valid UTF-8 after replacement")?;
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, evidence))
}

pub fn insert_before_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<(String, ApplyEvidence)> {
    let (start_byte, _) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len() + content_to_insert.len());
    buffer.extend_from_slice(&bytes[..start_byte]);
    buffer.extend_from_slice(content_to_insert.as_bytes());
    buffer.extend_from_slice(&bytes[start_byte..]);
    let result =
        String::from_utf8(buffer).with_context(|| "result is not valid UTF-8 after insertion")?;
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, evidence))
}

pub fn insert_after_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<(String, ApplyEvidence)> {
    let (_, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut buffer = Vec::with_capacity(bytes.len() + content_to_insert.len());
    buffer.extend_from_slice(&bytes[..end_byte]);
    buffer.extend_from_slice(content_to_insert.as_bytes());
    buffer.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(buffer).with_context(|| "result is not valid UTF-8 after insertion")?;
    let evidence = apply_full_write_with_evidence(project, relative_path, &result)
        .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((result, evidence))
}
```

- [ ] **Step 3: Update writer.rs unit tests for new return shapes**

If `crates/codelens-engine/src/file_ops/writer.rs` has its own `#[cfg(test)] mod tests`, update each test that asserted on the prior return shape:

- `let content = create_text_file(...)?;` → `let evidence = create_text_file(...)?; assert_eq!(evidence.status, ApplyStatus::Applied);`
- `let content = delete_lines(...)?;` → `let (content, evidence) = delete_lines(...)?;`
- `let content = insert_at_line(...)?;` → `let (content, evidence) = insert_at_line(...)?;`
- `let content = replace_lines(...)?;` → `let (content, evidence) = replace_lines(...)?;`
- `let (content, count) = replace_content(...)?;` → `let (content, count, evidence) = replace_content(...)?;`
- `let content = replace_symbol_body(...)?;` → `let (content, evidence) = replace_symbol_body(...)?;`
- `let content = insert_before_symbol(...)?;` → `let (content, evidence) = insert_before_symbol(...)?;`
- `let content = insert_after_symbol(...)?;` → `let (content, evidence) = insert_after_symbol(...)?;`

For the `create_text_file` evidence variant only, also import `use crate::edit_transaction::ApplyStatus;` at the top of the test module.

If any caller-site test (in this file or other engine modules) breaks, fix it locally with the same destructuring pattern.

- [ ] **Step 4: Add caller-site evidence integration test**

In writer.rs's test module (or create one if absent), append:

```rust
#[test]
fn replace_lines_evidence_post_apply_hash_matches_disk() {
    use crate::edit_transaction::ApplyStatus;
    use sha2::{Digest, Sha256};

    let dir = std::env::temp_dir().join(format!(
        "codelens-writer-evidence-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
    std::fs::write(dir.join("doc.txt"), "line1\nline2\nline3\n").unwrap();

    let (content, evidence) = replace_lines(&project, "doc.txt", 2, 3, "REPLACED\n").unwrap();
    assert!(content.contains("REPLACED"));
    assert_eq!(evidence.status, ApplyStatus::Applied);
    assert_eq!(evidence.modified_files, 1);
    assert_eq!(evidence.edit_count, 1);

    let on_disk = std::fs::read(dir.join("doc.txt")).unwrap();
    let mut hasher = Sha256::new();
    hasher.update(&on_disk);
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    let evidence_hash = &evidence.file_hashes_after["doc.txt"].sha256;
    assert_eq!(
        evidence_hash, &hex,
        "evidence post-apply hash must match disk content"
    );
}
```

Add to top of test module if not present:

```rust
use super::*;
```

If writer.rs has no `#[cfg(test)] mod tests` block currently, add one at the bottom of the file with this single test.

- [ ] **Step 5: Build + run all engine tests**

```bash
cargo build -p codelens-engine
cargo test -p codelens-engine
```

Expected: `0 errors`, all tests pass. The G4 baseline (engine 320 lib + integration) plus 6 new substrate tests plus 1 new caller-site test. Note any callers of the 8 functions in other engine modules that broke and fix them with the destructuring pattern (most likely: tests inside `git.rs`, `import_graph/mod.rs`, etc., and possibly mcp tests via integration crate path — those will be fixed in Tasks 8/9).

If engine compile passes but mcp does not yet, that is expected — Task 8 fixes mcp callers. Run engine-only checks at this stage:

```bash
cargo check -p codelens-engine
cargo test -p codelens-engine
```

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-engine/src/file_ops/writer.rs
git commit -m "$(cat <<'EOF'
feat(engine): migrate writer.rs 8 primitives onto full-write substrate

create_text_file / delete_lines / insert_at_line / replace_lines /
replace_content / replace_symbol_body / insert_before_symbol /
insert_after_symbol now expose ApplyEvidence in their return type and
delegate the final fs::write step to apply_full_write_with_evidence.
each gains TOCTOU verification, hash-based evidence, and rollback
on Phase 3 IO failure. existing unit tests destructured for new
tuple shape. caller-site evidence regression test confirms post-apply
sha256 matches disk content.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Migrate `auto_import.rs::add_import`

**Files:**

- Modify: `crates/codelens-engine/src/auto_import.rs`

**Goal:** Change `add_import` return type to `Result<(String, ApplyEvidence)>` and replace its `fs::write` with substrate call.

- [ ] **Step 1: Inspect current `add_import` implementation**

```bash
rg -n 'pub fn add_import|fs::write' crates/codelens-engine/src/auto_import.rs
```

Note exact line where `fs::write(&resolved, ...)` (or equivalent) lives. Read context from `pub fn add_import(...)` signature down through the function body.

- [ ] **Step 2: Update `add_import` signature and body**

Replace the function. Before:

```rust
pub fn add_import(
    project: &ProjectRoot,
    relative_path: &str,
    import_statement: &str,
) -> Result<String> {
    // ... compute new_content ...
    fs::write(&resolved, &new_content)?;
    Ok(new_content)
}
```

After:

```rust
pub fn add_import(
    project: &ProjectRoot,
    relative_path: &str,
    import_statement: &str,
) -> Result<(String, crate::edit_transaction::ApplyEvidence)> {
    // ... existing logic computes new_content ...
    let evidence =
        crate::edit_transaction::apply_full_write_with_evidence(project, relative_path, &new_content)
            .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    Ok((new_content, evidence))
}
```

Preserve all logic that produces `new_content`; only change the final `fs::write` line and the return.

- [ ] **Step 3: Update auto_import.rs unit tests**

Any test that did `let result = add_import(...)?;` and asserted on `result` (a String) → `let (result, evidence) = add_import(...)?;` and add `use crate::edit_transaction::ApplyStatus; assert_eq!(evidence.status, ApplyStatus::Applied);` to at least one happy-path test.

- [ ] **Step 4: Build + test engine**

```bash
cargo build -p codelens-engine
cargo test -p codelens-engine
```

Expected: 0 errors, all engine tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/auto_import.rs
git commit -m "$(cat <<'EOF'
feat(engine): migrate add_import onto full-write substrate

add_import now returns (String, ApplyEvidence) and delegates the final
fs::write to apply_full_write_with_evidence, gaining the same TOCTOU /
hash / rollback guarantees as writer.rs primitives. unit tests
destructured for the new tuple shape.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: MCP tool handlers — envelope merge for 9 handlers

**Files:**

- Modify: `crates/codelens-mcp/src/tools/mutation.rs`

**Goal:** Each of the 9 tool handlers unpacks `(content, evidence)` (or `(content, count, evidence)` / just `evidence`) and merges 6 evidence keys into the existing Phase 0 envelope via the `merge_raw_fs_envelope` helper.

The 9 handlers: `create_text_file_tool`, `delete_lines_tool`, `insert_at_line_tool`, `replace_lines_tool`, `replace_content_tool`, `replace_symbol_body_tool`, `insert_before_symbol_tool`, `insert_after_symbol_tool`, `add_import_tool`.

- [ ] **Step 1: Add evidence-merge helper at the top of mutation.rs**

In `crates/codelens-mcp/src/tools/mutation.rs`, add immediately below the existing `merge_raw_fs_envelope` function:

```rust
/// Merge 6 evidence keys into a tool response object: file_hashes_before,
/// file_hashes_after, apply_status, rollback_report, modified_files, edit_count.
/// Mirrors the G4 safe_delete_apply pattern.
fn merge_apply_evidence(
    mut value: Value,
    evidence: &codelens_engine::edit_transaction::ApplyEvidence,
) -> Value {
    if let Some(target) = value.as_object_mut() {
        target.insert(
            "file_hashes_before".to_owned(),
            serde_json::to_value(&evidence.file_hashes_before).unwrap_or(Value::Null),
        );
        target.insert(
            "file_hashes_after".to_owned(),
            serde_json::to_value(&evidence.file_hashes_after).unwrap_or(Value::Null),
        );
        target.insert(
            "apply_status".to_owned(),
            serde_json::to_value(&evidence.status).unwrap_or(Value::Null),
        );
        target.insert(
            "rollback_report".to_owned(),
            serde_json::to_value(&evidence.rollback_report).unwrap_or(Value::Null),
        );
        target.insert(
            "modified_files".to_owned(),
            json!(evidence.modified_files),
        );
        target.insert("edit_count".to_owned(), json!(evidence.edit_count));
    }
    value
}
```

Also update the import block at top of `mutation.rs`:

```rust
use codelens_engine::edit_transaction::{ApplyError, ApplyEvidence};
use codelens_engine::{
    add_import, analyze_missing_imports, create_text_file, delete_lines, insert_after_symbol,
    insert_at_line, insert_before_symbol, rename, replace_content, replace_lines,
    replace_symbol_body,
};
```

(Adjust to whatever the current `use` list is — only add the `edit_transaction::{ApplyError, ApplyEvidence}` line if it is not already present.)

- [ ] **Step 2: Update `create_text_file_tool`**

Replace the function body with:

```rust
pub fn create_text_file_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let content = required_string(arguments, "content")?;
    let overwrite = arguments
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let evidence = create_text_file(&state.project(), relative_path, content, overwrite)?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "created": relative_path }), "create_text_file"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::Filesystem, 0.7)))
}
```

- [ ] **Step 3: Update `delete_lines_tool`**

```rust
pub fn delete_lines_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("start_line".into()))?
        as usize;
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("end_line".into()))? as usize;
    let (content, evidence) =
        delete_lines(&state.project(), relative_path, start_line, end_line)?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "content": content }), "delete_lines"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::Filesystem, 0.7)))
}
```

- [ ] **Step 4: Update `insert_at_line_tool`**

```rust
pub fn insert_at_line_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let line = arguments
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("line".into()))? as usize;
    let content = required_string(arguments, "content")?;
    let (modified, evidence) = insert_at_line(&state.project(), relative_path, line, content)?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "content": modified }), "insert_at_line"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::Filesystem, 0.7)))
}
```

- [ ] **Step 5: Update `replace_lines_tool`**

```rust
pub fn replace_lines_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("start_line".into()))?
        as usize;
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("end_line".into()))? as usize;
    let new_content = required_string(arguments, "new_content")?;
    let (content, evidence) = replace_lines(
        &state.project(),
        relative_path,
        start_line,
        end_line,
        new_content,
    )?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "content": content }), "replace_lines"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::Filesystem, 0.7)))
}
```

- [ ] **Step 6: Update `replace_content_tool`**

```rust
pub fn replace_content_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let old_text = required_string(arguments, "old_text")?;
    let new_text = required_string(arguments, "new_text")?;
    let regex_mode = arguments
        .get("regex_mode")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let (content, count, evidence) = replace_content(
        &state.project(),
        relative_path,
        old_text,
        new_text,
        regex_mode,
    )?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(
            json!({ "content": content, "replacements": count }),
            "replace_content",
        ),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::Filesystem, 0.7)))
}
```

- [ ] **Step 7: Update `replace_symbol_body_tool`**

```rust
pub fn replace_symbol_body_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let new_body = required_string(arguments, "new_body")?;
    let (content, evidence) = replace_symbol_body(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        new_body,
    )?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "content": content }), "replace_symbol_body"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::TreeSitter, 0.95)))
}
```

- [ ] **Step 8: Update `insert_before_symbol_tool`**

```rust
pub fn insert_before_symbol_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let content = required_string(arguments, "content")?;
    let (modified, evidence) = insert_before_symbol(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        content,
    )?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "content": modified }), "insert_before_symbol"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::TreeSitter, 0.95)))
}
```

- [ ] **Step 9: Update `insert_after_symbol_tool`**

```rust
pub fn insert_after_symbol_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let content = required_string(arguments, "content")?;
    let (modified, evidence) = insert_after_symbol(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        content,
    )?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(json!({ "content": modified }), "insert_after_symbol"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::TreeSitter, 0.95)))
}
```

- [ ] **Step 10: Update `add_import_tool`**

```rust
pub fn add_import_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let import_statement = required_string(arguments, "import_statement")?;
    let (content, evidence) = add_import(&state.project(), file_path, import_statement)?;
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(
            json!({"success": true, "file_path": file_path, "content_length": content.len()}),
            "add_import",
        ),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::Filesystem, 0.7)))
}
```

(`insert_content_tool` and `replace_content_unified` remain dispatchers — no change.)

- [ ] **Step 11: Build mcp**

```bash
cargo build -p codelens-mcp
cargo build -p codelens-mcp --features http
```

Expected: 0 errors. Any caller of `create_text_file` / `delete_lines` etc. inside mcp test code will now fail to compile until destructured. Fix those locally to match the new tuple shape (most are likely in `crates/codelens-mcp/src/integration_tests/`).

- [ ] **Step 12: Run mcp test suite**

```bash
cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
```

Expected: All existing tests pass. Do not yet add new mutation_evidence tests — that is Task 9.

- [ ] **Step 13: Commit**

```bash
git add crates/codelens-mcp/src/tools/mutation.rs
git commit -m "$(cat <<'EOF'
feat(mcp): merge ApplyEvidence into 9 mutation tool responses

each tool handler unpacks (content, evidence) (or just evidence for
create_text_file) and merges 6 evidence keys into the existing Phase 0
envelope: file_hashes_before, file_hashes_after, apply_status,
rollback_report, modified_files, edit_count. mirrors the G4
safe_delete_apply contract.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: MCP integration tests (M1-M5)

**Files:**

- Create: `crates/codelens-mcp/src/integration_tests/mutation_evidence.rs`
- Modify: `crates/codelens-mcp/src/integration_tests/mod.rs`

**Goal:** Add 5 integration tests covering happy-path evidence (3 representative primitives), TOCTOU (E2 → Validation Err), and rollback (E4 → Ok + apply_status="rolled_back").

- [ ] **Step 1: Inspect existing integration_tests structure for test pattern**

```bash
rg -n 'parse_tool_response|fn handle_tool_call|crate::integration_tests' crates/codelens-mcp/src/integration_tests/mod.rs | head -20
```

Note the pattern used by existing tests (e.g., `crates/codelens-mcp/src/integration_tests/semantic_refactor.rs`) for: how a test calls a tool handler, how response JSON is asserted, how `state` / `project` is constructed.

- [ ] **Step 2: Create new file `crates/codelens-mcp/src/integration_tests/mutation_evidence.rs`**

Use the same scaffolding pattern as a sibling integration test. Below is a representative skeleton — adjust the helper function names (`parse_tool_response`, project setup) to match what other integration_tests files use:

```rust
use crate::tools::mutation::{
    add_import_tool, create_text_file_tool, replace_lines_tool,
};
use serde_json::json;

mod helpers {
    use crate::AppState;
    use std::path::PathBuf;

    pub fn make_state_with_file(name: &str, content: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-mut-evidence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join(name);
        std::fs::write(&file_path, content).unwrap();
        // Use whatever AppState constructor the rest of integration_tests uses.
        // (e.g., AppState::for_project(&dir) — if this signature differs,
        // mirror existing semantic_refactor.rs setup verbatim.)
        let state = AppState::for_project(&dir).expect("AppState");
        (state, file_path)
    }
}

#[test]
fn replace_lines_tool_response_includes_evidence() {
    let (state, _path) = helpers::make_state_with_file("doc.txt", "a\nb\nc\n");
    let result = replace_lines_tool(
        &state,
        &json!({
            "relative_path": "doc.txt",
            "start_line": 2,
            "end_line": 3,
            "new_content": "REPLACED\n"
        }),
    )
    .expect("replace_lines_tool ok");
    let (response, _meta) = result;
    let obj = response.as_object().expect("response is object");
    assert_eq!(obj["apply_status"].as_str(), Some("applied"));
    assert_eq!(obj["modified_files"].as_u64(), Some(1));
    assert_eq!(obj["edit_count"].as_u64(), Some(1));
    assert!(
        obj.contains_key("file_hashes_before") && obj.contains_key("file_hashes_after"),
        "expected file_hashes_before/after keys"
    );
    assert!(obj["rollback_report"].as_array().unwrap().is_empty());
    assert_eq!(
        obj["edit_authority"]["kind"].as_str(),
        Some("raw_fs"),
        "Phase 0 envelope still present"
    );
}

#[test]
fn create_text_file_tool_response_includes_evidence() {
    let (state, _) = helpers::make_state_with_file("seed.txt", "seed\n");
    let result = create_text_file_tool(
        &state,
        &json!({
            "relative_path": "fresh.txt",
            "content": "hello\n",
            "overwrite": false
        }),
    )
    .expect("create_text_file_tool ok");
    let (response, _meta) = result;
    let obj = response.as_object().expect("response is object");
    assert_eq!(obj["apply_status"].as_str(), Some("applied"));
    assert_eq!(obj["modified_files"].as_u64(), Some(1));
    assert_eq!(obj["edit_count"].as_u64(), Some(1));
    let after = obj["file_hashes_after"]
        .as_object()
        .expect("after is object");
    assert!(after.contains_key("fresh.txt"), "after has fresh.txt entry");
    let before = obj["file_hashes_before"]
        .as_object()
        .expect("before is object");
    assert!(
        !before.contains_key("fresh.txt"),
        "create against new path: no before entry"
    );
}

#[test]
fn add_import_tool_response_includes_evidence() {
    let (state, _path) =
        helpers::make_state_with_file("module.py", "def existing():\n    pass\n");
    let result = add_import_tool(
        &state,
        &json!({
            "file_path": "module.py",
            "import_statement": "import os"
        }),
    )
    .expect("add_import_tool ok");
    let (response, _meta) = result;
    let obj = response.as_object().expect("response is object");
    assert_eq!(obj["apply_status"].as_str(), Some("applied"));
    assert_eq!(obj["modified_files"].as_u64(), Some(1));
    assert!(obj.contains_key("file_hashes_before"));
    assert!(obj.contains_key("file_hashes_after"));
}

#[test]
fn replace_lines_tool_e2_toctou_returns_validation_err() {
    use codelens_engine::edit_transaction::FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY;
    let (state, path) = helpers::make_state_with_file("drift.txt", "before\n");
    FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY.with(|cell| {
        let path_clone = path.clone();
        let hook: Box<dyn FnOnce(&std::path::Path)> =
            Box::new(move |_p: &std::path::Path| {
                std::fs::write(&path_clone, "TAMPERED\n").unwrap();
            });
        *cell.borrow_mut() = Some(hook);
    });
    let result = replace_lines_tool(
        &state,
        &json!({
            "relative_path": "drift.txt",
            "start_line": 1,
            "end_line": 2,
            "new_content": "after\n"
        }),
    );
    assert!(result.is_err(), "TOCTOU drift must surface as Err");
    // Disk reflects external mutation, not the intended edit.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "TAMPERED\n");
}

#[cfg(unix)]
#[test]
fn replace_lines_tool_e4_rollback_response_shape() {
    use std::os::unix::fs::PermissionsExt;
    let (state, path) = helpers::make_state_with_file("ro.txt", "original\n");
    let parent = path.parent().unwrap().to_path_buf();
    let mut parent_perms = std::fs::metadata(&parent).unwrap().permissions();
    parent_perms.set_mode(0o555);
    std::fs::set_permissions(&parent, parent_perms).unwrap();

    let result = replace_lines_tool(
        &state,
        &json!({
            "relative_path": "ro.txt",
            "start_line": 1,
            "end_line": 2,
            "new_content": "new\n"
        }),
    );

    // Restore perms before assertions for tempdir cleanup.
    let mut restore = std::fs::metadata(&parent).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(&parent, restore).unwrap();

    // Hybrid contract: ApplyFailed surfaces as Ok response with apply_status="rolled_back".
    let (response, _meta) = result.expect("Hybrid: rollback should be Ok");
    let obj = response.as_object().expect("response is object");
    assert_eq!(obj["apply_status"].as_str(), Some("rolled_back"));
    assert!(
        obj.contains_key("error_message"),
        "rollback response must include error_message"
    );
    let report = obj["rollback_report"].as_array().expect("array");
    assert_eq!(report.len(), 1);
    assert_eq!(report[0]["file_path"].as_str(), Some("ro.txt"));
    assert_eq!(report[0]["restored"].as_bool(), Some(true));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original\n");
}
```

**Note**: M5 (`replace_lines_tool_e4_rollback_response_shape`) relies on the MCP handler's `Err` arm in `replace_lines_tool` translating `ApplyError::ApplyFailed` into an `Ok` response with `apply_status="rolled_back"` and `error_message`. The current handler returns `?` which propagates as `Err`. **Before this test will pass, Task 9 Step 4 must update the handler `Err` translation.**

- [ ] **Step 3: Register the new module in `crates/codelens-mcp/src/integration_tests/mod.rs`**

Append at the end of the existing `pub mod ...;` list:

```rust
pub mod mutation_evidence;
```

- [ ] **Step 4: Update `replace_lines_tool` (and the other 8 tool handlers) to translate `ApplyError::ApplyFailed` into Hybrid Ok response**

The Hybrid contract requires that the handler's `?` propagation be replaced with explicit `match` for any function returning `Result<..., ApplyError>` or anything that could be `ApplyError::ApplyFailed`. However, the engine functions return `anyhow::Result<...>` (not `ApplyError`) because the writer.rs functions wrap with `.map_err(|e| anyhow::Error::msg(e.to_string()))`. This makes Hybrid translation lossy at the engine boundary.

**Decision**: keep engine function signatures returning `anyhow::Result<(String, ApplyEvidence)>` for ergonomic ergonomic compatibility with existing `?` flows. To enable Hybrid response semantics in mcp, change the engine wrapper inside writer.rs (Task 6) and auto_import.rs (Task 7) to:

```rust
let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
    Ok(ev) => ev,
    Err(crate::edit_transaction::ApplyError::ApplyFailed { source, evidence }) => {
        // Hybrid: surface via Ok with the evidence carrying status=RolledBack
        // so the mcp handler can detect via evidence.status and add error_message.
        return Ok((result, evidence));
        // Note: the caller content (`result`) is the *intended* content; on disk
        // the file has been restored to pre-apply state.
    }
    Err(other) => return Err(anyhow::Error::msg(other.to_string())),
};
Ok((result, evidence))
```

**Plan adjustment**: revise Task 6 Step 2 and Task 7 Step 2 to use this `match` pattern instead of `.map_err(|e| anyhow::Error::msg(...))?`. Then in mcp tool handlers, after unpacking `(content, evidence)`, check `if matches!(evidence.status, ApplyStatus::RolledBack)` and add `error_message` field by stashing `source` somewhere — but `source` is consumed by the engine wrapper.

**Cleaner alternative**: change writer.rs functions to return `Result<(String, ApplyEvidence), ApplyError>` directly (no `anyhow` wrapping), and let mcp do the Hybrid translation. This is more invasive but correct. Update Task 6 Step 2 plan accordingly:

In writer.rs, each function:

- Pre-substrate errors (line out of range, regex invalid, UTF-8 invalid, file already exists) → `bail!` produces `anyhow::Error` (cannot be in the new `ApplyError` type since they don't fit semantically).

The cleanest way: keep writer.rs functions returning `anyhow::Result<(String, ApplyEvidence)>` for E6/E7 simplicity, but expose an additional `RolledBack` signal _through the evidence_ (which already carries `status`). MCP handler checks `evidence.status` and surfaces Ok+error_message when `RolledBack`. The actual `source` error is stored in the evidence by the engine wrapper as a synthesized field — or surfaced via `rollback_report[].reason` which already carries the underlying io::Error string.

**Final decision** (locked in for Task 6/7 actual implementation):

In writer.rs (and auto_import), wrap substrate call as:

```rust
let evidence = match apply_full_write_with_evidence(project, relative_path, &result) {
    Ok(ev) => ev,
    Err(crate::edit_transaction::ApplyError::ApplyFailed { source: _, evidence }) => {
        // status=RolledBack signals fail-closed; mcp will surface as Ok+error_message.
        // The underlying io error is preserved in rollback_report[].reason.
        evidence
    }
    Err(other) => return Err(anyhow::Error::msg(other.to_string())),
};
Ok((result, evidence))
```

In mcp tool handler (e.g., `replace_lines_tool`), after `let (content, evidence) = replace_lines(...)?;`, check status:

```rust
let mut response_obj = json!({ "content": content });
if matches!(evidence.status, codelens_engine::edit_transaction::ApplyStatus::RolledBack) {
    if let Some(obj) = response_obj.as_object_mut() {
        // Synthesize error_message from rollback_report
        let msg = evidence
            .rollback_report
            .iter()
            .filter_map(|e| e.reason.as_ref())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        obj.insert(
            "error_message".to_owned(),
            json!(format!(
                "apply failed: {}",
                if msg.is_empty() {
                    "unknown io error".to_owned()
                } else {
                    msg
                }
            )),
        );
    }
}
let response = merge_apply_evidence(
    merge_raw_fs_envelope(response_obj, "replace_lines"),
    &evidence,
);
```

**Action — apply this refinement to Task 6 Step 2, Task 7 Step 2, and Task 8 Steps 2-10 before running the actual code**. The handlers in Task 8 Steps 2-10 should use this `if matches!(evidence.status, ApplyStatus::RolledBack)` enrichment for _every_ tool to honour the Hybrid contract uniformly.

- [ ] **Step 5: Build + run new tests**

```bash
cargo build -p codelens-mcp
cargo test -p codelens-mcp mutation_evidence
cargo test -p codelens-mcp --features http mutation_evidence
```

Expected: M1, M2, M3, M5 PASS. M4 (TOCTOU): the integration test depends on `FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY` being `pub(crate)` accessible from mcp. If it is `#[cfg(test)]`-only inside engine, mcp cannot access it. In that case, simplify M4 to use a different mechanism (e.g., shorten the file before substrate's verify by spawning a thread, or skip M4 and rely on engine-level T3 only). Mark this as a **planned deviation point** — implement M1/M2/M3/M5 first, then decide on M4 based on what is actually accessible.

If `FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY` is `#[cfg(test)]`-only inside engine, expose a `#[cfg(any(test, feature = "test-only-substrate-hooks"))]` gate or similar mechanism so mcp tests can access it. Or write M4 as an engine-level test in `edit_transaction.rs::tests` instead of in mcp integration tests, and downgrade M4 in the AC to engine-only coverage (which is already T3).

**Pragmatic choice**: skip M4 from mcp layer if substrate hook visibility is awkward. T3 in engine already covers TOCTOU. Update spec AC-5 mapping: replace M4 with "T3 engine-level coverage suffices for E2 verification in this PR; mcp layer M4 deferred to G7c if cross-layer test infrastructure improves". Document this in the final commit message.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/integration_tests/mutation_evidence.rs \
        crates/codelens-mcp/src/integration_tests/mod.rs
git commit -m "$(cat <<'EOF'
test(mcp): integration tests for mutation evidence + rollback hybrid contract

M1 replace_lines / M2 create_text_file / M3 add_import: each tool
response contains 6 evidence keys with correct values for the happy
path. M5 (#[cfg(unix)]) verifies the Hybrid contract: chmod-driven
apply failure surfaces as Ok response with apply_status="rolled_back",
error_message synthesized from rollback_report[].reason, restore
verified by post-rollback disk read.

M4 (mcp-layer TOCTOU) deferred — engine T3 covers E2 / TOCTOU at
substrate level; mcp surface coverage left to a future PR if the
cfg(test) hook visibility can be made cross-crate without overexposing
internals.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Surface manifest regression + final verification

**Files:**

- No code changes (verification only)

**Goal:** Confirm Phase 0 G6 contracts still pass with no diff in mutation primitive operation matrix output, and run the full test gate matrix.

- [ ] **Step 1: Run surface manifest contract checks**

```bash
cargo run -p codelens-mcp --quiet -- --print-operation-matrix > /tmp/g7-matrix.json
python3 scripts/surface-manifest.py --check-operation-matrix /tmp/g7-matrix.json
```

Expected: no errors, no failures.

- [ ] **Step 2: Run contract fixture tests**

```bash
python3 scripts/test/test-surface-manifest-contracts.py
```

Expected: all 5 fixture tests PASS.

- [ ] **Step 3: Compare matrix output vs main**

```bash
git stash push -u -m "g7-matrix-stash"
git checkout main -- crates/codelens-mcp 2>/dev/null || true
cargo run -p codelens-mcp --quiet -- --print-operation-matrix > /tmp/main-matrix.json 2>/dev/null || \
  echo '{"operations":[]}' > /tmp/main-matrix.json
git checkout HEAD -- crates/codelens-mcp 2>/dev/null || git stash pop
diff /tmp/main-matrix.json /tmp/g7-matrix.json || true
```

Expected: zero diff in `operations[].support` / `authority` / `can_apply` / `verified` fields for the 9 mutation primitives. Any field changes would mean G7 broke Phase 0 envelope contract — investigate and fix.

If the diff command above is awkward in the environment, simply confirm the operation matrix has identical authority/can_apply/verified rows for the 9 mutation primitives manually:

```bash
cargo run -p codelens-mcp --quiet -- --print-operation-matrix | python3 -m json.tool | \
  rg -A 5 'create_text_file|delete_lines|insert_at_line|replace_lines|replace_content|replace_symbol_body|insert_before_symbol|insert_after_symbol|add_import' | head -100
```

- [ ] **Step 4: Run full test gate**

```bash
cargo test -p codelens-engine
cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
cargo test -p codelens-mcp --no-default-features
cargo clippy -- -W clippy::all 2>&1 | tee /tmp/g7-clippy.log
```

Expected:

- engine: G4 baseline + 6 new substrate tests + 1 new caller-site test all PASS
- mcp default: prior baseline (~448) + 4-5 new mutation_evidence tests all PASS
- mcp http: prior baseline (~527) + same new tests all PASS
- mcp no-default-features: clean compile, prior baseline tests PASS
- clippy: 0 NEW warnings (existing warnings stay; tally baseline before this PR if needed)

If clippy reports new warnings introduced by this PR, fix them inline with `#[allow(...)]` only if the lint is genuinely a false positive; otherwise refactor to satisfy the lint.

- [ ] **Step 5: Verify AC-3 grep**

```bash
rg 'fs::write\b' crates/codelens-engine/src/file_ops/writer.rs crates/codelens-engine/src/auto_import.rs
```

Expected: 0 lines (production code only — test scaffolding within those files is fine; if any production-path `fs::write` remain, that is an AC-3 failure).

- [ ] **Step 6: Verify AC-8 (no out-of-scope changes)**

```bash
git diff --name-only main..HEAD
```

Expected file list (approximate):

- `crates/codelens-engine/src/edit_transaction.rs` (G7 substrate function + tests)
- `crates/codelens-engine/src/file_ops/writer.rs` (8 functions migrated)
- `crates/codelens-engine/src/auto_import.rs` (1 function migrated)
- `crates/codelens-mcp/src/tools/mutation.rs` (9 handlers updated)
- `crates/codelens-mcp/src/integration_tests/mutation_evidence.rs` (NEW)
- `crates/codelens-mcp/src/integration_tests/mod.rs` (1-line module registration)
- `docs/superpowers/specs/2026-04-25-codelens-phase1-g7-fullfile-substrate-design.md` (already committed)
- `docs/superpowers/plans/2026-04-25-codelens-phase1-g7-fullfile-substrate.md` (already committed)

`crates/codelens-engine/src/move_symbol.rs` MUST NOT appear. The `WorkspaceEditTransaction` struct and its existing `apply_with_evidence` method MUST be unchanged in `edit_transaction.rs` (only additions: free function + tests).

Verify with:

```bash
git diff main..HEAD -- crates/codelens-engine/src/edit_transaction.rs | rg '^[-+]' | rg -v '^[-+]{3}' | head -50
```

The diff should show only additions of `apply_full_write_with_evidence`, the `FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY` thread_local (cfg test), and the 6 new tests — no removals or modifications to G4 types and methods.

- [ ] **Step 7: Final commit (clippy fix if needed) and PR push prep**

If Step 4 surfaced any clippy warnings that needed fixing, commit them:

```bash
git add -p  # review the clippy fixes
git commit -m "chore: suppress clippy warning introduced by G7 substrate"
```

Then verify branch state is ready to push:

```bash
git status
git log --oneline main..HEAD
```

Expected: clean working tree, ~10-11 commits on the branch (spec + plan + 6 substrate test commits + writer migration + auto_import migration + mcp handler merge + integration tests + final clippy fix if any).

- [ ] **Step 8: Acceptance criteria self-checklist**

Go through the spec §6 AC-1 through AC-9 and confirm each:

| AC   | Confirmation                                                                                                                                 |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| AC-1 | `rg 'pub fn apply_full_write_with_evidence' crates/codelens-engine/src/edit_transaction.rs` returns 1 line with the expected signature       |
| AC-2 | `cargo test -p codelens-engine apply_full_write 2>&1 \| rg 'test result'` shows 6 passed (T1-T6)                                             |
| AC-3 | `rg 'fs::write\b' crates/codelens-engine/src/file_ops/writer.rs crates/codelens-engine/src/auto_import.rs` returns 0 production-code matches |
| AC-4 | M1+M2+M3 PASS (Step 4 above)                                                                                                                 |
| AC-5 | T3 engine PASS (substrate-level TOCTOU coverage; M4 deferred per Task 9 Step 6 commit message)                                               |
| AC-6 | M5 PASS on unix (Step 4 above)                                                                                                               |
| AC-7 | Surface manifest contracts PASS (Steps 1-3 above) with zero diff in 9 mutation primitive matrix rows                                         |
| AC-8 | Step 6 confirms no out-of-scope changes                                                                                                      |
| AC-9 | Step 4 full test gate green; clippy 0 NEW warnings                                                                                           |

If any AC fails, return to the relevant task and remediate before declaring G7 complete.

- [ ] **Step 9: Document in feature_list.md**

Update `.claude/feature_list.md` to mark the G7 PR scope ✅ COMPLETED with a final-status block similar to G4's. (This is bookkeeping for compaction recovery; not a production code change.)

- [ ] **Step 10: Push branch + open PR**

(Stacked on `feat/phase1-g4-workspace-edit-transaction` until PR #83 merges; once #83 merges, rebase G7 onto `main`.)

```bash
git push -u origin feat/phase1-g7-fullfile-substrate
gh pr create \
  --base feat/phase1-g4-workspace-edit-transaction \
  --head feat/phase1-g7-fullfile-substrate \
  --title "feat(engine+mcp): Phase 1 G7 — single-file mutation substrate migration" \
  --body "$(cat <<'EOF'
## Summary

Migrates 9 single-file mutation primitives onto the G4 substrate so that
the Phase 0 envelope on `mutation.rs` is *honest* — agents now receive
hash-based evidence, TOCTOU verification, and rollback reports for every
raw_fs primitive call.

- New free function `apply_full_write_with_evidence` reuses G4
  `ApplyEvidence` / `ApplyStatus` / `RollbackEntry` / `FileHash` /
  `ApplyError` types; no new types added.
- 9 primitives (writer.rs 8 + auto_import 1) expose `ApplyEvidence` in
  return type; final `fs::write` line replaced with substrate call.
- 9 MCP tool handlers merge 6 evidence keys into Phase 0 envelope:
  `file_hashes_before` / `file_hashes_after` / `apply_status` /
  `rollback_report` / `modified_files` / `edit_count`.
- Hybrid failure policy: ApplyFailed surfaces as Ok response with
  `apply_status="rolled_back"` + `error_message` synthesised from
  `rollback_report[].reason`. PreReadFailed / PreApplyHashMismatch
  remain `Err`.
- 6 substrate tests (T1-T6) + 1 caller-site evidence test + 4 mcp
  integration tests (M1/M2/M3/M5; M4 deferred — covered by engine T3).
- Phase 0 G6 surface manifest contracts unchanged (0 diff in 9
  mutation primitive matrix rows).
- `move_symbol.rs` (2-file atomic) deferred to G7b separate PR.

## Test plan

- [x] `cargo test -p codelens-engine` (G4 baseline + 6 new + 1 caller test)
- [x] `cargo test -p codelens-mcp` (default features, includes M1/M2/M3/M5)
- [x] `cargo test -p codelens-mcp --features http`
- [x] `cargo test -p codelens-mcp --no-default-features`
- [x] `cargo clippy -- -W clippy::all` (0 NEW warnings)
- [x] `python3 scripts/surface-manifest.py --check-operation-matrix`
- [x] `python3 scripts/test/test-surface-manifest-contracts.py`
- [x] AC-1 through AC-9 self-checklist (see plan §10 Step 8)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-Review

**Spec coverage check (§1-§7 of spec):**

| Spec section                                                | Plan coverage                                                                                        |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| §1 Scope (9 primitives, in/out)                             | Tasks 6+7 cover all 9; Task 10 Step 6 verifies AC-8 no out-of-scope                                  |
| §2.1 substrate extension (`apply_full_write_with_evidence`) | Task 1                                                                                               |
| §2.2 9 primitive signatures                                 | Tasks 6 (writer.rs 8) + 7 (auto_import 1)                                                            |
| §2.3 MCP handler updates                                    | Task 8 (9 handlers, including Hybrid status check refinement in Task 9 Step 4)                       |
| §3.1-3.4 Data flow                                          | Task 1 implements the 4-phase sequence; Task 9 demonstrates flow with M1/M5                          |
| §4 7-row error matrix                                       | E1/E2/E3 in Tasks 1-2; E4/E5 in Tasks 4 + Task 9; E6/E7 unchanged from existing primitive validation |
| §5.1 6 substrate tests                                      | Tasks 1-5 (T1-T6)                                                                                    |
| §5.2 primitive test updates + 1 caller test                 | Task 6 (Step 3 + Step 4)                                                                             |
| §5.3 5 MCP integration tests                                | Task 9 (M1/M2/M3/M5; M4 deferred per Task 9 Step 6)                                                  |
| §5.4 surface manifest regression                            | Task 10 Steps 1-3                                                                                    |
| §6 AC-1 through AC-9                                        | Task 10 Step 8 self-checklist                                                                        |
| §7 Next steps                                               | Task 10 Step 10 push + PR                                                                            |

**Placeholder scan:** No "TBD" / "TODO" / "implement later" / "similar to" patterns. All code blocks are complete and runnable.

**Type consistency:** `ApplyEvidence` / `ApplyStatus::Applied/RolledBack/NoOp` / `RollbackEntry` / `FileHash{sha256, bytes}` / `ApplyError::{ResourceOpsUnsupported, PreReadFailed, PreApplyHashMismatch, ApplyFailed}` — all references match the existing G4 types verbatim. Function signatures consistent across Tasks 1, 6, 7, 8: `apply_full_write_with_evidence(project, relative_path, new_content) -> Result<ApplyEvidence, ApplyError>`.

**Known plan adjustment** (called out in Task 9 Step 4): The Hybrid contract requires the engine wrappers in writer.rs/auto_import.rs to surface `ApplyError::ApplyFailed` as `Ok((content, evidence))` so mcp can detect via `evidence.status == RolledBack`. This adjustment must be applied retroactively to Task 6 Step 2 and Task 7 Step 2 code blocks during execution. The plan documents this explicitly so the implementer makes the adjustment when reaching Task 9.

**M4 deferral**: Task 9 Step 6 commit message documents that M4 (mcp-layer TOCTOU) is deferred because the substrate `cfg(test)` hook visibility may be awkward across crate boundaries. Engine T3 covers TOCTOU at substrate level. Spec AC-5 maps to "T3 engine PASS" rather than M4.
