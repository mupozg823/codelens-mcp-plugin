//! Tool dispatch: static dispatch table + JSON-RPC tool call routing.

use crate::dispatch_access::validate_tool_access;
use crate::dispatch_response::{
    build_error_response, build_success_response, SuccessResponseInput,
};
use crate::error::CodeLensError;
use crate::mutation_gate::{
    evaluate_mutation_gate, is_refactor_gated_mutation_tool, MutationGateAllowance,
    MutationGateFailure,
};
use crate::protocol::JsonRpcResponse;
use crate::tool_defs::{default_budget_for_profile, is_content_mutation_tool, ToolProfile};
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

/// Normalized tool call request — extracted from raw JSON-RPC params.
pub(crate) struct ToolCallEnvelope {
    pub name: String,
    pub arguments: serde_json::Value,
    pub session: crate::session_context::SessionRequestContext,
    pub budget: usize,
    pub compact: bool,
    pub harness_phase: Option<String>,
}

impl ToolCallEnvelope {
    /// Parse raw JSON-RPC params into a normalized envelope.
    pub fn parse(
        params: &serde_json::Value,
        state: &AppState,
    ) -> Result<Self, (&'static str, i64)> {
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or(("Missing tool name", -32602i64))?
            .to_owned();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let session = crate::session_context::SessionRequestContext::from_json(&arguments);
        let budget = arguments
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
        let compact = arguments
            .get("_compact")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let harness_phase = arguments
            .get("_harness_phase")
            .and_then(|v| v.as_str())
            .map(|s| s.to_owned());
        Ok(Self {
            name,
            arguments,
            session,
            budget,
            compact,
            harness_phase,
        })
    }
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

    let semantic_query = crate::tools::symbols::semantic_query_for_retrieval(query);
    let expanded_query = crate::tools::symbols::expanded_query_for_retrieval(query);

    // Structural boosting: find name-matching candidates from SymbolIndex
    // and boost semantic results that overlap with structural hits.
    let structural_names: std::collections::HashSet<String> = state
        .symbol_index()
        .get_ranked_context(&expanded_query, None, 4000, false, 2)
        .map(|rc| {
            rc.symbols
                .into_iter()
                .map(|s| format!("{}:{}", s.file, s.name))
                .collect()
        })
        .unwrap_or_default();

    let candidate_limit = max_results.saturating_mul(4).clamp(max_results, 80);
    let mut results =
        crate::tools::symbols::semantic_results_for_query(state, query, candidate_limit, false);

    // Apply structural boost: +0.06 for results that also appear in structural candidates
    for result in &mut results {
        let key = format!("{}:{}", result.file_path, result.symbol_name);
        if structural_names.contains(&key) {
            result.score += 0.06;
        }
    }
    // Re-sort after boosting and truncate
    results = crate::tools::symbols::rerank_semantic_matches(query, results, max_results);

