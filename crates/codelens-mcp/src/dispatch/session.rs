//! Session context collection, mutation gate execution, and post-mutation side effects.

use crate::AppState;
use tracing::warn;

#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;

use super::rate_limit::hash_args_for_doom_loop;

/// Contextual data gathered from the session before executing a tool.
pub(super) struct SessionContext {
    pub(super) surface: crate::tool_defs::ToolSurface,
    pub(super) active_surface: String,
    pub(super) recent_tools: Vec<String>,
    pub(super) doom_count: usize,
    pub(super) doom_rapid: bool,
}

pub(super) struct OperationAudit<'a> {
    pub(super) state: &'a AppState,
    pub(super) name: &'a str,
    pub(super) arguments: &'a serde_json::Value,
    pub(super) session: &'a crate::session_context::SessionRequestContext,
    pub(super) active_surface: &'a str,
    pub(super) operation_id: &'a str,
}

impl<'a> OperationAudit<'a> {
    pub(super) fn new(
        state: &'a AppState,
        name: &'a str,
        arguments: &'a serde_json::Value,
        session: &'a crate::session_context::SessionRequestContext,
        active_surface: &'a str,
        operation_id: &'a str,
    ) -> Self {
        Self {
            state,
            name,
            arguments,
            session,
            active_surface,
            operation_id,
        }
    }
}

/// Gather doom-loop counts, file-access records, surface, and recent tools.
pub(super) fn collect_session_context(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    session: &crate::session_context::SessionRequestContext,
) -> SessionContext {
    let args_hash = hash_args_for_doom_loop(arguments);
    let (doom_count, doom_rapid) = state.doom_loop_count_for_session(session, name, args_hash);

    // Track file access for session-aware ranking boost.
    if let Some(fp) = arguments
        .get("file_path")
        .or_else(|| arguments.get("path"))
        .or_else(|| arguments.get("relative_path"))
        .and_then(|v| v.as_str())
    {
        state.record_file_access_for_session(session, fp);
    }

    let surface = state.execution_surface(session);
    let active_surface = surface.as_label().to_owned();
    let recent_tools = state.recent_tools_for_session(session);

    SessionContext {
        surface,
        active_surface,
        recent_tools,
        doom_count,
        doom_rapid,
    }
}

/// Generate the UUID `operation_id` shared by one invocation's audit
/// transitions, response envelope, and orchestration event.
pub(super) fn new_operation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn insert_orchestration_event(payload: &mut serde_json::Value, event: serde_json::Value) {
    if let Some(data) = payload.get_mut("data").and_then(|v| v.as_object_mut()) {
        data.entry("orchestration_event".to_owned())
            .or_insert(event);
        return;
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.entry("orchestration_event".to_owned()).or_insert(event);
    }
}

/// Capture the per-call session metadata that used to live in the
/// retired jsonl intent record. Stored as JSON inside the audit_log
/// `session_metadata` column — operators can query the trail without
/// joining a second store.
pub(super) fn session_metadata_for(
    state: &AppState,
    session: &crate::session_context::SessionRequestContext,
    active_surface: &str,
) -> serde_json::Value {
    serde_json::json!({
        "project_scope": state.current_project_scope(),
        "surface": active_surface,
        "daemon_mode": state.daemon_mode().as_str(),
        "trusted_client": session.trusted_client,
        "requested_profile": session.requested_profile,
        "client_name": session.client_name,
        "client_version": session.client_version,
    })
}

/// Inject the canonical `operation_id` and deprecated `transaction_id`
/// alias into a mutation response without overwriting handler values.
fn inject_operation_id(payload: &mut serde_json::Value, operation_id: &str) {
    if let Some(data) = payload.get_mut("data").and_then(|v| v.as_object_mut()) {
        data.entry("operation_id".to_owned())
            .or_insert_with(|| serde_json::Value::String(operation_id.to_owned()));
        data.entry("transaction_id".to_owned())
            .or_insert_with(|| serde_json::Value::String(operation_id.to_owned()));
        return;
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.entry("operation_id".to_owned())
            .or_insert_with(|| serde_json::Value::String(operation_id.to_owned()));
        obj.entry("transaction_id".to_owned())
            .or_insert_with(|| serde_json::Value::String(operation_id.to_owned()));
    }
}

