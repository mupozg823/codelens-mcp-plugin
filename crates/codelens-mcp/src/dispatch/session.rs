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
    record_audit_outcome(state, name, arguments, session);
    if !session.is_local() {
        tracing::info!(
            tool = name,
            session_id = session.session_id.as_str(),
            "mutation completed for trusted session"
        );
    }
}

/// ADR-0009 §2 + §3: write a single row to the durable audit_sink
/// describing the outcome of a successful mutation. For Phase 2-B this
/// uses placeholder state values (`Applying` → `Audited` and apply
/// status `applied`); the lifecycle state machine and rolled_back /
/// failed branches land in P2-D once handlers expose evidence to
/// dispatch.
///
/// Failures here are logged at warn but never propagated — losing one
/// audit row must not break the call. The legacy jsonl sink in
/// `mutation_audit.rs` still captures the intent record.
fn record_audit_outcome(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    session: &crate::session_context::SessionRequestContext,
) {
    let Some(sink) = state.audit_sink() else {
        return;
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let args_hash = crate::audit_sink::canonical_sha256_hex(arguments);
    // Transaction id derives from session + tool + args_hash + ts so
    // distinct calls of the same tool+args yield distinct ids while
    // a future state-machine transition for the same call can reuse
    // it (P2-D wires this through dispatch context).
    let transaction_id = format!("{}-{}-{}", session.session_id, name, &args_hash[..16]);
    let record = crate::audit_sink::AuditRecord {
        transaction_id,
        timestamp_ms: now_ms,
        // Phase 2-C populates this from the resolved principal binding.
        principal: None,
        tool: name.to_owned(),
        args_hash,
        apply_status: "applied".to_owned(),
        // Phase 2-D will set the actual previous state by querying the
        // sink for the same transaction_id.
        state_from: Some("Applying".to_owned()),
        state_to: "Audited".to_owned(),
        // Phase 2-D + handler-level evidence threading lands the real
        // hash; for P2-B this stays None as a deliberate marker.
        evidence_hash: None,
        rollback_restored: None,
        error_message: None,
    };
    if let Err(error) = sink.write(&record) {
        warn!(
            tool = name,
            error = %error,
            "failed to write audit_sink outcome row"
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
