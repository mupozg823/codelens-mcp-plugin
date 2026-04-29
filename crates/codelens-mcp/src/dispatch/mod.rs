//! Tool dispatch: static dispatch table + JSON-RPC tool call routing.
//!
//! ## Module layout
//!
//! - [`envelope`]: parse raw JSON-RPC params into a [`envelope::ToolCallEnvelope`].
//! - [`rate_limit`]: per-session rate limit + doom-loop argument hashing.
//! - [`table`]: static dispatch table with structural + semantic handler registrations.
//! - [`session`]: session context collection, mutation gate, post-mutation side effects.
//! - [`access`] / [`response`] / [`response_support`]: surface checks and response shaping.

mod access;
mod envelope;
mod query_engine;
mod rate_limit;
mod response;
mod response_support;
mod session;
mod table;

use crate::AppState;
use crate::protocol::JsonRpcResponse;
use crate::tool_defs::is_content_mutation_tool;
use access::{enforce_role_gate, validate_tool_access};
use envelope::{ToolCallEnvelope, validate_required_params};
use query_engine::QueryEngine;
use rate_limit::check_rate_limit;
use response::{SuccessResponseInput, build_error_response, build_success_response};
use session::{apply_post_mutation, collect_session_context, record_span_fields};

use tracing::info_span;

// Thread-local request budget — avoids race condition when multiple
// HTTP requests override the global token_budget concurrently.
thread_local! {
    static REQUEST_BUDGET: std::cell::Cell<usize> = const { std::cell::Cell::new(4000) };
}

pub(crate) fn dispatch_tool(
    state: &AppState,
    id: Option<serde_json::Value>,
    params: serde_json::Value,
) -> JsonRpcResponse {
    // 1. Parse, normalise & schema-validate request.
    let envelope = match ToolCallEnvelope::parse(&params, state) {
        Ok(env) => env,
        Err((msg, code)) => return JsonRpcResponse::error(id, code, msg),
    };
    let name = envelope.name.as_str();
    let arguments = &envelope.arguments;
    let session = &envelope.session;
    let compact = envelope.compact;
    REQUEST_BUDGET.set(envelope.budget);

    let span = info_span!(
        "tool_call",
        tool = name,
        otel.status_code = tracing::field::Empty,
        tool.success = tracing::field::Empty,
        tool.backend = tracing::field::Empty,
        tool.elapsed_ms = tracing::field::Empty,
        tool.surface = tracing::field::Empty,
    );
    let _guard = span.enter();
    let start = std::time::Instant::now();

    if let Err(validation_err) = validate_required_params(name, arguments) {
        return build_error_response(
            name,
            validation_err,
            None,
            arguments,
            "validation_failed",
            &session.session_id,
            state,
            start,
            id,
            0,
            false,
        );
    }

    // 2. Rate limit & session context (doom-loop, file-access, surface, recent tools).
    if let Some(err) = check_rate_limit(state, session) {
        return build_error_response(
            name,
            err,
            None,
            arguments,
            "rate_limited",
            &session.session_id,
            state,
            start,
            id,
            0,
            false,
        );
    }

    state.push_recent_tool_for_session(session, name);

    let ctx = collect_session_context(state, name, arguments, session);

    let harness_phase = envelope
        .harness_phase
        .clone()
        .or_else(|| crate::tools::infer_harness_phase(&ctx.recent_tools).map(str::to_owned));
    let _session_project_guard = match state.ensure_session_project(session) {
        Ok(guard) => guard,
        Err(project_err) => {
            return build_error_response(
                name,
                project_err,
                None,
                arguments,
                &ctx.active_surface,
                &session.session_id,
                state,
                start,
                id,
                ctx.doom_count,
                ctx.doom_rapid,
            );
        }
    };

    // 3. Auth & access (role gate + surface / namespace / tier / daemon mode).
    if let Err(role_err) = enforce_role_gate(state, name, arguments, session, &ctx.active_surface)
    {
        return build_error_response(
            name,
            role_err,
            None,
            arguments,
            &ctx.active_surface,
            &session.session_id,
            state,
            start,
            id,
            ctx.doom_count,
            ctx.doom_rapid,
        );
    }

    if let Err(access_err) = validate_tool_access(name, session, ctx.surface, state) {
        return build_error_response(
            name,
            access_err,
            None,
            arguments,
            &ctx.active_surface,
            &session.session_id,
            state,
            start,
            id,
            ctx.doom_count,
            ctx.doom_rapid,
        );
    }

    // 4. Execute via mutation gate (if applicable) or directly via dispatch table.
    let engine = QueryEngine::new(state);
    let (mut result, gate_allowance, gate_failure) =
        engine.submit_message(name, arguments, session, ctx.surface);

    // 5. Response: post-mutation side effects, doom-loop warning, and response shaping.
    if let Ok((payload, _)) = &mut result
        && is_content_mutation_tool(name)
    {
        apply_post_mutation(state, name, arguments, session, &ctx.active_surface, payload);
    }

    let elapsed_ms = start.elapsed().as_millis();
    record_span_fields(&span, name, &result, elapsed_ms, &ctx.active_surface);

    if ctx.doom_count >= 3 {
        tracing::warn!(
            tool = name,
            repeat_count = ctx.doom_count,
            rapid = ctx.doom_rapid,
            "doom-loop detected: same tool+args called {} times consecutively{}",
            ctx.doom_count,
            if ctx.doom_rapid { " (rapid burst)" } else { "" }
        );
    }

    match result {
        Ok((payload, meta)) => build_success_response(SuccessResponseInput {
            doom_loop_count: ctx.doom_count,
            doom_loop_rapid: ctx.doom_rapid,
            name,
            payload,
            meta,
            state,
            surface: ctx.surface,
            active_surface: &ctx.active_surface,
            arguments,
            logical_session_id: &session.session_id,
            recent_tools: ctx.recent_tools,
            gate_allowance: gate_allowance.as_ref(),
            compact,
            harness_phase: harness_phase.as_deref(),
            request_budget: envelope.budget,
            start,
            id,
        }),
        Err(error) => {
            if is_content_mutation_tool(name) {
                session::record_audit_failure(
                    state,
                    name,
                    arguments,
                    session,
                    &ctx.active_surface,
                    &error,
                );
            }
            build_error_response(
                name,
                error,
                gate_failure,
                arguments,
                &ctx.active_surface,
                &session.session_id,
                state,
                start,
                id,
                ctx.doom_count,
                ctx.doom_rapid,
            )
        }
    }
}