/// P2-E: inject the list of file paths whose engine caches were
/// invalidated by this mutation. Mirrors the placement rules of
/// [`inject_operation_id`]: prefer `payload.data.invalidated_paths`,
/// fall back to `payload.invalidated_paths`. Existing entries are not
/// overwritten so handlers that already populate the field (e.g.
/// future multi-file workflows) keep their own value. An empty `paths`
/// vec still surfaces an empty array — the response contract is "if
/// you mutated, you tell the caller which files moved" and an empty
/// list is the honest answer for, say, a no-op rewrite.
fn inject_invalidated_paths(payload: &mut serde_json::Value, paths: &[String]) {
    let value = serde_json::Value::Array(
        paths
            .iter()
            .map(|p| serde_json::Value::String(p.clone()))
            .collect(),
    );
    if let Some(data) = payload.get_mut("data").and_then(|v| v.as_object_mut()) {
        data.entry("invalidated_paths".to_owned()).or_insert(value);
        return;
    }
    if let Some(obj) = payload.as_object_mut() {
        obj.entry("invalidated_paths".to_owned()).or_insert(value);
    }
}

/// Apply graph invalidation, symbol reindex, embedding reindex, audit,
/// and operation-id surfacing after a successful content-mutation
/// tool call.
pub(super) fn apply_post_mutation(
    operation: &OperationAudit<'_>,
    audit_sink: &crate::audit_sink::AuditSink,
    payload: &mut serde_json::Value,
) -> Result<(), crate::error::CodeLensError> {
    let state = operation.state;
    let name = operation.name;
    let arguments = operation.arguments;
    let session = operation.session;
    let operation_id = operation.operation_id;
    state.graph_cache().invalidate();
    state.clear_recent_preflights();

    // P2-E: collect the file paths whose caches were invalidated so we
    //        can surface them in the response (`invalidated_paths`).
    let mut invalidated_paths: Vec<String> = Vec::new();

    // Incremental reindex: refresh symbol DB + embedding index for the mutated file.
    if let Some(fp) = arguments
        .get("file_path")
        .or_else(|| arguments.get("relative_path"))
        .and_then(|v| v.as_str())
    {
        invalidated_paths.push(fp.to_owned());
        if let Err(e) = state.symbol_index().refresh_file(fp) {
            tracing::debug!(file = fp, error = %e, "incremental symbol reindex failed");
        }
        // P2-E: BM25/FTS uses `content=symbols` external-content
        //        storage; mutating `symbols` does not auto-update the
        //        FTS shadow table, so the next BM25 search would see
        //        stale results until the lazy rebuild trigger fires.
        //        Invalidate the meta marker so the next search rebuilds.
        if let Err(e) = state.symbol_index().db().invalidate_fts() {
            tracing::debug!(file = fp, error = %e, "BM25/FTS invalidation failed");
        }
        // Refresh embedding index if it is active or an on-disk index already exists.
        #[cfg(feature = "semantic")]
        {
            let project = state.project();
            let configured_model = codelens_engine::configured_embedding_model_name();
            let embeddings_active = {
                let guard = state.embedding_ref();
                guard.as_ref().is_some_and(|engine| engine.is_indexed())
            };
            let on_disk_index_exists = EmbeddingEngine::inspect_existing_index(&project)
                .ok()
                .flatten()
                .is_some_and(|info| {
                    info.model_name == configured_model && info.indexed_symbols > 0
                });
            if embeddings_active || on_disk_index_exists {
                let guard = state.embedding_engine();
                if let Some(engine) = guard.as_ref() {
                    if let Err(e) = engine.index_changed_files(&project, &[fp]) {
                        tracing::debug!(
                            file = fp,
                            error = %e,
                            "incremental embedding reindex failed"
                        );
                    }
                } else {
                    tracing::debug!(
                        file = fp,
                        "embedding engine unavailable for incremental reindex"
                    );
                }
            }
        }
    }

    record_audit_outcome(operation, audit_sink, payload)?;
    inject_operation_id(payload, operation_id);
    inject_invalidated_paths(payload, &invalidated_paths);
    if let Some(run_id) = arguments
        .get("orchestration_run_id")
        .and_then(|value| value.as_str())
    {
        let apply_status = payload
            .get("data")
            .and_then(|data| data.get("apply_status"))
            .or_else(|| payload.get("apply_status"))
            .and_then(|value| value.as_str())
            .unwrap_or("applied");
        let event_name = if apply_status == "rolled_back" {
            "mutation_failed"
        } else {
            "mutation_applied"
        };
        let to_state = if apply_status == "rolled_back" {
            "failed"
        } else {
            "mutation_applied"
        };
        let mut extra = serde_json::Map::new();
        extra.insert("tool".to_owned(), serde_json::json!(name));
        extra.insert("operation_id".to_owned(), serde_json::json!(operation_id));
        extra.insert("apply_status".to_owned(), serde_json::json!(apply_status));
        extra.insert(
            "modified_files".to_owned(),
            payload
                .get("data")
                .and_then(|data| data.get("modified_files"))
                .or_else(|| payload.get("modified_files"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        );
        extra.insert(
            "invalidated_paths".to_owned(),
            serde_json::json!(invalidated_paths),
        );
        if let Some(event) = state.append_orchestration_event_for_current_scope(
            session.session_id.as_str(),
            run_id,
            event_name,
            Some("executing"),
            to_state,
            extra,
        ) {
            insert_orchestration_event(payload, event);
        }
    }
    if !session.is_local() {
        tracing::info!(
            tool = name,
            session_id = session.session_id.as_str(),
            "mutation completed for trusted session"
        );
    }
    Ok(())
}

pub(super) fn begin_mutation_operation(
    operation: &OperationAudit<'_>,
) -> Result<std::sync::Arc<crate::audit_sink::AuditSink>, crate::error::CodeLensError> {
    let state = operation.state;
    let name = operation.name;
    let arguments = operation.arguments;
    let session = operation.session;
    let active_surface = operation.active_surface;
    let operation_id = operation.operation_id;
    let sink = state.require_audit_sink()?;
    let record = crate::audit_sink::AuditRecord {
        operation_id: operation_id.to_owned(),
        timestamp_ms: crate::util::now_ms() as i64,
        principal: crate::principals::resolve_principal_id(session),
        tool: name.to_owned(),
        args_hash: crate::util::canonical_sha256_hex(arguments),
        apply_status: "verifying".to_owned(),
        state_from: None,
        state_to: crate::runtime_types::LifecycleState::Verifying
            .as_str()
            .to_owned(),
        evidence_hash: None,
        rollback_restored: None,
        error_message: None,
        session_metadata: Some(session_metadata_for(state, session, active_surface)),
    };
    sink.write(&record).map_err(|error| {
        crate::error::CodeLensError::Internal(anyhow::anyhow!(
            "failed to begin mutation audit operation {operation_id}: {error}"
        ))
    })?;
    Ok(sink)
}

pub(super) fn record_audit_rejection(
    operation: &OperationAudit<'_>,
    apply_status: &str,
    state_to: crate::runtime_types::LifecycleState,
    error: &crate::error::CodeLensError,
) -> Result<(), crate::error::CodeLensError> {
    let state = operation.state;
    let name = operation.name;
    let arguments = operation.arguments;
    let session = operation.session;
    let active_surface = operation.active_surface;
    let operation_id = operation.operation_id;
    let sink = state.require_audit_sink()?;
    let record = crate::audit_sink::AuditRecord {
        operation_id: operation_id.to_owned(),
        timestamp_ms: crate::util::now_ms() as i64,
        principal: crate::principals::resolve_principal_id(session),
        tool: name.to_owned(),
        args_hash: crate::util::canonical_sha256_hex(arguments),
        apply_status: apply_status.to_owned(),
        state_from: None,
        state_to: state_to.as_str().to_owned(),
        evidence_hash: None,
        rollback_restored: None,
        error_message: Some(error.to_string()),
        session_metadata: Some(session_metadata_for(state, session, active_surface)),
    };
    sink.write(&record).map_err(|write_error| {
        crate::error::CodeLensError::Internal(anyhow::anyhow!(
            "failed to record rejected operation {operation_id}: {write_error}; original error: {error}"
        ))
    })
}

/// ADR-0009 §2 + §3: write a single row to the durable audit_sink
/// describing the outcome of a successful mutation.
///
/// `state_from` is `Applying` (substrate Phase 3) and `state_to` is
/// determined from the response payload's `apply_status` field
/// (Hybrid contract from G7) — `applied` → `Audited`, `rolled_back` →
/// `RolledBack`, `no_op` → `Audited`. Unknown / missing apply_status
/// defaults to `Audited` since the handler succeeded.
///
/// `evidence_hash` is the canonical sha256 of the response payload's
/// `data` subobject if present (the structured evidence-bearing
/// section); else of the entire payload. This lets the audit log
/// verify replay equivalence without storing user content.
///
fn record_audit_outcome(
    operation: &OperationAudit<'_>,
    sink: &crate::audit_sink::AuditSink,
    payload: &serde_json::Value,
) -> Result<(), crate::error::CodeLensError> {
    let state = operation.state;
    let name = operation.name;
    let arguments = operation.arguments;
    let session = operation.session;
    let active_surface = operation.active_surface;
    let operation_id = operation.operation_id;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let args_hash = crate::util::canonical_sha256_hex(arguments);

    // ADR-0009 §3: derive terminal state from the Hybrid apply_status.
    // The handler returned Ok, so the call definitely traversed
    // Verifying → Applying. The payload's apply_status (set by G7
    // Hybrid contract on raw_fs primitives) determines whether we
    // reached Committed→Audited (applied/no_op) or RolledBack.
    let payload_apply_status = payload
        .get("data")
        .and_then(|d| d.get("apply_status"))
        .or_else(|| payload.get("apply_status"))
        .and_then(|v| v.as_str())
        .unwrap_or("applied")
        .to_owned();
    let state_to =
        crate::runtime_types::LifecycleState::terminal_for_apply_status(&payload_apply_status)
            .unwrap_or(crate::runtime_types::LifecycleState::Audited);

    // ADR-0009 §3 rollback_restored: when Hybrid returned RolledBack,
    // probe the rollback_report to summarise restore success. This is
    // a single-file aggregate ("did *every* restore succeed?"); the
    // detailed rollback_report stays in the response, not the audit
    // column.
    let rollback_restored = if state_to == crate::runtime_types::LifecycleState::RolledBack {
        payload
            .get("data")
            .and_then(|d| d.get("rollback_report"))
            .or_else(|| payload.get("rollback_report"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                !arr.is_empty()
                    && arr.iter().all(|entry| {
                        entry
                            .get("restored")
                            .and_then(|r| r.as_bool())
                            .unwrap_or(false)
                    })
            })
    } else {
        None
    };

    // ADR-0009 §3 evidence_hash: hash the structured `data` subobject
    // when the response uses the standard envelope; fall back to the
    // whole payload otherwise.
    let evidence_value = payload.get("data").unwrap_or(payload);
    let evidence_hash = Some(crate::util::canonical_sha256_hex(evidence_value));

    let error_message = if state_to == crate::runtime_types::LifecycleState::RolledBack {
        payload
            .get("data")
            .and_then(|d| d.get("error_message"))
            .or_else(|| payload.get("error_message"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    } else {
        None
    };

    let session_metadata = Some(session_metadata_for(state, session, active_surface));
    let record = crate::audit_sink::AuditRecord {
        operation_id: operation_id.to_owned(),
        timestamp_ms: now_ms,
        // P2-C resolves the principal id from CODELENS_PRINCIPAL.
        principal: crate::principals::resolve_principal_id(session),
        tool: name.to_owned(),
        args_hash,
        apply_status: payload_apply_status,
        state_from: Some(
            crate::runtime_types::LifecycleState::Applying
                .as_str()
                .to_owned(),
        ),
        state_to: state_to.as_str().to_owned(),
        evidence_hash,
        rollback_restored,
        error_message,
        session_metadata,
    };
    sink.write(&record).map_err(|error| {
        crate::error::CodeLensError::Internal(anyhow::anyhow!(
            "failed to finish mutation audit operation {operation_id}: {error}"
        ))
    })
}

/// ADR-0009 §3: write one row for a mutation that returned `Err`.
/// Terminal state is `Failed`; `apply_status` is `failed`. Used when
/// the handler reports an error before the substrate runs (e.g. line
/// out of range) or after a substrate write that could not even
/// roll back. Hybrid `RolledBack` is *not* an Err — that case is
/// handled by `record_audit_outcome` because the handler returns Ok.
pub(super) fn record_audit_failure(
    operation: &OperationAudit<'_>,
    sink: &crate::audit_sink::AuditSink,
    error: &crate::error::CodeLensError,
) -> Result<(), crate::error::CodeLensError> {
    let state = operation.state;
    let name = operation.name;
    let arguments = operation.arguments;
    let session = operation.session;
    let active_surface = operation.active_surface;
    let operation_id = operation.operation_id;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let args_hash = crate::util::canonical_sha256_hex(arguments);
    let session_metadata = Some(session_metadata_for(state, session, active_surface));
    let record = crate::audit_sink::AuditRecord {
        operation_id: operation_id.to_owned(),
        timestamp_ms: now_ms,
        principal: crate::principals::resolve_principal_id(session),
        tool: name.to_owned(),
        args_hash,
        apply_status: "failed".to_owned(),
        // Pre-substrate validation rejection happens before Applying;
        // we use Verifying as the most informative `state_from`
        // marker for "got past the role gate but the substrate did
        // not commit".
        state_from: Some(
            crate::runtime_types::LifecycleState::Verifying
                .as_str()
                .to_owned(),
        ),
        state_to: crate::runtime_types::LifecycleState::Failed
            .as_str()
            .to_owned(),
        evidence_hash: None,
        rollback_restored: None,
        error_message: Some(error.to_string()),
        session_metadata,
    };
    sink.write(&record).map_err(|io_error| {
        crate::error::CodeLensError::Internal(anyhow::anyhow!(
            "failed to record mutation failure for operation {operation_id}: {io_error}; original error: {error}"
        ))
    })
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn operation_id_is_unique_and_uuid_shaped() {
        let a = new_operation_id();
        let b = new_operation_id();
        assert_ne!(a, b, "every invocation needs a distinct operation id");
        assert!(
            uuid::Uuid::parse_str(&a).is_ok(),
            "operation id must be a UUID"
        );
        assert!(
            uuid::Uuid::parse_str(&b).is_ok(),
            "operation id must be a UUID"
        );
    }

    #[test]
    fn inject_operation_id_into_data_subobject() {
        let mut payload = json!({ "data": { "apply_status": "applied" } });
        inject_operation_id(&mut payload, "4b6899a0-bf76-4b0d-876e-0708e58d8422");
        assert_eq!(
            payload["data"]["operation_id"],
            "4b6899a0-bf76-4b0d-876e-0708e58d8422"
        );
        assert_eq!(
            payload["data"]["transaction_id"],
            "4b6899a0-bf76-4b0d-876e-0708e58d8422"
        );
        assert_eq!(payload["data"]["apply_status"], "applied");
    }

    #[test]
    fn inject_operation_id_into_root_when_no_data() {
        let mut payload = json!({ "apply_status": "applied" });
        inject_operation_id(&mut payload, "tx-123");
        assert_eq!(payload["operation_id"], "tx-123");
        assert_eq!(payload["transaction_id"], "tx-123");
    }

    #[test]
    fn inject_operation_id_does_not_overwrite_legacy_alias() {
        let mut payload = json!({ "data": { "transaction_id": "handler-tx" } });
        inject_operation_id(&mut payload, "dispatch-tx");
        assert_eq!(payload["data"]["transaction_id"], "handler-tx");
    }

    #[test]
    fn inject_operation_id_no_op_on_non_object_payload() {
        let mut scalar = json!("just a string");
        inject_operation_id(&mut scalar, "tx-123");
        assert_eq!(scalar, json!("just a string"));
    }

    #[test]
    fn inject_invalidated_paths_into_data_subobject() {
        let mut payload = json!({ "data": { "apply_status": "applied" } });
        inject_invalidated_paths(&mut payload, &["src/foo.rs".to_owned()]);
        assert_eq!(payload["data"]["invalidated_paths"], json!(["src/foo.rs"]));
    }

    #[test]
    fn inject_invalidated_paths_empty_list_still_surfaces() {
        // Empty list is not a no-op — it tells the agent "no caches
        // moved" rather than "the field was forgotten".
        let mut payload = json!({ "data": {} });
        inject_invalidated_paths(&mut payload, &[]);
        assert_eq!(payload["data"]["invalidated_paths"], json!([]));
    }

    #[test]
    fn inject_invalidated_paths_does_not_overwrite_existing() {
        let mut payload = json!({
            "data": { "invalidated_paths": ["handler/owned.rs"] }
        });
        inject_invalidated_paths(&mut payload, &["dispatch/added.rs".to_owned()]);
        assert_eq!(
            payload["data"]["invalidated_paths"],
            json!(["handler/owned.rs"])
        );
    }

    #[test]
    fn inject_invalidated_paths_falls_back_to_root_when_no_data() {
        let mut payload = json!({ "apply_status": "applied" });
        inject_invalidated_paths(&mut payload, &["src/x.rs".to_owned()]);
        assert_eq!(payload["invalidated_paths"], json!(["src/x.rs"]));
    }
}

/// Record OTel-compatible span fields and log slow executions.
pub(super) fn record_span_fields(
    span: &tracing::Span,
    name: &str,
    result: &crate::tool_runtime::ToolResult,
    elapsed_ms: u128,
    active_surface: &str,
) {
    let success = result.is_ok();
    span.record("tool.success", success);
    span.record("tool.elapsed_ms", elapsed_ms as u64);
    span.record("tool.surface", active_surface);
    if success {
        span.record("otel.status_code", "OK");
        if let Ok((_, meta)) = result {
            span.record("tool.backend", meta.backend_used.as_str());
        }
    } else {
        span.record("otel.status_code", "ERROR");
    }
    if elapsed_ms > 5000 {
        warn!(
            tool = name,
            elapsed_ms = elapsed_ms as u64,
            "slow tool execution"
        );
    }
}
