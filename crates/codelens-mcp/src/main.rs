mod protocol;
mod tools;

use anyhow::Result;
use codelens_core::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use protocol::{JsonRpcRequest, JsonRpcResponse, Tool, ToolAnnotations, ToolCallResponse};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};
use tools::ToolResult;

struct AppState {
    project: ProjectRoot,
    symbol_index: Arc<Mutex<SymbolIndex>>,
    lsp_pool: Mutex<LspSessionPool>,
    graph_cache: Arc<GraphCache>,
    preset: ToolPreset,
    memories_dir: std::path::PathBuf,
    watcher: Option<FileWatcher>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPreset {
    Minimal,  // 20 core tools — symbol/file/search only
    Balanced, // 30 tools — + analysis, git, editing
    Full,     // all tools
}

impl ToolPreset {
    fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "minimal" | "min" => Self::Minimal,
            "balanced" | "bal" => Self::Balanced,
            _ => Self::Full,
        }
    }
}

const MINIMAL_TOOLS: &[&str] = &[
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

const BALANCED_EXCLUDES: &[&str] = &[
    "find_circular_dependencies",
    "get_change_coupling",
    "get_symbol_importance",
    "find_dead_code",
    "refactor_extract_function",
    "get_complexity",
    "search_symbols_fuzzy",
    "check_lsp_status",
];

impl AppState {
    fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let symbol_index = Arc::new(Mutex::new(SymbolIndex::new(project.clone())));
        let lsp_pool = LspSessionPool::new(project.clone());
        let graph_cache = Arc::new(GraphCache::new(30));
        let memories_dir = project.as_path().join(".serena").join("memories");

        let watcher = FileWatcher::start(
            project.as_path(),
            Arc::clone(&symbol_index),
            Arc::clone(&graph_cache),
        )
        .ok();

        Self {
            project,
            symbol_index,
            lsp_pool: Mutex::new(lsp_pool),
            graph_cache,
            preset,
            memories_dir,
            watcher,
        }
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let project_arg = args.get(1).map(|s| s.as_str()).unwrap_or(".");
    let preset = args
        .iter()
        .position(|a| a == "--preset")
        .and_then(|i| args.get(i + 1))
        .map(|s| ToolPreset::from_str(s))
        .or_else(|| {
            std::env::var("CODELENS_PRESET")
                .ok()
                .map(|s| ToolPreset::from_str(&s))
        })
        .unwrap_or(ToolPreset::Balanced);

    // Project root resolution priority:
    // 1. Explicit path argument (if not ".")
    // 2. CLAUDE_PROJECT_DIR environment variable (set by Claude Code)
    // 3. MCP_PROJECT_DIR environment variable (generic)
    // 4. Current working directory with .git/.cargo marker detection
    let effective_path = if project_arg != "." {
        project_arg.to_string()
    } else if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        dir
    } else if let Ok(dir) = std::env::var("MCP_PROJECT_DIR") {
        dir
    } else {
        ".".to_string()
    };

    let project = ProjectRoot::new(&effective_path)?;
    run_stdio(AppState::new(project, preset))
}

fn run_stdio(state: AppState) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => handle_request(&state, request),
            Err(error) => JsonRpcResponse::error(None, -32700, format!("Parse error: {error}")),
        };
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_request(state: &AppState, request: JsonRpcRequest) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return JsonRpcResponse::error(request.id, -32600, "Unsupported jsonrpc version");
    }

    match request.method.as_str() {
        "initialize" => JsonRpcResponse::result(
            request.id,
            json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {
                    "tools": {},
                    "resources": { "listChanged": false },
                    "prompts": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "codelens-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
        "resources/list" => {
            JsonRpcResponse::result(request.id, json!({ "resources": resources(state) }))
        }
        "resources/read" => {
            let uri = request
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            JsonRpcResponse::result(request.id, read_resource(state, uri))
        }
        "prompts/list" => JsonRpcResponse::result(request.id, json!({ "prompts": prompts() })),
        "prompts/get" => {
            let name = request
                .params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = request
                .params
                .as_ref()
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));
            JsonRpcResponse::result(request.id, get_prompt(state, name, &args))
        }
        "tools/list" => {
            let filtered: Vec<_> = tools()
                .into_iter()
                .filter(|t| match state.preset {
                    ToolPreset::Full => true,
                    ToolPreset::Minimal => MINIMAL_TOOLS.contains(&t.name),
                    ToolPreset::Balanced => !BALANCED_EXCLUDES.contains(&t.name),
                })
                .collect();
            JsonRpcResponse::result(request.id, json!({ "tools": filtered }))
        }
        "tools/call" => match request.params {
            Some(params) => dispatch_tool(state, request.id, params),
            None => JsonRpcResponse::error(request.id, -32602, "Missing params"),
        },
        method => JsonRpcResponse::error(request.id, -32601, format!("Method not found: {method}")),
    }
}

