use crate::AppState;
use crate::tool_defs::ToolSurface;
use serde_json::json;

#[cfg(feature = "semantic")]
use crate::tool_defs::is_tool_in_surface;

/// Four-way decomposition of why `semantic_search` might not be
/// currently runnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SemanticSearchStatus {
    #[cfg(feature = "semantic")]
    Available,
    #[cfg(feature = "semantic")]
    ModelAssetsUnavailable,
    #[cfg(feature = "semantic")]
    NotInActiveSurface,
    #[cfg(feature = "semantic")]
    IndexMissing,
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

/// Compute the current `SemanticSearchStatus` without loading the
/// embedding engine unless it is already resident.
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
