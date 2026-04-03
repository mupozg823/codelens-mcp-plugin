//! Tool dispatch: static dispatch table + JSON-RPC tool call routing.

use crate::error::CodeLensError;
use crate::mutation_gate::{
    evaluate_mutation_gate, is_refactor_gated_mutation_tool, is_verifier_source_tool,
    MutationGateAllowance, MutationGateFailure,
};
use crate::protocol::{JsonRpcResponse, ToolCallResponse, ToolResponseMeta};
use crate::tool_defs::{
    default_budget_for_profile, is_content_mutation_tool, is_read_only_surface, is_tool_in_surface,
    preferred_namespaces, preferred_tier_labels, tool_definition, tool_namespace, tool_tier_label,
    ToolProfile, ToolSurface,
};
use crate::tools::{self, ToolHandler, ToolResult};
use crate::AppState;
use serde_json::json;
use std::collections::HashMap;
use std::sync::LazyLock;
use tracing::{info_span, warn};

// Thread-local request budget — avoids race condition when multiple
// HTTP requests override the global token_budget concurrently.
thread_local! {
    static REQUEST_BUDGET: std::cell::Cell<usize> = const { std::cell::Cell::new(4000) };
}

/// Get the per-request token budget (set by dispatch_tool).
pub(crate) fn request_token_budget() -> usize {
    REQUEST_BUDGET.get()
}

fn summarize_structured_content(value: &serde_json::Value, depth: usize) -> serde_json::Value {
    const MAX_STRING_CHARS: usize = 240;
    const MAX_ARRAY_ITEMS: usize = 3;
    const MAX_OBJECT_DEPTH: usize = 4;

    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            value.clone()
        }
        serde_json::Value::String(text) => {
            if text.chars().count() <= MAX_STRING_CHARS {
                value.clone()
            } else {
                let truncated = text.chars().take(MAX_STRING_CHARS).collect::<String>();
                serde_json::Value::String(format!("{truncated}..."))
            }
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|item| summarize_structured_content(item, depth + 1))
                .collect(),
        ),
        serde_json::Value::Object(map) => {
            let max_items = if depth >= MAX_OBJECT_DEPTH {
                MAX_ARRAY_ITEMS
            } else {
                usize::MAX
            };
            let mut summarized = serde_json::Map::with_capacity(map.len().min(max_items));
            for (index, (key, item)) in map.iter().enumerate() {
                if index >= max_items {
                    break;
                }
                summarized.insert(key.clone(), summarize_structured_content(item, depth + 1));
            }
            // Preserve the tool's declared schema shape: only mark truncation when the payload
            // already exposes that field.
            if map.contains_key("truncated") {
                summarized.insert("truncated".to_owned(), serde_json::Value::Bool(true));
            }
            serde_json::Value::Object(summarized)
        }
    }
}

pub(crate) fn logical_session_id(arguments: &serde_json::Value) -> &str {
    arguments
        .get("_session_id")
        .and_then(|value| value.as_str())
        .unwrap_or("local")
}

