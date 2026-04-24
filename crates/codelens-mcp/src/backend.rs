//! Semantic backend abstraction (P2 scaffold).
//!
//! Serena-comparison §Adopt 2 calls out a formal backend adapter interface.
//! This module establishes the passive half of that abstraction: a capability
//! vocabulary and a `SemanticBackend` trait implemented by each existing
//! retrieval/edit engine. The resource surface reports which backend covers
//! which capability so callers can reason about the substrate without
//! committing to a specific engine.
//!
//! The trait does NOT yet own dispatch. Concrete tool handlers still call
//! into the relevant engine directly. This file is the stable declaration
//! point; actual routing through the trait is tracked separately.

use crate::AppState;
use serde::Serialize;
use serde_json::{Value, json};

/// Capabilities a semantic backend can claim to fulfil.
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

/// Passive descriptor for a backend. Future work replaces this with a real
/// trait object that executes retrieval/edit. For now each concrete backend
/// is a unit struct whose `report` returns a stable snapshot.
pub trait SemanticBackend {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> &'static [BackendCapability];
}

pub struct RustEngineBackend;
pub struct LspBridgeBackend;
pub struct ScipBridgeBackend;

impl SemanticBackend for RustEngineBackend {
    fn name(&self) -> &'static str {
        "rust-engine"
    }

    fn capabilities(&self) -> &'static [BackendCapability] {
        &[
            BackendCapability::SymbolLookup,
            BackendCapability::SymbolsOverview,
            BackendCapability::References,
            BackendCapability::Rename,
            BackendCapability::Edit,
            BackendCapability::ImpactAnalysis,
            BackendCapability::SemanticSearch,
            BackendCapability::Embeddings,
        ]
    }
}

impl SemanticBackend for LspBridgeBackend {
    fn name(&self) -> &'static str {
        "lsp-bridge"
    }

    fn capabilities(&self) -> &'static [BackendCapability] {
        &[
            BackendCapability::References,
            BackendCapability::TypeHierarchy,
            BackendCapability::Rename,
            BackendCapability::Diagnostics,
        ]
    }
}

impl SemanticBackend for ScipBridgeBackend {
    fn name(&self) -> &'static str {
        "scip-bridge"
    }

    fn capabilities(&self) -> &'static [BackendCapability] {
        &[
            BackendCapability::SymbolLookup,
            BackendCapability::References,
            BackendCapability::ImpactAnalysis,
        ]
    }
}

/// Snapshot describing one backend at a single point in time. Used by the
/// `codelens://backend/capabilities` resource.
#[derive(Debug, Clone, Serialize)]
pub struct BackendReport {
    pub name: &'static str,
    pub declared: bool,
    pub compiled: bool,
    pub available: bool,
    pub active: bool,
    pub active_reason: String,
    pub capabilities: Vec<&'static str>,
    pub runtime: Value,
}

/// Enumerate all known backends with their current availability.
pub fn enumerate_backends(state: &AppState) -> Vec<BackendReport> {
    let backends: [&dyn SemanticBackend; 3] =
        [&RustEngineBackend, &LspBridgeBackend, &ScipBridgeBackend];
    backends
        .iter()
        .map(|backend| {
            let runtime = runtime_status_for_backend(backend.name(), state);
            BackendReport {
                name: backend.name(),
                declared: true,
                compiled: runtime.compiled,
                available: runtime.available,
                active: runtime.active,
                active_reason: runtime.active_reason,
                capabilities: backend
                    .capabilities()
                    .iter()
                    .map(|cap| cap.as_str())
                    .collect(),
                runtime: runtime.details,
            }
        })
        .collect()
}

struct BackendRuntimeStatus {
    compiled: bool,
    available: bool,
    active: bool,
    active_reason: String,
    details: Value,
}