    let result_scores = results
        .iter()
        .map(|result| {
            let (prior_delta, adjusted_score) =
                crate::tools::symbols::semantic_adjusted_score_parts(query, result);
            (
                (prior_delta * 1000.0).round() / 1000.0,
                (adjusted_score * 1000.0).round() / 1000.0,
            )
        })
        .collect::<Vec<_>>();
    let mut payload = json!({
        "query": query,
        "results": results,
        "count": results.len(),
        "retrieval": {
            "semantic_enabled": true,
            "requested_query": query,
            "semantic_query": semantic_query,
        }
    });
    if let Some(entries) = payload
        .get_mut("results")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (idx, entry) in entries.iter_mut().enumerate() {
            if let Some(map) = entry.as_object_mut() {
                let (prior_delta, adjusted_score) =
                    result_scores.get(idx).copied().unwrap_or((0.0, 0.0));
                map.insert(
                    "provenance".to_owned(),
                    json!({
                        "source": "semantic",
                        "retrieval_rank": idx + 1,
                        "prior_delta": prior_delta,
                        "adjusted_score": adjusted_score,
                    }),
                );
            }
        }
    }
    Ok((
        payload,
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

// ── Static dispatch table ──────────────────────────────────────────────

static DISPATCH_TABLE: LazyLock<HashMap<&'static str, ToolHandler>> = LazyLock::new(|| {
    let m = tools::dispatch_table();
    #[cfg(feature = "semantic")]
    let mut m = m;
    #[cfg(feature = "semantic")]
    {
        m.insert("semantic_search", semantic_search_handler);
        m.insert("index_embeddings", index_embeddings_handler);
        m.insert("find_similar_code", find_similar_code_handler);
        m.insert("find_code_duplicates", |s, a| {
            find_code_duplicates_handler(s, a)
        });
        m.insert("classify_symbol", classify_symbol_handler);
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
    // 1. Parse and normalize request
    let envelope = match ToolCallEnvelope::parse(&params, state) {
        Ok(env) => env,
        Err((msg, code)) => return JsonRpcResponse::error(id, code, msg),
    };
    let name = envelope.name.as_str();
    let arguments = &envelope.arguments;
    let session = &envelope.session;
    let compact = envelope.compact;
    let harness_phase = envelope.harness_phase;
    REQUEST_BUDGET.set(envelope.budget);

    let span = info_span!("tool_call", tool = name);
    let _guard = span.enter();
    let start = std::time::Instant::now();
    state.push_recent_tool(name);

    // Doom-loop detection: hash arguments, check consecutive repeat count
    let args_hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        arguments.to_string().hash(&mut hasher);
        hasher.finish()
    };
    let doom_count = state.doom_loop_count(name, args_hash);

    // Track file access for session-aware ranking boost
    if let Some(fp) = arguments
        .get("file_path")
        .or_else(|| arguments.get("path"))
        .or_else(|| arguments.get("relative_path"))
        .and_then(|v| v.as_str())
    {
        state.record_file_access(fp);
    }
    let surface = *state.surface();
    let active_surface = surface.as_label().to_owned();

    // 2. Validate tool access (surface, namespace, tier, daemon mode)
    if let Err(access_err) = validate_tool_access(name, session, surface, state) {
        return build_error_response(name, access_err, None, &active_surface, state, start, id);
    }

    // 3. Mutation gate check + 4. Execute tool via DISPATCH_TABLE
    let mut gate_allowance: Option<MutationGateAllowance> = None;
    let mut gate_failure: Option<MutationGateFailure> = None;

    let result: ToolResult = if is_refactor_gated_mutation_tool(name) {
        state.metrics().record_mutation_preflight_checked();
        match evaluate_mutation_gate(state, name, session, surface, arguments) {
            Ok(allowance) => {
                gate_allowance = allowance;
                match DISPATCH_TABLE.get(name) {
                    Some(handler) => handler(state, arguments),
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
            Some(handler) => handler(state, arguments),
            None => Err(CodeLensError::ToolNotFound(name.to_owned())),
        }
    };

    // 5. Post-mutation side effects (graph invalidation, audit, incremental reindex)
    if result.is_ok() && is_content_mutation_tool(name) {
        state.graph_cache().invalidate();
        // Incremental reindex: refresh symbol DB + embedding index for the mutated file
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
                let configured_model = codelens_core::configured_embedding_model_name();
                let embeddings_active = state
                    .embedding
                    .get()
                    .and_then(|engine| engine.as_ref())
                    .is_some_and(|engine| engine.is_indexed());
                let on_disk_index_exists = EmbeddingEngine::inspect_existing_index(&project)
                    .ok()
                    .flatten()
                    .is_some_and(|info| {
                        info.model_name == configured_model && info.indexed_symbols > 0
                    });
                if embeddings_active || on_disk_index_exists {
                    if let Some(engine) = state
                        .embedding
                        .get_or_init(|| EmbeddingEngine::new(&project).ok())
                    {
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
        if let Err(error) = state.record_mutation_audit(name, &active_surface, arguments, session) {
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

    let elapsed_ms = start.elapsed().as_millis();
    if elapsed_ms > 5000 {
        warn!(
            tool = name,
            elapsed_ms = elapsed_ms as u64,
            "slow tool execution"
        );
    }

    // 6. Build response
    if doom_count >= 3 {
        tracing::warn!(
            tool = name,
            repeat_count = doom_count,
            "doom-loop detected: same tool+args called {} times consecutively",
            doom_count
        );
    }
    match result {
        Ok((payload, meta)) => build_success_response(SuccessResponseInput {
            doom_loop_count: doom_count,
            name,
            payload,
            meta,
            state,
            surface,
            active_surface: &active_surface,
            arguments: arguments,
            logical_session_id: &session.session_id,
            gate_allowance: gate_allowance.as_ref(),
            compact,
            harness_phase: harness_phase.as_deref(),
            request_budget: envelope.budget,
            start,
            id,
        }),
        Err(error) => {
            build_error_response(name, error, gate_failure, &active_surface, state, start, id)
        }
    }
}

#[cfg(all(test, feature = "semantic"))]
mod semantic_tests {
    use crate::tools::symbols::rerank_semantic_matches;
    use codelens_core::SemanticMatch;

    fn semantic_match(file_path: &str, symbol_name: &str, kind: &str, score: f64) -> SemanticMatch {
        SemanticMatch {
            file_path: file_path.to_owned(),
            symbol_name: symbol_name.to_owned(),
            kind: kind.to_owned(),
            line: 1,
            signature: String::new(),
            name_path: symbol_name.to_owned(),
            score,
        }
    }

    #[test]
    fn prefers_extract_entrypoint_over_script_variables() {
        let reranked = rerank_semantic_matches(
            "extract lines of code into a new function",
            vec![
                semantic_match(
                    "scripts/finetune/build_codex_dataset.py",
                    "line",
                    "variable",
                    0.233,
                ),
                semantic_match(
                    "benchmarks/harness/task-bootstrap.py",
                    "lines",
                    "variable",
                    0.219,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/tools/composite.rs",
                    "refactor_extract_function",
                    "function",
                    0.184,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "refactor_extract_function");
    }

    #[test]
    fn prefers_dispatch_entrypoint_over_handler_types() {
        let reranked = rerank_semantic_matches(
            "route an incoming tool request to the right handler",
            vec![
                semantic_match(
                    "crates/codelens-mcp/src/tools/mod.rs",
                    "ToolHandler",
                    "unknown",
                    0.313,
                ),
                semantic_match(
                    "benchmarks/harness/harness_runner_common.py",
                    "tool_list",
                    "variable",
                    0.266,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/dispatch.rs",
                    "dispatch_tool",
                    "function",
                    0.224,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "dispatch_tool");
    }

    #[test]
    fn prefers_stdio_entrypoint_over_generic_read_helpers() {
        let reranked = rerank_semantic_matches(
            "read input from stdin line by line run_stdio stdio stdin",
            vec![
                semantic_match(
                    "crates/codelens-core/src/file_ops/mod.rs",
                    "read_line_at",
                    "function",
                    0.261,
                ),
                semantic_match(
                    "crates/codelens-core/src/file_ops/reader.rs",
                    "read_file",
                    "function",
                    0.258,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/server/transport_stdio.rs",
                    "run_stdio",
                    "function",
                    0.148,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "run_stdio");
    }

    #[test]
    fn prefers_mutation_gate_entrypoint_over_telemetry_helpers() {
        let reranked = rerank_semantic_matches(
            "mutation gate preflight check before editing evaluate_mutation_gate mutation_gate preflight",
            vec![
                semantic_match(
                    "crates/codelens-mcp/src/telemetry.rs",
                    "record_mutation_preflight_checked",
                    "function",
                    0.402,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/telemetry.rs",
                    "record_mutation_preflight_gate_denied",
                    "function",
                    0.314,
                ),
                semantic_match(
                    "crates/codelens-mcp/src/mutation_gate.rs",
                    "evaluate_mutation_gate",
                    "function",
                    0.280,
                ),
            ],
            3,
        );

        assert_eq!(reranked[0].symbol_name, "evaluate_mutation_gate");
    }

    #[test]
    fn expands_stdio_alias_terms() {
        let expanded = crate::tools::symbols::expanded_query_for_retrieval(
            "read input from stdin line by line",
        );
        assert!(expanded.contains("run_stdio"));
        assert!(expanded.contains("stdio"));
    }
}
