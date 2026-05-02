//! Tool registry: static TOOLS vec, lookup functions, and the build_tools constructor.

use super::output_schemas::*;
use super::presets::tool_namespace;
use crate::protocol::{Tool, ToolAnnotations, ToolTier};
use serde_json::json;
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

    tools.extend(vec![
        // ── Rule corpus retrieval ───────────────────────────────────────
        Tool::new("find_relevant_rules", "[CodeLens:Workflow] BM25 search over CLAUDE.md + project memory for policy snippets matching a query. Separate corpus from code retrieval — rule text never pollutes semantic_search results.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string","description":"Natural-language query; identifier tokens are preserved"},"top_k":{"type":"integer","description":"Top-K results (1-20, default 3)"}}})).with_annotations(ro_a.clone()).with_max_response_tokens(2048),

        // ── Memory ──────────────────────────────────────────────────────
        Tool::new("list_memories", "[CodeLens:Memory] List project memory files under .codelens/memories.", json!({"type":"object","properties":{"topic":{"type":"string","description":"Optional topic to filter"}}})).with_output_schema(memory_list_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("read_memory", "[CodeLens:Memory] Read a named project memory file.", json!({"required":["memory_name"],"type":"object","properties":{"memory_name":{"type":"string"}}})).with_annotations(ro_p.clone()),
        Tool::new("write_memory", "[CodeLens:Memory] Create or overwrite a project memory file.", json!({"required":["memory_name","content"],"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}}})).with_annotations(mutating.clone()),
        Tool::new("delete_memory", "[CodeLens:Memory] Delete a project memory file.", json!({"required":["memory_name"],"type":"object","properties":{"memory_name":{"type":"string"}}})).with_annotations(destructive.clone()),
        Tool::new("rename_memory", "[CodeLens:Memory] Rename a project memory file.", json!({"required":["old_name","new_name"],"type":"object","properties":{"old_name":{"type":"string"},"new_name":{"type":"string"}}})).with_annotations(mut_p.clone()),
    ]);

    // ── Semantic (feature-gated) ────────────────────────────────────
    #[cfg(feature = "semantic")]
    {
        tools.push(Tool::new("semantic_search", "[CodeLens:Symbol] Natural language code search via embeddings — find code by meaning.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string","description":"Natural language search query"},"max_results":{"type":"integer","description":"Max results (default 20)"}}})).with_output_schema(semantic_search_output_schema()).with_annotations(ro_p.clone()));
        tools.push(Tool::new("index_embeddings", "[CodeLens:Symbol] Build semantic embedding index and optionally prewarm query embeddings. Required before semantic_search.", json!({"type":"object","properties":{"background":{"type":"boolean","description":"Run as a durable background job and poll with get_analysis_job"},"prewarm_queries":{"type":"array","items":{"type":"string"},"description":"Representative semantic_search queries to warm immediately after indexing"},"prewarm_limit":{"type":"integer","description":"Maximum prewarm query count (default 128, max 1024)"}}})).with_annotations(ro.clone()));
        tools.push(Tool::new("find_similar_code", "[CodeLens:Analysis] Find semantically similar code to a given symbol — clone detection, reuse opportunities.", json!({"required":["file_path","symbol_name"],"type":"object","properties":{"file_path":{"type":"string","description":"File containing the symbol"},"symbol_name":{"type":"string","description":"Symbol to find similar code for"},"max_results":{"type":"integer","description":"Max results (default 10)"}}})).with_output_schema(find_similar_code_output_schema()).with_annotations(ro_a.clone()));
        tools.push(Tool::new("find_code_duplicates", "[CodeLens:Analysis] Find near-duplicate code pairs across the codebase — DRY violations.", json!({"type":"object","properties":{"threshold":{"type":"number","description":"Cosine similarity threshold (default 0.85)"},"max_pairs":{"type":"integer","description":"Max pairs to return (default 20)"}}})).with_output_schema(find_code_duplicates_output_schema()).with_annotations(ro_a.clone()));
        tools.push(Tool::new("classify_symbol", "[CodeLens:Analysis] Zero-shot classify a symbol into categories — e.g. error handling, auth, database.", json!({"required":["file_path","symbol_name","categories"],"type":"object","properties":{"file_path":{"type":"string"},"symbol_name":{"type":"string"},"categories":{"type":"array","items":{"type":"string"},"description":"Category labels to classify against"}}})).with_output_schema(classify_symbol_output_schema()).with_annotations(ro_a.clone()));
        tools.push(Tool::new("find_misplaced_code", "[CodeLens:Analysis] Find symbols that are semantic outliers in their file — possible misplacement.", json!({"type":"object","properties":{"max_results":{"type":"integer","description":"Max outliers to return (default 10)"}}})).with_output_schema(find_misplaced_code_output_schema()).with_annotations(ro));
    }

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
