//! MCP tool definitions and preset filtering.

use crate::protocol::{Tool, ToolAnnotations};
use serde_json::json;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPreset {
    Minimal,  // 21 core tools — symbol/file/search only
    Balanced, // 34 tools — excludes niche + built-in overlaps
    Full,     // all 50 tools (52 with semantic feature)
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
    "get_current_config",
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    "get_symbols_overview",
    "find_symbol",
    "get_ranked_context",
    "find_referencing_symbols",
    "get_type_hierarchy",
    "refresh_symbol_index",
    "get_file_diagnostics",
    "search_workspace_symbols",
    "plan_symbol_rename",
    "rename_symbol",
    "replace_symbol_body",
    "insert_before_symbol",
    "insert_after_symbol",
    "create_text_file",
    "replace_content",
    "find_referencing_code_snippets",
];

pub(crate) const BALANCED_EXCLUDES: &[&str] = &[
    // Niche analysis tools
    "find_circular_dependencies",
    "get_change_coupling",
    "get_symbol_importance",
    "find_dead_code",
    "refactor_extract_function",
    "get_complexity",
    "search_symbols_fuzzy",
    "check_lsp_status",
    // Overlap with Claude Code built-in tools (Read, Glob, Grep)
    "read_file",
    "list_dir",
    "find_file",
    "search_for_pattern",
    // Redundant session/memory tools
    "edit_memory", // identical to write_memory
    "prepare_for_new_conversation",
    "initial_instructions",
    "check_onboarding_performed",
];

static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(build_tools);

pub(crate) fn tools() -> &'static [Tool] {
    &TOOLS
}

