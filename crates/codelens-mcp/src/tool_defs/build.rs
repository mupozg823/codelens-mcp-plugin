//! Tool registry: static TOOLS vec, lookup functions, and the build_tools constructor.
//!
//! After ADR-0013 PR-A..PR-F, the per-tool `Tool::new(...)` rows live
//! entirely in `tools.toml` and are emitted by `super::generated`.
//! This file now only owns the annotation-binding locals consumed by
//! the generated category functions and the post-build pass that
//! attaches namespace/title/estimated_tokens.

use super::presets::tool_namespace;
use crate::protocol::{Tool, ToolAnnotations, ToolTier};
use std::sync::LazyLock;

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(build_tools);

fn estimate_serialized_tokens(tool: &Tool) -> usize {
    serde_json::to_string(tool)
        .map(|body| body.len() / 4)
        .unwrap_or(0)
}

fn tool_title_override(name: &str) -> Option<&'static str> {
    match name {
        "get_current_config" => Some("Current Config"),
        "get_project_structure" => Some("Project Structure"),
        "get_symbols_overview" => Some("Symbols Overview"),
        "get_ranked_context" => Some("Ranked Context"),
        "get_complexity" => Some("Complexity"),
        "check_lsp_status" => Some("LSP Status"),
        "get_lsp_recipe" => Some("LSP Recipe"),
        "get_changed_files" => Some("Changed Files"),
        "get_impact_analysis" => Some("Impact Analysis"),
        "get_symbol_importance" => Some("Symbol Importance"),
        "get_change_coupling" => Some("Change Coupling"),
        "get_file_diagnostics" => Some("File Diagnostics"),
        "get_analysis_job" => Some("Analysis Job"),
        "list_analysis_jobs" => Some("Analysis Jobs"),
        "list_analysis_artifacts" => Some("Analysis Artifacts"),
        "get_analysis_section" => Some("Analysis Section"),
        "get_tool_metrics" => Some("Tool Metrics"),
        "list_memories" => Some("Memories"),
        "list_queryable_projects" => Some("Queryable Projects"),
        "get_capabilities" => Some("Capabilities"),
        _ => None,
    }
}

fn title_word(part: &str) -> String {
    match part {
        "ai" => "AI".to_owned(),
        "ci" => "CI".to_owned(),
        "lsp" => "LSP".to_owned(),
        "mcp" => "MCP".to_owned(),
        _ => {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_ascii_uppercase().to_string();
                    word.push_str(chars.as_str());
                    word
                }
                None => String::new(),
            }
        }
    }
}

fn tool_title(name: &str) -> String {
    if let Some(title) = tool_title_override(name) {
        return title.to_owned();
    }

    name.split('_')
        .filter(|part| !part.is_empty())
        .map(title_word)
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn tools() -> &'static [Tool] {
    &TOOLS
}

pub(crate) fn tool_definition(name: &str) -> Option<&'static Tool> {
    tools().iter().find(|tool| tool.name == name)
}

fn build_tools() -> Vec<Tool> {
    let ro = ToolAnnotations::read_only();
    let destructive = ToolAnnotations::destructive();
    let mutating = ToolAnnotations::mutating();
    // Tier-specific annotations for cleaner builder chains
    let ro_p = ro.clone().with_tier(ToolTier::Primitive);
    let ro_a = ro.clone().with_tier(ToolTier::Analysis);
    let ro_w = ro.clone().with_tier(ToolTier::Workflow);
    let approved_mutating = mutating
        .clone()
        .with_approval_required(true)
        .with_audit_category("mutation");
    let approved_destructive = destructive
        .clone()
        .with_approval_required(true)
        .with_audit_category("destructive");
    let mut_p = approved_mutating.clone().with_tier(ToolTier::Primitive);
    let dest_a = approved_destructive.clone().with_tier(ToolTier::Analysis);
    let mut_w = approved_mutating.clone().with_tier(ToolTier::Workflow);
    // `mutating` flavour for the multi-agent advisory surface — keeps
    // the legacy `audit_category="coordination"` tag without forcing
    // approval prompts on register_agent_work / claim_files /
    // release_files.
    let mut_coord = mutating.clone().with_audit_category("coordination");
    // ── File I/O / Symbol / LSP / Editing / Analysis / Composite ───
    // Migrated to `tools.toml` (ADR-0013). The generator emits the same
    // Tool::new chain as the legacy categories that follow.
    let mut tools = super::generated::file_io_tools(&ro_p);
    tools.extend(super::generated::symbol_tools(&mut_w, &ro_a, &ro_p));
    tools.extend(super::generated::lsp_tools(&ro_a, &ro_p));
    tools.extend(super::generated::analysis_tools(&ro_a, &ro_p));
    tools.extend(super::generated::editing_tools(
        &dest_a,
        &destructive,
        &mut_p,
        &mut_w,
        &mutating,
    ));
    tools.extend(super::generated::composite_tools(
        &mut_w, &ro_a, &ro_p, &ro_w,
    ));
    tools.extend(super::generated::workflow_first_tools(&ro_w));
    tools.extend(super::generated::session_tools(
        &mut_coord, &mut_p, &mutating, &ro_a, &ro_p, &ro_w,
    ));
    tools.extend(super::generated::rule_corpus_tools(&ro_a));
    tools.extend(super::generated::memory_tools(
        &destructive,
        &mut_p,
        &mutating,
        &ro_p,
    ));

    #[cfg(feature = "semantic")]
    tools.extend(super::generated::semantic_tools(&ro, &ro_a, &ro_p));

    for tool in &mut tools {
        let annotations = tool
            .annotations
            .take()
            .unwrap_or_else(crate::protocol::ToolAnnotations::read_only)
            .with_namespace(tool_namespace(tool.name))
            .with_title(tool_title(tool.name));
        tool.annotations = Some(annotations);
        tool.estimated_tokens = estimate_serialized_tokens(tool);
    }

    tools
}
