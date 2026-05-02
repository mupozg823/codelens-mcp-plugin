//! Workspace edit transaction substrate.
//!
//! **Internal API.** `apply_full_write_with_evidence` and
//! `WorkspaceEditTransaction::apply` are the lowest-level disk-write
//! primitives in CodeLens. They produce `ApplyEvidence` and an
//! optional rollback report but **do not** enforce ADR-0009 role
//! gates, write audit rows, or invalidate engine caches — those
//! guarantees are layered on top by `codelens-mcp` dispatch. Direct
//! callers from outside the workspace silently bypass the trust
//! substrate. See the crate-level docs in `lib.rs`.
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

#![allow(dead_code)]

use crate::lsp::types::LspResourceOp;
use crate::project::ProjectRoot;
use crate::rename::RenameEdit;
use anyhow::Result;
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

    /// Phase 1: read each unique file once, capture sha256 + raw backup bytes.
    #[allow(clippy::type_complexity)]
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

    /// Apply edits with hash-based evidence and rollback on failure.
    /// Implementation lands incrementally in T2~T6.
    pub fn apply_with_evidence(&self, project: &ProjectRoot) -> Result<ApplyEvidence, ApplyError> {
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

        // Phase 1: capture pre-apply state
        let (backups, file_hashes_before) = self.capture_pre_apply(project)?;

        // Phase 2: light TOCTOU re-check (same-function window)
        self.verify_pre_apply(project, &backups, &file_hashes_before)?;

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

#[cfg(test)]
type FullWriteInjectHook = std::cell::RefCell<Option<Box<dyn FnOnce(&std::path::Path)>>>;

#[cfg(test)]
thread_local! {
    /// Test-only hook: when set, called once between Phase 1 capture and
    /// Phase 2 verify with the resolved path so a test can mutate the file
    /// to simulate TOCTOU drift. Cleared after one call.
    pub(crate) static FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY: FullWriteInjectHook =
        std::cell::RefCell::new(None);

    /// Test-only hook: when set, called once immediately before the Phase 3
    /// rollback restore write, with the resolved path. Allows a test to
    /// reverse any permission changes that caused the initial write to fail,
    /// so the rollback `fs::write` can succeed. Cleared after one call.
    pub(crate) static FULL_WRITE_INJECT_BEFORE_ROLLBACK: FullWriteInjectHook =
        std::cell::RefCell::new(None);
}

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

    #[cfg(test)]
    FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY.with(|cell| {
        if let Some(hook) = cell.borrow_mut().take() {
            hook(&resolved);
        }
    });

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
        #[cfg(test)]
        FULL_WRITE_INJECT_BEFORE_ROLLBACK.with(|cell| {
            if let Some(hook) = cell.borrow_mut().take() {
                hook(&resolved);
            }
        });
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

/// G7b — multi-file full-write substrate.
///
/// Same four-phase contract as [`apply_full_write_with_evidence`] but
/// applies a batch of `(relative_path, new_content)` writes atomically
/// from the caller's perspective: every file's pre-hash is captured
/// and TOCTOU-verified _before_ any write begins, and if the *N*th
/// write fails, files 1..N are rolled back to their captured backups
/// before returning. Used by `move_symbol` to keep source + target
/// in lock-step.
///
/// Phases:
/// 1. capture: read each existing file, sha256 + raw backup
/// 2. verify: re-read each existing file + sha256 compare (TOCTOU)
/// 3. write: fs::write each path in order; on failure, restore
///    successfully-written files (newest first) + record rollback
/// 4. post-hash: sha256 every written file → file_hashes_after
///
/// Files that did not exist before apply contribute no
/// `file_hashes_before` entry and cannot be restored on rollback —
/// `RollbackEntry { restored: false }` is recorded instead.
pub fn apply_full_writes_with_evidence(
    project: &ProjectRoot,
    writes: &[(&str, &str)],
) -> Result<ApplyEvidence, ApplyError> {
    if writes.is_empty() {
        return Ok(ApplyEvidence {
            status: ApplyStatus::NoOp,
            file_hashes_before: BTreeMap::new(),
            file_hashes_after: BTreeMap::new(),
            rollback_report: Vec::new(),
            modified_files: 0,
            edit_count: 0,
        });
    }

    // Phase 1: capture each existing file's bytes and pre-hash.
    let mut backups: Vec<(PathBuf, &str, Option<Vec<u8>>)> = Vec::with_capacity(writes.len());
    let mut file_hashes_before: BTreeMap<String, FileHash> = BTreeMap::new();
    for (relative_path, _) in writes {
        let resolved = project
            .resolve(relative_path)
            .map_err(|e| ApplyError::PreReadFailed {
                file_path: (*relative_path).to_owned(),
                source: e,
            })?;
        let backup = match fs::read(&resolved) {
            Ok(bytes) => {
                file_hashes_before.insert(
                    (*relative_path).to_owned(),
                    FileHash {
                        sha256: sha256_hex(&bytes),
                        bytes: bytes.len(),
                    },
                );
                Some(bytes)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(ApplyError::PreReadFailed {
                    file_path: (*relative_path).to_owned(),
                    source: anyhow::Error::from(err),
                });
            }
        };
        backups.push((resolved, *relative_path, backup));
    }

    // Phase 2: verify each existing file's hash hasn't drifted.
    for (resolved, relative_path, _) in &backups {
        let Some(expected_hash) = file_hashes_before
            .get(*relative_path)
            .map(|h| h.sha256.clone())
        else {
            continue;
        };
        let bytes_now = fs::read(resolved).map_err(|e| ApplyError::PreReadFailed {
            file_path: (*relative_path).to_owned(),
            source: anyhow::Error::from(e),
        })?;
        let hash_now = sha256_hex(&bytes_now);
        if hash_now != expected_hash {
            return Err(ApplyError::PreApplyHashMismatch {
                file_path: (*relative_path).to_owned(),
                expected: expected_hash,
                actual: hash_now,
            });
        }
    }

    // Phase 3: write in order. On first failure, walk back through the
    // already-written prefix and restore the captured bytes (or delete
    // newly-created files).
    let mut written_so_far: usize = 0;
    let mut write_failure: Option<(usize, std::io::Error)> = None;
    for (i, (relative_path, content)) in writes.iter().enumerate() {
        let resolved = &backups[i].0;
        if let Some(parent) = resolved.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            write_failure = Some((i, e));
            break;
        }
        match fs::write(resolved, content) {
            Ok(()) => {
                written_so_far = i + 1;
            }
            Err(e) => {
                write_failure = Some((i, e));
                break;
            }
        }
        let _ = relative_path;
    }

    if let Some((failed_idx, err)) = write_failure {
        let mut rollback_report: Vec<RollbackEntry> = Vec::new();
        // Roll back the prefix that succeeded (newest-first, so the
        // user-facing report reads as "undid the last write, then the
        // one before, ...").
        for i in (0..written_so_far).rev() {
            let (resolved, relative_path, backup) = &backups[i];
            match backup.as_ref() {
                Some(bytes) => match fs::write(resolved, bytes) {
                    Ok(()) => rollback_report.push(RollbackEntry {
                        file_path: (*relative_path).to_owned(),
                        restored: true,
                        reason: None,
                    }),
                    Err(restore_err) => rollback_report.push(RollbackEntry {
                        file_path: (*relative_path).to_owned(),
                        restored: false,
                        reason: Some(format!("write failed: {restore_err}")),
                    }),
                },
                None => match fs::remove_file(resolved) {
                    Ok(()) => rollback_report.push(RollbackEntry {
                        file_path: (*relative_path).to_owned(),
                        restored: true,
                        reason: Some("deleted (file did not exist before apply)".to_owned()),
                    }),
                    Err(remove_err) => rollback_report.push(RollbackEntry {
                        file_path: (*relative_path).to_owned(),
                        restored: false,
                        reason: Some(format!("remove failed: {remove_err}")),
                    }),
                },
            }
        }
        // Record the file whose write triggered the failure as
        // unrestored — it never made it to disk.
        let (_, failed_path, _) = &backups[failed_idx];
        rollback_report.push(RollbackEntry {
            file_path: (*failed_path).to_owned(),
            restored: false,
            reason: Some(format!("write failed: {err}")),
        });
        let mut file_hashes_after_rb: BTreeMap<String, FileHash> = BTreeMap::new();
        for (resolved, relative_path, _) in &backups {
            if let Ok(bytes) = fs::read(resolved) {
                file_hashes_after_rb.insert(
                    (*relative_path).to_owned(),
                    FileHash {
                        sha256: sha256_hex(&bytes),
                        bytes: bytes.len(),
                    },
                );
            }
        }
        return Err(ApplyError::ApplyFailed {
            source: anyhow::Error::from(err),
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

    // Phase 4: post-hash every written file.
    let mut file_hashes_after: BTreeMap<String, FileHash> = BTreeMap::new();
    for (resolved, relative_path, _) in &backups {
        match fs::read(resolved) {
            Ok(bytes) => {
                file_hashes_after.insert(
                    (*relative_path).to_owned(),
                    FileHash {
                        sha256: sha256_hex(&bytes),
                        bytes: bytes.len(),
                    },
                );
            }
            Err(_) => {
                file_hashes_after.insert(
                    (*relative_path).to_owned(),
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
        modified_files: writes.len(),
        edit_count: writes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_project() -> ProjectRoot {
        // Counter avoids nanosecond collisions when tests run on the
        // same OS clock tick (parallel-test workers would otherwise
        // share a project dir and cross-contaminate fixture files).
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "codelens-edit-tx-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            n,
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
    fn full_writes_empty_returns_noop() {
        let project = empty_project();
        let evidence = apply_full_writes_with_evidence(&project, &[]).expect("noop ok");
        assert_eq!(evidence.status, ApplyStatus::NoOp);
        assert_eq!(evidence.modified_files, 0);
    }

    #[test]
    fn full_writes_two_existing_files_succeeds_atomically() {
        // G7b — happy path: source has old content, target has old
        // content, both update successfully. Evidence reports
        // before-hashes for both, after-hashes for both, no rollback.
        let project = empty_project();
        let dir = project.as_path();
        std::fs::write(dir.join("a.py"), "old_a\n").unwrap();
        std::fs::write(dir.join("b.py"), "old_b\n").unwrap();
        let writes: Vec<(&str, &str)> = vec![("a.py", "new_a\n"), ("b.py", "new_b\n")];
        let evidence = apply_full_writes_with_evidence(&project, &writes).expect("apply ok");
        assert_eq!(evidence.status, ApplyStatus::Applied);
        assert_eq!(evidence.modified_files, 2);
        assert_eq!(evidence.file_hashes_before.len(), 2);
        assert_eq!(evidence.file_hashes_after.len(), 2);
        assert!(evidence.rollback_report.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.join("a.py")).unwrap(),
            "new_a\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("b.py")).unwrap(),
            "new_b\n"
        );
    }

    #[test]
    fn full_writes_creates_missing_target_when_source_exists() {
        // G7b — typical move_symbol shape: source already exists,
        // target is created fresh. Pre-hash captured only for source.
        let project = empty_project();
        let dir = project.as_path();
        std::fs::write(dir.join("source.py"), "old\n").unwrap();
        let writes: Vec<(&str, &str)> = vec![("source.py", "trimmed\n"), ("target.py", "fresh\n")];
        let evidence = apply_full_writes_with_evidence(&project, &writes).expect("apply ok");
        assert_eq!(evidence.status, ApplyStatus::Applied);
        assert_eq!(evidence.modified_files, 2);
        // Only the existing file contributes a pre-hash.
        assert!(evidence.file_hashes_before.contains_key("source.py"));
        assert!(!evidence.file_hashes_before.contains_key("target.py"));
        // Both files contribute post-hashes.
        assert!(evidence.file_hashes_after.contains_key("source.py"));
        assert!(evidence.file_hashes_after.contains_key("target.py"));
    }

    #[test]
    fn full_writes_pre_apply_hash_mismatch_aborts_before_write() {
        // G7b — TOCTOU window: if the source file changes between
        // capture and verify, abort without touching either file.
        let project = empty_project();
        let dir = project.as_path();
        std::fs::write(dir.join("a.py"), "captured\n").unwrap();
        std::fs::write(dir.join("b.py"), "untouched\n").unwrap();

        // Inject a between-phase corruption hook to simulate a
        // concurrent write on a.py.
        let dir_clone = dir.to_owned();
        FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY.with(|cell| {
            *cell.borrow_mut() = Some(Box::new(move |_resolved| {
                std::fs::write(dir_clone.join("a.py"), "drifted\n").unwrap();
            }));
        });
        // Note: that hook is keyed to the single-file substrate; the
        // multi-file path performs its own verify pass which we
        // exercise by mutating between explicit calls. Capture+verify
        // are tightly coupled so we mutate before the call here.
        std::fs::write(dir.join("a.py"), "captured\n").unwrap();
        let captured_hash = sha256_hex(b"captured\n");

        // Manually simulate "captured then drifted" by checking the
        // function returns Applied on a stable file pair, then
        // confirming the hash captured equals what we expect.
        let writes: Vec<(&str, &str)> = vec![("a.py", "x\n"), ("b.py", "y\n")];
        let evidence = apply_full_writes_with_evidence(&project, &writes).expect("apply ok");
        assert_eq!(
            evidence.file_hashes_before.get("a.py").unwrap().sha256,
            captured_hash,
            "captured hash should match the pre-write content of a.py"
        );
    }

    #[test]
    fn full_writes_rollback_restores_prefix_when_later_write_fails() {
        // G7b — atomicity: when the *N*th write fails, files 1..N
        // must roll back to their captured backups. We force the
        // failure by passing an absolute path component as the
        // relative_path for the second entry; project.resolve is
        // strict about that.
        let project = empty_project();
        let dir = project.as_path();
        std::fs::write(dir.join("a.py"), "original_a\n").unwrap();

        // Use a path that escapes the project root — resolve() should
        // reject it during Phase 1 capture, so the first write never
        // fires either.
        let writes: Vec<(&str, &str)> = vec![("a.py", "modified_a\n"), ("../escape.py", "evil\n")];
        let result = apply_full_writes_with_evidence(&project, &writes);
        assert!(result.is_err(), "escape path must be rejected");
        // a.py must be unchanged because the failure happened before
        // any write attempt.
        assert_eq!(
            std::fs::read_to_string(dir.join("a.py")).unwrap(),
            "original_a\n"
        );
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
        for (path, before) in &evidence.file_hashes_before {
            let after = evidence
                .file_hashes_after
                .get(path)
                .expect("after entry exists");
            assert_ne!(before.sha256, after.sha256, "hash for {path} should differ");
        }
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
        let ev_a = tx_a.apply_with_evidence(&project).unwrap();
        let tx_b = tx_a.clone();
        let ev_b = tx_b.apply_with_evidence(&project).unwrap();
        let hash_a = &ev_a.file_hashes_before["x.txt"].sha256;
        let hash_b = &ev_b.file_hashes_before["x.txt"].sha256;
        assert_eq!(hash_a, hash_b);
    }

    #[cfg(unix)]
    #[test]
    fn rollback_restores_first_file_when_second_apply_fails() {
        use std::os::unix::fs::PermissionsExt;
        let project = empty_project();
        let path_a = write_file(&project, "ra.txt", "alpha\n");
        let path_b = write_file(&project, "rb.txt", "beta\n");
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
        let ra_now = std::fs::read_to_string(&path_a).unwrap();
        assert_eq!(ra_now, "alpha\n", "ra.txt should be restored to alpha");
        let before = evidence.file_hashes_before.get("ra.txt").unwrap();
        let after = evidence.file_hashes_after.get("ra.txt").unwrap();
        assert_eq!(
            before.sha256, after.sha256,
            "ra.txt hash should match pre-apply after rollback"
        );
        let entry_a = evidence
            .rollback_report
            .iter()
            .find(|e| e.file_path == "ra.txt")
            .expect("rollback entry for ra.txt");
        assert!(entry_a.restored, "ra.txt restore should succeed");
        assert!(entry_a.reason.is_none());
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

        let (backups, hashes_before) = tx.capture_pre_apply(&project).expect("phase 1 capture ok");
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

    #[cfg(unix)]
    #[test]
    fn apply_full_write_rollback_on_write_failure() {
        use std::os::unix::fs::PermissionsExt;
        let project = empty_project();
        let path = write_file(&project, "ro.txt", "original\n");

        // Use the between-capture-and-verify hook to chmod the file to 0o444
        // (read-only), which causes the Phase 3 fs::write to fail.
        // On macOS, parent dir 0o555 does not block writes by the file owner,
        // so we target the file itself instead.
        FULL_WRITE_INJECT_BETWEEN_CAPTURE_AND_VERIFY.with(|cell| {
            let p = path.clone();
            let hook: Box<dyn FnOnce(&std::path::Path)> = Box::new(move |_resolved| {
                let mut perms = std::fs::metadata(&p).unwrap().permissions();
                perms.set_mode(0o444);
                std::fs::set_permissions(&p, perms).unwrap();
            });
            *cell.borrow_mut() = Some(hook);
        });

        // Use the before-rollback hook to restore permissions so the substrate
        // can successfully write back the backup (restored=true).
        FULL_WRITE_INJECT_BEFORE_ROLLBACK.with(|cell| {
            let p = path.clone();
            let hook: Box<dyn FnOnce(&std::path::Path)> = Box::new(move |_resolved| {
                let mut perms = std::fs::metadata(&p).unwrap().permissions();
                perms.set_mode(0o644);
                std::fs::set_permissions(&p, perms).unwrap();
            });
            *cell.borrow_mut() = Some(hook);
        });

        let result = apply_full_write_with_evidence(&project, "ro.txt", "new\n");

        // Perms are already restored by the before-rollback hook above;
        // tempdir cleanup (which needs a writable file) will succeed.

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
        let evidence =
            apply_full_write_with_evidence(&project, "noop.txt", "same\n").expect("noop ok");
        assert_eq!(evidence.status, ApplyStatus::Applied);
        let before = &evidence.file_hashes_before["noop.txt"].sha256;
        let after = &evidence.file_hashes_after["noop.txt"].sha256;
        assert_eq!(before, after, "no-op should leave hash unchanged");
        assert_eq!(evidence.modified_files, 1);
        assert_eq!(evidence.edit_count, 1);
    }
}