fn session_loaded_namespaces(arguments: &serde_json::Value) -> Vec<&str> {
    arguments
        .get("_session_loaded_namespaces")
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn session_loaded_tiers(arguments: &serde_json::Value) -> Vec<&str> {
    arguments
        .get("_session_loaded_tiers")
        .and_then(|value| value.as_array())
        .map(|values| values.iter().filter_map(|value| value.as_str()).collect())
        .unwrap_or_default()
}

fn session_full_tool_exposure(arguments: &serde_json::Value) -> bool {
    arguments
        .get("_session_full_tool_exposure")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn is_deferred_namespace_access_allowed(
    name: &str,
    arguments: &serde_json::Value,
    surface: ToolSurface,
) -> bool {
    let deferred_requested = arguments
        .get("_session_deferred_tool_loading")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !deferred_requested || logical_session_id(arguments) == "local" {
        return true;
    }
    if session_full_tool_exposure(arguments) {
        return true;
    }
    let namespace = tool_namespace(name);
    let preferred = preferred_namespaces(surface);
    if preferred.contains(&namespace) {
        return true;
    }
    session_loaded_namespaces(arguments).contains(&namespace)
}

fn is_deferred_tier_access_allowed(
    name: &str,
    arguments: &serde_json::Value,
    surface: ToolSurface,
) -> bool {
    let deferred_requested = arguments
        .get("_session_deferred_tool_loading")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if !deferred_requested || logical_session_id(arguments) == "local" {
        return true;
    }
    if session_full_tool_exposure(arguments) {
        return true;
    }
    let tier = tool_tier_label(name);
    let preferred = preferred_tier_labels(surface);
    if preferred.contains(&tier) {
        return true;
    }
    session_loaded_tiers(arguments).contains(&tier)
}

// ── Semantic handlers (feature-gated) ──────────────────────────────────

#[cfg(feature = "semantic")]
use codelens_core::EmbeddingEngine;

#[cfg(feature = "semantic")]
fn semantic_search_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = tools::required_string(arguments, "query")?;
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let project = state.project();
    let engine = state
        .embedding
        .get_or_init(|| {
            EmbeddingEngine::new(&project)
                .map_err(|e| tracing::error!("EmbeddingEngine init failed: {e}"))
                .ok()
        })
        .as_ref()
        .ok_or_else(|| {
            anyhow::anyhow!("Embedding engine not available. Build with --features semantic")
        })?;

    if !engine.is_indexed() {
        return Err(CodeLensError::FeatureUnavailable(
            "Embedding index is empty. Call index_embeddings first to build the semantic index."
                .into(),
        ));
    }

    let results = engine.search(query, max_results)?;
    Ok((
        json!({"query": query, "results": results, "count": results.len()}),
        tools::success_meta(crate::protocol::BackendKind::Semantic, 0.85),
    ))
}

#[cfg(feature = "semantic")]
fn index_embeddings_handler(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let project = state.project();
    let engine = state
        .embedding
        .get_or_init(|| {
            EmbeddingEngine::new(&project)
                .map_err(|e| tracing::error!("EmbeddingEngine init failed: {e}"))
                .ok()
        })
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let count = engine.index_from_project(&project)?;
    Ok((
        json!({"indexed_symbols": count, "status": "ok"}),
        tools::success_meta(crate::protocol::BackendKind::Semantic, 0.95),
    ))
}

#[cfg(feature = "semantic")]
fn find_similar_code_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = tools::required_string(arguments, "file_path")?;
    let symbol_name = tools::required_string(arguments, "symbol_name")?;
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let project = state.project();
    let engine = state
        .embedding
        .get_or_init(|| EmbeddingEngine::new(&project).ok())
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let results = engine.find_similar_code(file_path, symbol_name, max_results)?;
    Ok((
        json!({"query_symbol": symbol_name, "file": file_path, "similar": results, "count": results.len()}),
        tools::success_meta(crate::protocol::BackendKind::Semantic, 0.80),
    ))
}

#[cfg(feature = "semantic")]
fn find_code_duplicates_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let threshold = arguments
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.85);
    let max_pairs = arguments
        .get("max_pairs")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    let project = state.project();
    let engine = state
        .embedding
        .get_or_init(|| EmbeddingEngine::new(&project).ok())
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let pairs = engine.find_duplicates(threshold, max_pairs)?;
    Ok((
        json!({"threshold": threshold, "duplicates": pairs, "count": pairs.len()}),
        tools::success_meta(crate::protocol::BackendKind::Semantic, 0.80),
    ))
}

#[cfg(feature = "semantic")]
fn classify_symbol_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = tools::required_string(arguments, "file_path")?;
    let symbol_name = tools::required_string(arguments, "symbol_name")?;
    let categories = arguments
        .get("categories")
        .and_then(|v| v.as_array())
        .ok_or_else(|| CodeLensError::MissingParam("categories".into()))?;
    let cat_strs: Vec<&str> = categories.iter().filter_map(|v| v.as_str()).collect();

    let project = state.project();
    let engine = state
        .embedding
        .get_or_init(|| EmbeddingEngine::new(&project).ok())
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let scores = engine.classify_symbol(file_path, symbol_name, &cat_strs)?;
    Ok((
        json!({"symbol": symbol_name, "file": file_path, "classifications": scores}),
        tools::success_meta(crate::protocol::BackendKind::Semantic, 0.75),
    ))
}

#[cfg(feature = "semantic")]
fn find_misplaced_code_handler(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let project = state.project();
    let engine = state
        .embedding
        .get_or_init(|| EmbeddingEngine::new(&project).ok())
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;

    let outliers = engine.find_misplaced_code(max_results)?;
    Ok((
        json!({"outliers": outliers, "count": outliers.len()}),
        tools::success_meta(crate::protocol::BackendKind::Semantic, 0.70),
    ))
}

