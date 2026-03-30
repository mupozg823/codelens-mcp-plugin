//! MCP tool definitions and preset filtering.

use crate::protocol::{Tool, ToolAnnotations, ToolTier};
use serde_json::json;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPreset {
    Minimal,  // core tools — symbol/file/search + safe edits
    Balanced, // default — excludes niche analysis + built-in overlaps
    Full,     // all tools
}

impl ToolPreset {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "minimal" | "min" => Self::Minimal,
            "balanced" | "bal" => Self::Balanced,
            _ => Self::Full,
        }
    }
}

pub(crate) const MINIMAL_TOOLS: &[&str] = &[
    "activate_project",
    "get_current_config",
    // File (kept for non-Claude-Code clients)
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    // Symbol (core)
    "get_symbols_overview",
    "find_symbol",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_type_hierarchy",
    "refresh_symbol_index",
    "get_file_diagnostics",
    "search_workspace_symbols",
    // Mutation (safe)
    "plan_symbol_rename",
    "rename_symbol",
    "replace_symbol_body",
    "insert_content",
    "create_text_file",
    "replace",
];

pub(crate) const BALANCED_EXCLUDES: &[&str] = &[
    // ── Niche analysis (use Full preset for these) ──
    "find_circular_dependencies",
    "get_change_coupling",
    "get_symbol_importance",
    "find_dead_code",
    "refactor_extract_function",
    "get_complexity",
    "search_symbols_fuzzy",
    "check_lsp_status",
    "get_lsp_recipe",
    // ── Overlap with Claude Code built-in tools ──
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    // ── Diagnostics / session (not needed for normal work) ──
    "prepare_for_new_conversation",
    "get_watch_status",
    "get_tool_metrics",
    "export_session_markdown",
    "summarize_changes",
    "summarize_file",
    // ── Superseded by unified tools (insert_content, replace) ──
    "insert_at_line",
    "insert_before_symbol",
    "insert_after_symbol",
    "replace_lines",
    // ── Superseded by onboard_project ──
    "get_project_structure",
];

// ── Output schemas for core tools ───────────────────────────────────────
// These enable downstream agents to understand return shapes without calling.

fn symbol_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "symbols": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "kind": {"type": "string", "enum": ["function","class","method","interface","enum","variable","module","typealias"]},
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "signature": {"type": "string"},
                        "body": {"type": ["string", "null"]},
                        "name_path": {"type": "string"},
                        "id": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

fn references_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "references": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "column": {"type": "integer"},
                        "line_content": {"type": "string"},
                        "is_declaration": {"type": "boolean"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

fn impact_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "file": {"type": "string"},
            "symbol_count": {"type": "integer"},
            "total_affected_files": {"type": "integer"},
            "blast_radius": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "depth": {"type": "integer"},
                        "symbol_count": {"type": "integer"}
                    }
                }
            }
        }
    })
}

fn diagnostics_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "diagnostics": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file": {"type": "string"},
                        "line": {"type": "integer"},
                        "severity": {"type": "string"},
                        "message": {"type": "string"}
                    }
                }
            },
            "count": {"type": "integer"}
        }
    })
}

fn rename_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "success": {"type": "boolean"},
            "modified_files": {"type": "integer"},
            "total_replacements": {"type": "integer"},
            "edits": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "file_path": {"type": "string"},
                        "line": {"type": "integer"},
                        "old_text": {"type": "string"},
                        "new_text": {"type": "string"}
                    }
                }
            }
        }
    })
}

fn file_content_output_schema() -> serde_json::Value {
    json!({"type":"object","properties":{"content":{"type":"string"}}})
}

fn changed_files_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "files": {"type": "array", "items": {"type": "object", "properties": {
                "path": {"type": "string"}, "status": {"type": "string"},
                "symbol_count": {"type": "integer"}
            }}},
            "count": {"type": "integer"}
        }
    })
}

