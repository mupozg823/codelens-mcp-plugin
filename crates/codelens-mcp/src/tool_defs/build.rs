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
    // ── File I/O / Symbol / LSP / Editing / Analysis ───────────────
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

    tools.extend(vec![
        // ── Composite (multi-step workflows) ────────────────────────────
        Tool::new("explore_codebase", "[CodeLens:Workflow] Problem-first entrypoint for codebase exploration. Use query for targeted context, or call without arguments for onboarding.", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"max_tokens":{"type":"integer"},"include_body":{"type":"boolean"},"depth":{"type":"integer"},"disable_semantic":{"type":"boolean"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(16384),
        Tool::new("trace_request_path", "[CodeLens:Workflow] Trace a request or execution path from a function, symbol, or entrypoint.", json!({"type":"object","properties":{"function_name":{"type":"string"},"symbol":{"type":"string"},"entrypoint":{"type":"string"},"max_depth":{"type":"integer"},"max_results":{"type":"integer"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("explain_code_flow", "[CodeLens:Analysis] Summarize how a function fits in the call graph: callers, callees, and a one-line flow summary. Lighter than trace_request_path.", json!({"required":["function_name"],"type":"object","properties":{"function_name":{"type":"string"},"max_depth":{"type":"integer"},"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("review_architecture", "[CodeLens:Workflow] Review project or module architecture, boundaries, coupling, and optionally render a diagram.", json!({"type":"object","properties":{"path":{"type":"string"},"include_diagram":{"type":"boolean"},"max_nodes":{"type":"integer"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("plan_safe_refactor", "[CodeLens:Workflow] Preview a safe refactor plan. Uses rename safety when file_path+symbol are given; otherwise falls back to broader refactor safety analysis.", json!({"type":"object","properties":{"task":{"type":"string"},"symbol":{"type":"string"},"path":{"type":"string"},"file_path":{"type":"string"},"new_name":{"type":"string"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("cleanup_duplicate_logic", "[CodeLens:Workflow] Surface duplicate or removable logic before cleanup. Uses semantic duplicate search when available, otherwise bounded dead-code evidence.", json!({"type":"object","properties":{"threshold":{"type":"number"},"max_pairs":{"type":"integer"},"scope":{"type":"string"},"max_results":{"type":"integer"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("review_changes", "[CodeLens:Workflow] Pre-merge review: diff-aware references or impact analysis for changed files.", json!({
            "type": "object",
            "properties": {
                "changed_files": {"type": "array", "items": {"type": "string"}, "description": "File paths that changed"},
                "task": {"type": "string", "description": "Review focus description"},
                "path": {"type": "string", "description": "Scope path"}
            }
        })).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("diagnose_issues", "[CodeLens:Workflow] Diagnostics: file-level issues or unresolved reference check.", json!({
            "type": "object",
            "properties": {
                "file_path": {"type": "string", "description": "File to diagnose"},
                "path": {"type": "string", "description": "Directory scope"},
                "symbol": {"type": "string", "description": "Symbol to check references for"}
            }
        })).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("onboard_project", "[CodeLens:Session] One-shot onboarding: structure, key files, cycles, stats.", json!({"type":"object","properties":{}})).with_output_schema(onboard_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("analyze_change_request", "[CodeLens:Workflow] Compress a change request into ranked files, key symbols, risk, and next actions.", json!({"required":["task"],"type":"object","properties":{"task":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}},"profile_hint":{"type":"string","enum":["planner-readonly","builder-minimal","reviewer-graph","refactor-full","ci-audit"]}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("verify_change_readiness", "[CodeLens:Workflow] Verifier-first preflight: blockers, readiness, and next evidence before editing.", json!({"required":["task"],"type":"object","properties":{"task":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}},"profile_hint":{"type":"string","enum":["planner-readonly","builder-minimal","reviewer-graph","refactor-full","ci-audit"]}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("find_minimal_context_for_change", "[CodeLens:Workflow] Return the smallest useful file and symbol context needed to start a change.", json!({"required":["task"],"type":"object","properties":{"task":{"type":"string"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("summarize_symbol_impact", "[CodeLens:Workflow] Summarize callers, references, and affected files for one symbol.", json!({"required":["symbol"],"type":"object","properties":{"symbol":{"type":"string"},"file_path":{"type":"string"},"depth":{"type":"integer"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("module_boundary_report", "[CodeLens:Workflow] Summarize dependency boundaries, coupling, and cycle risk for a module or path.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("mermaid_module_graph", "[CodeLens:Workflow] Render upstream/downstream module dependencies as a Mermaid flowchart ready to embed in GitHub/GitLab Markdown.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string"},"max_nodes":{"type":"integer","description":"Max nodes rendered per side (default 10)"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("safe_rename_report", "[CodeLens:Workflow] Assess rename safety, blockers, and preview edits before refactoring.", json!({"required":["file_path","symbol"],"type":"object","properties":{"file_path":{"type":"string"},"symbol":{"type":"string"},"new_name":{"type":"string"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("unresolved_reference_check", "[CodeLens:Workflow] Lightweight unresolved or ambiguous reference guard before rename or broad edits.", json!({"required":["file_path"],"type":"object","properties":{"file_path":{"type":"string"},"symbol":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("dead_code_report", "[CodeLens:Workflow] Summarize dead-code candidates with bounded evidence and deletion risk.", json!({"type":"object","properties":{"scope":{"type":"string"},"max_results":{"type":"integer"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("impact_report", "[CodeLens:Workflow] Summarize changed-file impact, references, and blast radius with a bounded report.", json!({"type":"object","properties":{"path":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("refactor_safety_report", "[CodeLens:Workflow] Combine boundary, symbol impact, and test cues into a preview-first refactor report.", json!({"type":"object","properties":{"task":{"type":"string"},"symbol":{"type":"string"},"path":{"type":"string"},"file_path":{"type":"string"}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("diff_aware_references", "[CodeLens:Workflow] Compress references for changed files into a bounded reviewer/CI report.", json!({"type":"object","properties":{"changed_files":{"type":"array","items":{"type":"string"}}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("semantic_code_review", "[CodeLens:Workflow] Semantic code review — analyze changed symbols via references, embedding similarity, and risk assessment.", json!({"type":"object","properties":{"changed_files":{"type":"array","items":{"type":"string"}}}})).with_output_schema(analysis_handle_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(2048),
        Tool::new("start_analysis_job", "[CodeLens:Workflow] Start a durable analysis job and return a job handle for polling.", json!({"required":["kind"],"type":"object","properties":{"kind":{"type":"string","enum":["impact_report","dead_code_report","refactor_safety_report","semantic_code_review","eval_session_audit"]},"task":{"type":"string"},"symbol":{"type":"string"},"path":{"type":"string"},"file_path":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}},"profile_hint":{"type":"string","enum":["planner-readonly","builder-minimal","reviewer-graph","refactor-full","ci-audit"]}}})).with_output_schema(analysis_job_output_schema()).with_annotations(ro_w.clone()),
        Tool::new("get_analysis_job", "[CodeLens:Workflow] Poll a durable analysis job by job_id.", json!({"required":["job_id"],"type":"object","properties":{"job_id":{"type":"string"}}})).with_output_schema(analysis_job_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("cancel_analysis_job", "[CodeLens:Workflow] Cancel a queued or running analysis job by job_id.", json!({"required":["job_id"],"type":"object","properties":{"job_id":{"type":"string"}}})).with_output_schema(analysis_job_output_schema()).with_annotations(mut_w.clone()),
        Tool::new("retry_analysis_job", "[CodeLens:Workflow] Retry a failed or cancelled analysis job by job_id; reuses the original kind and profile_hint.", json!({"required":["job_id"],"type":"object","properties":{"job_id":{"type":"string"}}})).with_output_schema(analysis_job_output_schema()).with_annotations(mut_w.clone()),
        Tool::new("list_analysis_jobs", "[CodeLens:Workflow] List durable analysis jobs with status counts and any attached analysis handles.", json!({"type":"object","properties":{"status":{"type":"string","enum":["queued","running","completed","cancelled","error"]}}})).with_output_schema(analysis_job_list_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("list_analysis_artifacts", "[CodeLens:Workflow] List stored analysis artifacts with summary resource handles for reuse.", json!({"type":"object","properties":{}})).with_output_schema(analysis_artifact_list_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_analysis_section", "[CodeLens:Workflow] Expand a stored analysis section by analysis_id.", json!({"required":["analysis_id","section"],"type":"object","properties":{"analysis_id":{"type":"string"},"section":{"type":"string"}}})).with_output_schema(analysis_section_output_schema()).with_annotations(ro_p.clone()),

        // ── Rule corpus retrieval ───────────────────────────────────────
        Tool::new("find_relevant_rules", "[CodeLens:Workflow] BM25 search over CLAUDE.md + project memory for policy snippets matching a query. Separate corpus from code retrieval — rule text never pollutes semantic_search results.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string","description":"Natural-language query; identifier tokens are preserved"},"top_k":{"type":"integer","description":"Top-K results (1-20, default 3)"}}})).with_annotations(ro_a.clone()).with_max_response_tokens(2048),

        // ── Memory ──────────────────────────────────────────────────────
        Tool::new("list_memories", "[CodeLens:Memory] List project memory files under .codelens/memories.", json!({"type":"object","properties":{"topic":{"type":"string","description":"Optional topic to filter"}}})).with_output_schema(memory_list_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("read_memory", "[CodeLens:Memory] Read a named project memory file.", json!({"required":["memory_name"],"type":"object","properties":{"memory_name":{"type":"string"}}})).with_annotations(ro_p.clone()),
        Tool::new("write_memory", "[CodeLens:Memory] Create or overwrite a project memory file.", json!({"required":["memory_name","content"],"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}}})).with_annotations(mutating.clone()),
        Tool::new("delete_memory", "[CodeLens:Memory] Delete a project memory file.", json!({"required":["memory_name"],"type":"object","properties":{"memory_name":{"type":"string"}}})).with_annotations(destructive.clone()),
        Tool::new("rename_memory", "[CodeLens:Memory] Rename a project memory file.", json!({"required":["old_name","new_name"],"type":"object","properties":{"old_name":{"type":"string"},"new_name":{"type":"string"}}})).with_annotations(mut_p.clone()),

        // ── Session ─────────────────────────────────────────────────────
        Tool::new("activate_project", "[CodeLens:Session] Activate project — auto-detect preset, index, frameworks.", json!({"type":"object","properties":{"project":{"type":"string","description":"Optional project name or path"}}})).with_output_schema(activate_project_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("prepare_harness_session", "[CodeLens:Session] Official bootstrap/status entrypoint for harnesses — activate project, summarize surface, capabilities, visible tools, and optionally auto-recover a small stale index in one call.", json!({"type":"object","properties":{"project":{"type":"string","description":"Optional project name or path"},"profile":{"type":"string","enum":["planner-readonly","builder-minimal","reviewer-graph","refactor-full","ci-audit"]},"preset":{"type":"string","enum":["minimal","balanced","full"]},"token_budget":{"type":"integer","description":"Optional explicit token budget override after activation"},"file_path":{"type":"string","description":"Optional file path for language-specific capability checks"},"detail":{"type":"string","enum":["compact","full"],"description":"compact returns the harness preflight essentials only; full also includes the heavier config snapshot"},"host_context":{"type":"string","enum":["claude-code","codex","cursor","cline","windsurf","vscode","jetbrains","api-agent"],"description":"Optional host/runtime hint used to compile advisory bootstrap routing without changing the active tool surface"},"task_overlay":{"type":"string","enum":["planning","editing","review","onboarding","batch-analysis","interactive"],"description":"Optional task-mode hint used to compile advisory bootstrap routing without changing the active tool surface"},"preferred_entrypoints":{"type":"array","items":{"type":"string"},"description":"Optional ordered entrypoints so the server can report which are immediately visible"},"auto_refresh_stale":{"type":"boolean","description":"When true (default), bootstrap auto-refreshes a small stale symbol index before reporting capabilities"},"auto_refresh_stale_threshold":{"type":"integer","description":"Maximum stale file count eligible for automatic refresh during bootstrap (default 32)"}}})).with_output_schema(prepare_harness_session_output_schema()).with_annotations(mutating.clone()),
        Tool::new("register_agent_work", "[CodeLens:Session] Register the current agent intent, branch, and worktree for advisory multi-agent coordination.", json!({"required":["agent_name","branch","worktree","intent"],"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"agent_name":{"type":"string"},"branch":{"type":"string"},"worktree":{"type":"string"},"intent":{"type":"string"},"ttl_secs":{"type":"integer","description":"Optional advisory TTL in seconds (default 300, clamped to 30-3600)."}}})).with_output_schema(register_agent_work_output_schema()).with_annotations(mutating.clone().with_audit_category("coordination")),
        Tool::new("list_active_agents", "[CodeLens:Session] List active agent registrations and their claimed paths for the current project scope.", json!({"type":"object","properties":{}})).with_output_schema(list_active_agents_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("claim_files", "[CodeLens:Session] Advisory file claim for the active session. Claims downgrade readiness to caution for overlapping sessions but never hard-block writes.", json!({"required":["paths","reason"],"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"paths":{"type":"array","items":{"type":"string"},"description":"Project-relative paths to claim"},"reason":{"type":"string"},"ttl_secs":{"type":"integer","description":"Optional advisory TTL in seconds (default 300, clamped to 30-3600)."}}})).with_output_schema(claim_files_output_schema()).with_annotations(mutating.clone().with_audit_category("coordination")),
        Tool::new("release_files", "[CodeLens:Session] Release previously claimed files for the active session.", json!({"required":["paths"],"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"paths":{"type":"array","items":{"type":"string"},"description":"Project-relative paths to release"}}})).with_output_schema(release_files_output_schema()).with_annotations(mutating.clone().with_audit_category("coordination")),
        Tool::new("prepare_for_new_conversation", "[CodeLens:Session] Project context summary for a new conversation.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("summarize_changes", "[CodeLens:Session] Summarize recent git changes with symbol context.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("get_watch_status", "[CodeLens:Session] File watcher status: running, events, reindexed files.", json!({"type":"object","properties":{}})).with_output_schema(watch_status_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("prune_index_failures", "[CodeLens:Session] Remove stale index-failure records for deleted files.", json!({"type":"object","properties":{}})).with_output_schema(prune_index_failures_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("add_queryable_project", "[CodeLens:Session] Register external project for cross-project queries.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string","description":"Absolute path to the project directory"}}})).with_annotations(mutating.clone()),
        Tool::new("remove_queryable_project", "[CodeLens:Session] Unregister an external project.", json!({"required":["name"],"type":"object","properties":{"name":{"type":"string","description":"Project name to remove"}}})).with_annotations(mutating.clone()),
        Tool::new("query_project", "[CodeLens:Session] Search symbols in a registered external project.", json!({"required":["project_name","symbol_name"],"type":"object","properties":{"project_name":{"type":"string","description":"Name of the registered project"},"symbol_name":{"type":"string","description":"Symbol name to search for"},"max_results":{"type":"integer","description":"Max results (default 20)"}}})).with_annotations(ro_a.clone()),
        Tool::new("list_queryable_projects", "[CodeLens:Session] List all registered projects (active + external).", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("set_preset", "[CodeLens:Session] Switch tool preset at runtime. Auto-adjusts token budget.", json!({"required":["preset"],"type":"object","properties":{"preset":{"type":"string","enum":["minimal","balanced","full"],"description":"Target preset"},"token_budget":{"type":"integer","description":"Override token budget (default: auto per preset)"}}})).with_annotations(mutating.clone()),
        Tool::new("set_profile", "[CodeLens:Session] Switch the active role profile. Preferred for harness-oriented workflows.", json!({"required":["profile"],"type":"object","properties":{"profile":{"type":"string","enum":["planner-readonly","builder-minimal","reviewer-graph","refactor-full","ci-audit"]},"token_budget":{"type":"integer","description":"Override token budget for the active profile"}}})).with_annotations(mutating.clone()),
        Tool::new("get_capabilities", "[CodeLens:Session] Check LSP, embeddings, index freshness. Use before advanced tools.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"Optional file path to check language-specific capabilities"},"detail":{"type":"string","enum":["compact","full"],"description":"compact returns 12 core fields (~1K); full returns all 38 fields including CoreML runtime, SCIP counts, build_info (~5K). Default: full (backward-compatible)."}}})).with_output_schema(get_capabilities_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("get_tool_metrics", "[CodeLens:Session] Per-tool call counts, latency, errors. Use for self-diagnosis.", json!({"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. When present, return only that session's metrics."}}})).with_output_schema(tool_metrics_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("audit_builder_session", "[CodeLens:Session] Audit a builder/refactor session for preflight, diagnostics, and coordination discipline.", json!({"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"detail":{"type":"string","enum":["compact","full"],"description":"compact returns the ordered audit checks only; full also includes session metrics and coordination snapshot."}}})).with_output_schema(builder_session_audit_output_schema()).with_annotations(ro_a.clone()).with_max_response_tokens(4096),
        Tool::new("audit_planner_session", "[CodeLens:Session] Audit a planner/reviewer session for bootstrap, workflow-first routing, and read-side evidence discipline.", json!({"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"detail":{"type":"string","enum":["compact","full"],"description":"compact returns the ordered audit checks only; full also includes session metrics."}}})).with_output_schema(planner_session_audit_output_schema()).with_annotations(ro_a.clone()).with_max_response_tokens(4096),
        Tool::new("export_session_markdown", "[CodeLens:Session] Export session telemetry as markdown report.", json!({"type":"object","properties":{"name":{"type":"string","description":"Session name for the report header"},"session_id":{"type":"string","description":"Optional logical session id. When present, the markdown includes the role-appropriate builder or planner audit summary."}}})).with_output_schema(session_markdown_output_schema()).with_annotations(ro_p.clone()).with_max_response_tokens(4096),
        Tool::new("audit_log_query", "[CodeLens:Admin] Query the durable mutation audit log (`<project>/.codelens/audit/audit_log.sqlite`). Filter by transaction_id and/or since_ms; default limit 100 rows. Requires Admin role.", json!({"type":"object","properties":{"transaction_id":{"type":"string","description":"Stable id from a mutation response (payload.data.transaction_id). Returns the rows for that one call."},"since_ms":{"type":"integer","description":"Earliest timestamp_ms (epoch millis) to include."},"limit":{"type":"integer","description":"Max rows (default 100, capped at 1000)."}}})).with_annotations(ro_a.clone()),
        Tool::new("summarize_file", "[CodeLens:Session] Get AI-generated summary of a file's purpose and structure.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string","description":"File path to summarize"}}})).with_annotations(ro_w.clone()),
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