// ── Budget hint (TALE-inspired) ────────────────────────────────────────

fn budget_hint(tool_name: &str, tokens: usize, budget: usize) -> String {
    // Overview/structure tools → always suggest drilling deeper
    if matches!(
        tool_name,
        "get_project_structure" | "get_symbols_overview" | "get_current_config" | "onboard_project"
    ) {
        return "overview complete — drill into specific files or symbols".to_owned();
    }
    // Over budget → strongly suggest narrowing
    if tokens > budget {
        return format!(
            "response ({tokens} tokens) exceeds budget ({budget}) — narrow with path filter or max_tokens"
        );
    }
    // Large relative to budget → suggest narrowing
    if tokens > budget * 3 / 4 {
        return format!("near budget ({tokens}/{budget} tokens) — consider narrowing scope");
    }
    // Medium → sufficient
    if tokens > 100 {
        return "context sufficient — proceed to edit or analysis".to_owned();
    }
    // Small/empty → suggest broadening
    if tokens < 50 {
        return "minimal results — try broader query or different tool".to_owned();
    }
    "focused result — ready for next step".to_owned()
}

// ── Static dispatch table ──────────────────────────────────────────────

static DISPATCH_TABLE: LazyLock<HashMap<&'static str, ToolHandler>> = LazyLock::new(|| {
    let m = tools::dispatch_table();
    #[cfg(feature = "semantic")]
    let mut m = m;
    #[cfg(feature = "semantic")]
    {
        m.insert("semantic_search", |s, a| semantic_search_handler(s, a));
        m.insert("index_embeddings", |s, a| index_embeddings_handler(s, a));
        m.insert("find_similar_code", |s, a| find_similar_code_handler(s, a));
        m.insert("find_code_duplicates", |s, a| {
            find_code_duplicates_handler(s, a)
        });
        m.insert("classify_symbol", |s, a| classify_symbol_handler(s, a));
        m.insert("find_misplaced_code", |s, a| {
            find_misplaced_code_handler(s, a)
        });
    }
    m
});

// ── Dispatch helpers ───────────────────────────────────────────────────

