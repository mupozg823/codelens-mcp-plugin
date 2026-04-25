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
    let mut tools = vec![
        // ── File I/O ────────────────────────────────────────────────────
        Tool::new("get_current_config", "[CodeLens:Session] Project config and index stats. Use to verify project is active.", json!({"type":"object","properties":{}})).with_output_schema(get_current_config_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("read_file", "[CodeLens:File] Read file contents with optional line range.", json!({"required":["relative_path"],"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}}})).with_output_schema(file_content_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("list_dir", "[CodeLens:File] List directory contents, optionally recursive.", json!({"required":["relative_path"],"type":"object","properties":{"relative_path":{"type":"string"},"recursive":{"type":"boolean"}}})).with_annotations(ro_p.clone()),
        Tool::new("find_file", "[CodeLens:File] Find files by wildcard pattern.", json!({"required":["wildcard_pattern"],"type":"object","properties":{"wildcard_pattern":{"type":"string"},"relative_dir":{"type":"string"}}})).with_annotations(ro_p.clone()),
        Tool::new("search_for_pattern", "[CodeLens:File] Regex search across files. Use smart=true for enclosing symbol context.", json!({"type":"object","properties":{"pattern":{"type":"string"},"substring_pattern":{"type":"string"},"file_glob":{"type":"string"},"max_results":{"type":"integer"},"smart":{"type":"boolean","description":"Include enclosing symbol context for each match"},"context_lines":{"type":"integer","description":"Number of context lines before and after each match (default 0)"},"context_lines_before":{"type":"integer","description":"Context lines before each match (overrides context_lines)"},"context_lines_after":{"type":"integer","description":"Context lines after each match (overrides context_lines)"}}})).with_output_schema(search_for_pattern_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("find_annotations", "[CodeLens:File] Find TODO/FIXME/HACK comments across the project.", json!({"type":"object","properties":{"tags":{"type":"string"},"max_results":{"type":"integer"}}})).with_output_schema(find_annotations_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("find_tests", "[CodeLens:File] Find test functions and test modules.", json!({"type":"object","properties":{"path":{"type":"string"},"max_results":{"type":"integer"}}})).with_output_schema(find_tests_output_schema()).with_annotations(ro_p.clone()),

        // ── Symbol Lookup (use these to understand code) ────────────────
        Tool::new("get_symbols_overview", "[CodeLens:Symbol] List all symbols in a file — structural map. Use first to understand a file.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}}})).with_output_schema(symbol_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("find_symbol", "[CodeLens:Symbol] Find function/class by exact name. Returns signature + body.", json!({"type":"object","properties":{"name":{"type":"string","description":"Symbol name to search for"},"symbol_id":{"type":"string","description":"Stable symbol ID (file#kind:name_path). Overrides name."},"file_path":{"type":"string"},"include_body":{"type":"boolean"},"exact_match":{"type":"boolean"},"max_matches":{"type":"integer"}}})).with_output_schema(symbol_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_ranked_context", "[CodeLens:Symbol] Smart context retrieval — best symbols for a query within token budget.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"max_tokens":{"type":"integer"},"include_body":{"type":"boolean"},"depth":{"type":"integer"},"disable_semantic":{"type":"boolean","description":"Disable semantic/hybrid ranking and use structural signals only"}}})).with_output_schema(ranked_context_output_schema()).with_annotations(ro_a.clone()).with_max_response_tokens(4096),
        Tool::new("bm25_symbol_search", "[CodeLens:Symbol] Sparse BM25-F symbol retrieval — best for identifiers, signatures, path tokens, and short lexical phrases.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string"},"max_results":{"type":"integer","description":"Maximum number of results to return (default 10)"},"include_tests":{"type":"boolean","description":"Include test symbols in the candidate pool"},"include_generated":{"type":"boolean","description":"Include generated symbols in the candidate pool"}}})).with_output_schema(bm25_symbol_search_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("search_symbols_fuzzy", "[CodeLens:Symbol] Fuzzy symbol search — tolerates typos and partial names.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string","description":"Symbol name to search for"},"max_results":{"type":"integer","description":"Maximum number of results to return (default 30)"},"fuzzy_threshold":{"type":"number","description":"Minimum jaro_winkler similarity 0.0-1.0 for fuzzy matches (default 0.6)"},"disable_semantic":{"type":"boolean","description":"Disable semantic score blending and use lexical search only"}}})).with_annotations(ro_a.clone()),
        Tool::new("get_complexity", "[CodeLens:Analysis] Cyclomatic complexity for functions. Use to find code needing refactoring.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string"},"symbol_name":{"type":"string"}}})).with_annotations(ro_a.clone()),
        Tool::new("refresh_symbol_index", "[CodeLens:Symbol] Rebuild the symbol database. Use if index is stale.", json!({"type":"object","properties":{}})).with_annotations(mut_w.clone()),
        Tool::new("get_project_structure", "[CodeLens:Symbol] Directory-level overview — file counts and symbol density per directory.", json!({"type":"object","properties":{}})).with_output_schema(get_project_structure_output_schema()).with_annotations(ro_p.clone()),

        // ── LSP (type-aware operations) ─────────────────────────────────
        Tool::new("find_referencing_symbols", "[CodeLens:Symbol] Find all usages of a symbol. use_lsp=true for type-aware precision.", json!({"required":["file_path"],"type":"object","properties":{"file_path":{"type":"string","description":"File containing or declaring the symbol"},"symbol_name":{"type":"string","description":"Symbol name (default: tree-sitter search)"},"line":{"type":"integer","description":"Line number (triggers LSP path)"},"column":{"type":"integer","description":"Column number (triggers LSP path)"},"use_lsp":{"type":"boolean","description":"Force LSP lookup (slower but type-aware, requires LSP server)"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}}})).with_output_schema(references_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_file_diagnostics", "[CodeLens:Symbol] Type errors and lint issues via LSP. Use after editing code.", json!({"required":["file_path"],"type":"object","properties":{"file_path":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}}})).with_output_schema(diagnostics_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("search_workspace_symbols", "[CodeLens:Symbol] LSP workspace symbol search. Use when you need type-system-aware results. Requires an LSP server binary via `command` (e.g. rust-analyzer / pyright); the handler returns a structured hint toward `bm25_symbol_search` when omitted.", json!({"required":["query"],"type":"object","properties":{"query":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}}})).with_annotations(ro_p.clone()),
        Tool::new("get_type_hierarchy", "[CodeLens:Symbol] Inheritance hierarchy — supertypes and subtypes of a class/interface.", json!({"type":"object","properties":{"name_path":{"type":"string"},"fully_qualified_name":{"type":"string"},"relative_path":{"type":"string"},"hierarchy_type":{"type":"string","enum":["super","sub","both"]},"depth":{"type":"integer"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}}})).with_output_schema(get_type_hierarchy_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("plan_symbol_rename", "[CodeLens:Symbol] Preview rename refactoring via LSP — check before applying.", json!({"required":["file_path","line","column"],"type":"object","properties":{"file_path":{"type":"string"},"line":{"type":"integer"},"column":{"type":"integer"},"new_name":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}}})).with_annotations(ro_a.clone()),
        Tool::new("check_lsp_status", "[CodeLens:Session] Check installed LSP servers with install commands.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("get_lsp_recipe", "[CodeLens:Session] Get LSP server install instructions for a file extension.", json!({"required":["extension"],"type":"object","properties":{"extension":{"type":"string","description":"File extension (e.g. 'py', 'rs')"}}})).with_annotations(ro_p.clone()),

        // ── Analysis (architecture & dependencies) ──────────────────────
        Tool::new("get_changed_files", "[CodeLens:Analysis] Files changed since a git ref with symbol counts. Use for diff review.", json!({"type":"object","properties":{"ref":{"type":"string"},"include_untracked":{"type":"boolean"}}})).with_output_schema(changed_files_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_impact_analysis", "[DEPRECATED v1.12 → removal v2.0] Use impact_report directly. [CodeLens:Analysis] Blast radius — what files break if you change this file. Use before risky edits.", json!({"required":["file_path"],"type":"object","properties":{"file_path":{"type":"string"},"max_depth":{"type":"integer"}}})).with_output_schema(impact_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("get_callers", "[CodeLens:Analysis] Find functions that call a function. Returns bounded call-graph edges with backend/confidence metadata.", json!({"required":["function_name"],"type":"object","properties":{"function_name":{"type":"string"},"file_path":{"type":"string","description":"Optional file scope for caller search"},"max_results":{"type":"integer"}}})).with_output_schema(get_callers_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("get_callees", "[CodeLens:Analysis] Find functions called by a function. Use file_path when duplicate function names exist.", json!({"required":["function_name"],"type":"object","properties":{"function_name":{"type":"string"},"file_path":{"type":"string"},"max_results":{"type":"integer"}}})).with_output_schema(get_callees_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("find_scoped_references", "[CodeLens:Analysis] Classify each reference as definition/read/write/import.", json!({"required":["symbol_name"],"type":"object","properties":{"symbol_name":{"type":"string","description":"Symbol name to find references for"},"file_path":{"type":"string","description":"Declaration file (for sorting, optional)"},"max_results":{"type":"integer","description":"Max results (default 50)"}}})).with_output_schema(references_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("get_symbol_importance", "[CodeLens:Analysis] PageRank file importance — find the most critical files in the project.", json!({"type":"object","properties":{"top_n":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("find_dead_code", "[DEPRECATED v1.12 → removal v2.0] Use dead_code_report directly. [CodeLens:Analysis] Detect unused files and unreferenced symbols via call-graph.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("find_circular_dependencies", "[CodeLens:Analysis] Detect circular imports using Tarjan SCC algorithm.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("get_change_coupling", "[CodeLens:Analysis] Files that frequently change together in git history.", json!({"type":"object","properties":{"months":{"type":"integer"},"min_strength":{"type":"number"},"min_commits":{"type":"integer"},"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),

        // ── Editing (code mutations) ────────────────────────────────────
        Tool::new("rename_symbol", "[CodeLens:Edit] Rename across project — safe multi-file refactoring. Use dry_run=true to preview.", json!({"required":["file_path","new_name"],"type":"object","properties":{"file_path":{"type":"string","description":"File containing the symbol declaration"},"symbol_name":{"type":"string","description":"Current symbol name"},"name":{"type":"string","description":"Alias for symbol_name"},"new_name":{"type":"string","description":"Desired new name"},"name_path":{"type":"string","description":"Qualified name path (e.g. 'Class/method')"},"scope":{"type":"string","enum":["file","project"],"description":"Rename scope (default: project; tree-sitter backend only)"},"semantic_edit_backend":{"type":"string","enum":["tree-sitter","lsp"],"description":"Opt-in precise edit backend. Default tree-sitter preserves fast local behavior; lsp uses textDocument/rename."},"line":{"type":"integer","description":"1-based declaration line for semantic_edit_backend=lsp; derived from symbol index when omitted"},"column":{"type":"integer","description":"1-based declaration column for semantic_edit_backend=lsp; derived from symbol index when omitted"},"command":{"type":"string","description":"Optional LSP server command for semantic_edit_backend=lsp"},"args":{"type":"array","items":{"type":"string"},"description":"Optional LSP server args for semantic_edit_backend=lsp"},"dry_run":{"type":"boolean","description":"Preview changes without modifying files"}}})).with_output_schema(rename_output_schema()).with_annotations(dest_a.clone()),
        Tool::new("replace_symbol_body", "[CodeLens:Edit] Replace function/class body by name — tree-sitter finds boundaries. No line numbers needed.", json!({"required":["relative_path","symbol_name","new_body"],"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"new_body":{"type":"string"}}})).with_output_schema(file_content_output_schema()).with_annotations(mut_w.clone()),
        Tool::new("replace_content", "[CodeLens:Edit] Find-and-replace text in a file — literal or regex mode.", json!({"required":["relative_path","old_text","new_text"],"type":"object","properties":{"relative_path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"regex_mode":{"type":"boolean"}}})).with_output_schema(replace_content_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("replace_lines", "[CodeLens:Edit] Replace a line range (1-indexed). Use when you know exact line numbers.", json!({"required":["relative_path","start_line","end_line","new_content"],"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_content":{"type":"string"}}})).with_output_schema(file_content_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("delete_lines", "[CodeLens:Edit] Delete a line range (1-indexed, end exclusive).", json!({"required":["relative_path","start_line","end_line"],"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}}})).with_output_schema(file_content_output_schema()).with_annotations(destructive.clone()),
        Tool::new("insert_at_line", "[CodeLens:Edit] Insert content at a line number. Use when you know the exact position.", json!({"required":["relative_path","line","content"],"type":"object","properties":{"relative_path":{"type":"string"},"line":{"type":"integer"},"content":{"type":"string"}}})).with_output_schema(file_content_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("insert_before_symbol", "[CodeLens:Edit] Insert code before a named symbol — tree-sitter finds position.", json!({"required":["relative_path","symbol_name","content"],"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}}})).with_output_schema(file_content_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("insert_after_symbol", "[CodeLens:Edit] Insert code after a named symbol — tree-sitter finds position.", json!({"required":["relative_path","symbol_name","content"],"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}}})).with_output_schema(file_content_output_schema()).with_annotations(mut_p.clone()),
        // ── Unified tools (preferred in BALANCED/MINIMAL) ───────────
        Tool::new("insert_content", "[CodeLens:Edit] Insert code at position='line'|'before_symbol'|'after_symbol'.", json!({"required":["relative_path","content"],"type":"object","properties":{"relative_path":{"type":"string"},"content":{"type":"string"},"position":{"type":"string","enum":["line","before_symbol","after_symbol"],"description":"Insertion position type (default: line)"},"line":{"type":"integer","description":"Line number (for position=line)"},"symbol_name":{"type":"string","description":"Symbol name (for position=before_symbol or after_symbol)"},"name_path":{"type":"string","description":"Qualified name path"}}})).with_output_schema(file_content_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("replace", "[CodeLens:Edit] Replace text or line range. Set mode='text' (find-replace) or mode='lines' (line range).", json!({"required":["relative_path"],"type":"object","properties":{"relative_path":{"type":"string"},"mode":{"type":"string","enum":["text","lines"],"description":"Replace mode (default: text)"},"old_text":{"type":"string","description":"Text to find (mode=text)"},"new_text":{"type":"string","description":"Replacement text (mode=text)"},"regex_mode":{"type":"boolean","description":"Use regex (mode=text)"},"start_line":{"type":"integer","description":"Start line (mode=lines)"},"end_line":{"type":"integer","description":"End line (mode=lines)"},"new_content":{"type":"string","description":"New content (mode=lines)"}}})).with_output_schema(replace_content_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("create_text_file", "[CodeLens:Edit] Create a new file. Fails if exists unless overwrite=true.", json!({"required":["relative_path","content"],"type":"object","properties":{"relative_path":{"type":"string"},"content":{"type":"string"},"overwrite":{"type":"boolean"}}})).with_output_schema(create_text_file_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("analyze_missing_imports", "[CodeLens:Edit] Detect unresolved symbols and suggest imports.", json!({"required":["file_path"],"type":"object","properties":{"file_path":{"type":"string","description":"File to analyze"}}})).with_annotations(mutating.clone()),
        Tool::new("add_import", "[CodeLens:Edit] Insert an import statement at the correct position.", json!({"required":["file_path","import_statement"],"type":"object","properties":{"file_path":{"type":"string"},"import_statement":{"type":"string","description":"Import statement to add"}}})).with_output_schema(add_import_output_schema()).with_annotations(mut_p.clone()),
        Tool::new("refactor_extract_function", "[CodeLens:Edit] Extract line range into new function with automatic call-site replacement.", json!({"required":["file_path","start_line","end_line","new_name"],"type":"object","properties":{"file_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_name":{"type":"string","description":"Name for the new function"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}}})).with_annotations(mut_w.clone()),
        Tool::new("refactor_inline_function", "[CodeLens:Edit] Inline a function: replace all call sites with body, remove definition.", json!({"required":["file_path","function_name"],"type":"object","properties":{"file_path":{"type":"string","description":"File containing the function definition"},"function_name":{"type":"string","description":"Function to inline"},"name_path":{"type":"string","description":"Qualified name path (e.g. Class/method)"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}}})).with_annotations(mut_w.clone()),
        Tool::new("refactor_move_to_file", "[CodeLens:Edit] Move a symbol to another file, updating imports across the project.", json!({"required":["file_path","symbol_name","target_file"],"type":"object","properties":{"file_path":{"type":"string","description":"Source file"},"symbol_name":{"type":"string","description":"Symbol to move"},"target_file":{"type":"string","description":"Destination file"},"name_path":{"type":"string","description":"Qualified name path"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}}})).with_annotations(dest_a.clone()),
        Tool::new("refactor_change_signature", "[CodeLens:Edit] Change function parameters and update all call sites.", json!({"required":["name"],"type":"object","properties":{"file_path":{"type":"string","description":"File containing the function"},"function_name":{"type":"string","description":"Function to modify"},"name_path":{"type":"string","description":"Qualified name path"},"new_parameters":{"type":"array","items":{"type":"object","properties":{"name":{"type":"string"},"type":{"type":"string"},"default":{"type":"string"}}},"description":"New parameter list"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}},"required":["file_path","function_name","new_parameters"]})).with_annotations(dest_a.clone()),

        Tool::new("propagate_deletions", "[CodeLens:Edit] Analyze what breaks if a symbol is deleted and list affected references/imports for cleanup.", json!({"required":["file_path","symbol_name"],"type":"object","properties":{"file_path":{"type":"string","description":"File containing the symbol"},"symbol_name":{"type":"string","description":"Symbol to analyze for deletion"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}}})).with_annotations(mut_w.clone()),

        // ── Composite (multi-step workflows) ────────────────────────────
        Tool::new("explore_codebase", "[CodeLens:Workflow] Problem-first entrypoint for codebase exploration. Use query for targeted context, or call without arguments for onboarding.", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"max_tokens":{"type":"integer"},"include_body":{"type":"boolean"},"depth":{"type":"integer"},"disable_semantic":{"type":"boolean"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("trace_request_path", "[CodeLens:Workflow] Trace a request or execution path from a function, symbol, or entrypoint.", json!({"type":"object","properties":{"function_name":{"type":"string"},"symbol":{"type":"string"},"entrypoint":{"type":"string"},"max_depth":{"type":"integer"},"max_results":{"type":"integer"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("review_architecture", "[CodeLens:Workflow] Review project or module architecture, boundaries, coupling, and optionally render a diagram.", json!({"type":"object","properties":{"path":{"type":"string"},"include_diagram":{"type":"boolean"},"max_nodes":{"type":"integer"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("plan_safe_refactor", "[CodeLens:Workflow] Preview a safe refactor plan. Uses rename safety when file_path+symbol are given; otherwise falls back to broader refactor safety analysis.", json!({"type":"object","properties":{"task":{"type":"string"},"symbol":{"type":"string"},"path":{"type":"string"},"file_path":{"type":"string"},"new_name":{"type":"string"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("audit_security_context", "[DEPRECATED v1.12 → removal v2.0] Use semantic_code_review directly. [CodeLens:Workflow] Review changed files for security-sensitive context, references, and semantic risk cues.", json!({"type":"object","properties":{"changed_files":{"type":"array","items":{"type":"string"}}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("analyze_change_impact", "[DEPRECATED v1.12 → removal v2.0] Use impact_report directly. [CodeLens:Workflow] Problem-first impact entrypoint for changed files or a target path.", json!({"type":"object","properties":{"path":{"type":"string"},"changed_files":{"type":"array","items":{"type":"string"}}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("cleanup_duplicate_logic", "[CodeLens:Workflow] Surface duplicate or removable logic before cleanup. Uses semantic duplicate search when available, otherwise bounded dead-code evidence.", json!({"type":"object","properties":{"threshold":{"type":"number"},"max_pairs":{"type":"integer"},"scope":{"type":"string"},"max_results":{"type":"integer"}}})).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("review_changes", "[CodeLens:Workflow] Pre-merge review: diff-aware references or impact analysis for changed files.", json!({
            "type": "object",
            "properties": {
                "changed_files": {"type": "array", "items": {"type": "string"}, "description": "File paths that changed"},
                "task": {"type": "string", "description": "Review focus description"},
                "path": {"type": "string", "description": "Scope path"}
            }
        })).with_output_schema(workflow_alias_output_schema()).with_annotations(ro_w.clone()).with_max_response_tokens(3072),
        Tool::new("assess_change_readiness", "[DEPRECATED v1.12 → removal v2.0] Use verify_change_readiness directly. [CodeLens:Workflow] Preflight gate: verify mutation safety before code changes.", json!({
            "type": "object",
            "properties": {
                "file_path": {"type": "string", "description": "Target file to check"},
                "path": {"type": "string", "description": "Directory scope"},
                "task": {"type": "string", "description": "Description of the planned change"}
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
        Tool::new("get_capabilities", "[CodeLens:Session] Check LSP, embeddings, index freshness. Use before advanced tools.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"Optional file path to check language-specific capabilities"}}})).with_output_schema(get_capabilities_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("get_tool_metrics", "[CodeLens:Session] Per-tool call counts, latency, errors. Use for self-diagnosis.", json!({"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. When present, return only that session's metrics."}}})).with_output_schema(tool_metrics_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("audit_builder_session", "[CodeLens:Session] Audit a builder/refactor session for preflight, diagnostics, and coordination discipline.", json!({"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"detail":{"type":"string","enum":["compact","full"],"description":"compact returns the ordered audit checks only; full also includes session metrics and coordination snapshot."}}})).with_output_schema(builder_session_audit_output_schema()).with_annotations(ro_a.clone()).with_max_response_tokens(4096),
        Tool::new("audit_planner_session", "[CodeLens:Session] Audit a planner/reviewer session for bootstrap, workflow-first routing, and read-side evidence discipline.", json!({"type":"object","properties":{"session_id":{"type":"string","description":"Optional logical session id. Defaults to the active _session_id."},"detail":{"type":"string","enum":["compact","full"],"description":"compact returns the ordered audit checks only; full also includes session metrics."}}})).with_output_schema(planner_session_audit_output_schema()).with_annotations(ro_a.clone()).with_max_response_tokens(4096),
        Tool::new("export_session_markdown", "[CodeLens:Session] Export session telemetry as markdown report.", json!({"type":"object","properties":{"name":{"type":"string","description":"Session name for the report header"},"session_id":{"type":"string","description":"Optional logical session id. When present, the markdown includes the role-appropriate builder or planner audit summary."}}})).with_output_schema(session_markdown_output_schema()).with_annotations(ro_p.clone()).with_max_response_tokens(4096),
        Tool::new("summarize_file", "[CodeLens:Session] Get AI-generated summary of a file's purpose and structure.", json!({"required":["path"],"type":"object","properties":{"path":{"type":"string","description":"File path to summarize"}}})).with_annotations(ro_w.clone()),
    ];

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
