use crate::AppState;
use crate::tool_defs::ToolSurface;
use serde_json::{Value, json};

#[cfg(feature = "semantic")]
use crate::tool_defs::is_tool_in_surface;

/// Four-way decomposition of why `semantic_search` might not be
/// currently runnable. Phase 4a, §capability-reporting: the previous
/// single reason string "embeddings not loaded — call
/// index_embeddings first" conflated four distinct root causes, the
/// only one of which the user could actually act on was
/// `index_missing`. This enum keeps them separate so the caller can
/// suggest the right remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticSearchStatus {
    /// The `semantic_search` handler is reachable, either because the
    /// engine is already loaded in memory or because an on-disk index
    /// exists and the engine will be lazy-initialized on first call.
    #[cfg(feature = "semantic")]
    Available,
    /// The bundled CodeSearchNet ONNX model file is missing or
    /// corrupt. User remediation: reinstall with a binary that ships
    /// the model, or set `SYMBIOTE_MODEL_DIR` / `CODELENS_MODEL_DIR`.
    #[cfg(feature = "semantic")]
    ModelAssetsUnavailable,
    /// The active tool surface (preset or profile) does not include
    /// `semantic_search`. User remediation: switch profile via
    /// `set_profile` / `set_preset`, or use a client that activates a
    /// richer surface.
    #[cfg(feature = "semantic")]
    NotInActiveSurface,
    /// The on-disk symbol index has no embedding rows yet. User
    /// remediation: call `index_embeddings` to build the index.
    #[cfg(feature = "semantic")]
    IndexMissing,
    /// The binary was built without the `semantic` cargo feature.
    /// User remediation: rebuild with `cargo build --features semantic`.
    ///
    /// Only constructed in the `#[cfg(not(feature = "semantic"))]`
    /// branch of `determine_semantic_search_status`. The default
    /// feature set for this crate enables `semantic`, so under a
    /// normal build this variant is unreachable — `#[allow(dead_code)]`
    /// silences the warning without dropping the variant, which we
    /// still want available for no-feature builds and for
    /// `semantic_search_status_reason_strings_are_distinct` to pin
    /// its reason text.
    #[allow(dead_code)]
    FeatureDisabled,
}

impl SemanticSearchStatus {
    pub(crate) fn status_key(&self) -> &'static str {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => "available",
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => "model_assets_unavailable",
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => "not_in_active_surface",
            #[cfg(feature = "semantic")]
            Self::IndexMissing => "index_missing",
            Self::FeatureDisabled => "feature_disabled",
        }
    }

    pub(crate) fn reason_str(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some(
                "model assets unavailable — reinstall with bundled model or set SYMBIOTE_MODEL_DIR or CODELENS_MODEL_DIR",
            ),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some(
                "not in active surface — call set_profile/set_preset to include semantic_search",
            ),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => {
                Some("index missing — call index_embeddings to build the embedding index")
            }
            Self::FeatureDisabled => {
                Some("feature disabled — rebuild with `cargo build --features semantic`")
            }
        }
    }

    pub(crate) fn reason_code(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some("semantic_model_assets_unavailable"),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some("semantic_not_in_active_surface"),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => Some("semantic_index_missing"),
            Self::FeatureDisabled => Some("semantic_feature_disabled"),
        }
    }

    pub(crate) fn recommended_action(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some("configure_model_assets"),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some("switch_tool_surface"),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => Some("run_index_embeddings"),
            Self::FeatureDisabled => Some("rebuild_with_semantic_feature"),
        }
    }

    pub(crate) fn action_target(&self) -> Option<&'static str> {
        match self {
            #[cfg(feature = "semantic")]
            Self::Available => None,
            #[cfg(feature = "semantic")]
            Self::ModelAssetsUnavailable => Some("model_assets"),
            #[cfg(feature = "semantic")]
            Self::NotInActiveSurface => Some("tool_surface"),
            #[cfg(feature = "semantic")]
            Self::IndexMissing => Some("embedding_index"),
            Self::FeatureDisabled => Some("binary"),
        }
    }

    pub(crate) fn included_in_profiles(&self) -> Option<Vec<&'static str>> {
        #[cfg(feature = "semantic")]
        {
            match self {
                Self::NotInActiveSurface => Some(vec![
                    "planner-readonly",
                    "builder-minimal",
                    "reviewer-graph",
                ]),
                _ => None,
            }
        }
        #[cfg(not(feature = "semantic"))]
        {
            None
        }
    }

    pub(crate) fn guidance_payload(&self) -> serde_json::Value {
        let mut payload = json!({
            "status": self.status_key(),
            "available": self.is_available(),
            "reason": self.reason_str(),
            "reason_code": self.reason_code(),
            "recommended_action": self.recommended_action(),
            "action_target": self.action_target(),
        });
        if let Some(profiles) = self.included_in_profiles() {
            let recommended_profile = profiles.first().copied();
            payload["included_in"] = serde_json::json!(profiles);
            if let Some(first) = recommended_profile {
                payload["recommended_profile"] = serde_json::json!(first);
            }
        }
        payload
    }

    pub(crate) fn is_available(&self) -> bool {
        #[cfg(feature = "semantic")]
        {
            matches!(self, Self::Available)
        }
        #[cfg(not(feature = "semantic"))]
        {
            false
        }
    }
}

