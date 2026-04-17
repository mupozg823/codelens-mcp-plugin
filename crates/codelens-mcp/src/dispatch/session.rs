//! Session context collection, mutation gate execution, and post-mutation side effects.

use crate::AppState;
use crate::error::CodeLensError;
use crate::mutation_gate::{
    MutationGateAllowance, MutationGateFailure, evaluate_mutation_gate,
    is_refactor_gated_mutation_tool,
};
use crate::tools;
use tracing::warn;

#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;

use super::rate_limit::hash_args_for_doom_loop;
use super::table::DISPATCH_TABLE;

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

/// Execute the tool through the mutation gate (if applicable) or directly.
/// Returns `(tool_result, gate_allowance, gate_failure)`.
pub(super) fn run_gate_and_execute(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    session: &crate::session_context::SessionRequestContext,
    surface: crate::tool_defs::ToolSurface,
) -> (
    tools::ToolResult,
    Option<MutationGateAllowance>,
    Option<MutationGateFailure>,
) {
    if is_refactor_gated_mutation_tool(name) {
        state.metrics().record_mutation_preflight_checked();
        match evaluate_mutation_gate(state, name, session, surface, arguments) {
            Ok(allowance) => {
                let result = match DISPATCH_TABLE.get(name) {
                    Some(handler) => handler(state, arguments),
                    None => Err(CodeLensError::ToolNotFound(name.to_owned())),
                };
                (result, allowance, None)
            }
            Err(failure) => {
                if failure.missing_preflight || failure.stale {
                    state.metrics().record_mutation_without_preflight();
                }
                if failure.rename_without_symbol_preflight {
                    state.metrics().record_rename_without_symbol_preflight();
                }
                state
                    .metrics()
                    .record_mutation_preflight_gate_denied(failure.stale);
                let message = failure.message.clone();
                (Err(CodeLensError::Validation(message)), None, Some(failure))
            }
        }
    } else {
        let result = match DISPATCH_TABLE.get(name) {
            Some(handler) => handler(state, arguments),
            None => Err(CodeLensError::ToolNotFound(name.to_owned())),
        };
        (result, None, None)
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
    if !session.is_local() {
        tracing::info!(
            tool = name,
            session_id = session.session_id.as_str(),
            "mutation completed for trusted session"
        );
    }
}

/// Record OTel-compatible span fields and log slow executions.
pub(super) fn record_span_fields(
    span: &tracing::Span,
    name: &str,
    result: &tools::ToolResult,
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