// ── Tool dispatch (thin router) ─────────────────────────────────────────

fn dispatch_tool(
    state: &AppState,
    id: Option<serde_json::Value>,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let Some(name) = params.get("name").and_then(|value| value.as_str()) else {
        return JsonRpcResponse::error(id, -32602, "Missing tool name");
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result: ToolResult = match name {
        // Filesystem / search
        "get_current_config" => tools::filesystem::get_current_config(state, &arguments),
        "read_file" => tools::filesystem::read_file_tool(state, &arguments),
        "list_dir" => tools::filesystem::list_dir_tool(state, &arguments),
        "find_file" => tools::filesystem::find_file_tool(state, &arguments),
        "search_for_pattern" => tools::filesystem::search_for_pattern_tool(state, &arguments),
        "find_annotations" => tools::filesystem::find_annotations(state, &arguments),
        "find_tests" => tools::filesystem::find_tests(state, &arguments),

        // Symbols / index
        "get_symbols_overview" => tools::symbols::get_symbols_overview(state, &arguments),
        "find_symbol" => tools::symbols::find_symbol(state, &arguments),
        "get_ranked_context" => tools::symbols::get_ranked_context(state, &arguments),
        "refresh_symbol_index" => tools::symbols::refresh_symbol_index(state, &arguments),
        "get_complexity" => tools::symbols::get_complexity(state, &arguments),
        "search_symbols_fuzzy" => tools::symbols::search_symbols_fuzzy(state, &arguments),

        // LSP
        "find_referencing_symbols" => tools::lsp::find_referencing_symbols(state, &arguments),
        "get_file_diagnostics" => tools::lsp::get_file_diagnostics(state, &arguments),
        "search_workspace_symbols" => tools::lsp::search_workspace_symbols(state, &arguments),
        "get_type_hierarchy" => tools::lsp::get_type_hierarchy(state, &arguments),
        "plan_symbol_rename" => tools::lsp::plan_symbol_rename(state, &arguments),
        "check_lsp_status" => tools::lsp::check_lsp_status(state, &arguments),
        "get_lsp_recipe" => tools::lsp::get_lsp_recipe(state, &arguments),

        // Graph / analysis
        "get_changed_files" | "get_diff_symbols" => {
            tools::graph::get_changed_files_tool(state, &arguments)
        }
        "get_blast_radius" => tools::graph::get_blast_radius_tool(state, &arguments),
        "get_impact_analysis" => tools::graph::get_impact_analysis(state, &arguments),
        "find_importers" => tools::graph::find_importers_tool(state, &arguments),
        "get_symbol_importance" => tools::graph::get_symbol_importance(state, &arguments),
        "find_dead_code" | "find_dead_code_v2" => {
            tools::graph::find_dead_code_v2_tool(state, &arguments)
        }
        // find_referencing_code_snippets: kept as alias for backward compat, delegates to search_for_pattern
        "find_referencing_code_snippets" => {
            tools::graph::find_referencing_code_snippets(state, &arguments)
        }
        "find_scoped_references" => tools::graph::find_scoped_references_tool(state, &arguments),
        "get_callers" => tools::graph::get_callers_tool(state, &arguments),
        "get_callees" => tools::graph::get_callees_tool(state, &arguments),
        "find_circular_dependencies" => {
            tools::graph::find_circular_dependencies_tool(state, &arguments)
        }
        "get_change_coupling" => tools::graph::get_change_coupling_tool(state, &arguments),

        // Mutation / editing
        "rename_symbol" => tools::mutation::rename_symbol(state, &arguments),
        "create_text_file" => tools::mutation::create_text_file_tool(state, &arguments),
        "delete_lines" => tools::mutation::delete_lines_tool(state, &arguments),
        "insert_at_line" => tools::mutation::insert_at_line_tool(state, &arguments),
        "replace_lines" => tools::mutation::replace_lines_tool(state, &arguments),
        "replace_content" => tools::mutation::replace_content_tool(state, &arguments),
        "replace_symbol_body" => tools::mutation::replace_symbol_body_tool(state, &arguments),
        "insert_before_symbol" => tools::mutation::insert_before_symbol_tool(state, &arguments),
        "insert_after_symbol" => tools::mutation::insert_after_symbol_tool(state, &arguments),
        "analyze_missing_imports" => {
            tools::mutation::analyze_missing_imports_tool(state, &arguments)
        }
        "add_import" => tools::mutation::add_import_tool(state, &arguments),

        // Memory
        "list_memories" => tools::memory::list_memories(state, &arguments),
        "read_memory" => tools::memory::read_memory(state, &arguments),
        "write_memory" => tools::memory::write_memory(state, &arguments),
        "delete_memory" => tools::memory::delete_memory(state, &arguments),
        "edit_memory" => tools::memory::edit_memory(state, &arguments),
        "rename_memory" => tools::memory::rename_memory(state, &arguments),

        // Session / config
        "activate_project" => tools::session::activate_project(state, &arguments),
        "check_onboarding_performed" => {
            tools::session::check_onboarding_performed(state, &arguments)
        }
        "initial_instructions" => tools::session::initial_instructions(state, &arguments),
        "onboarding" => tools::session::onboarding(state, &arguments),
        "prepare_for_new_conversation" => {
            tools::session::prepare_for_new_conversation(state, &arguments)
        }
        "summarize_changes" => tools::session::summarize_changes(state, &arguments),
        "list_queryable_projects" => tools::session::list_queryable_projects(state, &arguments),
        "get_watch_status" => tools::session::get_watch_status(state, &arguments),
        "think_about_collected_information"
        | "think_about_task_adherence"
        | "think_about_whether_you_are_done" => tools::session::think_noop(state, &arguments),
        "switch_modes" => tools::session::switch_modes(state, &arguments),

        // Composite / agent
        "summarize_file" => tools::composite::summarize_file(state, &arguments),
        "explain_code_flow" => tools::composite::explain_code_flow(state, &arguments),
        "refactor_extract_function" => {
            tools::composite::refactor_extract_function(state, &arguments)
        }

        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    };

    match result {
        Ok((payload, meta)) => JsonRpcResponse::result(
            id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string(&ToolCallResponse::success(payload, meta))
                            .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned())
                    }
                ]
            }),
        ),
        Err(error) => JsonRpcResponse::result(
            id,
            json!({
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string(&ToolCallResponse::error(error.to_string()))
                            .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned())
                    }
                ]
            }),
        ),
    }
}

