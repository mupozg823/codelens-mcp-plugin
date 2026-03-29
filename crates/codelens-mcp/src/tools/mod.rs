pub mod composite;
pub mod filesystem;
pub mod graph;
pub mod lsp;
pub mod memory;
pub mod mutation;
pub mod session;
pub mod symbols;

use crate::error::CodeLensError;
use crate::protocol::{AnalysisSource, Freshness, ToolResponseMeta};
use crate::AppState;
use std::collections::HashMap;

/// Tool handler result type — every handler returns this.
pub type ToolResult = Result<(serde_json::Value, ToolResponseMeta), CodeLensError>;

/// Function pointer type for tool handlers.
pub type ToolHandler = fn(&AppState, &serde_json::Value) -> ToolResult;

/// Build the static dispatch table mapping tool names to handler functions.
/// Includes backward-compat aliases (e.g., "get_diff_symbols" → get_changed_files_tool).
pub fn dispatch_table() -> HashMap<&'static str, ToolHandler> {
    let mut m: HashMap<&'static str, ToolHandler> = HashMap::with_capacity(70);

    // Filesystem / search
    m.insert("get_current_config", filesystem::get_current_config);
    m.insert("read_file", filesystem::read_file_tool);
    m.insert("list_dir", filesystem::list_dir_tool);
    m.insert("find_file", filesystem::find_file_tool);
    m.insert("search_for_pattern", filesystem::search_for_pattern_tool);
    m.insert("find_annotations", filesystem::find_annotations);
    m.insert("find_tests", filesystem::find_tests);

    // Symbols / index
    m.insert("get_symbols_overview", symbols::get_symbols_overview);
    m.insert("find_symbol", symbols::find_symbol);
    m.insert("get_ranked_context", symbols::get_ranked_context);
    m.insert("refresh_symbol_index", symbols::refresh_symbol_index);
    m.insert("get_complexity", symbols::get_complexity);
    m.insert("search_symbols_fuzzy", symbols::search_symbols_fuzzy);
    m.insert("get_project_structure", symbols::get_project_structure);

    // LSP
    m.insert("find_referencing_symbols", lsp::find_referencing_symbols);
    m.insert("get_file_diagnostics", lsp::get_file_diagnostics);
    m.insert("search_workspace_symbols", lsp::search_workspace_symbols);
    m.insert("get_type_hierarchy", lsp::get_type_hierarchy);
    m.insert("plan_symbol_rename", lsp::plan_symbol_rename);
    m.insert("check_lsp_status", lsp::check_lsp_status);
    m.insert("get_lsp_recipe", lsp::get_lsp_recipe);

    // Graph / analysis
    m.insert("get_changed_files", graph::get_changed_files_tool);
    m.insert("get_diff_symbols", graph::get_changed_files_tool); // alias
    m.insert("get_blast_radius", graph::get_blast_radius_tool);
    m.insert("get_impact_analysis", graph::get_impact_analysis);
    m.insert("find_importers", graph::find_importers_tool);
    m.insert("get_symbol_importance", graph::get_symbol_importance);
    m.insert("find_dead_code", graph::find_dead_code_v2_tool);
    m.insert("find_dead_code_v2", graph::find_dead_code_v2_tool); // alias
    m.insert(
        "find_referencing_code_snippets",
        graph::find_referencing_code_snippets,
    );
    m.insert("find_scoped_references", graph::find_scoped_references_tool);
    m.insert("get_callers", graph::get_callers_tool);
    m.insert("get_callees", graph::get_callees_tool);
    m.insert(
        "find_circular_dependencies",
        graph::find_circular_dependencies_tool,
    );
    m.insert("get_change_coupling", graph::get_change_coupling_tool);

    // Mutation / editing
    m.insert("rename_symbol", mutation::rename_symbol);
    m.insert("create_text_file", mutation::create_text_file_tool);
    m.insert("delete_lines", mutation::delete_lines_tool);
    m.insert("insert_at_line", mutation::insert_at_line_tool);
    m.insert("replace_lines", mutation::replace_lines_tool);
    m.insert("replace_content", mutation::replace_content_tool);
    m.insert("replace_symbol_body", mutation::replace_symbol_body_tool);
    m.insert("insert_before_symbol", mutation::insert_before_symbol_tool);
    m.insert("insert_after_symbol", mutation::insert_after_symbol_tool);
    m.insert(
        "analyze_missing_imports",
        mutation::analyze_missing_imports_tool,
    );
    m.insert("add_import", mutation::add_import_tool);

    // Memory
    m.insert("list_memories", memory::list_memories);
    m.insert("read_memory", memory::read_memory);
    m.insert("write_memory", memory::write_memory);
    m.insert("delete_memory", memory::delete_memory);
    m.insert("edit_memory", memory::edit_memory);
    m.insert("rename_memory", memory::rename_memory);

    // Session / config
    m.insert("activate_project", session::activate_project);
    m.insert(
        "check_onboarding_performed",
        session::check_onboarding_performed,
    );
    m.insert("initial_instructions", session::initial_instructions);
    m.insert("onboarding", session::onboarding);
    m.insert(
        "prepare_for_new_conversation",
        session::prepare_for_new_conversation,
    );
    m.insert("summarize_changes", session::summarize_changes);
    m.insert("list_queryable_projects", session::list_queryable_projects);
    m.insert("get_watch_status", session::get_watch_status);
    m.insert("think_about_collected_information", session::think_noop);
    m.insert("think_about_task_adherence", session::think_noop);
    m.insert("think_about_whether_you_are_done", session::think_noop);
    m.insert("switch_modes", session::switch_modes);
    m.insert("set_preset", session::set_preset);
    m.insert("get_capabilities", session::get_capabilities);

    // Composite / agent
    m.insert("summarize_file", composite::summarize_file);
    m.insert("explain_code_flow", composite::explain_code_flow);
    m.insert(
        "refactor_extract_function",
        composite::refactor_extract_function,
    );
    m.insert("onboard_project", composite::onboard_project);

    m
}

