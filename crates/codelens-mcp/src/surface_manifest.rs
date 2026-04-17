use crate::AppState;
use crate::state::RuntimeDaemonMode;
use crate::tool_defs::{
    ALL_PRESETS, ALL_PROFILES, ToolPreset, ToolProfile, ToolSurface, preferred_namespaces,
    preferred_tier_labels, tool_namespace, tool_tier_label, tools, visible_tools,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const SURFACE_MANIFEST_SCHEMA_VERSION: &str = "codelens-surface-manifest-v1";
pub(crate) const HARNESS_MODES_SCHEMA_VERSION: &str = "codelens-harness-modes-v1";
pub(crate) const SURFACE_MANIFEST_DOC_PATH: &str = "docs/generated/surface-manifest.json";

const WORKSPACE_CARGO_TOML: &str = include_str!("../../../Cargo.toml");

pub(crate) fn build_surface_manifest_for_state(state: &AppState) -> Value {
    build_surface_manifest(*state.surface(), state.daemon_mode())
}

pub(crate) fn build_surface_manifest(
    surface: ToolSurface,
    daemon_mode: RuntimeDaemonMode,
) -> Value {
    let workspace_members = workspace_members();
    let workspace_member_count = workspace_members.len();
    let tool_definitions = tools();
    let total_tool_count = tool_definitions.len();
    let output_schema_count = tool_definitions
        .iter()
        .filter(|tool| tool.output_schema.is_some())
        .count();

    let namespace_counts = tool_definitions
        .iter()
        .fold(BTreeMap::new(), |mut acc, tool| {
            *acc.entry(tool_namespace(tool.name).to_owned())
                .or_insert(0usize) += 1;
            acc
        });
    let tier_counts = tool_definitions
        .iter()
        .fold(BTreeMap::new(), |mut acc, tool| {
            *acc.entry(tool_tier_label(tool.name).to_owned())
                .or_insert(0usize) += 1;
            acc
        });

    let profiles = ALL_PROFILES
        .iter()
        .map(|profile| {
            let surface = ToolSurface::Profile(*profile);
            let visible = visible_tools(surface);
            json!({
                "name": profile.as_str(),
                "tool_count": visible.len(),
                "preferred_namespaces": preferred_namespaces(surface),
                "preferred_tiers": preferred_tier_labels(surface),
                "tools": visible.iter().map(|tool| tool.name).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let presets = ALL_PRESETS
        .iter()
        .map(|preset| {
            let surface = ToolSurface::Preset(*preset);
            let visible = visible_tools(surface);
            json!({
                "name": preset_label(*preset),
                "tool_count": visible.len(),
                "preferred_namespaces": preferred_namespaces(surface),
                "preferred_tiers": preferred_tier_labels(surface),
                "tools": visible.iter().map(|tool| tool.name).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let language_inventory = build_language_inventory();
    let harness_modes = build_harness_modes();
    let language_family_count = language_inventory["language_family_count"]
        .as_u64()
        .unwrap_or_default();
    let extension_count = language_inventory["extension_count"]
        .as_u64()
        .unwrap_or_default();
    let server_card_features = server_card_features();

    json!({
        "schema_version": SURFACE_MANIFEST_SCHEMA_VERSION,
        "workspace": {
            "version": env!("CARGO_PKG_VERSION"),
            "description": env!("CARGO_PKG_DESCRIPTION"),
            "members": workspace_members,
            "member_count": workspace_member_count,
        },
        "tool_registry": {
            "definition_count": total_tool_count,
            "output_schema_count": output_schema_count,
            "namespaces": namespace_counts,
            "tiers": tier_counts,
            "tools": tool_definitions.iter().map(|tool| {
                json!({
                    "name": tool.name,
                    "namespace": tool_namespace(tool.name),
                    "tier": tool_tier_label(tool.name),
                    "has_output_schema": tool.output_schema.is_some(),
                    "estimated_tokens": tool.estimated_tokens,
                })
            }).collect::<Vec<_>>(),
        },
        "surfaces": {
            "profiles": profiles,
            "presets": presets,
        },
        "harness_modes": harness_modes,
        "languages": language_inventory,
        "runtime": {
            "server_name": "codelens-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "transport": transport_support(),
            "active_surface": surface.as_label(),
            "visible_tool_count": visible_tools(surface).len(),
            "daemon_mode": daemon_mode.as_str(),
            "supports_http": cfg!(feature = "http"),
            "supports_semantic": cfg!(feature = "semantic"),
            "supports_scip_backend": cfg!(feature = "scip-backend"),
            "supports_otel": cfg!(feature = "otel"),
            "server_card_features": server_card_features,
        },
        "summary": {
            "workspace_version": env!("CARGO_PKG_VERSION"),
            "workspace_member_count": workspace_member_count,
            "registered_tool_definitions": total_tool_count,
            "tool_output_schemas": {
                "declared": output_schema_count,
                "total": total_tool_count,
            },
            "harness_mode_count": 4,
            "supported_language_families": language_family_count,
            "supported_extensions": extension_count,
        }
    })
}

pub(crate) fn build_server_card(state: &AppState) -> Value {
    let manifest = build_surface_manifest_for_state(state);
    let runtime = &manifest["runtime"];
    let languages = &manifest["languages"];
    json!({
        "name": runtime["server_name"],
        "version": runtime["version"],
        "description": format!(
            "Compressed context and verification tool for agent harnesses ({} daemon)",
            runtime["daemon_mode"].as_str().unwrap_or("standard")
        ),
        "transport": runtime["transport"],
        "capabilities": {
            "tools": true,
            "resources": true,
            "prompts": true,
            "sampling": false
        },
        "tool_count": runtime["visible_tool_count"],
        "active_surface": runtime["active_surface"],
        "daemon_mode": runtime["daemon_mode"],
        "languages": languages["language_family_count"],
        "features": runtime["server_card_features"],
        "surface_manifest": {
            "schema_version": manifest["schema_version"],
            "path": SURFACE_MANIFEST_DOC_PATH,
        }
    })
}

fn transport_support() -> Vec<&'static str> {
    let mut transport = vec!["stdio"];
    if cfg!(feature = "http") {
        transport.push("streamable-http");
    }
    transport
}

fn server_card_features() -> Vec<&'static str> {
    let mut features = vec![
        "role-based-tool-surfaces",
        "composite-workflow-tools",
        "analysis-handles-and-sections",
        "durable-analysis-jobs",
        "mutation-audit-log",
        "session-resume",
        "session-client-metadata",
        "deferred-tool-loading",
        "tree-sitter-symbol-parsing",
        "import-graph-analysis",
        "lsp-integration",
        "token-budget-control",
        "surface-manifest",
        "harness-modes",
    ];
    if cfg!(feature = "semantic") {
        features.push("semantic-search");
    }
    if cfg!(feature = "http") {
        features.push("streamable-http");
    }
    if cfg!(feature = "scip-backend") {
        features.push("scip-precise-backend");
    }
    features
}

fn build_harness_modes() -> Value {
    json!({
        "schema_version": HARNESS_MODES_SCHEMA_VERSION,
        "communication_policy": {
            "default_pattern": "asymmetric-handoff",
            "live_bidirectional_agent_chat": "discouraged",
            "planner_to_builder_delegation": "recommended",
            "builder_to_planner_escalation": "explicit-only",
            "shared_substrate": "codelens-http-daemon-and-session-audit"
        },
        "modes": [
            harness_mode_solo_local(),
            harness_mode_planner_builder(),
            harness_mode_reviewer_gate(),
            harness_mode_batch_analysis(),
        ]
    })
}

fn harness_mode_solo_local() -> Value {
    json!({
        "name": "solo-local",
        "purpose": "Single-agent local work without cross-agent coordination overhead.",
        "best_fit": "One editor or terminal session exploring and editing the repository directly.",
        "topology": {
            "transport": "stdio-or-single-http",
            "daemon_shape": "single-session",
            "recommended_ports": []
        },
        "communication_pattern": "single-agent",
        "mutation_policy": "same session can plan and edit; refactor-full still requires verifier evidence before mutation",
        "roles": [
            harness_role(
                "solo-agent",
                &[ToolProfile::PlannerReadonly, ToolProfile::BuilderMinimal],
                false,
                "one session handles both planning and implementation"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "explore_codebase",
            "trace_request_path or review_changes",
            "plan_safe_refactor before broad edits"
        ],
        "recommended_audits": [
            "audit_builder_session for write-heavy runs",
            "audit_planner_session for read-side review runs"
        ]
    })
}

fn harness_mode_planner_builder() -> Value {
    json!({
        "name": "planner-builder",
        "purpose": "Primary multi-agent pattern: read-only planning/review paired with mutation-enabled implementation.",
        "best_fit": "Claude planning/review plus Codex building, or any equivalent planner/builder split.",
        "topology": {
            "transport": "http",
            "daemon_shape": "dual-daemon",
            "recommended_ports": [7837, 7838]
        },
        "communication_pattern": "asymmetric-handoff",
        "mutation_policy": "exactly one mutation-enabled agent per worktree; planners stay read-only",
        "roles": [
            harness_role(
                "planner-reviewer",
                &[ToolProfile::PlannerReadonly, ToolProfile::ReviewerGraph],
                false,
                "bootstrap, rank context, and verify change readiness before dispatch"
            ),
            harness_role(
                "builder-refactor",
                &[ToolProfile::BuilderMinimal, ToolProfile::RefactorFull],
                true,
                "execute bounded edits after preflight, diagnostics, and claims"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "get_symbols_overview per target file",
            "get_file_diagnostics per target file",
            "verify_change_readiness",
            "register_agent_work",
            "claim_files",
            "mutation pass",
            "audit_builder_session",
            "release_files"
        ],
        "recommended_audits": [
            "audit_planner_session on the planner session",
            "audit_builder_session on the builder session",
            "export_session_markdown(session_id=...) for human review artifacts"
        ]
    })
}

fn harness_mode_reviewer_gate() -> Value {
    json!({
        "name": "reviewer-gate",
        "purpose": "Read-only signoff lane that checks builder output before merge or handoff.",
        "best_fit": "PR review, risk signoff, CI-facing structural review, or planner validation after a builder run.",
        "topology": {
            "transport": "http",
            "daemon_shape": "read-only-daemon",
            "recommended_ports": [7837]
        },
        "communication_pattern": "review-signoff",
        "mutation_policy": "no content mutation; fail the session audit if mutation traces appear",
        "roles": [
            harness_role(
                "reviewer",
                &[ToolProfile::ReviewerGraph, ToolProfile::CiAudit],
                false,
                "diff-aware review, impact analysis, and audit signoff"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "review_changes or impact_report",
            "audit_planner_session",
            "audit_builder_session if reviewing a prior builder session",
            "export_session_markdown"
        ],
        "recommended_audits": [
            "audit_planner_session for the reviewer session",
            "audit_builder_session for the session under review"
        ]
    })
}

fn harness_mode_batch_analysis() -> Value {
    json!({
        "name": "batch-analysis",
        "purpose": "Asynchronous analysis lane for repo-wide or long-running read-side jobs.",
        "best_fit": "Dead-code sweeps, architecture scans, semantic review queues, and non-interactive evaluation passes.",
        "topology": {
            "transport": "http",
            "daemon_shape": "read-only-daemon",
            "recommended_ports": [7837]
        },
        "communication_pattern": "artifact-handoff",
        "mutation_policy": "read-only; use analysis handles and job artifacts rather than direct edits",
        "roles": [
            harness_role(
                "analysis-runner",
                &[ToolProfile::WorkflowFirst, ToolProfile::EvaluatorCompact, ToolProfile::CiAudit],
                false,
                "start durable jobs and consume bounded sections instead of raw full reports"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "start_analysis_job",
            "get_analysis_job",
            "get_analysis_section",
            "codelens://analysis/{id}/summary"
        ],
        "recommended_audits": [
            "audit_planner_session when the run stayed on planner/reviewer surfaces",
            "get_tool_metrics(session_id=...) for job-heavy telemetry"
        ]
    })
}

fn harness_role(
    role: &str,
    profiles: &[ToolProfile],
    can_mutate: bool,
    responsibility: &str,
) -> Value {
    json!({
        "role": role,
        "can_mutate": can_mutate,
        "responsibility": responsibility,
        "profiles": profiles.iter().map(|profile| {
            json!({
                "name": profile.as_str(),
                "tool_count": visible_tools(ToolSurface::Profile(*profile)).len(),
            })
        }).collect::<Vec<_>>(),
    })
}

fn preset_label(preset: ToolPreset) -> &'static str {
    match preset {
        ToolPreset::Minimal => "minimal",
        ToolPreset::Balanced => "balanced",
        ToolPreset::Full => "full",
    }
}

fn workspace_members() -> Vec<String> {
    let mut members = Vec::new();
    let mut in_members_block = false;
    for line in WORKSPACE_CARGO_TOML.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members = [") {
            in_members_block = true;
            continue;
        }
        if in_members_block {
            if trimmed == "]" {
                break;
            }
            if let Some(member) = trimmed
                .trim_end_matches(',')
                .trim()
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
            {
                members.push(member.to_owned());
            }
        }
    }
    members
}

fn build_language_inventory() -> Value {
    let mut families = BTreeMap::<String, LanguageFamily>::new();
    for entry in codelens_engine::lang_registry::all_entries() {
        let family = families
            .entry(entry.canonical.to_owned())
            .or_insert_with(|| LanguageFamily::new(entry.canonical));
        family.extensions.insert(entry.ext.to_owned());
        family.language_ids.insert(entry.language_id.to_owned());
        if entry.supports_imports {
            family.supports_imports = true;
        }
    }

    let import_capable_extension_count =
        codelens_engine::lang_registry::import_extensions().count();
    let extension_count = codelens_engine::lang_registry::all_extensions().count();
    let language_families = families
        .values()
        .map(|family| {
            json!({
                "canonical": family.canonical,
                "display_name": family.display_name(),
                "extensions": family.extensions,
                "language_ids": family.language_ids,
                "supports_imports": family.supports_imports,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "language_family_count": language_families.len(),
        "extension_count": extension_count,
        "import_capable_extension_count": import_capable_extension_count,
        "families": language_families,
    })
}

struct LanguageFamily {
    canonical: String,
    extensions: BTreeSet<String>,
    language_ids: BTreeSet<String>,
    supports_imports: bool,
}

impl LanguageFamily {
    fn new(canonical: &str) -> Self {
        Self {
            canonical: canonical.to_owned(),
            extensions: BTreeSet::new(),
            language_ids: BTreeSet::new(),
            supports_imports: false,
        }
    }

    fn display_name(&self) -> &'static str {
        match self.canonical.as_str() {
            "py" => "Python",
            "js" => "JavaScript",
            "ts" => "TypeScript",
            "tsx" => "TSX/JSX",
            "go" => "Go",
            "java" => "Java",
            "kt" => "Kotlin",
            "rs" => "Rust",
            "c" => "C",
            "cpp" => "C++",
            "php" => "PHP",
            "swift" => "Swift",
            "scala" => "Scala",
            "rb" => "Ruby",
            "cs" => "C#",
            "dart" => "Dart",
            "lua" => "Lua",
            "zig" => "Zig",
            "ex" => "Elixir",
            "hs" => "Haskell",
            "ml" => "OCaml",
            "erl" => "Erlang",
            "r" => "R",
            "sh" => "Bash/Shell",
            "jl" => "Julia",
            "css" => "CSS",
            "html" => "HTML",
            "toml" => "TOML",
            "yaml" => "YAML",
            "clj" => "Clojure/ClojureScript",
            _ => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_defs::ToolProfile;

    #[test]
    fn manifest_matches_registry_counts() {
        let manifest = build_surface_manifest(
            ToolSurface::Profile(ToolProfile::PlannerReadonly),
            RuntimeDaemonMode::ReadOnly,
        );
        assert_eq!(
            manifest["workspace"]["version"],
            json!(env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(
            manifest["tool_registry"]["definition_count"],
            json!(tools().len())
        );
        assert_eq!(
            manifest["tool_registry"]["output_schema_count"],
            json!(
                tools()
                    .iter()
                    .filter(|tool| tool.output_schema.is_some())
                    .count()
            )
        );
        assert_eq!(
            manifest["runtime"]["visible_tool_count"],
            json!(visible_tools(ToolSurface::Profile(ToolProfile::PlannerReadonly)).len())
        );
        assert_eq!(manifest["workspace"]["member_count"], json!(3));
        assert_eq!(manifest["summary"]["harness_mode_count"], json!(4));
        assert_eq!(
            manifest["harness_modes"]["schema_version"],
            json!(HARNESS_MODES_SCHEMA_VERSION)
        );
        assert!(
            manifest["harness_modes"]["modes"]
                .as_array()
                .is_some_and(|modes| modes.iter().any(|mode| mode["name"] == json!("planner-builder")))
        );

        let manifest_profiles = manifest["surfaces"]["profiles"]
            .as_array()
            .expect("profiles array");
        for profile in ALL_PROFILES {
            let entry = manifest_profiles
                .iter()
                .find(|item| item["name"] == json!(profile.as_str()))
                .expect("profile entry");
            assert_eq!(
                entry["tool_count"],
                json!(visible_tools(ToolSurface::Profile(*profile)).len())
            );
        }

        let manifest_presets = manifest["surfaces"]["presets"]
            .as_array()
            .expect("presets array");
        for preset in ALL_PRESETS {
            let entry = manifest_presets
                .iter()
                .find(|item| item["name"] == json!(preset_label(*preset)))
                .expect("preset entry");
            assert_eq!(
                entry["tool_count"],
                json!(visible_tools(ToolSurface::Preset(*preset)).len())
            );
        }
    }
}
