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
#[cfg(feature = "semantic")]
mod embedding_coverage;
mod envelope;
mod query_engine;
mod rate_limit;
mod response;
mod response_support;
#[cfg(feature = "semantic")]
pub(crate) mod semantic;
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
use session::{
    OperationAudit, apply_post_mutation, begin_mutation_operation, collect_session_context,
    new_operation_id, record_span_fields,
};

use tracing::info_span;

pub(crate) fn registered_tool_names() -> std::collections::BTreeSet<String> {
    table::DISPATCH_TABLE
        .keys()
        .map(|name| (*name).to_owned())
        .collect()
}

/// Invoke a registered tool through the same execution path as JSON-RPC.
///
/// Verb facades use this for direct handler compatibility. Routing through
/// [`QueryEngine`] preserves target-tool access, role, and mutation gates
/// instead of calling a dispatch-table handler without its execution context.
pub(crate) fn invoke_registered(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
) -> Option<crate::tool_runtime::ToolResult> {
    if !table::DISPATCH_TABLE.contains_key(name) {
        return None;
    }
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let surface = state.execution_surface(&session);
    let (result, _, _) = QueryEngine::new(state).submit_message(name, arguments, &session, surface);
    Some(result)
}

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
    let lean = envelope.lean;
    let operation_id = new_operation_id();
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

    let operation = OperationAudit::new(
        state,
        name,
        arguments,
        session,
        &ctx.active_surface,
        &operation_id,
    );

    // 3. Auth & access (role gate + surface / namespace / tier / daemon mode).
    if let Err(role_err) = enforce_role_gate(&operation) {
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

    if let Err(mut access_err) = validate_tool_access(name, session, ctx.surface, state) {
        if is_content_mutation_tool(name)
            && let Err(audit_error) = session::record_audit_rejection(
                &operation,
                "denied",
                crate::runtime_types::LifecycleState::Denied,
                &access_err,
            )
        {
            access_err = audit_error;
        }
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

    if let Err(mut validation_err) = validate_required_params(name, arguments) {
        if is_content_mutation_tool(name)
            && let Err(audit_error) = session::record_audit_rejection(
                &operation,
                "failed",
                crate::runtime_types::LifecycleState::Failed,
                &validation_err,
            )
        {
            validation_err = audit_error;
        }
        return build_error_response(
            name,
            validation_err,
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

    let mutation_audit_sink = if is_content_mutation_tool(name) {
        match begin_mutation_operation(&operation) {
            Ok(sink) => Some(sink),
            Err(audit_error) => {
                return build_error_response(
                    name,
                    audit_error,
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
        }
    } else {
        None
    };

    // 4. Execute through the QueryEngine. It resolves verb targets before
    // applying their schema, role, surface, and mutation gates.
    let engine = QueryEngine::new(state);
    #[allow(unused_mut)] // HTTP project-binding hints mutate only HTTP builds.
    let (mut result, gate_allowance, gate_failure) =
        engine.submit_message(name, arguments, session, ctx.surface);

    // 5. Response: post-mutation side effects, doom-loop warning, and response shaping.
    if let (Ok((payload, _)), Some(audit_sink)) = (&mut result, mutation_audit_sink.as_deref())
        && let Err(audit_error) = apply_post_mutation(&operation, audit_sink, payload)
    {
        result = Err(audit_error);
    }

    // Hidden-alias observability (ADR-0016): a registered tool that is not
    // listed on the active surface stays callable — registration + the
    // mutation gate govern callability, the preset/profile listing governs
    // discovery only. Flag such calls with a `surface_note` so hosts can
    // observe the aliasing. It is not an error, and verb-facade calls (whose
    // outer name *is* listed) are unaffected; only a direct call to an unlisted
    // registered tool is tagged.
    if let Ok((payload, _)) = &mut result
        && !crate::tool_defs::is_tool_in_surface(name, ctx.surface)
        && let Some(map) = payload.as_object_mut()
    {
        map.entry("surface_note".to_owned())
            .or_insert_with(|| serde_json::json!("hidden_alias"));
    }

    // #347: shared-daemon project trap. An HTTP session that never
    // declared its workspace is pinned to the daemon's default project —
    // usually NOT the caller's repo. Attach a loud hint to every
    // project-scoped success payload until the session binds explicitly
    // (initialize `project`/`x-codelens-project` header, or
    // prepare_harness_session / activate_project with `project=`).
    #[cfg(feature = "http")]
    if let Ok((payload, _)) = &mut result
        && state.should_route_to_session(session)
        && crate::tool_defs::tool_namespace(name) != "session"
        && !state.session_project_binding_explicit(session.session_id.as_str())
        && let Some(map) = payload.as_object_mut()
    {
        let active_project = state.current_project_scope();
        let session_project = state.session_project_path(session.session_id.as_str());
        let active_project_matches_session_project =
            session_project.as_deref() == Some(active_project.as_str());
        map.insert(
            "project_binding".to_owned(),
            serde_json::json!({
                "bound": false,
                "reason": "implicit_session_project_binding",
                "active_project": active_project,
                "session_project": session_project,
                "active_project_matches_session_project": active_project_matches_session_project,
                "hint": "This HTTP session has only an implicit project binding. The active project may be inherited from the daemon default or another session; call prepare_harness_session with project=<absolute workspace root>, or attach with the x-codelens-project header to bind automatically.",
            }),
        );
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
            lean,
            harness_phase: harness_phase.as_deref(),
            request_budget: envelope.budget,
            start,
            id,
        }),
        Err(mut error) => {
            if let Some(audit_sink) = mutation_audit_sink.as_deref()
                && let Err(audit_error) =
                    session::record_audit_failure(&operation, audit_sink, &error)
            {
                error = audit_error;
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
