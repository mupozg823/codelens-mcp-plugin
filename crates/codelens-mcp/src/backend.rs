//! Backend capability registry (P2 scaffold).
//!
//! This module is a product capability registry, not a dispatch abstraction.
//! Concrete handlers still call the relevant engine directly. The registry is
//! deliberately descriptor-based so a single implementation cannot grow into a
//! fake all-purpose semantic backend trait.

use crate::AppState;
use crate::backend_operation_matrix::semantic_edit_operation_matrix;
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
    SemanticEditBackend,
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
            Self::SemanticEditBackend => "semantic_edit_backend",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BackendDescriptor {
    name: &'static str,
    capabilities: &'static [BackendCapability],
}

const RUST_ENGINE_CAPABILITIES: &[BackendCapability] = &[
    BackendCapability::SymbolLookup,
    BackendCapability::SymbolsOverview,
    BackendCapability::References,
    BackendCapability::Rename,
    BackendCapability::Edit,
    BackendCapability::ImpactAnalysis,
    BackendCapability::SemanticSearch,
    BackendCapability::Embeddings,
];

const LSP_BRIDGE_CAPABILITIES: &[BackendCapability] = &[
    BackendCapability::References,
    BackendCapability::TypeHierarchy,
    BackendCapability::Rename,
    BackendCapability::Diagnostics,
];

const SCIP_BRIDGE_CAPABILITIES: &[BackendCapability] = &[
    BackendCapability::SymbolLookup,
    BackendCapability::References,
    BackendCapability::ImpactAnalysis,
];

const SEMANTIC_EDIT_CAPABILITIES: &[BackendCapability] = &[
    BackendCapability::SemanticEditBackend,
    BackendCapability::Rename,
    BackendCapability::Edit,
    BackendCapability::Diagnostics,
];

