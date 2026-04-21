//! Runtime-backed backend capability reporting.
//!
//! These descriptors power `codelens://backend/capabilities` and the
//! operator dashboard. They report only capabilities that are both
//! runtime-backed and reachable from the active tool surface.

use crate::AppState;
use crate::tool_defs::{ToolSurface, is_tool_callable_in_surface};
use crate::tools::session::metrics_config::determine_semantic_search_status;
use serde::Serialize;

/// Capabilities a semantic backend can fulfil.
///
/// Ordered roughly from symbol-surface primitives up to higher-level
/// reasoning so reporting enumerations read top-down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendCapability {
    SymbolLookup,
    SymbolsOverview,
    References,
    TypeHierarchy,
    Rename,
    Edit,
    Diagnostics,
    ImpactAnalysis,
    SemanticSearch,
    Embeddings,
}

impl BackendCapability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SymbolLookup => "symbol_lookup",
            Self::SymbolsOverview => "symbols_overview",
            Self::References => "references",
            Self::TypeHierarchy => "type_hierarchy",
            Self::Rename => "rename",
            Self::Edit => "edit",
            Self::Diagnostics => "diagnostics",
            Self::ImpactAnalysis => "impact_analysis",
            Self::SemanticSearch => "semantic_search",
            Self::Embeddings => "embeddings",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RuntimeAvailability {
    semantic_available: bool,
    scip_available: bool,
}

/// Snapshot describing one backend at a single point in time. Used by the
/// `codelens://backend/capabilities` resource.
#[derive(Debug, Clone, Serialize)]
pub struct BackendReport {
    pub name: &'static str,
    pub available: bool,
    pub capabilities: Vec<&'static str>,
}

const ALL_CAPABILITIES: [BackendCapability; 10] = [
    BackendCapability::SymbolLookup,
    BackendCapability::SymbolsOverview,
    BackendCapability::References,
    BackendCapability::TypeHierarchy,
    BackendCapability::Rename,
    BackendCapability::Edit,
    BackendCapability::Diagnostics,
    BackendCapability::ImpactAnalysis,
    BackendCapability::SemanticSearch,
    BackendCapability::Embeddings,
];

fn runtime_availability(state: &AppState, surface: ToolSurface) -> RuntimeAvailability {
    RuntimeAvailability {
        semantic_available: determine_semantic_search_status(state, surface).is_available(),
        scip_available: scip_index_available(state),
    }
}

fn scip_index_available(state: &AppState) -> bool {
    cfg!(feature = "scip-backend") && {
        let project_root = state.project();
        project_root.as_path().join("index.scip").exists()
            || project_root.as_path().join(".scip/index.scip").exists()
            || project_root.as_path().join(".codelens/index.scip").exists()
    }
}

fn surface_supports_any(surface: ToolSurface, tools: &[&str]) -> bool {
    tools
        .iter()
        .any(|tool_name| is_tool_callable_in_surface(tool_name, surface))
}

fn rust_engine_capabilities(
    surface: ToolSurface,
    availability: RuntimeAvailability,
) -> Vec<BackendCapability> {
    let mut capabilities = Vec::new();

    if surface_supports_any(surface, &["find_symbol"]) {
        capabilities.push(BackendCapability::SymbolLookup);
    }
    if surface_supports_any(surface, &["get_symbols_overview"]) {
        capabilities.push(BackendCapability::SymbolsOverview);
    }
    if surface_supports_any(surface, &["find_referencing_symbols"]) {
        capabilities.push(BackendCapability::References);
    }
    if surface_supports_any(surface, &["rename_symbol"]) {
        capabilities.push(BackendCapability::Rename);
    }
    if surface_supports_any(
        surface,
        &[
            "replace_symbol_body",
            "replace_lines",
            "delete_lines",
            "insert_at_line",
            "insert_before_symbol",
            "insert_after_symbol",
            "replace",
            "insert_content",
            "create_text_file",
            "add_import",
        ],
    ) {
        capabilities.push(BackendCapability::Edit);
    }
    if surface_supports_any(surface, &["get_impact_analysis", "impact_report"]) {
        capabilities.push(BackendCapability::ImpactAnalysis);
    }
    if availability.semantic_available && surface_supports_any(surface, &["semantic_search"]) {
        capabilities.push(BackendCapability::SemanticSearch);
    }
    if availability.semantic_available
        && surface_supports_any(surface, &["semantic_search", "index_embeddings"])
    {
        capabilities.push(BackendCapability::Embeddings);
    }

    capabilities
}