/// Rough token count estimate: 1 token ≈ 4 bytes of UTF-8 text.
pub fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

pub fn success_meta(backend_used: &str, confidence: f64) -> ToolResponseMeta {
    ToolResponseMeta {
        backend_used: backend_used.to_owned(),
        confidence,
        degraded_reason: None,
        source: AnalysisSource::Native,
        partial: false,
        freshness: Freshness::Live,
        staleness_ms: None,
    }
}

pub fn required_string<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, CodeLensError> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam(key.to_owned()))
}

/// Parse LSP args from arguments, falling back to defaults for the given command.
pub fn parse_lsp_args(arguments: &serde_json::Value, command: &str) -> Vec<String> {
    arguments
        .get("args")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| default_lsp_args_for_command(command))
}

pub fn default_lsp_command_for_path(file_path: &str) -> Option<String> {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => Some("pyright-langserver".to_owned()),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            Some("typescript-language-server".to_owned())
        }
        "rs" => Some("rust-analyzer".to_owned()),
        "cs" => Some("csharp-ls".to_owned()),
        "dart" => Some("dart".to_owned()),
        "go" => Some("gopls".to_owned()),
        "java" => Some("jdtls".to_owned()),
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" => Some("clangd".to_owned()),
        "rb" => Some("solargraph".to_owned()),
        "php" => Some("intelephense".to_owned()),
        "kt" | "kts" => Some("kotlin-language-server".to_owned()),
        "scala" | "sc" => Some("metals".to_owned()),
        "swift" => Some("sourcekit-lsp".to_owned()),
        _ => None,
    }
}

pub fn default_lsp_args_for_command(command: &str) -> Vec<String> {
    match command {
        "pyright-langserver" => vec!["--stdio".to_owned()],
        "typescript-language-server" => vec!["--stdio".to_owned()],
        "dart" => vec!["language-server".to_owned(), "--protocol=lsp".to_owned()],
        "clangd" => vec!["--background-index".to_owned()],
        "solargraph" => vec!["stdio".to_owned()],
        "intelephense" => vec!["--stdio".to_owned()],
        _ => Vec::new(),
    }
}

