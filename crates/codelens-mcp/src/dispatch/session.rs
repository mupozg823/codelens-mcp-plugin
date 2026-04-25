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

/// Apply graph invalidation, symbol reindex, embedding reindex, and audit
/// after a successful content-mutation tool call.
pub(super) fn apply_post_mutation(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    session: &crate::session_context::SessionRequestContext,
    active_surface: &str,
    payload: &serde_json::Value,
) {
    state.graph_cache().invalidate();
    state.clear_recent_preflights();

    // Incremental reindex: refresh symbol DB + embedding index for the mutated file.
    if let Some(fp) = arguments
        .get("file_path")
        .or_else(|| arguments.get("relative_path"))
        .and_then(|v| v.as_str())
    {
        if let Err(e) = state.symbol_index().refresh_file(fp) {
            tracing::debug!(file = fp, error = %e, "incremental symbol reindex failed");
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

    if let Err(error) = state.record_mutation_audit(name, active_surface, arguments, session) {
        warn!(tool = name, error = %error, "failed to write mutation audit event");
    }
    record_audit_outcome(state, name, arguments, session, payload);
    if !session.is_local() {
        tracing::info!(
            tool = name,
            session_id = session.session_id.as_str(),
            "mutation completed for trusted session"
        );
    }
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
/// Failures here are logged at warn but never propagated — losing one
/// audit row must not break the call. The legacy jsonl sink in
/// `mutation_audit.rs` still captures the intent record.
fn record_audit_outcome(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    session: &crate::session_context::SessionRequestContext,
    payload: &serde_json::Value,
) {
    let Some(sink) = state.audit_sink() else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let args_hash = crate::audit_sink::canonical_sha256_hex(arguments);
    let transaction_id = format!("{}-{}-{}", session.session_id, name, &args_hash[..16]);

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
        crate::lifecycle::LifecycleState::terminal_for_apply_status(&payload_apply_status)
            .unwrap_or(crate::lifecycle::LifecycleState::Audited);

    // ADR-0009 §3 rollback_restored: when Hybrid returned RolledBack,
    // probe the rollback_report to summarise restore success. This is
    // a single-file aggregate ("did *every* restore succeed?"); the
    // detailed rollback_report stays in the response, not the audit
    // column.
    let rollback_restored = if state_to == crate::lifecycle::LifecycleState::RolledBack {
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
    let evidence_hash = Some(crate::audit_sink::canonical_sha256_hex(evidence_value));

    let error_message = if state_to == crate::lifecycle::LifecycleState::RolledBack {
        payload
            .get("data")
            .and_then(|d| d.get("error_message"))
            .or_else(|| payload.get("error_message"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    } else {
        None
    };

    let record = crate::audit_sink::AuditRecord {
        transaction_id,
        timestamp_ms: now_ms,
        // P2-C resolves the principal id from CODELENS_PRINCIPAL.
        principal: crate::principals::current_principal_id(),
        tool: name.to_owned(),
        args_hash,
        apply_status: payload_apply_status,
        state_from: Some(
            crate::lifecycle::LifecycleState::Applying
                .as_str()
                .to_owned(),
        ),
        state_to: state_to.as_str().to_owned(),
        evidence_hash,
        rollback_restored,
        error_message,
    };
    if let Err(error) = sink.write(&record) {
        warn!(
            tool = name,
            error = %error,
            "failed to write audit_sink outcome row"
        );
    }
}

/// ADR-0009 §3: write one row for a mutation that returned `Err`.
/// Terminal state is `Failed`; `apply_status` is `failed`. Used when
/// the handler reports an error before the substrate runs (e.g. line
/// out of range) or after a substrate write that could not even
/// roll back. Hybrid `RolledBack` is *not* an Err — that case is
/// handled by `record_audit_outcome` because the handler returns Ok.
pub(super) fn record_audit_failure(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    session: &crate::session_context::SessionRequestContext,
    error: &crate::error::CodeLensError,
) {
    let Some(sink) = state.audit_sink() else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let args_hash = crate::audit_sink::canonical_sha256_hex(arguments);
    let transaction_id = format!("{}-{}-{}", session.session_id, name, &args_hash[..16]);
    let record = crate::audit_sink::AuditRecord {
        transaction_id,
        timestamp_ms: now_ms,
        principal: crate::principals::current_principal_id(),
        tool: name.to_owned(),
        args_hash,
        apply_status: "failed".to_owned(),
        // Pre-substrate validation rejection happens before Applying;
        // we use Verifying as the most informative `state_from`
        // marker for "got past the role gate but the substrate did
        // not commit".
        state_from: Some(
            crate::lifecycle::LifecycleState::Verifying
                .as_str()
                .to_owned(),
        ),
        state_to: crate::lifecycle::LifecycleState::Failed.as_str().to_owned(),
        evidence_hash: None,
        rollback_restored: None,
        error_message: Some(error.to_string()),
    };
    if let Err(io_err) = sink.write(&record) {
        warn!(
            tool = name,
            error = %io_err,
            "failed to write audit_sink failure row"
        );
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