/// Compute the current `SemanticSearchStatus` from three observations:
///   1. whether the binary was built with the `semantic` feature,
///   2. whether the CodeSearchNet model assets are on disk,
///   3. whether `semantic_search` is in the active tool surface,
///   4. whether the on-disk symbol-index contains embedding rows.
///
/// The precedence order is deliberately "fix the easiest thing first":
/// feature → model assets → surface → index. A user hitting
/// `FeatureDisabled` must rebuild; a user hitting `IndexMissing` just
/// has to run one tool call.
///
/// **Important (§capability-reporting AC3)**: when the engine is not
/// yet loaded in memory but the on-disk index exists and the surface
/// includes `semantic_search`, the status is `Available` — the actual
/// handler code path calls `state.embedding_engine()` which
/// lazy-initializes the engine under a write lock. Reporting
/// "engine not loaded yet" would be a misleading telemetry-vs-runtime
/// mismatch.
#[cfg(feature = "semantic")]
pub(crate) fn determine_semantic_search_status(
    state: &AppState,
    surface: ToolSurface,
) -> SemanticSearchStatus {
    if !codelens_engine::embedding_model_assets_available() {
        return SemanticSearchStatus::ModelAssetsUnavailable;
    }
    if !is_tool_in_surface("semantic_search", surface) {
        return SemanticSearchStatus::NotInActiveSurface;
    }
    // Check on-disk index status without loading the engine. If the
    // engine is already loaded, `index_info().indexed_symbols` is the
    // authoritative count; otherwise fall back to the on-disk
    // `inspect_existing_index` probe which opens the SQLite file read-only.
    let indexed_count = {
        let guard = state.embedding_ref();
        match guard.as_ref() {
            Some(engine) => engine.index_info().indexed_symbols,
            None => codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
                .ok()
                .flatten()
                .map(|info| info.indexed_symbols)
                .unwrap_or(0),
        }
    };
    if indexed_count == 0 {
        return SemanticSearchStatus::IndexMissing;
    }
    SemanticSearchStatus::Available
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn determine_semantic_search_status(
    _state: &AppState,
    _surface: ToolSurface,
) -> SemanticSearchStatus {
    SemanticSearchStatus::FeatureDisabled
}

pub(crate) fn build_health_summary(
    index_stats: Option<&codelens_engine::IndexStats>,
    semantic_status: &SemanticSearchStatus,
    daemon_binary_drift: &serde_json::Value,
) -> serde_json::Value {
    let indexed_files = index_stats.map(|s| s.indexed_files).unwrap_or(0);
    let supported_files = index_stats.map(|s| s.supported_files).unwrap_or(0);
    let stale_files = index_stats.map(|s| s.stale_files).unwrap_or(0);
    let mut warnings = Vec::new();

    let mut push_warning =
        |code: &str,
         message: String,
         recommended_action: Option<&str>,
         action_target: Option<&str>,
         extras: Option<serde_json::Map<String, Value>>| {
            let mut warning = serde_json::Map::new();
            warning.insert("code".to_owned(), json!(code));
            warning.insert("severity".to_owned(), json!("warn"));
            warning.insert("message".to_owned(), json!(message));
            warning.insert("recommended_action".to_owned(), json!(recommended_action));
            warning.insert("action_target".to_owned(), json!(action_target));
            if let Some(extras) = extras {
                for (key, value) in extras {
                    warning.insert(key, value);
                }
            }
            warnings.push(Value::Object(warning));
        };

    if supported_files == 0 {
        push_warning(
            "no_supported_files",
            "no supported source files detected".to_string(),
            None,
            None,
            None,
        );
    }
    if indexed_files == 0 {
        push_warning(
            "empty_index",
            "symbol index is empty".to_string(),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
            None,
        );
    }
    if supported_files > 0 && indexed_files < supported_files {
        let unindexed = supported_files.saturating_sub(indexed_files);
        // Issue #218: callers (Claude Code agents) need to know not just
        // *that* a refresh is recommended, but exactly which tool to call.
        // `recommended_action` is a verb; `remediation` is the trigger
        // recipe — co-located so the agent does not have to guess at the
        // tool name (refresh_symbol_index vs onboard_project vs
        // activate_project) or its argument shape.
        let extras = json!({
            "indexed_files": indexed_files,
            "supported_files": supported_files,
            "unindexed_files": unindexed,
            "remediation": {
                "method": "tool_call",
                "tool": "refresh_symbol_index",
                "args": {},
            },
        });
        push_warning(
            "partial_index_coverage",
            format!("index coverage incomplete ({indexed_files}/{supported_files})"),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
            extras.as_object().cloned(),
        );
    }
    if stale_files > 0 {
        let extras = json!({
            "stale_files": stale_files,
            "indexed_files": indexed_files,
            "supported_files": supported_files,
            "remediation": {
                "method": "tool_call",
                "tool": "refresh_symbol_index",
                "args": {},
            },
        });
        push_warning(
            "stale_index",
            format!("{stale_files} indexed files are stale"),
            Some("refresh_symbol_index"),
            Some("symbol_index"),
            extras.as_object().cloned(),
        );
    }

    #[cfg(feature = "semantic")]
    match semantic_status {
        SemanticSearchStatus::ModelAssetsUnavailable | SemanticSearchStatus::IndexMissing => {
            push_warning(
                semantic_status
                    .reason_code()
                    .unwrap_or("semantic_unavailable"),
                semantic_status
                    .reason_str()
                    .unwrap_or("semantic search unavailable")
                    .to_string(),
                semantic_status.recommended_action(),
                semantic_status.action_target(),
                None,
            );
        }
        _ => {}
    }

    #[cfg(not(feature = "semantic"))]
    if matches!(semantic_status, SemanticSearchStatus::FeatureDisabled) {
        push_warning(
            semantic_status
                .reason_code()
                .unwrap_or("semantic_feature_disabled"),
            semantic_status
                .reason_str()
                .unwrap_or("semantic feature disabled")
                .to_string(),
            semantic_status.recommended_action(),
            semantic_status.action_target(),
            None,
        );
    }

    if daemon_binary_drift
        .get("stale_daemon")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        push_warning(
            daemon_binary_drift
                .get("reason_code")
                .and_then(|v| v.as_str())
                .unwrap_or("stale_daemon"),
            daemon_binary_drift
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("daemon binary drift detected")
                .to_string(),
            daemon_binary_drift
                .get("recommended_action")
                .and_then(|v| v.as_str()),
            daemon_binary_drift
                .get("action_target")
                .and_then(|v| v.as_str()),
            None,
        );
    }

    json!({
        "status": if warnings.is_empty() { "ok" } else { "warn" },
        "warning_count": warnings.len(),
        "warnings": warnings,
    })
}
