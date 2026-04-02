//! Tool dispatch: static dispatch table + JSON-RPC tool call routing.

use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::{JsonRpcResponse, ToolCallResponse};
use crate::tool_defs::{
    ToolProfile, default_budget_for_profile, is_content_mutation_tool, is_read_only_surface,
    is_tool_in_surface,
};
use crate::tools::{self, ToolHandler, ToolResult};
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

    // Request-scoped profile: set per-request budget via thread-local
    // (avoids race condition when multiple HTTP requests run concurrently)
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

    let span = info_span!("tool_call", tool = name);
    let _guard = span.enter();
    let start = std::time::Instant::now();
    state.push_recent_tool(name);
    let surface = *state.surface();
    let active_surface = surface.as_label().to_owned();
    let session_trusted_client = arguments
        .get("_session_trusted_client")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let session_id = arguments
        .get("_session_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    let result: ToolResult = if !is_tool_in_surface(name, surface) {
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` is not available in active surface `{active_surface}`"
        )))
    } else if is_content_mutation_tool(name)
        && matches!(state.daemon_mode(), crate::state::RuntimeDaemonMode::MutationEnabled)
        && !session_trusted_client
    {
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` requires a trusted HTTP client in daemon mode `{}`",
            state.daemon_mode().as_str()
        )))
    } else if is_content_mutation_tool(name) && !state.mutation_allowed_in_runtime() {
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` is blocked by daemon mode `{}`",
            state.daemon_mode().as_str()
        )))
    } else if is_read_only_surface(surface) && is_content_mutation_tool(name) {
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` is blocked in read-only surface `{active_surface}`"
        )))
    } else {
        match DISPATCH_TABLE.get(name) {
            Some(handler) => handler(state, &arguments),
            None => Err(CodeLensError::ToolNotFound(name.to_owned())),
        }
    };

    if result.is_ok() && is_content_mutation_tool(name) {
        state.graph_cache().invalidate();
        if let Err(error) = state.record_mutation_audit(name, &active_surface, &arguments) {
            warn!(tool = name, error = %error, "failed to write mutation audit event");
        }
        if !session_id.is_empty() {
            tracing::info!(tool = name, session_id, "mutation completed for trusted session");
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

    match result {
        Ok((payload, meta)) => {
            let mut resp = ToolCallResponse::success(payload, meta);
            resp.suggested_next_tools = tools::suggest_next_contextual(name, &state.recent_tools());
            let payload_estimate = serde_json::to_string(&resp.data)
                .map(|s| tools::estimate_tokens(&s))
                .unwrap_or(0);
            resp.token_estimate = Some(payload_estimate);
            let budget = request_token_budget();
            resp.budget_hint = Some(budget_hint(name, payload_estimate, budget));
            resp.elapsed_ms = Some(elapsed_ms as u64);
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

            let mut text = serde_json::to_string(&resp).unwrap_or_else(|_| {
                "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
            });

            // Global safety net: replace oversized responses with a valid JSON summary.
            // This prevents any single tool from blowing up the context window.
            let max_chars = budget * 8; // 2x budget in chars
            let mut truncated = false;
            if text.len() > max_chars {
                truncated = true;
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
            state.metrics().record_call_with_tokens(
                name,
                elapsed_ms as u64,
                true,
                payload_estimate,
                &active_surface,
                truncated,
            );
            if emitted_composite_guidance {
                state.metrics().record_composite_guidance_emitted();
            }
            JsonRpcResponse::result(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }]
                }),
            )
        }
        Err(error) => {
            state.metrics().record_call_with_tokens(
                name,
                elapsed_ms as u64,
                false,
                0,
                &active_surface,
                false,
            );
            // Protocol-level errors: return as JSON-RPC error response
            if error.is_protocol_error() {
                return JsonRpcResponse::error(id, error.jsonrpc_code(), error.to_string());
            }
            // Tool execution errors: return as MCP isError content
            let resp = ToolCallResponse::error(error.to_string());
            let text = serde_json::to_string(&resp).unwrap_or_else(|_| {
                "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
            });
            JsonRpcResponse::result(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": true
                }),
            )
        }
    }
}