// ── Tool definitions ────────────────────────────────────────────────────

fn tools() -> Vec<Tool> {
    let ro = ToolAnnotations::read_only();
    let destructive = ToolAnnotations::destructive();
    let mutating = ToolAnnotations::mutating();
    vec![
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
        Tool::new("get_symbols_overview", "Get an overview of code symbols in a file or directory.", json!({"type":"object","properties":{"path":{"type":"string"},"depth":{"type":"integer"}},"required":["path"]})).with_annotations(ro.clone()),
        Tool::new("find_symbol", "Find a symbol by name or stable ID. Use symbol_id (e.g. 'src/main.py#function:Service/greet') for fastest exact lookup.", json!({"type":"object","properties":{"name":{"type":"string","description":"Symbol name to search for"},"symbol_id":{"type":"string","description":"Stable symbol ID (file#kind:name_path). Overrides name."},"file_path":{"type":"string"},"include_body":{"type":"boolean"},"exact_match":{"type":"boolean"},"max_matches":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("get_ranked_context", "Return the most relevant symbols for a query within a token budget.", json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"max_tokens":{"type":"integer"},"include_body":{"type":"boolean"},"depth":{"type":"integer"}},"required":["query"]})).with_annotations(ro.clone()),
        Tool::new("search_symbols_fuzzy", "Hybrid symbol search: exact match (score 100), substring match (score 60), and fuzzy jaro_winkler match (score by similarity).", json!({"type":"object","properties":{"query":{"type":"string","description":"Symbol name to search for"},"max_results":{"type":"integer","description":"Maximum number of results to return (default 30)"},"fuzzy_threshold":{"type":"number","description":"Minimum jaro_winkler similarity 0.0-1.0 for fuzzy matches (default 0.6)"}},"required":["query"]})).with_annotations(ro.clone()),
        Tool::new("refresh_symbol_index", "Rebuild the cached symbol index for the current project.", json!({"type":"object","properties":{}})).with_annotations(mutating.clone()),

        // ── LSP ──────────────────────────────────────────────────────────
        Tool::new("find_referencing_symbols", "Find references via LSP or text-based fallback. Provide symbol_name for direct text search without LSP, or line/column for LSP (with automatic text fallback on failure).", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File containing or declaring the symbol"},"symbol_name":{"type":"string","description":"Symbol name for text-based search (skips LSP)"},"line":{"type":"integer","description":"Line number for LSP lookup"},"column":{"type":"integer","description":"Column number for LSP lookup"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["file_path"]})).with_annotations(ro.clone()),
        Tool::new("get_file_diagnostics", "Get file diagnostics through a pooled stdio LSP server. command/args may be provided explicitly.", json!({"type":"object","properties":{"file_path":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["file_path"]})).with_annotations(ro.clone()),
        Tool::new("search_workspace_symbols", "Search workspace symbols through a pooled stdio LSP server. command is required because no file path is available for inference.", json!({"type":"object","properties":{"query":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"max_results":{"type":"integer"}},"required":["query","command"]})).with_annotations(ro.clone()),
        Tool::new("get_type_hierarchy", "Get the type hierarchy through a pooled stdio LSP server.", json!({"type":"object","properties":{"name_path":{"type":"string"},"fully_qualified_name":{"type":"string"},"relative_path":{"type":"string"},"hierarchy_type":{"type":"string","enum":["super","sub","both"]},"depth":{"type":"integer"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}}})).with_annotations(ro.clone()),
        Tool::new("plan_symbol_rename", "Plan a safe symbol rename through pooled LSP prepareRename without applying edits.", json!({"type":"object","properties":{"file_path":{"type":"string"},"line":{"type":"integer"},"column":{"type":"integer"},"new_name":{"type":"string"},"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}}},"required":["file_path","line","column"]})).with_annotations(ro.clone()),
        Tool::new("check_lsp_status", "Check which LSP servers are installed on this machine and which are missing, with install commands.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        // get_lsp_recipe: migrated to Skill, kept in dispatch for compat

        // ── Graph / analysis (read-only) ─────────────────────────────────
        Tool::new("get_changed_files", "Return files changed compared to a git ref, with symbol counts. Also accepts legacy name 'get_diff_symbols'.", json!({"type":"object","properties":{"ref":{"type":"string"},"include_untracked":{"type":"boolean"}}})).with_annotations(ro.clone()),
        Tool::new("get_impact_analysis", "One-shot impact analysis: symbols + importers + blast radius. Replaces find_importers and get_blast_radius.", json!({"type":"object","properties":{"file_path":{"type":"string"},"max_depth":{"type":"integer"}},"required":["file_path"]})).with_annotations(ro.clone()),
        Tool::new("get_symbol_importance", "Return file importance ranking based on import-graph PageRank for supported Python/JS/TS projects.", json!({"type":"object","properties":{"top_n":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("find_dead_code", "Multi-pass dead code detection: unreferenced files + unreferenced symbols via call-graph. Also accepts legacy name 'find_dead_code_v2'.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),
        // find_referencing_code_snippets: kept in dispatch for compat, use search_for_pattern instead
        Tool::new("find_scoped_references", "Scope-aware reference search using tree-sitter AST. Classifies each reference as definition/read/write/import with enclosing scope context.", json!({"type":"object","properties":{"symbol_name":{"type":"string","description":"Symbol name to find references for"},"file_path":{"type":"string","description":"Declaration file (for sorting, optional)"},"max_results":{"type":"integer","description":"Max results (default 50)"}},"required":["symbol_name"]})).with_annotations(ro.clone()),
        // get_callers/get_callees: kept in dispatch for compat, use explain_code_flow instead
        Tool::new("find_circular_dependencies", "Detect circular import dependencies in the project using Tarjan SCC algorithm on the import graph.", json!({"type":"object","properties":{"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),
        Tool::new("get_change_coupling", "Analyze git history to find files that frequently change together (temporal coupling).", json!({"type":"object","properties":{"months":{"type":"integer"},"min_strength":{"type":"number"},"min_commits":{"type":"integer"},"max_results":{"type":"integer"}}})).with_annotations(ro.clone()),

        // ── Mutation / editing ───────────────────────────────────────────
        Tool::new("rename_symbol", "Rename a symbol across one file (file scope) or the entire project. Supports dry_run for preview.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File containing the symbol declaration"},"symbol_name":{"type":"string","description":"Current symbol name"},"name":{"type":"string","description":"Alias for symbol_name"},"new_name":{"type":"string","description":"Desired new name"},"name_path":{"type":"string","description":"Qualified name path (e.g. 'Class/method')"},"scope":{"type":"string","enum":["file","project"],"description":"Rename scope (default: project)"},"dry_run":{"type":"boolean","description":"Preview changes without modifying files"}},"required":["file_path","new_name"]})).with_annotations(destructive.clone()),
        Tool::new("delete_lines", "Delete lines [start_line, end_line) from a file (1-indexed, end exclusive). Returns the modified content.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"}},"required":["relative_path","start_line","end_line"]})).with_annotations(destructive.clone()),
        Tool::new("create_text_file", "Create a new file with the given content. If overwrite is false and the file already exists, returns an error.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"content":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["relative_path","content"]})).with_annotations(mutating.clone()),
        Tool::new("insert_at_line", "Insert content at a given line number (1-indexed) in a file. Returns the modified content.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"line":{"type":"integer"},"content":{"type":"string"}},"required":["relative_path","line","content"]})).with_annotations(mutating.clone()),
        Tool::new("replace_lines", "Replace lines [start_line, end_line) in a file with new_content (1-indexed, end exclusive). Returns the modified content.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_content":{"type":"string"}},"required":["relative_path","start_line","end_line","new_content"]})).with_annotations(mutating.clone()),
        Tool::new("replace_content", "Replace old_text with new_text in a file, either literal or regex. Returns modified content and replacement count.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"old_text":{"type":"string"},"new_text":{"type":"string"},"regex_mode":{"type":"boolean"}},"required":["relative_path","old_text","new_text"]})).with_annotations(mutating.clone()),
        Tool::new("replace_symbol_body", "Replace the entire body of a named symbol (function, class, etc.) in a file using tree-sitter byte offsets. Optionally disambiguate with name_path (e.g. 'ClassName/method').", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"new_body":{"type":"string"}},"required":["relative_path","symbol_name","new_body"]})).with_annotations(mutating.clone()),
        Tool::new("insert_before_symbol", "Insert content immediately before a named symbol in a file using tree-sitter byte offsets. Optionally disambiguate with name_path.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}},"required":["relative_path","symbol_name","content"]})).with_annotations(mutating.clone()),
        Tool::new("insert_after_symbol", "Insert content immediately after a named symbol in a file using tree-sitter byte offsets. Optionally disambiguate with name_path.", json!({"type":"object","properties":{"relative_path":{"type":"string"},"symbol_name":{"type":"string"},"name_path":{"type":"string"},"content":{"type":"string"}},"required":["relative_path","symbol_name","content"]})).with_annotations(mutating.clone()),
        // Auto-import
        Tool::new("analyze_missing_imports", "Detect unresolved symbols in a file and suggest import statements from the project's symbol index.", json!({"type":"object","properties":{"file_path":{"type":"string","description":"File to analyze"}},"required":["file_path"]})).with_annotations(mutating.clone()),
        Tool::new("add_import", "Insert an import statement at the correct position in a file.", json!({"type":"object","properties":{"file_path":{"type":"string"},"import_statement":{"type":"string","description":"Import statement to add"}},"required":["file_path","import_statement"]})).with_annotations(mutating.clone()),

        // ── Composite ────────────────────────────────────────────────────
        // summarize_file, explain_code_flow: migrated to Skills, kept in dispatch for compat
        Tool::new("refactor_extract_function", "Extract a line range into a new function. Replaces the original lines with a function call. Use dry_run=true to preview.", json!({"type":"object","properties":{"file_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_name":{"type":"string","description":"Name for the new function"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}},"required":["file_path","start_line","end_line","new_name"]})).with_annotations(mutating.clone()),

        // ── Memory ───────────────────────────────────────────────────────
        // No-op (kept in dispatch for backward compat, not listed in tools)
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
        // onboarding: migrated to Skill, kept in dispatch for compat
        Tool::new("prepare_for_new_conversation", "Returns project context for a new conversation.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        Tool::new("get_watch_status", "Returns file watcher status: running, events processed, files reindexed.", json!({"type":"object","properties":{}})).with_annotations(ro.clone()),
        // summarize_changes, list_queryable_projects: kept in dispatch for compat, not listed
    ]
}

// ── MCP Resources ────────────────────────────────────────────────────────

fn resources(state: &AppState) -> Vec<serde_json::Value> {
    let project_name = state
        .project
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    vec![
        json!({
            "uri": "codelens://project/overview",
            "name": format!("Project: {project_name}"),
            "description": "Project root path and symbol index statistics",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://symbols/index",
            "name": "Symbol Index",
            "description": "All indexed files and symbol counts",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list",
            "name": "Available Tools",
            "description": "List of all 62 MCP tools with descriptions",
            "mimeType": "application/json"
        }),
    ]
}

fn read_resource(state: &AppState, uri: &str) -> serde_json::Value {
    match uri {
        "codelens://project/overview" => {
            let stats = state
                .symbol_index
                .lock()
                .ok()
                .and_then(|idx| idx.stats().ok());
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "project_root": state.project.as_path().to_string_lossy(),
                        "symbol_index": stats,
                        "memories_dir": state.memories_dir.to_string_lossy(),
                        "tool_count": 62
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://symbols/index" => {
            let stats = state
                .symbol_index
                .lock()
                .ok()
                .and_then(|idx| idx.stats().ok());
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "stats": stats
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://tools/list" => {
            let tool_names: Vec<&str> = tools().iter().map(|t| t.name).collect();
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&tool_names).unwrap_or_default()
                }]
            })
        }
        _ => json!({
            "contents": [{
                "uri": uri,
                "mimeType": "text/plain",
                "text": format!("Unknown resource: {uri}")
            }]
        }),
    }
}

// ── MCP Prompts ──────────────────────────────────────────────────────────

fn prompts() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "review-file",
            "description": "Review a file for code quality, bugs, and improvements",
            "arguments": [{ "name": "file_path", "description": "File to review", "required": true }]
        }),
        json!({
            "name": "onboard-project",
            "description": "Get a comprehensive overview of the project for onboarding",
            "arguments": []
        }),
        json!({
            "name": "analyze-impact",
            "description": "Analyze the impact of modifying a specific file",
            "arguments": [{ "name": "file_path", "description": "File to analyze", "required": true }]
        }),
    ]
}

