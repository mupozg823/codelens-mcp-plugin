//! Authority model for Native/LSP analysis routing.
//!
//! Centralizes the decision of which analysis backend to use for each feature,
//! and provides consistent provenance metadata for tool responses.

use crate::protocol::{AnalysisSource, Freshness, ToolResponseMeta};

/// Analysis authority: which backend is primary for a given feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Authority {
    /// Native analysis (tree-sitter, regex) is primary. No fallback needed.
    NativeOnly,
    /// Native is primary, LSP can verify/enhance.
    NativeFirst,
    /// LSP is primary, native is fallback.
    LspFirst,
    /// LSP is required, no native fallback available.
    LspOnly,
    /// Both are used and results are merged.
    Hybrid,
}

/// Get the authority for a given feature name.
#[allow(dead_code)]
pub fn feature_authority(feature: &str) -> Authority {
    match feature {
        "symbols" | "search" | "dead_code" | "blast_radius" | "callers" | "callees" | "imports"
        | "complexity" | "annotations" | "tests" => Authority::NativeOnly,

        "rename" => Authority::NativeFirst,

        "references" => Authority::Hybrid,

        "type_hierarchy" => Authority::LspFirst,

        "diagnostics" | "workspace_symbols" | "rename_plan" => Authority::LspOnly,

        _ => Authority::NativeOnly,
    }
}

/// Build a ToolResponseMeta with the correct provenance for the given backend.
pub fn meta_for_backend(backend: &str, confidence: f64) -> ToolResponseMeta {
    let source = match backend {
        "lsp_pooled" | "lsp" => AnalysisSource::Lsp,
        "text_search" | "text_fallback" | "tree-sitter-native" | "filesystem" | "watcher"
        | "session" | "noop" | "capability-check" | "memory" => AnalysisSource::Native,
        "composite-onboard" | "composite" => AnalysisSource::Hybrid,
        "semantic-embedding" => AnalysisSource::Native,
        _ => AnalysisSource::Native,
    };

    let freshness = match backend {
        "text_search" | "text_fallback" | "tree-sitter-native" | "filesystem" => Freshness::Live,
        _ => Freshness::Indexed,
    };

    ToolResponseMeta {
        backend_used: backend.to_owned(),
        confidence,
        degraded_reason: None,
        source,
        partial: false,
        freshness,
        staleness_ms: None,
        decisions: Vec::new(),
    }
}

/// Build a degraded provenance meta (e.g., LSP failed, fell back to native).
pub fn meta_degraded(backend: &str, confidence: f64, reason: &str) -> ToolResponseMeta {
    let mut meta = meta_for_backend(backend, confidence);
    meta.degraded_reason = Some(reason.to_owned());
    meta
}

/// Check if a feature requires LSP and LSP is not available.
/// Returns an appropriate error message if so.
#[allow(dead_code)]
pub fn check_lsp_required(feature: &str, lsp_available: bool) -> Option<String> {
    let auth = feature_authority(feature);
    if matches!(auth, Authority::LspOnly) && !lsp_available {
        Some(format!(
            "Feature '{feature}' requires an LSP server but none is attached. \
             Use check_lsp_status to see available servers, or get_lsp_recipe for installation."
        ))
    } else {
        None
    }
}