pub fn suggest_next(tool_name: &str) -> Option<Vec<String>> {
    let suggestions: &[&str] = match tool_name {
        // ── Symbols / index ──────────────────────────────────────────
        "get_symbols_overview" => &["find_symbol", "get_impact_analysis", "get_ranked_context"],
        "find_symbol" => &[
            "find_referencing_symbols",
            "get_impact_analysis",
            "replace_symbol_body",
        ],
        "get_ranked_context" => &["find_symbol", "replace_symbol_body", "semantic_search"],
        "refresh_symbol_index" => &["index_embeddings", "get_symbols_overview"],
        "get_project_structure" => &["get_symbols_overview", "get_ranked_context", "find_symbol"],
        "get_complexity" => &["find_symbol", "get_symbols_overview"],
        "search_symbols_fuzzy" => &["find_symbol", "get_ranked_context"],

        // ── LSP ──────────────────────────────────────────────────────
        "find_referencing_symbols" => &["get_impact_analysis", "rename_symbol"],
        "get_file_diagnostics" => &["find_symbol", "get_symbols_overview"],
        "search_workspace_symbols" => &["find_symbol", "get_symbols_overview"],
        "get_type_hierarchy" => &["find_referencing_symbols", "get_symbols_overview"],
        "plan_symbol_rename" => &["rename_symbol"],
        "check_lsp_status" => &["get_capabilities", "get_file_diagnostics"],
        "get_lsp_recipe" => &["check_lsp_status"],

        // ── Graph / analysis ─────────────────────────────────────────
        "get_changed_files" => &["get_impact_analysis", "get_symbols_overview"],
        "get_blast_radius" => &["get_importers", "find_referencing_symbols"],
        "get_impact_analysis" => &["find_referencing_symbols", "get_symbols_overview"],
        "get_importers" => &["get_blast_radius", "get_symbol_importance"],
        "get_symbol_importance" => &["get_importers", "get_blast_radius"],
        "find_dead_code" => &["get_symbols_overview", "delete_lines"],
        "find_circular_dependencies" => &["get_impact_analysis", "get_symbols_overview"],
        "get_change_coupling" => &["get_impact_analysis", "get_blast_radius"],
        "get_callers" => &["get_callees", "find_symbol"],
        "get_callees" => &["get_callers", "find_symbol"],
        "find_scoped_references" => &["rename_symbol", "find_referencing_symbols"],

        // ── Filesystem ───────────────────────────────────────────────
        "get_current_config" => &["get_capabilities", "get_project_structure"],
        "read_file" => &["get_symbols_overview", "find_symbol"],
        "search_for_pattern" => &["find_referencing_symbols", "get_ranked_context"],
        "find_annotations" => &["get_symbols_overview", "find_symbol"],
        "find_tests" => &["get_symbols_overview"],

        // ── Mutation ─────────────────────────────────────────────────
        "rename_symbol" => &["find_referencing_symbols", "get_file_diagnostics"],
        "replace_symbol_body" => &["find_symbol", "get_file_diagnostics"],
        "replace_content" => &["get_file_diagnostics", "get_symbols_overview"],
        "replace_lines" => &["get_file_diagnostics"],
        "delete_lines" => &["get_file_diagnostics"],
        "insert_at_line" => &["get_file_diagnostics"],
        "insert_before_symbol" => &["get_file_diagnostics", "find_symbol"],
        "insert_after_symbol" => &["get_file_diagnostics", "find_symbol"],
        "create_text_file" => &["get_symbols_overview"],
        "add_import" => &["get_file_diagnostics", "analyze_missing_imports"],
        "analyze_missing_imports" => &["add_import"],

        // ── Memory ───────────────────────────────────────────────────
        "write_memory" => &["list_memories", "read_memory"],
        "read_memory" => &["write_memory", "list_memories"],
        "list_memories" => &["read_memory", "write_memory"],

        // ── Session ──────────────────────────────────────────────────
        "activate_project" => &[
            "get_project_structure",
            "get_current_config",
            "get_capabilities",
        ],
        "onboard_project" => &["get_ranked_context", "find_symbol", "get_capabilities"],
        "get_watch_status" => &["refresh_symbol_index"],
        "set_preset" => &["get_capabilities"],
        "get_capabilities" => &[
            "get_project_structure",
            "get_ranked_context",
            "check_lsp_status",
        ],

        // ── Semantic ─────────────────────────────────────────────────
        "semantic_search" => &["find_symbol", "get_symbols_overview", "get_ranked_context"],
        "index_embeddings" => &["semantic_search"],

        // ── Composite ────────────────────────────────────────────────
        "summarize_file" => &["get_symbols_overview", "find_symbol"],
        "explain_code_flow" => &["get_callers", "get_callees"],
        "refactor_extract_function" => &["get_file_diagnostics", "find_symbol"],

        _ => return None,
    };
    Some(suggestions.iter().map(|s| s.to_string()).collect())
}
