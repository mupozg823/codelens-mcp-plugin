//! Tool dispatch: static dispatch table + JSON-RPC tool call routing.

use crate::error::CodeLensError;
use crate::protocol::{JsonRpcResponse, ToolCallResponse};
use crate::tool_defs::{
    default_budget_for_profile, is_content_mutation_tool, is_read_only_surface, is_tool_in_surface,
    preferred_namespaces, preferred_tier_labels, tool_definition, tool_namespace, tool_tier_label,
    ToolProfile,
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

#[derive(Clone)]
struct MutationGateAllowance {
    caution: bool,
}

struct MutationGateFailure {
    message: String,
    analysis_id: Option<String>,
    suggested_next_tools: Vec<String>,
    budget_hint: String,
    stale: bool,
    rename_without_symbol_preflight: bool,
    missing_preflight: bool,
}

fn logical_session_id(arguments: &serde_json::Value) -> &str {
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
    surface: crate::tool_defs::ToolSurface,
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
    surface: crate::tool_defs::ToolSurface,
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn is_verifier_source_tool(name: &str) -> bool {
    matches!(
        name,
        "verify_change_readiness"
            | "safe_rename_report"
            | "unresolved_reference_check"
            | "refactor_safety_report"
    )
}

fn is_refactor_gated_mutation_tool(name: &str) -> bool {
    matches!(
        name,
        "rename_symbol"
            | "replace_symbol_body"
            | "delete_lines"
            | "insert_at_line"
            | "insert_before_symbol"
            | "insert_after_symbol"
            | "insert_content"
            | "replace_content"
            | "replace_lines"
            | "replace"
            | "create_text_file"
            | "add_import"
            | "refactor_extract_function"
            | "refactor_inline_function"
            | "refactor_move_to_file"
            | "refactor_change_signature"
    )
}

fn is_symbol_aware_mutation_tool(name: &str) -> bool {
    matches!(name, "rename_symbol")
}

fn mutation_gate_failure(
    name: &str,
    reason: impl Into<String>,
    analysis_id: Option<String>,
    stale: bool,
    rename_without_symbol_preflight: bool,
    missing_preflight: bool,
) -> MutationGateFailure {
    let suggested_next_tools = if is_symbol_aware_mutation_tool(name) {
        vec![
            "safe_rename_report".to_owned(),
            "unresolved_reference_check".to_owned(),
            "get_analysis_section".to_owned(),
        ]
    } else {
        vec![
            "verify_change_readiness".to_owned(),
            "get_analysis_section".to_owned(),
            "get_file_diagnostics".to_owned(),
        ]
    };
    let budget_hint = if is_symbol_aware_mutation_tool(name) {
        "Run symbol-aware preflight before rename, then expand evidence if the target is ambiguous."
            .to_owned()
    } else {
        "Run preflight first, then expand verifier evidence before mutation.".to_owned()
    };
    MutationGateFailure {
        message: reason.into(),
        analysis_id,
        suggested_next_tools,
        budget_hint,
        stale,
        rename_without_symbol_preflight,
        missing_preflight,
    }
}

fn evaluate_mutation_gate(
    state: &AppState,
    name: &str,
    arguments: &serde_json::Value,
    surface: crate::tool_defs::ToolSurface,
) -> Result<Option<MutationGateAllowance>, MutationGateFailure> {
    if !matches!(
        surface,
        crate::tool_defs::ToolSurface::Profile(ToolProfile::RefactorFull)
    ) || !is_refactor_gated_mutation_tool(name)
    {
        return Ok(None);
    }

    let logical_session = logical_session_id(arguments);
    let Some(preflight) = state.recent_preflight(logical_session) else {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` requires a fresh preflight in `refactor-full`. Run `verify_change_readiness` first."
            ),
            None,
            false,
            false,
            true,
        ));
    };

    if now_ms().saturating_sub(preflight.timestamp_ms) > crate::state::PREFLIGHT_TTL_MS {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked because the last `{}` preflight from surface `{}` is stale. Re-run verifier tools within {} seconds before mutating.",
                preflight.tool_name,
                preflight.surface,
                state.preflight_ttl_seconds()
            ),
            preflight.analysis_id.clone(),
            true,
            false,
            false,
        ));
    }

    let mutation_paths = state.extract_target_paths(arguments);
    if mutation_paths.is_empty() {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked because no mutation target path was provided for preflight matching."
            ),
            preflight.analysis_id.clone(),
            false,
            is_symbol_aware_mutation_tool(name),
            false,
        ));
    }
    let path_overlap = mutation_paths
        .iter()
        .any(|path| preflight.target_paths.iter().any(|target| target == path));
    if !path_overlap {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked because the recent preflight does not cover the requested target paths."
            ),
            preflight.analysis_id.clone(),
            false,
            false,
            false,
        ));
    }

    if is_symbol_aware_mutation_tool(name) {
        if !matches!(
            preflight.tool_name.as_str(),
            "safe_rename_report" | "unresolved_reference_check"
        ) {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` requires a symbol-aware preflight. Run `safe_rename_report` or `unresolved_reference_check` first."
                ),
                preflight.analysis_id.clone(),
                false,
                true,
                false,
            ));
        }
        let Some(mutation_symbol) = state.extract_symbol_hint(arguments) else {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` requires an exact symbol hint plus symbol-aware preflight evidence."
                ),
                preflight.analysis_id.clone(),
                false,
                true,
                false,
            ));
        };
        if preflight
            .symbol
            .as_deref()
            .map(|symbol| symbol != mutation_symbol)
            .unwrap_or(true)
        {
            return Err(mutation_gate_failure(
                name,
                format!(
                    "Tool `{name}` is blocked because the symbol-aware preflight does not match `{mutation_symbol}`."
                ),
                preflight.analysis_id.clone(),
                false,
                true,
                false,
            ));
        }
    }

    if preflight.readiness.mutation_ready == "blocked" {
        return Err(mutation_gate_failure(
            name,
            format!(
                "Tool `{name}` is blocked by verifier readiness. The last `{}` preflight reported {} blocker(s); resolve them before mutation.",
                preflight.tool_name, preflight.blocker_count
            ),
            preflight.analysis_id.clone(),
            false,
            false,
            false,
        ));
    }

    Ok(Some(MutationGateAllowance {
        caution: preflight.readiness.mutation_ready == "caution",
    }))
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
    let session_trusted_client = arguments
        .get("_session_trusted_client")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let session_id = arguments
        .get("_session_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let mut gate_allowance: Option<MutationGateAllowance> = None;
    let mut gate_failure: Option<MutationGateFailure> = None;

    let result: ToolResult = if !is_tool_in_surface(name, surface) {
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` is not available in active surface `{active_surface}`"
        )))
    } else if !is_deferred_namespace_access_allowed(name, &arguments, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in namespace `{}`. Call `tools/list` with `{{\"namespace\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_namespace(name),
            tool_namespace(name)
        )))
    } else if !is_deferred_tier_access_allowed(name, &arguments, surface) {
        state.metrics().record_deferred_hidden_tool_call_denied();
        Err(CodeLensError::Validation(format!(
            "Tool `{name}` is hidden by deferred loading in tier `{}`. Call `tools/list` with `{{\"tier\":\"{}\"}}` or `{{\"full\":true}}` first.",
            tool_tier_label(name),
            tool_tier_label(name)
        )))
    } else if is_content_mutation_tool(name)
        && matches!(
            state.daemon_mode(),
            crate::state::RuntimeDaemonMode::MutationEnabled
        )
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
    } else if is_refactor_gated_mutation_tool(name) {
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
                        .map(|failure| failure.message.clone())
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

    if result.is_ok() && is_content_mutation_tool(name) {
        state.graph_cache().invalidate();
        if let Err(error) = state.record_mutation_audit(name, &active_surface, &arguments) {
            warn!(tool = name, error = %error, "failed to write mutation audit event");
        }
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

    match result {
        Ok((payload, meta)) => {
            if is_verifier_source_tool(name) {
                state.record_recent_preflight_from_payload(
                    name,
                    &active_surface,
                    logical_session_id(&arguments),
                    &arguments,
                    &payload,
                );
            }
            if gate_allowance.as_ref().map(|allowance| allowance.caution) == Some(true) {
                state.metrics().record_mutation_with_caution();
            }
            let has_output_schema = tool_definition(name)
                .and_then(|tool| tool.output_schema.as_ref())
                .is_some();
            let mut structured_content = has_output_schema.then(|| payload.clone());
            let mut resp = ToolCallResponse::success(payload, meta);
            resp.suggested_next_tools = tools::suggest_next_contextual(
                name,
                &state.recent_tools(),
                harness_phase.as_deref(),
            );
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

            let mut text = serde_json::to_string(&resp).unwrap_or_else(|_| {
                "{\"success\":false,\"error\":\"serialization failed\"}".to_owned()
            });

            // Global safety net: replace oversized responses with a valid JSON summary.
            // This prevents any single tool from blowing up the context window.
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
            let mut result = json!({
                "content": [{ "type": "text", "text": text }]
            });
            if let Some(structured_content) = structured_content {
                result["structuredContent"] = structured_content;
            }
            JsonRpcResponse::result(id, result)
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