/// Check surface, namespace, tier, and daemon-mode access.
/// Returns `Ok(())` if the tool is allowed, `Err(CodeLensError)` if blocked.
fn validate_tool_access(
    name: &str,
    arguments: &serde_json::Value,
    surface: ToolSurface,
    state: &AppState,
) -> Result<(), CodeLensError> {
    let active_surface = surface.as_label();

    if !is_tool_in_surface(name, surface) {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is not available in active surface `{active_surface}`"
        )));
    }

    if !is_deferred_namespace_access_allowed(name, arguments, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in namespace `{}`. Call `tools/list` with `{{\"namespace\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_namespace(name),
            tool_namespace(name)
        )));
    }

    if !is_deferred_tier_access_allowed(name, arguments, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in tier `{}`. Call `tools/list` with `{{\"tier\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_tier_label(name),
            tool_tier_label(name)
        )));
    }

    let session_trusted_client = arguments
        .get("_session_trusted_client")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if is_content_mutation_tool(name)
        && matches!(
            state.daemon_mode(),
            crate::state::RuntimeDaemonMode::MutationEnabled
        )
        && !session_trusted_client
    {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` requires a trusted HTTP client in daemon mode `{}`",
            state.daemon_mode().as_str()
        )));
    }

    if is_content_mutation_tool(name) && !state.mutation_allowed_in_runtime() {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is blocked by daemon mode `{}`",
            state.daemon_mode().as_str()
        )));
    }

    if is_read_only_surface(surface) && is_content_mutation_tool(name) {
        return Err(CodeLensError::Validation(format!(
            "Tool `{name}` is blocked in read-only surface `{active_surface}`"
        )));
    }

    Ok(())
}

/// Assemble a successful tool response with suggestions, budget, truncation, metrics.
fn build_success_response(
    name: &str,
    payload: serde_json::Value,
    meta: ToolResponseMeta,
    state: &AppState,
    surface: ToolSurface,
    active_surface: &str,
    arguments: &serde_json::Value,
    gate_allowance: Option<&MutationGateAllowance>,
    compact: bool,
    harness_phase: Option<&str>,
    start: std::time::Instant,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let elapsed_ms = start.elapsed().as_millis();

    // Record preflight for verifier tools
    if is_verifier_source_tool(name) {
        state.record_recent_preflight_from_payload(
            name,
            active_surface,
            logical_session_id(arguments),
            arguments,
            &payload,
        );
    }

    // Record caution metric
    if gate_allowance.map(|a| a.caution) == Some(true) {
        state.metrics().record_mutation_with_caution();
    }

    // Output schema / structured content handling
    let has_output_schema = tool_definition(name)
        .and_then(|tool| tool.output_schema.as_ref())
        .is_some();
    let mut structured_content = has_output_schema.then(|| payload.clone());

    let mut resp = ToolCallResponse::success(payload, meta);

    // suggested_next_tools
    resp.suggested_next_tools =
        tools::suggest_next_contextual(name, &state.recent_tools(), harness_phase);

    // token estimate + budget hint
    let payload_estimate = serde_json::to_string(&resp.data)
        .map(|s| tools::estimate_tokens(&s))
        .unwrap_or(0);
    resp.token_estimate = Some(payload_estimate);
    let budget = request_token_budget();
    resp.budget_hint = Some(budget_hint(name, payload_estimate, budget));
    resp.elapsed_ms = Some(elapsed_ms as u64);

    // Composite guidance
    let mut emitted_composite_guidance = false;
    if let Some((guided_tools, guidance_hint)) =
        tools::composite_guidance_for_chain(name, &state.recent_tools(), surface)
    {
        emitted_composite_guidance = true;
        let mut suggestions = guided_tools;
        if let Some(existing) = resp.suggested_next_tools.take() {
            for tool in existing {
                if suggestions.len() >= 3 {
                    break;
                }
                if !suggestions.iter().any(|candidate| candidate == &tool) {
                    suggestions.push(tool);
                }
            }
        }
        resp.suggested_next_tools = Some(suggestions);
        resp.budget_hint = Some(match resp.budget_hint.take() {
            Some(existing) => format!("{existing} {guidance_hint}"),
            None => guidance_hint,
        });
    }

    // Strip non-essential fields in compact mode (saves ~300 tokens for harness evaluators)
    if compact {
        if let Some(ref mut data) = resp.data {
            if let Some(obj) = data.as_object_mut() {
                obj.remove("quality_focus");
                obj.remove("recommended_checks");
                obj.remove("performance_watchpoints");
                obj.remove("available_sections");
                obj.remove("evidence_handles");
                obj.remove("schema_version");
                obj.remove("report_kind");
                obj.remove("profile");
            }
        }
    }

    let mut text = serde_json::to_string(&resp)
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned());

    // Truncation safety net: replace oversized responses with a valid JSON summary.
    let max_chars = budget * 8; // 2x budget in chars
    let mut truncated = false;
    if text.len() > max_chars {
        truncated = true;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(summarize_structured_content(existing, 0));
        }
        text = serde_json::to_string(&json!({
            "success": true,
            "truncated": true,
            "error": format!(
                "Response too large ({} tokens, budget {}). Narrow with path, max_tokens, or depth.",
                payload_estimate, budget
            ),
            "token_estimate": payload_estimate,
        }))
        .unwrap_or_else(|_| "{\"success\":false,\"truncated\":true}".to_owned());
    }

    // Metrics recording
    state.metrics().record_call_with_tokens(
        name,
        elapsed_ms as u64,
        true,
        payload_estimate,
        active_surface,
        truncated,
    );
    if emitted_composite_guidance {
        state.metrics().record_composite_guidance_emitted();
    }

    // Final JSON-RPC response
    let mut result = json!({
        "content": [{ "type": "text", "text": text }]
    });
    if let Some(structured_content) = structured_content {
        result["structuredContent"] = structured_content;
    }
    JsonRpcResponse::result(id, result)
}

/// Assemble an error response with gate failure details if applicable.
fn build_error_response(
    name: &str,
    error: CodeLensError,
    gate_failure: Option<MutationGateFailure>,
    active_surface: &str,
    state: &AppState,
    start: std::time::Instant,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let elapsed_ms = start.elapsed().as_millis();

    // Error metrics recording
    state.metrics().record_call_with_tokens(
        name,
        elapsed_ms as u64,
        false,
        0,
        active_surface,
        false,
    );

    // Protocol-level errors: return as JSON-RPC error response
    if error.is_protocol_error() {
        return JsonRpcResponse::error(id, error.jsonrpc_code(), error.to_string());
    }

    // Tool execution errors: return as MCP isError content
    let mut resp = ToolCallResponse::error(error.to_string());
    if let Some(failure) = gate_failure {
        let analysis_hint = failure
            .analysis_id
            .as_ref()
            .map(|analysis_id| format!(" Last related analysis_id: `{analysis_id}`."))
            .unwrap_or_default();
        resp.error = Some(format!("{}{}", failure.message, analysis_hint));
        resp.suggested_next_tools = Some(failure.suggested_next_tools);
        resp.budget_hint = Some(failure.budget_hint);
    }
    let text = serde_json::to_string(&resp)
        .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned());
    JsonRpcResponse::result(
        id,
        json!({
            "content": [{ "type": "text", "text": text }],
            "isError": true
        }),
    )
}

// ── Dispatch entry point ───────────────────────────────────────────────

pub(crate) fn dispatch_tool(
    state: &AppState,
    id: Option<serde_json::Value>,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|value| value.as_str()) else {
        return JsonRpcResponse::error(id, -32602, "Missing tool name");
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // 1. Extract params (_profile, _compact, _harness_phase)
    let request_budget = arguments
        .get("_profile")
        .and_then(|v| v.as_str())
        .map(|profile| {
            ToolProfile::from_str(profile)
                .map(default_budget_for_profile)
                .unwrap_or_else(|| match profile {
                    "fast_local" => 2000usize,
                    "deep_semantic" => 16000,
                    "safe_mutation" => 4000,
                    _ => state.token_budget(),
                })
        })
        .unwrap_or_else(|| state.token_budget());
    REQUEST_BUDGET.set(request_budget);

    let compact = arguments
        .get("_compact")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let harness_phase = arguments
        .get("_harness_phase")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());

    let span = info_span!("tool_call", tool = name);
    let _guard = span.enter();
    let start = std::time::Instant::now();
    state.push_recent_tool(name);
    let surface = *state.surface();
    let active_surface = surface.as_label().to_owned();

    // 2. Validate tool access (surface, namespace, tier, daemon mode)
    if let Err(access_err) = validate_tool_access(name, &arguments, surface, state) {
        return build_error_response(name, access_err, None, &active_surface, state, start, id);
    }

    // 3. Mutation gate check + 4. Execute tool via DISPATCH_TABLE
    let mut gate_allowance: Option<MutationGateAllowance> = None;
    let mut gate_failure: Option<MutationGateFailure> = None;

    let result: ToolResult = if is_refactor_gated_mutation_tool(name) {
        state.metrics().record_mutation_preflight_checked();
        match evaluate_mutation_gate(state, name, &arguments, surface) {
            Ok(allowance) => {
                gate_allowance = allowance;
                match DISPATCH_TABLE.get(name) {
                    Some(handler) => handler(state, &arguments),
                    None => Err(CodeLensError::ToolNotFound(name.to_owned())),
                }
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
                gate_failure = Some(failure);
                Err(CodeLensError::Validation(
                    gate_failure
                        .as_ref()
                        .map(|f| f.message.clone())
                        .unwrap_or_else(|| "mutation preflight rejected".to_owned()),
                ))
            }
        }
    } else {
        match DISPATCH_TABLE.get(name) {
            Some(handler) => handler(state, &arguments),
            None => Err(CodeLensError::ToolNotFound(name.to_owned())),
        }
    };

    // 5. Post-mutation side effects (graph invalidation, audit)
    if result.is_ok() && is_content_mutation_tool(name) {
        state.graph_cache().invalidate();
        if let Err(error) = state.record_mutation_audit(name, &active_surface, &arguments) {
            warn!(tool = name, error = %error, "failed to write mutation audit event");
        }
        let session_id = arguments
            .get("_session_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if !session_id.is_empty() {
            tracing::info!(
                tool = name,
                session_id,
                "mutation completed for trusted session"
            );
        }
    }

    let elapsed_ms = start.elapsed().as_millis();
    if elapsed_ms > 5000 {
        warn!(
            tool = name,
            elapsed_ms = elapsed_ms as u64,
            "slow tool execution"
        );
    }

    // 6. Build response
    match result {
        Ok((payload, meta)) => build_success_response(
            name,
            payload,
            meta,
            state,
            surface,
            &active_surface,
            &arguments,
            gate_allowance.as_ref(),
            compact,
            harness_phase.as_deref(),
            start,
            id,
        ),
        Err(error) => {
            build_error_response(name, error, gate_failure, &active_surface, state, start, id)
        }
    }
}