fn onboard_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "directory_structure": {"type": "array"},
            "key_files": {"type": "array"},
            "circular_dependencies": {"type": "array"},
            "health": {"type": "object"},
            "semantic": {"type": "object"}
        }
    })
}

fn memory_list_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "memories": {"type": "array", "items": {"type": "string"}},
            "count": {"type": "integer"}
        }
    })
}

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(build_tools);

pub(crate) fn tools() -> &'static [Tool] {
    &TOOLS
}

/// Check if a tool is included in a given preset.
#[cfg(feature = "http")]
pub(crate) fn is_tool_in_preset(name: &str, preset: ToolPreset) -> bool {
    match preset {
        ToolPreset::Full => true,
        ToolPreset::Minimal => MINIMAL_TOOLS.contains(&name),
        ToolPreset::Balanced => !BALANCED_EXCLUDES.contains(&name),
    }
}

fn build_tools() -> Vec<Tool> {
    let ro = ToolAnnotations::read_only();
    let destructive = ToolAnnotations::destructive();
    let mutating = ToolAnnotations::mutating();
    // Tier-specific annotations for cleaner builder chains
    let ro_p = ro.clone().with_tier(ToolTier::Primitive);
    let ro_a = ro.clone().with_tier(ToolTier::Analysis);
    let ro_w = ro.clone().with_tier(ToolTier::Workflow);
    let mut_p = mutating.clone().with_tier(ToolTier::Primitive);
    let dest_a = destructive.clone().with_tier(ToolTier::Analysis);
    let mut_w = mutating.clone().with_tier(ToolTier::Workflow);
    let mut tools = vec![
        // ── File I/O ────────────────────────────────────────────────────
        Tool::new("get_current_config", "[CodeLens:Session] Project config and index stats. Use to verify project is active.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("read_file", "[CodeLens:File] Read file contents with optional line range.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}},"required":["relative_path"]})).with_output_schema(file_content_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("list_dir", "[CodeLens:File] List directory contents, optionally recursive.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"recursive":{"type":"boolean"}},"required":["relative_path"]})).with_annotations(ro_p.clone()),
        Tool::new("find_file", "[CodeLens:File] Find files by wildcard pattern.", json!({"type":"object","properties":{"wildcard_pattern":{"type":"string"},"relative_dir":{"type":"string"}},"required":["wildcard_pattern"]})).with_annotations(ro_p.clone()),
        Tool::new("search_for_pattern", "[CodeLens:File] Regex search across files. Use smart=true for enclosing symbol context.", json!({"type":"object","properties":{"pattern":{"type":"string"},"substring_pattern":{"type":"string"},"file_glob":{"type":"string"},"max_results":{"type":"integer"},"smart":{"type":"boolean","description":"Include enclosing symbol context for each match"},"context_lines":{"type":"integer","description":"Number of context lines before and after each match (default 0)"},"context_lines_before":{"type":"integer","description":"Context lines before each match (overrides context_lines)"},"context_lines_after":{"type":"integer","description":"Context lines after each match (overrides context_lines)"}}})).with_annotations(ro_p.clone()),
        Tool::new("find_annotations", "[CodeLens:File] Find TODO/FIXME/HACK comments across the project.", json!({"type":"object","properties":{"tags":{"type":"string"},"max_results":{"type":"integer"}}})).with_annotations(ro_p.clone()),
        Tool::new("find_tests", "[CodeLens:File] Find test functions and test modules.", json!({"type":"object","properties":{"path":{"type":"string"},"max_results":{"type":"integer"}}})).with_annotations(ro_p.clone()),

        // ── Symbol Lookup (use these to understand code) ────────────────
        Tool::new("get_symbols_overview", "[CodeLens:Symbol] List all symbols in a file — structural map. Use first to understand a file.", json!({"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]})).with_output_schema(symbol_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("find_symbol", "[CodeLens:Symbol] Find function/class by exact name. Returns signature + body. Use when you know the name.", json!({"type":"object","properties":{"name":{"type":"string","description":"Symbol name to search for"},"symbol_id":{"type":"string","description":"Stable symbol ID (file#kind:name_path). Overrides name."},"file_path":{"type":"string"},"include_body":{"type":"boolean"},"exact_match":{"type":"boolean"},"max_matches":{"type":"integer"}}})).with_output_schema(symbol_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_ranked_context", "[CodeLens:Symbol] Smart context retrieval — best symbols for a query within token budget. Use for broad questions.", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"max_tokens":{"type":"integer"},"include_body":{"type":"boolean"},"depth":{"type":"integer"}},"required":["query"]})).with_output_schema(symbol_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("search_symbols_fuzzy", "[CodeLens:Symbol] Fuzzy symbol search — tolerates typos and partial names. Use when exact name is unknown.", json!({"type":"object","properties":{"query":{"type":"string","description":"Symbol name to search for"},"max_results":{"type":"integer","description":"Maximum number of results to return (default 30)"},"fuzzy_threshold":{"type":"number","description":"Minimum jaro_winkler similarity 0.0-1.0 for fuzzy matches (default 0.6)"}},"required":["query"]})).with_annotations(ro_a.clone()),
        Tool::new("get_complexity", "[CodeLens:Analysis] Cyclomatic complexity for functions. Use to find code needing refactoring.", json!({"type":"object","properties":{"path":{"type":"string"},"symbol_name":{"type":"string"}},"required":["path"]})).with_annotations(ro_a.clone()),
        Tool::new("refresh_symbol_index", "[CodeLens:Symbol] Rebuild the symbol database. Use if index is stale.", json!({"type":"object","properties":{}})).with_annotations(mut_w.clone()),
        Tool::new("get_project_structure", "[CodeLens:Symbol] Directory-level overview — file counts and symbol density per directory.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),

        // ── LSP (type-aware operations) ─────────────────────────────────
        Tool::new("find_referencing_symbols", "[CodeLens:Symbol] Find all usages of a symbol. Default: fast tree-sitter search. Set use_lsp=true for type-aware LSP precision.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File containing or declaring the symbol"},"symbol_name":{"type":"string","description":"Symbol name (default: tree-sitter search)"},"line":{"type":"integer","description":"Line number (triggers LSP path)"},"column":{"type":"integer","description":"Column number (triggers LSP path)"},"use_lsp":{"type":"boolean","description":"Force LSP lookup (slower but type-aware, requires LSP server)"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["file_path"]})).with_output_schema(references_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_file_diagnostics", "[CodeLens:Symbol] Type errors and lint issues via LSP. Use after editing code.", json!({"type":"object","properties":{"file_path":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["file_path"]})).with_output_schema(diagnostics_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("search_workspace_symbols", "[CodeLens:Symbol] LSP workspace symbol search. Use when you need type-system-aware results.", json!({"type":"object","properties":{"query":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["query","command"]})).with_annotations(ro_p.clone()),
        Tool::new("get_type_hierarchy", "[CodeLens:Symbol] Inheritance hierarchy — supertypes and subtypes of a class/interface.", json!({"type":"object","properties":{"name_path":{"type":"string"},"fully_qualified_name":{"type":"string"},"relative_path":{"type":"string"},"hierarchy_type":{"type":"string","enum":["super","sub","both"]},"depth":{"type":"integer"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}}})).with_annotations(ro_a.clone()),
        Tool::new("plan_symbol_rename", "[CodeLens:Symbol] Preview rename refactoring via LSP — check before applying.", json!({"type":"object","properties":{"file_path":{"type":"string"},"line":{"type":"integer"},"column":{"type":"integer"},"new_name":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["file_path","line","column"]})).with_annotations(ro_a.clone()),
        Tool::new("check_lsp_status", "[CodeLens:Session] Check installed LSP servers with install commands.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("get_lsp_recipe", "[CodeLens:Session] Get LSP server install instructions for a file extension.", json!({"type":"object","properties":{"extension":{"type":"string","description":"File extension (e.g. 'py', 'rs')"}},"required":["extension"]})).with_annotations(ro_p.clone()),

        // ── Analysis (architecture & dependencies) ──────────────────────
        Tool::new("get_changed_files", "[CodeLens:Analysis] Files changed since a git ref with symbol counts. Use for diff review.", json!({"type":"object","properties":{"ref":{"type":"string"},"include_untracked":{"type":"boolean"}}})).with_output_schema(changed_files_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("get_impact_analysis", "[CodeLens:Analysis] Blast radius — what files break if you change this file. Use before risky edits.", json!({"type":"object","properties":{"file_path":{"type":"string"},"max_depth":{"type":"integer"}},"required":["file_path"]})).with_output_schema(impact_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("find_scoped_references", "[CodeLens:Analysis] Classify each reference as definition/read/write/import. Use for deep refactoring analysis.", json!({"type":"object","properties":{"symbol_name":{"type":"string","description":"Symbol name to find references for"},"file_path":{"type":"string","description":"Declaration file (for sorting, optional)"},"max_results":{"type":"integer","description":"Max results (default 50)"}},"required":["symbol_name"]})).with_output_schema(references_output_schema()).with_annotations(ro_a.clone()),
        Tool::new("get_symbol_importance", "[CodeLens:Analysis] PageRank file importance — find the most critical files in the project.", json!({"type":"object","properties":{"top_n":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("find_dead_code", "[CodeLens:Analysis] Detect unused files and unreferenced symbols via call-graph.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("find_circular_dependencies", "[CodeLens:Analysis] Detect circular imports using Tarjan SCC algorithm.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),
        Tool::new("get_change_coupling", "[CodeLens:Analysis] Files that frequently change together in git history.", json!({"type":"object","properties":{"months":{"type":"integer"},"min_strength":{"type":"number"},"min_commits":{"type":"integer"},"max_results":{"type":"integer"}}})).with_annotations(ro_a.clone()),

        // ── Editing (code mutations) ────────────────────────────────────
        Tool::new("rename_symbol", "[CodeLens:Edit] Rename across project — safe multi-file refactoring. Use dry_run=true to preview.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File containing the symbol declaration"},"symbol_name":{"type":"string","description":"Current symbol name"},"name":{"type":"string","description":"Alias for symbol_name"},"new_name":{"type":"string","description":"Desired new name"},"name_path":{"type":"string","description":"Qualified name path (e.g. 'Class/method')"},"scope":{"type":"string","enum":["file","project"],"description":"Rename scope (default: project)"},"dry_run":{"type":"boolean","description":"Preview changes without modifying files"}},"required":["file_path","new_name"]})).with_output_schema(rename_output_schema()).with_annotations(dest_a.clone()),
        Tool::new("replace_symbol_body", "[CodeLens:Edit] Replace function/class body by name — tree-sitter finds boundaries. No line numbers needed.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"new_body":{"type":"string"}},"required":["relative_path","symbol_name","new_body"]})).with_output_schema(file_content_output_schema()).with_annotations(mut_w.clone()),
        Tool::new("replace_content", "[CodeLens:Edit] Find-and-replace text in a file — literal or regex mode.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"regex_mode":{"type":"boolean"}},"required":["relative_path","old_text","new_text"]})).with_annotations(mut_p.clone()),
        Tool::new("replace_lines", "[CodeLens:Edit] Replace a line range (1-indexed). Use when you know exact line numbers.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_content":{"type":"string"}},"required":["relative_path","start_line","end_line","new_content"]})).with_annotations(mut_p.clone()),
        Tool::new("delete_lines", "[CodeLens:Edit] Delete a line range (1-indexed, end exclusive).", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}},"required":["relative_path","start_line","end_line"]})).with_annotations(destructive.clone()),
        Tool::new("insert_at_line", "[CodeLens:Edit] Insert content at a line number. Use when you know the exact position.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"line":{"type":"integer"},"content":{"type":"string"}},"required":["relative_path","line","content"]})).with_annotations(mut_p.clone()),
        Tool::new("insert_before_symbol", "[CodeLens:Edit] Insert code before a named symbol — tree-sitter finds position.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}},"required":["relative_path","symbol_name","content"]})).with_annotations(mut_p.clone()),
        Tool::new("insert_after_symbol", "[CodeLens:Edit] Insert code after a named symbol — tree-sitter finds position.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}},"required":["relative_path","symbol_name","content"]})).with_annotations(mut_p.clone()),
        // ── Unified tools (preferred in BALANCED/MINIMAL) ───────────
        Tool::new("insert_content", "[CodeLens:Edit] Insert code at line, before symbol, or after symbol. Set position='line'|'before_symbol'|'after_symbol'.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"content":{"type":"string"},"position":{"type":"string","enum":["line","before_symbol","after_symbol"],"description":"Insertion position type (default: line)"},"line":{"type":"integer","description":"Line number (for position=line)"},"symbol_name":{"type":"string","description":"Symbol name (for position=before_symbol or after_symbol)"},"name_path":{"type":"string","description":"Qualified name path"}},"required":["relative_path","content"]})).with_annotations(mut_p.clone()),
        Tool::new("replace", "[CodeLens:Edit] Replace text or line range. Set mode='text' (find-replace) or mode='lines' (line range).", json!({"type":"object","properties":{"relative_path":{"type":"string"},"mode":{"type":"string","enum":["text","lines"],"description":"Replace mode (default: text)"},"old_text":{"type":"string","description":"Text to find (mode=text)"},"new_text":{"type":"string","description":"Replacement text (mode=text)"},"regex_mode":{"type":"boolean","description":"Use regex (mode=text)"},"start_line":{"type":"integer","description":"Start line (mode=lines)"},"end_line":{"type":"integer","description":"End line (mode=lines)"},"new_content":{"type":"string","description":"New content (mode=lines)"}},"required":["relative_path"]})).with_annotations(mut_p.clone()),
        Tool::new("create_text_file", "[CodeLens:Edit] Create a new file. Fails if exists unless overwrite=true.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"content":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["relative_path","content"]})).with_annotations(mut_p.clone()),
        Tool::new("analyze_missing_imports", "[CodeLens:Edit] Detect unresolved symbols and suggest imports.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File to analyze"}},"required":["file_path"]})).with_annotations(mutating.clone()),
        Tool::new("add_import", "[CodeLens:Edit] Insert an import statement at the correct position.", json!({"type":"object","properties":{"file_path":{"type":"string"},"import_statement":{"type":"string","description":"Import statement to add"}},"required":["file_path","import_statement"]})).with_annotations(mut_p.clone()),
        Tool::new("refactor_extract_function", "[CodeLens:Edit] Extract line range into new function with automatic call-site replacement.", json!({"type":"object","properties":{"file_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_name":{"type":"string","description":"Name for the new function"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}},"required":["file_path","start_line","end_line","new_name"]})).with_annotations(mut_w.clone()),

        // ── Composite (multi-step workflows) ────────────────────────────
        Tool::new("onboard_project", "[CodeLens:Session] One-shot onboarding: structure, key files (PageRank), cycles, stats. Call first on any codebase.", json!({"type":"object","properties":{}})).with_output_schema(onboard_output_schema()).with_annotations(ro_w.clone()),

        // ── Memory ──────────────────────────────────────────────────────
        Tool::new("list_memories", "[CodeLens:Memory] List project memory files under .codelens/memories.", json!({"type":"object","properties":{"topic":{"type":"string","description":"Optional topic to filter"}}})).with_output_schema(memory_list_output_schema()).with_annotations(ro_p.clone()),
        Tool::new("read_memory", "[CodeLens:Memory] Read a named project memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"}},"required":["memory_name"]})).with_annotations(ro_p.clone()),
        Tool::new("write_memory", "[CodeLens:Memory] Create or overwrite a project memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}},"required":["memory_name","content"]})).with_annotations(mutating.clone()),
        Tool::new("delete_memory", "[CodeLens:Memory] Delete a project memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"}},"required":["memory_name"]})).with_annotations(destructive.clone()),
        Tool::new("rename_memory", "[CodeLens:Memory] Rename a project memory file.", json!({"type":"object","properties":{"old_name":{"type":"string"},"new_name":{"type":"string"}},"required":["old_name","new_name"]})).with_annotations(mut_p.clone()),

        // ── Session ─────────────────────────────────────────────────────
        Tool::new("activate_project", "[CodeLens:Session] Activate project — auto-detect preset, index, frameworks.", json!({"type":"object","properties":{"project":{"type":"string","description":"Optional project name or path"}}})).with_annotations(ro_p.clone()),
        Tool::new("prepare_for_new_conversation", "[CodeLens:Session] Project context summary for a new conversation.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("summarize_changes", "[CodeLens:Session] Summarize recent git changes with symbol context.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("get_watch_status", "[CodeLens:Session] File watcher status: running, events, reindexed files.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("add_queryable_project", "[CodeLens:Session] Register external project for cross-project queries.", json!({"type":"object","properties":{"path":{"type":"string","description":"Absolute path to the project directory"}},"required":["path"]})).with_annotations(mutating.clone()),
        Tool::new("remove_queryable_project", "[CodeLens:Session] Unregister an external project.", json!({"type":"object","properties":{"name":{"type":"string","description":"Project name to remove"}},"required":["name"]})).with_annotations(mutating.clone()),
        Tool::new("query_project", "[CodeLens:Session] Search symbols in a registered external project.", json!({"type":"object","properties":{"project_name":{"type":"string","description":"Name of the registered project"},"symbol_name":{"type":"string","description":"Symbol name to search for"},"max_results":{"type":"integer","description":"Max results (default 20)"}},"required":["project_name","symbol_name"]})).with_annotations(ro_a.clone()),
        Tool::new("list_queryable_projects", "[CodeLens:Session] List all registered projects (active + external).", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("set_preset", "[CodeLens:Session] Switch tool preset at runtime. Auto-adjusts token budget.", json!({"type":"object","properties":{"preset":{"type":"string","enum":["minimal","balanced","full"],"description":"Target preset"},"token_budget":{"type":"integer","description":"Override token budget (default: auto per preset)"}},"required":["preset"]})).with_annotations(mutating.clone()),
        Tool::new("get_capabilities", "[CodeLens:Session] Check LSP, embeddings, index freshness. Use before advanced tools.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"Optional file path to check language-specific capabilities"}}})).with_annotations(ro_a.clone()),
        Tool::new("get_tool_metrics", "[CodeLens:Session] Per-tool call counts, latency, errors. Use for self-diagnosis.", json!({"type":"object","properties":{}})).with_annotations(ro_p.clone()),
        Tool::new("export_session_markdown", "[CodeLens:Session] Export session telemetry as markdown report.", json!({"type":"object","properties":{"name":{"type":"string","description":"Session name for the report header"}}})).with_annotations(ro_p.clone()),
        Tool::new("summarize_file", "[CodeLens:Session] Get AI-generated summary of a file's purpose and structure.", json!({"type":"object","properties":{"path":{"type":"string","description":"File path to summarize"}},"required":["path"]})).with_annotations(ro_w.clone()),
    ];

    // ── Semantic (feature-gated) ────────────────────────────────────
    #[cfg(feature = "semantic")]
    {
        let ro = ro;
        tools.push(Tool::new("semantic_search", "[CodeLens:Symbol] Natural language code search via embeddings — find code by meaning.", json!({"type":"object","properties":{"query":{"type":"string","description":"Natural language search query"},"max_results":{"type":"integer","description":"Max results (default 20)"}},"required":["query"]})).with_annotations(ro_p.clone()));
        tools.push(Tool::new("index_embeddings", "[CodeLens:Symbol] Build semantic embedding index. Required before semantic_search.", json!({"type":"object","properties":{}})).with_annotations(ro));
    }

    tools
}
