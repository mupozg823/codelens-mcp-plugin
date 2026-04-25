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

#![allow(dead_code, unused_imports)]

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
    /// Implementation lands incrementally in T2~T6.
    pub fn apply_with_evidence(&self, project: &ProjectRoot) -> Result<ApplyEvidence, ApplyError> {
        let _ = project;
        unimplemented!("apply_with_evidence implemented in T2~T6")
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