fn lsp_bridge_capabilities(surface: ToolSurface) -> Vec<BackendCapability> {
    let mut capabilities = Vec::new();

    if surface_supports_any(surface, &["find_referencing_symbols"]) {
        capabilities.push(BackendCapability::References);
    }
    if surface_supports_any(surface, &["get_type_hierarchy"]) {
        capabilities.push(BackendCapability::TypeHierarchy);
    }
    if surface_supports_any(surface, &["rename_symbol"]) {
        capabilities.push(BackendCapability::Rename);
    }
    if surface_supports_any(surface, &["get_file_diagnostics"]) {
        capabilities.push(BackendCapability::Diagnostics);
    }

    capabilities
}

fn scip_bridge_capabilities(
    surface: ToolSurface,
    availability: RuntimeAvailability,
) -> Vec<BackendCapability> {
    if !availability.scip_available {
        return Vec::new();
    }

    let mut capabilities = Vec::new();

    if surface_supports_any(surface, &["find_symbol"]) {
        capabilities.push(BackendCapability::SymbolLookup);
    }
    if surface_supports_any(surface, &["find_referencing_symbols"]) {
        capabilities.push(BackendCapability::References);
    }
    if surface_supports_any(surface, &["get_impact_analysis", "impact_report"]) {
        capabilities.push(BackendCapability::ImpactAnalysis);
    }
    if surface_supports_any(surface, &["get_file_diagnostics"]) {
        capabilities.push(BackendCapability::Diagnostics);
    }

    capabilities
}

fn backend_report(name: &'static str, capabilities: Vec<BackendCapability>) -> BackendReport {
    BackendReport {
        name,
        available: !capabilities.is_empty(),
        capabilities: capabilities.into_iter().map(|cap| cap.as_str()).collect(),
    }
}

/// Enumerate all known backends with their current availability.
pub fn enumerate_backends(state: &AppState, surface: ToolSurface) -> Vec<BackendReport> {
    let availability = runtime_availability(state, surface);
    vec![
        backend_report(
            "rust-engine",
            rust_engine_capabilities(surface, availability),
        ),
        backend_report("lsp-bridge", lsp_bridge_capabilities(surface)),
        backend_report(
            "scip-bridge",
            scip_bridge_capabilities(surface, availability),
        ),
    ]
}

/// Reverse index: for every capability currently reachable in the active
/// surface, which backends can fulfil it.
pub fn capability_coverage(
    state: &AppState,
    surface: ToolSurface,
) -> Vec<(BackendCapability, Vec<&'static str>)> {
    let reports = enumerate_backends(state, surface);
    ALL_CAPABILITIES
        .iter()
        .filter_map(|capability| {
            let fulfillers = reports
                .iter()
                .filter(|report| {
                    report
                        .capabilities
                        .iter()
                        .any(|cap| *cap == capability.as_str())
                })
                .map(|report| report.name)
                .collect::<Vec<_>>();
            if fulfillers.is_empty() {
                None
            } else {
                Some((*capability, fulfillers))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_defs::ToolPreset;

    fn full_surface() -> ToolSurface {
        ToolSurface::Preset(ToolPreset::Full)
    }

    #[test]
    fn rust_engine_report_tracks_surface_reachable_capabilities() {
        let capabilities = rust_engine_capabilities(
            full_surface(),
            RuntimeAvailability {
                semantic_available: true,
                scip_available: false,
            },
        );
        assert!(capabilities.contains(&BackendCapability::SymbolLookup));
        assert!(capabilities.contains(&BackendCapability::Edit));
        #[cfg(feature = "semantic")]
        assert!(capabilities.contains(&BackendCapability::SemanticSearch));
    }

    #[test]
    fn lsp_report_tracks_lsp_specific_capabilities() {
        let capabilities = lsp_bridge_capabilities(full_surface());
        assert!(capabilities.contains(&BackendCapability::References));
        assert!(capabilities.contains(&BackendCapability::Diagnostics));
        assert!(capabilities.contains(&BackendCapability::TypeHierarchy));
    }

    #[test]
    fn scip_report_requires_runtime_availability() {
        let unavailable = scip_bridge_capabilities(
            full_surface(),
            RuntimeAvailability {
                semantic_available: false,
                scip_available: false,
            },
        );
        assert!(unavailable.is_empty());

        let available = scip_bridge_capabilities(
            full_surface(),
            RuntimeAvailability {
                semantic_available: false,
                scip_available: true,
            },
        );
        assert!(available.contains(&BackendCapability::SymbolLookup));
        assert!(available.contains(&BackendCapability::Diagnostics));
    }

    #[test]
    fn capability_string_round_trip_is_stable() {
        for capability in ALL_CAPABILITIES {
            assert!(
                !capability.as_str().is_empty(),
                "capability {:?} produced empty string",
                capability
            );
        }
    }
}