fn get_prompt(state: &AppState, name: &str, args: &serde_json::Value) -> serde_json::Value {
    let project_root = state.project.as_path().to_string_lossy().to_string();
    match name {
        "review-file" => {
            let file_path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Please review the file `{file_path}` in the project at `{project_root}`.\n\n\
                            Use these tools to analyze:\n\
                            1. `get_symbols_overview` to understand the file structure\n\
                            2. `find_scoped_references` to check how symbols are used\n\
                            3. `get_complexity` to identify complex functions\n\
                            4. `analyze_missing_imports` to find import issues\n\n\
                            Focus on: bugs, performance, readability, and missing error handling."
                        )
                    }
                }]
            })
        }
        "onboard-project" => {
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "I'm new to the project at `{project_root}`. Help me understand it.\n\n\
                            Use these tools:\n\
                            1. `get_symbols_overview` on the root to see top-level structure\n\
                            2. `get_symbol_importance` to find the most important files\n\
                            3. `find_circular_dependencies` to understand architecture issues\n\
                            4. `search_for_pattern` for key patterns (main entry, config, tests)\n\n\
                            Give me: architecture overview, key files, entry points, and test strategy."
                        )
                    }
                }]
            })
        }
        "analyze-impact" => {
            let file_path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Analyze the impact of modifying `{file_path}` in `{project_root}`.\n\n\
                            Use these tools:\n\
                            1. `get_blast_radius` to find affected files\n\
                            2. `find_importers` to find direct dependents\n\
                            3. `get_symbols_overview` to understand what's in the file\n\
                            4. `find_scoped_references` for each exported symbol\n\n\
                            Assess: risk level, affected modules, required test coverage."
                        )
                    }
                }]
            })
        }
        _ => json!({
            "messages": [{
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!("Unknown prompt: {name}")
                }
            }]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_request, tools};
    use codelens_core::ProjectRoot;
    use serde_json::json;
    use std::fs;

    #[test]
    fn lists_tools() {
        let project = project_root();
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/list".to_owned(),
                params: None,
            },
        );
        assert_eq!(tools().len(), 49);
        let encoded = serde_json::to_string(&response).expect("serialize");
        assert!(encoded.contains("get_symbols_overview"));
    }

    #[test]
    fn reads_file_via_tool_call() {
        let project = project_root();
        let state = make_state(&project);
        let payload = call_tool(&state, "read_file", json!({ "relative_path": "hello.txt" }));
        assert_eq!(payload["success"], json!(true));
        assert_eq!(payload["backend_used"], json!("filesystem"));
    }

    #[test]
    fn returns_symbols_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("sample.py"),
            "class Foo:\n    def bar(self):\n        pass\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "get_symbols_overview",
            json!({ "path": "sample.py" }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn reports_symbol_index_stats() {
        let project = project_root();
        fs::write(
            project.as_path().join("stats_test.py"),
            "def alpha():\n    pass\ndef beta():\n    pass\n",
        )
        .unwrap();
        let state = make_state(&project);
        call_tool(&state, "refresh_symbol_index", json!({}));
        let payload = call_tool(&state, "get_current_config", json!({}));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_ranked_context_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("rank.py"),
            "def search_users(query):\n    pass\ndef delete_user(uid):\n    pass\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "get_ranked_context",
            json!({ "query": "search users" }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_blast_radius_via_tool_call() {
        let project = project_root();
        fs::create_dir_all(project.as_path().join("pkg")).unwrap();
        fs::write(project.as_path().join("pkg/core.py"), "X = 1\n").unwrap();
        fs::write(
            project.as_path().join("pkg/util.py"),
            "from pkg.core import X\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "get_blast_radius",
            json!({ "file_path": "pkg/core.py" }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_importers_via_tool_call() {
        let project = project_root();
        fs::create_dir_all(project.as_path().join("lib")).unwrap();
        fs::write(project.as_path().join("lib/base.py"), "BASE = 42\n").unwrap();
        fs::write(
            project.as_path().join("lib/derived.py"),
            "from lib.base import BASE\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "find_importers",
            json!({ "file_path": "lib/base.py" }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_symbol_importance_via_tool_call() {
        let project = project_root();
        fs::create_dir_all(project.as_path().join("importance_pkg")).unwrap();
        fs::write(
            project.as_path().join("importance_pkg/hub.py"),
            "HUB = True\n",
        )
        .unwrap();
        fs::write(
            project.as_path().join("importance_pkg/spoke_a.py"),
            "from importance_pkg.hub import HUB\n",
        )
        .unwrap();
        fs::write(
            project.as_path().join("importance_pkg/spoke_b.py"),
            "from importance_pkg.hub import HUB\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(&state, "get_symbol_importance", json!({ "top_n": 5 }));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_dead_code_via_tool_call() {
        let project = project_root();
        fs::create_dir_all(project.as_path().join("dc_pkg")).unwrap();
        fs::write(project.as_path().join("dc_pkg/used.py"), "X = 1\n").unwrap();
        fs::write(project.as_path().join("dc_pkg/orphan.py"), "Y = 2\n").unwrap();
        fs::write(
            project.as_path().join("dc_pkg/consumer.py"),
            "from dc_pkg.used import X\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(&state, "find_dead_code", json!({ "max_results": 10 }));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_annotations_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("annotated.py"),
            "# TODO: fix this\n# FIXME: broken\ndef ok():\n    pass\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(&state, "find_annotations", json!({}));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_tests_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("test_sample.py"),
            "def test_one():\n    assert True\ndef test_two():\n    assert True\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(&state, "find_tests", json!({}));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_complexity_via_tool_call() {
        let project = project_root();
        fs::write(project.as_path().join("complex.py"), "def decide(x):\n    if x > 0:\n        if x > 10:\n            return 'big'\n        return 'small'\n    return 'neg'\n").unwrap();
        let state = make_state(&project);
        let payload = call_tool(&state, "get_complexity", json!({ "path": "complex.py" }));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_changed_files_via_tool_call() {
        let project = project_root();
        run_git(&project, &["init"]);
        run_git(&project, &["add", "."]);
        run_git(
            &project,
            &[
                "-c",
                "user.email=test@test.com",
                "-c",
                "user.name=Test",
                "commit",
                "-m",
                "init",
            ],
        );
        fs::write(project.as_path().join("new_file.py"), "X = 1\n").unwrap();
        let state = make_state(&project);
        let payload = call_tool(&state, "get_changed_files", json!({}));
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_lsp_references_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("ref_target.py"),
            "class MyClass:\n    pass\n\nobj = MyClass()\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "find_referencing_symbols",
            json!({ "file_path": "ref_target.py", "symbol_name": "MyClass" }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_lsp_diagnostics_via_tool_call() {
        let project = project_root();
        let mock_lsp = "#!/usr/bin/env python3\nimport sys,json\nfor line in sys.stdin:\n    if not line.strip():continue\n    try:\n        l=line.strip()\n        if l.startswith('Content-Length:'):continue\n        msg=json.loads(l)\n    except:continue\n    rid=msg.get('id',0)\n    method=msg.get('method','')\n    if method=='initialize':\n        r={'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}}\n    elif method=='initialized':\n        continue\n    elif method=='textDocument/diagnostic':\n        r={'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':5}},'severity':2,'message':'test warning'}]}}\n    elif method=='shutdown':\n        r={'jsonrpc':'2.0','id':rid,'result':None}\n    else:\n        r={'jsonrpc':'2.0','id':rid,'result':None}\n    out=json.dumps(r)\n    sys.stdout.write(f'Content-Length: {len(out)}\\r\\n\\r\\n{out}')\n    sys.stdout.flush()\n";
        let mock_path = project.as_path().join("mock_lsp.py");
        fs::write(&mock_path, mock_lsp).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(project.as_path().join("diag_target.py"), "x = 1\n").unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "get_file_diagnostics",
            json!({ "file_path": "diag_target.py", "command": "python3", "args": [mock_path.to_string_lossy()] }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_workspace_symbols_via_tool_call() {
        let project = project_root();
        let mock_lsp = "#!/usr/bin/env python3\nimport sys,json\nfor line in sys.stdin:\n    if not line.strip():continue\n    try:\n        l=line.strip()\n        if l.startswith('Content-Length:'):continue\n        msg=json.loads(l)\n    except:continue\n    rid=msg.get('id',0)\n    method=msg.get('method','')\n    if method=='initialize':\n        r={'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'workspaceSymbolProvider':True}}}\n    elif method=='initialized':\n        continue\n    elif method=='workspace/symbol':\n        r={'jsonrpc':'2.0','id':rid,'result':[{'name':'TestSymbol','kind':5,'location':{'uri':'file:///test.py','range':{'start':{'line':0,'character':0},'end':{'line':0,'character':10}}}}]}\n    elif method=='shutdown':\n        r={'jsonrpc':'2.0','id':rid,'result':None}\n    else:\n        r={'jsonrpc':'2.0','id':rid,'result':None}\n    out=json.dumps(r)\n    sys.stdout.write(f'Content-Length: {len(out)}\\r\\n\\r\\n{out}')\n    sys.stdout.flush()\n";
        let mock_path = project.as_path().join("mock_ws_lsp.py");
        fs::write(&mock_path, mock_lsp).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "search_workspace_symbols",
            json!({ "query": "Test", "command": "python3", "args": [mock_path.to_string_lossy()] }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_type_hierarchy_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("hierarchy.py"),
            "class Animal:\n    pass\nclass Dog(Animal):\n    pass\nclass Cat(Animal):\n    pass\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "get_type_hierarchy",
            json!({ "name_path": "Animal", "relative_path": "hierarchy.py" }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    #[test]
    fn returns_rename_plan_via_tool_call() {
        let project = project_root();
        fs::write(
            project.as_path().join("rename_target.py"),
            "def old_name():\n    pass\n\nold_name()\n",
        )
        .unwrap();
        let mock_lsp = "#!/usr/bin/env python3\nimport sys,json\nfor line in sys.stdin:\n    if not line.strip():continue\n    try:\n        l=line.strip()\n        if l.startswith('Content-Length:'):continue\n        msg=json.loads(l)\n    except:continue\n    rid=msg.get('id',0)\n    method=msg.get('method','')\n    if method=='initialize':\n        r={'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'renameProvider':{'prepareProvider':True}}}}\n    elif method=='initialized':\n        continue\n    elif method=='textDocument/prepareRename':\n        r={'jsonrpc':'2.0','id':rid,'result':{'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':12}},'placeholder':'old_name'}}\n    elif method=='shutdown':\n        r={'jsonrpc':'2.0','id':rid,'result':None}\n    else:\n        r={'jsonrpc':'2.0','id':rid,'result':None}\n    out=json.dumps(r)\n    sys.stdout.write(f'Content-Length: {len(out)}\\r\\n\\r\\n{out}')\n    sys.stdout.flush()\n";
        let mock_path = project.as_path().join("mock_rename_lsp.py");
        fs::write(&mock_path, mock_lsp).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "plan_symbol_rename",
            json!({ "file_path": "rename_target.py", "line": 1, "column": 5, "new_name": "new_name", "command": "python3", "args": [mock_path.to_string_lossy()] }),
        );
        assert_eq!(payload["success"], json!(true));
    }

    // ── Test helpers ─────────────────────────────────────────────────────

    fn make_state(project: &ProjectRoot) -> super::AppState {
        super::AppState::new(project.clone(), super::ToolPreset::Full)
    }

    fn call_tool(
        state: &super::AppState,
        name: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        let response = handle_request(
            state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({ "name": name, "arguments": arguments })),
            },
        );
        let text = extract_tool_text(&response);
        parse_tool_payload(&text)
    }

    fn extract_tool_text(response: &super::protocol::JsonRpcResponse) -> String {
        let v = serde_json::to_value(response).expect("serialize");
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string()
    }

    fn parse_tool_payload(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap_or(json!({}))
    }

    fn project_root() -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("hello.txt"), "hello world\n").unwrap();
        ProjectRoot::new(dir.to_str().unwrap()).unwrap()
    }

    fn run_git(project: &ProjectRoot, args: &[&str]) {
        std::process::Command::new("git")
            .args(args)
            .current_dir(project.as_path())
            .output()
            .expect("git command failed");
    }
}
