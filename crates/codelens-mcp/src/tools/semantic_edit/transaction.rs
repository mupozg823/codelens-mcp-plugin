use crate::AppState;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

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
    /// Diagnostics captured on the edited file before/after the edit landed.
    /// Empty arrays when the snapshot was skipped or unavailable; the response
    /// `verification.diagnostics_status` carries the distinction.
    pub(crate) diagnostics_before: Value,
    pub(crate) diagnostics_after: Value,
    /// When `Some`, evidence is the source of truth for file hashes,
    /// rollback report, apply status, modified files, and edit count.
    pub(crate) evidence: Option<&'a codelens_engine::ApplyEvidence>,
}

pub(crate) fn semantic_transaction_contract(input: SemanticTransactionContractInput<'_>) -> Value {
    let (
        file_hashes_before_value,
        file_hashes_after_value,
        rollback_report_value,
        rollback_available,
        modified_files,
        edit_count,
        apply_status_resolved,
    ) = match input.evidence {
        Some(ev) => {
            let hashes_before = serde_json::to_value(&ev.file_hashes_before).unwrap_or(Value::Null);
            let hashes_after = serde_json::to_value(&ev.file_hashes_after).unwrap_or(Value::Null);
            let rollback =
                serde_json::to_value(&ev.rollback_report).unwrap_or(Value::Array(Vec::new()));
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
        &file_hashes_before_value,
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
        "file_hashes_before": file_hashes_before_value,
        "file_hashes_after": file_hashes_after_value,
        "rollback_report": rollback_report_value,
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
        "diagnostics_before": input.diagnostics_before,
        "diagnostics_after": input.diagnostics_after,
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

fn file_hashes_before(state: &AppState, file_paths: &[String]) -> Value {
    let mut hashes = Map::new();
    for file_path in unique_file_paths(file_paths) {
        let value = match state
            .project()
            .resolve(&file_path)
            .and_then(|path| std::fs::read(&path).map_err(anyhow::Error::from))
        {
            Ok(bytes) => json!({
                "sha256": sha256_digest_hex(&bytes),
                "bytes": bytes.len(),
            }),
            Err(error) => json!({
                "error": error.to_string(),
            }),
        };
        hashes.insert(file_path, value);
    }
    Value::Object(hashes)
}

pub(super) fn unique_file_paths(file_paths: &[String]) -> Vec<String> {
    file_paths
        .iter()
        .filter(|path| !path.is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn transaction_id(
    backend_id: &str,
    operation: &str,
    file_paths: &[String],
    file_hashes_before: &Value,
) -> String {
    let mut digest = Sha256::new();
    digest.update(backend_id.as_bytes());
    digest.update(b"\0");
    digest.update(operation.as_bytes());
    digest.update(b"\0");
    for file_path in unique_file_paths(file_paths) {
        digest.update(file_path.as_bytes());
        digest.update(b"\0");
    }
    digest.update(file_hashes_before.to_string().as_bytes());
    format!("semantic-tx-{}", hex_bytes(&digest.finalize()))
}

fn sha256_digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_bytes(&digest)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