fn build_tools() -> Vec<Tool> {
    let ro = ToolAnnotations::read_only();
    let destructive = ToolAnnotations::destructive();
    let mutating = ToolAnnotations::mutating();
    let mut tools = vec![
        // ── Filesystem / search (read-only) ─────────────────────────────
        Tool::new("get_current_config", "Return Rust core runtime information and symbol index stats.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("read_file", "Read the contents of a file with optional line range.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}},"required":["relative_path"]})).with_annotations(ro.clone()),
        Tool::new("list_dir", "List contents of a directory with optional recursive traversal.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"recursive":{"type":"boolean"}},"required":["relative_path"]})).with_annotations(ro.clone()),
        Tool::new("find_file", "Find files matching a wildcard pattern within the project or specified directory.", json!({"type":"object","properties":{"wildcard_pattern":{"type":"string"},"relative_dir":{"type":"string"}},"required":["wildcard_pattern"]})).with_annotations(ro.clone()),
        Tool::new("search_for_pattern", "Search for a regex pattern across project files. Set smart=true to include enclosing symbol context (Smart Excerpt).", json!({"type":"object","properties":{"pattern":{"type":"string"},"substring_pattern":{"type":"string"},"file_glob":{"type":"string"},"max_results":{"type":"integer"},"smart":{"type":"boolean","description":"Include enclosing symbol context for each match"},"context_lines":{"type":"integer","description":"Number of context lines before and after each match (default 0)"},"context_lines_before":{"type":"integer","description":"Context lines before each match (overrides context_lines)"},"context_lines_after":{"type":"integer","description":"Context lines after each match (overrides context_lines)"}}})).with_annotations(ro.clone()),
        Tool::new("find_annotations", "Find annotation comments such as TODO, FIXME, and HACK across the project.", json!({"type":"object","properties":{"tags":{"type":"string"},"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("find_tests", "Find test functions or test blocks across the project using regex heuristics.", json!({"type":"object","properties":{"path":{"type":"string"},"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),

        // ── Symbols / index ──────────────────────────────────────────────
        Tool::new("get_complexity", "Calculate approximate cyclomatic complexity for functions or methods in a file.", json!({"type":"object","properties":{"path":{"type":"string"},"symbol_name":{"type":"string"}},"required":["path"]})).with_annotations(ro.clone()),
        Tool::new("get_symbols_overview", "[CodeLens] List all functions, classes, methods in a file — structural code map without reading the full file.", json!({"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]})).with_annotations(ro.clone()),
        Tool::new("find_symbol", "[CodeLens] Find a function/class/method by name or stable ID. Returns signature, location, and optionally the body — no need to read the whole file.", json!({"type":"object","properties":{"name":{"type":"string","description":"Symbol name to search for"},"symbol_id":{"type":"string","description":"Stable symbol ID (file#kind:name_path). Overrides name."},"file_path":{"type":"string"},"include_body":{"type":"boolean"},"exact_match":{"type":"boolean"},"max_matches":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("get_ranked_context", "[CodeLens] Get the most relevant code symbols for a query, auto-ranked within a token budget — smart context selection.", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"max_tokens":{"type":"integer"},"include_body":{"type":"boolean"},"depth":{"type":"integer"}},"required":["query"]})).with_annotations(ro.clone()),
        Tool::new("search_symbols_fuzzy", "Hybrid symbol search: exact match (score 100), substring match (score 60), and fuzzy jaro_winkler match (score by similarity).", json!({"type":"object","properties":{"query":{"type":"string","description":"Symbol name to search for"},"max_results":{"type":"integer","description":"Maximum number of results to return (default 30)"},"fuzzy_threshold":{"type":"number","description":"Minimum jaro_winkler similarity 0.0-1.0 for fuzzy matches (default 0.6)"}},"required":["query"]})).with_annotations(ro.clone()),
        Tool::new("refresh_symbol_index", "Rebuild the cached symbol index for the current project.", json!({"type":"object","properties":{}})).with_annotations(mutating.clone()),
        Tool::new("get_project_structure", "[CodeLens] Hierarchical project overview — directory-level file count and symbol density. Use as Level 1 pruning before drilling into specific files.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),

        // ── LSP ──────────────────────────────────────────────────────────
        Tool::new("find_referencing_symbols", "[CodeLens] Find all usages of a symbol across the project — uses LSP when available, falls back to scope-aware text search.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File containing or declaring the symbol"},"symbol_name":{"type":"string","description":"Symbol name for text-based search (skips LSP)"},"line":{"type":"integer","description":"Line number for LSP lookup"},"column":{"type":"integer","description":"Column number for LSP lookup"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["file_path"]})).with_annotations(ro.clone()),
        Tool::new("get_file_diagnostics", "[CodeLens] Get type errors, warnings, and lint issues for a file via LSP — catches bugs before running the code.", json!({"type":"object","properties":{"file_path":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["file_path"]})).with_annotations(ro.clone()),
        Tool::new("search_workspace_symbols", "Search workspace symbols through a pooled stdio LSP server. command is required because no file path is available for inference.", json!({"type":"object","properties":{"query":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["query","command"]})).with_annotations(ro.clone()),
        Tool::new("get_type_hierarchy", "Get the type hierarchy through a pooled stdio LSP server.", json!({"type":"object","properties":{"name_path":{"type":"string"},"fully_qualified_name":{"type":"string"},"relative_path":{"type":"string"},"hierarchy_type":{"type":"string","enum":["super","sub","both"]},"depth":{"type":"integer"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}}})).with_annotations(ro.clone()),
        Tool::new("plan_symbol_rename", "Plan a safe symbol rename through pooled LSP prepareRename without applying edits.", json!({"type":"object","properties":{"file_path":{"type":"string"},"line":{"type":"integer"},"column":{"type":"integer"},"new_name":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["file_path","line","column"]})).with_annotations(ro.clone()),
        Tool::new("check_lsp_status", "Check which LSP servers are installed on this machine and which are missing, with install commands.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),

        // ── Graph / analysis (read-only) ─────────────────────────────────
        Tool::new("get_changed_files", "Return files changed compared to a git ref, with symbol counts. Also accepts legacy name 'get_diff_symbols'.", json!({"type":"object","properties":{"ref":{"type":"string"},"include_untracked":{"type":"boolean"}}})).with_annotations(ro.clone()),
        Tool::new("get_impact_analysis", "[CodeLens] Analyze what breaks if you change a file — shows affected files (blast radius), reverse dependencies, and symbol count.", json!({"type":"object","properties":{"file_path":{"type":"string"},"max_depth":{"type":"integer"}},"required":["file_path"]})).with_annotations(ro.clone()),
        Tool::new("get_symbol_importance", "Return file importance ranking based on import-graph PageRank for supported Python/JS/TS projects.", json!({"type":"object","properties":{"top_n":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("find_dead_code", "Multi-pass dead code detection: unreferenced files + unreferenced symbols via call-graph. Also accepts legacy name 'find_dead_code_v2'.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("find_scoped_references", "Scope-aware reference search using tree-sitter AST. Classifies each reference as definition/read/write/import with enclosing scope context.", json!({"type":"object","properties":{"symbol_name":{"type":"string","description":"Symbol name to find references for"},"file_path":{"type":"string","description":"Declaration file (for sorting, optional)"},"max_results":{"type":"integer","description":"Max results (default 50)"}},"required":["symbol_name"]})).with_annotations(ro.clone()),
        Tool::new("find_circular_dependencies", "Detect circular import dependencies in the project using Tarjan SCC algorithm on the import graph.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("get_change_coupling", "Analyze git history to find files that frequently change together (temporal coupling).", json!({"type":"object","properties":{"months":{"type":"integer"},"min_strength":{"type":"number"},"min_commits":{"type":"integer"},"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),

        // ── Mutation / editing ───────────────────────────────────────────
        Tool::new("rename_symbol", "[CodeLens] Rename a function/class/variable across the entire project — safe multi-file refactoring with dry_run preview.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File containing the symbol declaration"},"symbol_name":{"type":"string","description":"Current symbol name"},"name":{"type":"string","description":"Alias for symbol_name"},"new_name":{"type":"string","description":"Desired new name"},"name_path":{"type":"string","description":"Qualified name path (e.g. 'Class/method')"},"scope":{"type":"string","enum":["file","project"],"description":"Rename scope (default: project)"},"dry_run":{"type":"boolean","description":"Preview changes without modifying files"}},"required":["file_path","new_name"]})).with_annotations(destructive.clone()),
        Tool::new("delete_lines", "Delete lines [start_line, end_line) from a file (1-indexed, end exclusive). Returns the modified content.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}},"required":["relative_path","start_line","end_line"]})).with_annotations(destructive.clone()),
        Tool::new("create_text_file", "Create a new file with the given content. If overwrite is false and the file already exists, returns an error.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"content":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["relative_path","content"]})).with_annotations(mutating.clone()),
        Tool::new("insert_at_line", "Insert content at a given line number (1-indexed) in a file. Returns the modified content.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"line":{"type":"integer"},"content":{"type":"string"}},"required":["relative_path","line","content"]})).with_annotations(mutating.clone()),
        Tool::new("replace_lines", "Replace lines [start_line, end_line) in a file with new_content (1-indexed, end exclusive). Returns the modified content.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_content":{"type":"string"}},"required":["relative_path","start_line","end_line","new_content"]})).with_annotations(mutating.clone()),
        Tool::new("replace_content", "Replace old_text with new_text in a file, either literal or regex. Returns modified content and replacement count.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"regex_mode":{"type":"boolean"}},"required":["relative_path","old_text","new_text"]})).with_annotations(mutating.clone()),
        Tool::new("replace_symbol_body", "[CodeLens] Replace a function/class body by name — no line counting needed. Tree-sitter finds the exact symbol boundaries.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"new_body":{"type":"string"}},"required":["relative_path","symbol_name","new_body"]})).with_annotations(mutating.clone()),
        Tool::new("insert_before_symbol", "Insert content immediately before a named symbol in a file using tree-sitter byte offsets. Optionally disambiguate with name_path.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}},"required":["relative_path","symbol_name","content"]})).with_annotations(mutating.clone()),
        Tool::new("insert_after_symbol", "Insert content immediately after a named symbol in a file using tree-sitter byte offsets. Optionally disambiguate with name_path.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}},"required":["relative_path","symbol_name","content"]})).with_annotations(mutating.clone()),
        // Auto-import
        Tool::new("analyze_missing_imports", "Detect unresolved symbols in a file and suggest import statements from the project's symbol index.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File to analyze"}},"required":["file_path"]})).with_annotations(mutating.clone()),
        Tool::new("add_import", "Insert an import statement at the correct position in a file.", json!({"type":"object","properties":{"file_path":{"type":"string"},"import_statement":{"type":"string","description":"Import statement to add"}},"required":["file_path","import_statement"]})).with_annotations(mutating.clone()),

        // ── Composite ────────────────────────────────────────────────────
        Tool::new("onboard_project", "One-shot project onboarding: returns directory structure, top-10 important files (PageRank), circular dependencies, and index stats. Call this first when exploring a new codebase.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("refactor_extract_function", "Extract a line range into a new function. Replaces the original lines with a function call. Use dry_run=true to preview.", json!({"type":"object","properties":{"file_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_name":{"type":"string","description":"Name for the new function"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}},"required":["file_path","start_line","end_line","new_name"]})).with_annotations(mutating.clone()),

        // ── Memory ───────────────────────────────────────────────────────
        Tool::new("list_memories", "Lists all memory files stored under .serena/memories.", json!({"type":"object","properties":{"topic":{"type":"string","description":"Optional topic to filter"}}})).with_annotations(ro.clone()),
        Tool::new("read_memory", "Reads the content of a named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"}},"required":["memory_name"]})).with_annotations(ro.clone()),
        Tool::new("write_memory", "Writes (creates or overwrites) a named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}},"required":["memory_name","content"]})).with_annotations(mutating.clone()),
        Tool::new("delete_memory", "Deletes a named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"}},"required":["memory_name"]})).with_annotations(destructive.clone()),
        Tool::new("edit_memory", "Replaces the content of an existing named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}},"required":["memory_name","content"]})).with_annotations(mutating.clone()),
        Tool::new("rename_memory", "Renames a memory file.", json!({"type":"object","properties":{"old_name":{"type":"string"},"new_name":{"type":"string"}},"required":["old_name","new_name"]})).with_annotations(mutating.clone()),

        // ── Session ──────────────────────────────────────────────────────
        Tool::new("activate_project", "Activates and validates the current project.", json!({"type":"object","properties":{"project":{"type":"string","description":"Optional project name or path"}}})).with_annotations(ro.clone()),
        Tool::new("check_onboarding_performed", "Checks whether Serena onboarding memories are present.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("initial_instructions", "Returns initial instructions for starting work.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("prepare_for_new_conversation", "Returns project context for a new conversation.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("get_watch_status", "Returns file watcher status: running, events processed, files reindexed.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("set_preset", "Switch tool preset at runtime. Changes which tools appear in tools/list.", json!({"type":"object","properties":{"preset":{"type":"string","enum":["minimal","balanced","full"],"description":"Target preset: minimal (21 tools), balanced (34), full (50+)"}},"required":["preset"]})).with_annotations(mutating.clone()),
    ];

    // ── Semantic (feature-gated) ────────────────────────────────────
    #[cfg(feature = "semantic")]
    {
        let ro = ro;
        tools.push(Tool::new("semantic_search", "Search symbols by natural language query using vector embeddings. Call index_embeddings first to build the index.", json!({"type":"object","properties":{"query":{"type":"string","description":"Natural language search query"},"max_results":{"type":"integer","description":"Max results (default 20)"}},"required":["query"]})).with_annotations(ro.clone()));
        tools.push(Tool::new("index_embeddings", "Build the semantic embedding index from all project symbols. Required before semantic_search.", json!({"type":"object","properties":{}})).with_annotations(ro));
    }

    tools
}