fn runtime_status_for_backend(name: &str, state: &AppState) -> BackendRuntimeStatus {
    match name {
        "rust-engine" => BackendRuntimeStatus {
            compiled: true,
            available: true,
            active: true,
            active_reason: "always_available".to_owned(),
            details: json!({"fast_path": "tree_sitter"}),
        },
        "lsp-bridge" => {
            let statuses = codelens_engine::check_lsp_status();
            let installed_server_count = statuses.iter().filter(|status| status.installed).count();
            BackendRuntimeStatus {
                compiled: true,
                available: installed_server_count > 0,
                active: false,
                active_reason: "explicit_use_required".to_owned(),
                details: json!({
                    "recipe_count": statuses.len(),
                    "installed_server_count": installed_server_count,
                    "activation": "use_lsp_true_or_position_request",
                }),
            }
        }
        "scip-bridge" => scip_runtime_status(state),
        _ => BackendRuntimeStatus {
            compiled: false,
            available: false,
            active: false,
            active_reason: "unknown_backend".to_owned(),
            details: json!({}),
        },
    }
}

fn scip_runtime_status(state: &AppState) -> BackendRuntimeStatus {
    #[cfg(feature = "scip-backend")]
    {
        let index_path = codelens_engine::ScipBackend::detect(state.project().as_path());
        let loaded = index_path.is_some() && state.scip().is_some();
        return BackendRuntimeStatus {
            compiled: true,
            available: loaded,
            active: loaded,
            active_reason: if loaded {
                "index_loaded".to_owned()
            } else if index_path.is_some() {
                "index_load_failed".to_owned()
            } else {
                "index_missing".to_owned()
            },
            details: json!({
                "index_path": index_path.map(|path| path.to_string_lossy().to_string()),
            }),
        };
    }
    #[cfg(not(feature = "scip-backend"))]
    {
        let _ = state;
        BackendRuntimeStatus {
            compiled: false,
            available: false,
            active: false,
            active_reason: "feature_not_compiled".to_owned(),
            details: json!({"index_path": null}),
        }
    }
}

/// Reverse index: for every capability, which backends claim to fulfil it
/// (regardless of current availability). Callers use this to decide which
/// backend to route a capability to once the dispatch half of P2 lands.
pub fn capability_coverage() -> Vec<(BackendCapability, Vec<&'static str>)> {
    let backends: [&dyn SemanticBackend; 3] =
        [&RustEngineBackend, &LspBridgeBackend, &ScipBridgeBackend];
    let all_caps = [
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
    all_caps
        .iter()
        .map(|cap| {
            let fulfillers = backends
                .iter()
                .filter(|backend| backend.capabilities().contains(cap))
                .map(|backend| backend.name())
                .collect::<Vec<_>>();
            (*cap, fulfillers)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_engine_backend_claims_core_capabilities() {
        let backend = RustEngineBackend;
        let caps = backend.capabilities();
        assert!(caps.contains(&BackendCapability::SymbolLookup));
        assert!(caps.contains(&BackendCapability::Rename));
        assert!(caps.contains(&BackendCapability::Edit));
    }

    #[test]
    fn lsp_backend_claims_diagnostics() {
        let backend = LspBridgeBackend;
        assert!(
            backend
                .capabilities()
                .contains(&BackendCapability::Diagnostics)
        );
    }

    #[test]
    fn every_capability_has_at_least_one_backend() {
        // Regression guard: if a capability is declared but no backend
        // claims it, the reverse index would surface a gap.
        for (cap, fulfillers) in capability_coverage() {
            assert!(
                !fulfillers.is_empty(),
                "capability {:?} has zero fulfilling backends",
                cap
            );
        }
    }

    #[test]
    fn capability_string_round_trip_is_stable() {
        for cap in [
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
        ] {
            assert!(
                !cap.as_str().is_empty(),
                "capability {:?} produced empty string",
                cap
            );
        }
    }

    #[test]
    fn scip_backend_claims_symbol_lookup_and_references() {
        let backend = ScipBridgeBackend;
        let caps = backend.capabilities();
        assert!(caps.contains(&BackendCapability::SymbolLookup));
        assert!(caps.contains(&BackendCapability::References));
        // Impact analysis is the distinguishing workload for SCIP.
        assert!(caps.contains(&BackendCapability::ImpactAnalysis));
    }
}