const BACKENDS: &[BackendDescriptor] = &[
    BackendDescriptor {
        name: "rust-engine",
        capabilities: RUST_ENGINE_CAPABILITIES,
    },
    BackendDescriptor {
        name: "lsp-bridge",
        capabilities: LSP_BRIDGE_CAPABILITIES,
    },
    BackendDescriptor {
        name: "scip-bridge",
        capabilities: SCIP_BRIDGE_CAPABILITIES,
    },
    BackendDescriptor {
        name: "semantic-edit-backend",
        capabilities: SEMANTIC_EDIT_CAPABILITIES,
    },
];

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
    BACKENDS
        .iter()
        .map(|backend| {
            let runtime = runtime_status_for_backend(backend.name, state);
            BackendReport {
                name: backend.name,
                declared: true,
                compiled: runtime.compiled,
                available: runtime.available,
                active: runtime.active,
                active_reason: runtime.active_reason,
                capabilities: backend
                    .capabilities
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
        "semantic-edit-backend" => semantic_edit_runtime_status(state),
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
        BackendRuntimeStatus {
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
        }
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

fn semantic_edit_runtime_status(state: &AppState) -> BackendRuntimeStatus {
    let configured = crate::env_compat::dual_prefix_env("CODELENS_SEMANTIC_EDIT_BACKEND");
    let jetbrains_available = std::env::var_os("CODELENS_JETBRAINS_ADAPTER_CMD").is_some();
    let roslyn_available = std::env::var_os("CODELENS_ROSLYN_ADAPTER_CMD").is_some();
    let lsp_statuses = codelens_engine::check_lsp_status();
    let installed_lsp_server_count = lsp_statuses
        .iter()
        .filter(|status| status.installed)
        .count();
    let _ = state;
    let lsp_available = installed_lsp_server_count > 0;
    let configured_lsp = configured.as_deref() == Some("lsp");
    let configured_jetbrains = configured.as_deref() == Some("jetbrains");
    let configured_roslyn = configured.as_deref() == Some("roslyn");
    let configured_default = matches!(
        configured.as_deref(),
        Some("default" | "off" | "tree-sitter" | "tree_sitter")
    );
    let active = (configured_lsp && lsp_available)
        || (configured_jetbrains && jetbrains_available)
        || (configured_roslyn && roslyn_available);
    let active_reason = if active {
        "env_opt_in_semantic_edit_backend".to_owned()
    } else if configured_lsp {
        "configured_lsp_unavailable".to_owned()
    } else if configured_jetbrains {
        "configured_jetbrains_adapter_unavailable".to_owned()
    } else if configured_roslyn {
        "configured_roslyn_adapter_unavailable".to_owned()
    } else if configured_default {
        "disabled_or_default_backend".to_owned()
    } else if configured.is_some() {
        "unsupported_config".to_owned()
    } else {
        "opt_in_required".to_owned()
    };

    BackendRuntimeStatus {
        compiled: true,
        available: lsp_available,
        active,
        active_reason,
        details: json!({
            "configured_backend": configured,
            "candidate_backends": ["lsp-bridge", "jetbrains-adapter", "roslyn-adapter"],
            "installed_lsp_server_count": installed_lsp_server_count,
            "activation": "set semantic_edit_backend=lsp per call or CODELENS_SEMANTIC_EDIT_BACKEND=lsp",
            "dispatch": "rename_symbol routes to LSP textDocument/rename; extract/inline/move/change-signature route to LSP codeAction only when explicitly requested",
            "external_adapters": {
                "jetbrains": {"available": jetbrains_available, "activation": "set CODELENS_JETBRAINS_ADAPTER_CMD to a local WorkspaceEdit adapter", "failure_policy": "fail_closed"},
                "roslyn": {"available": roslyn_available, "activation": "set CODELENS_ROSLYN_ADAPTER_CMD to a local WorkspaceEdit adapter", "failure_policy": "fail_closed"}
            },
            "operation_matrix": semantic_edit_operation_matrix(),
        }),
    }
}

/// Reverse index: for every capability, which backends claim to fulfil it
/// (regardless of current availability). Callers use this to decide which
/// backend to route a capability to once the dispatch half of P2 lands.
pub fn capability_coverage() -> Vec<(BackendCapability, Vec<&'static str>)> {
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
        BackendCapability::SemanticEditBackend,
    ];
    all_caps
        .iter()
        .map(|cap| {
            let fulfillers = BACKENDS
                .iter()
                .filter(|backend| backend.capabilities.contains(cap))
                .map(|backend| backend.name)
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
        let caps = backend_capabilities("rust-engine");
        assert!(caps.contains(&BackendCapability::SymbolLookup));
        assert!(caps.contains(&BackendCapability::Rename));
        assert!(caps.contains(&BackendCapability::Edit));
    }

    #[test]
    fn lsp_backend_claims_diagnostics() {
        let caps = backend_capabilities("lsp-bridge");
        assert!(caps.contains(&BackendCapability::Diagnostics));
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
            BackendCapability::SemanticEditBackend,
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
        let caps = backend_capabilities("scip-bridge");
        assert!(caps.contains(&BackendCapability::SymbolLookup));
        assert!(caps.contains(&BackendCapability::References));
        // Impact analysis is the distinguishing workload for SCIP.
        assert!(caps.contains(&BackendCapability::ImpactAnalysis));
    }

    #[test]
    fn semantic_edit_backend_is_separate_opt_in_capability() {
        let caps = backend_capabilities("semantic-edit-backend");
        assert!(caps.contains(&BackendCapability::SemanticEditBackend));
        assert!(caps.contains(&BackendCapability::Edit));
        assert!(!caps.contains(&BackendCapability::SemanticSearch));
    }

    fn backend_capabilities(name: &str) -> &'static [BackendCapability] {
        BACKENDS
            .iter()
            .find(|backend| backend.name == name)
            .map(|backend| backend.capabilities)
            .unwrap_or_else(|| panic!("missing backend descriptor {name}"))
    }

    #[test]
    fn semantic_edit_operation_matrix_does_not_overclaim_refactors() {
        let matrix = semantic_edit_operation_matrix();
        let operations = matrix["operations"].as_array().unwrap();
        assert!(operations.iter().any(|op| {
            op["operation"] == "rename"
                && op["backend"] == "lsp"
                && op["support"] == "authoritative_apply"
                && op["authority"] == "workspace_edit"
                && op["can_apply"] == true
        }));
        assert!(operations.iter().any(|op| {
            op["operation"] == "safe_delete_check"
                && op["backend"] == "lsp"
                && op["support"] == "authoritative_check"
                && op["authority"] == "semantic_readonly"
                && op["can_apply"] == false
        }));
        for operation in [
            "extract_function",
            "inline_function",
            "move_symbol",
            "change_signature",
        ] {
            let descriptor = operations
                .iter()
                .find(|op| op["operation"] == operation && op["backend"] == "lsp")
                .unwrap_or_else(|| panic!("missing LSP descriptor for {operation}"));
            assert_ne!(descriptor["support"], "authoritative_apply");
            assert_eq!(descriptor["authority"], "workspace_edit");
            assert_eq!(descriptor["can_apply"], false);
            assert_eq!(descriptor["verified"], false);
            assert!(
                descriptor["blocker_reason"].as_str().is_some_and(|reason| {
                    reason.contains("fixture") && reason.contains("WorkspaceEdit")
                }),
                "{descriptor}"
            );
        }
        let tree_sitter_rename = operations
            .iter()
            .find(|op| op["operation"] == "rename" && op["backend"] == "tree-sitter")
            .expect("missing tree-sitter rename descriptor");
        assert_eq!(tree_sitter_rename["authority"], "syntax");
        assert_eq!(tree_sitter_rename["can_apply"], false);
        assert!(operations.iter().any(|op| {
            op["operation"] == "rename"
                && op["backend"] == "roslyn"
                && op["support"] == "conditional_authoritative_apply"
                && op["can_apply"] == false
        }));
        assert!(
            !operations.iter().any(|op| {
                op["backend"] == "jetbrains" && op["support"] == "authoritative_apply"
            })
        );
        assert!(
            operations
                .iter()
                .any(|op| { op["backend"] == "scip" && op["support"] == "evidence_only" })
        );
    }
}
