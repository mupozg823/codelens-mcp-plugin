mod protocol;

use anyhow::Result;
use codelens_core::{
    add_import, analyze_missing_imports, check_lsp_status, create_text_file, delete_lines,
    extract_word_at_position, find_circular_dependencies, find_dead_code, find_dead_code_v2,
    find_files, find_referencing_symbols_via_text, find_scoped_references, get_blast_radius,
    get_callees, get_callers, get_change_coupling, get_changed_files, get_diff_symbols,
    get_importance, get_importers, get_lsp_recipe, get_symbols_overview, get_type_hierarchy_native,
    insert_after_symbol, insert_at_line, insert_before_symbol, list_dir, read_file, rename,
    replace_content, replace_lines, replace_symbol_body, search_for_pattern,
    search_for_pattern_smart, search_symbols_hybrid, LspDiagnosticRequest, LspRenamePlanRequest,
    LspRequest, LspSessionPool, LspTypeHierarchyRequest, LspWorkspaceSymbolRequest, ProjectRoot,
    SymbolIndex, SymbolInfo, SymbolKind,
};
use protocol::{JsonRpcRequest, JsonRpcResponse, Tool, ToolCallResponse, ToolResponseMeta};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::sync::Mutex;

struct AppState {
    project: ProjectRoot,
    symbol_index: Mutex<SymbolIndex>,
    lsp_pool: Mutex<LspSessionPool>,
    preset: ToolPreset,
    memories_dir: std::path::PathBuf,
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
    "get_callers",
    "get_callees",
    "get_symbol_importance",
    "find_dead_code",
];

impl AppState {
    fn new(project: ProjectRoot, preset: ToolPreset) -> Self {
        let symbol_index = SymbolIndex::new(project.clone());
        let lsp_pool = LspSessionPool::new(project.clone());
        let memories_dir = project.as_path().join(".serena").join("memories");
        Self {
            project,
            symbol_index: Mutex::new(symbol_index),
            lsp_pool: Mutex::new(lsp_pool),
            preset,
            memories_dir,
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
        .unwrap_or(ToolPreset::Full);

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
        "resources/list" => JsonRpcResponse::result(
            request.id,
            json!({
                "resources": resources(state)
            }),
        ),
        "resources/read" => {
            let uri = request
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            JsonRpcResponse::result(request.id, read_resource(state, uri))
        }
        "prompts/list" => JsonRpcResponse::result(
            request.id,
            json!({
                "prompts": prompts()
            }),
        ),
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

    let result: anyhow::Result<(serde_json::Value, ToolResponseMeta)> = (|| match name {
        "get_current_config" => {
            let stats = state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
                .stats()?;
            Ok((
                json!({
                    "runtime": "rust-core",
                    "project_root": state.project.as_path().display().to_string(),
                    "editor_integration": false,
                    "available_backends": ["filesystem", "tree-sitter-cached", "lsp_pooled"],
                    "symbol_index": stats
                }),
                success_meta("rust-core", 1.0),
            ))
        }
        "read_file" => {
            let path = required_string(&arguments, "relative_path")?;
            let start_line = arguments
                .get("start_line")
                .and_then(|value| value.as_u64())
                .map(|v| v as usize);
            let end_line = arguments
                .get("end_line")
                .and_then(|value| value.as_u64())
                .map(|v| v as usize);
            read_file(&state.project, path, start_line, end_line)
                .map(|value| (json!(value), success_meta("filesystem", 1.0)))
        }
        "list_dir" => {
            let path = required_string(&arguments, "relative_path")?;
            let recursive = arguments
                .get("recursive")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            list_dir(&state.project, path, recursive).map(|value| {
                (
                    json!({ "entries": value, "count": value.len() }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "find_file" => {
            let pattern = required_string(&arguments, "wildcard_pattern")?;
            let dir = arguments
                .get("relative_dir")
                .and_then(|value| value.as_str());
            find_files(&state.project, pattern, dir).map(|value| {
                (
                    json!({ "files": value, "count": value.len() }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "search_for_pattern" => {
            let pattern = arguments
                .get("pattern")
                .or_else(|| arguments.get("substring_pattern"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing pattern"))?;
            let file_glob = arguments.get("file_glob").and_then(|value| value.as_str());
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;
            let smart = arguments
                .get("smart")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let ctx_fallback = arguments
                .get("context_lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let ctx_before = arguments
                .get("context_lines_before")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(ctx_fallback);
            let ctx_after = arguments
                .get("context_lines_after")
                .and_then(|v| v.as_u64())
                .map(|v| v as usize)
                .unwrap_or(ctx_fallback);
            if smart {
                search_for_pattern_smart(
                    &state.project,
                    pattern,
                    file_glob,
                    max_results,
                    ctx_before,
                    ctx_after,
                )
                .map(|value| {
                    (
                        json!({ "matches": value, "count": value.len() }),
                        success_meta("tree-sitter+filesystem", 0.96),
                    )
                })
            } else {
                search_for_pattern(
                    &state.project,
                    pattern,
                    file_glob,
                    max_results,
                    ctx_before,
                    ctx_after,
                )
                .map(|value| {
                    (
                        json!({ "matches": value, "count": value.len() }),
                        success_meta("filesystem", 0.98),
                    )
                })
            }
        }
        "find_annotations" => {
            let tags = arguments
                .get("tags")
                .and_then(|value| value.as_str())
                .unwrap_or("TODO,FIXME,HACK,DEPRECATED,XXX,NOTE");
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(100) as usize;
            let tag_list = tags
                .split(',')
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .collect::<Vec<_>>();
            let pattern = format!(r"\b({})\b[:\s]*(.*)", tag_list.join("|"));
            search_for_pattern(&state.project, &pattern, None, max_results, 0, 0).map(|value| {
                let grouped = tag_list
                    .iter()
                    .filter_map(|tag| {
                        let matches = value
                            .iter()
                            .filter(|entry| {
                                entry.matched_text.eq_ignore_ascii_case(tag)
                                    || entry.line_content.contains(tag)
                            })
                            .map(|entry| {
                                json!({
                                    "file": entry.file_path,
                                    "line": entry.line,
                                    "text": entry.line_content
                                })
                            })
                            .collect::<Vec<_>>();
                        if matches.is_empty() {
                            None
                        } else {
                            Some(((*tag).to_owned(), serde_json::Value::Array(matches)))
                        }
                    })
                    .collect::<serde_json::Map<String, serde_json::Value>>();
                (
                    json!({
                        "tags": grouped,
                        "total": value.len()
                    }),
                    success_meta("filesystem", 0.97),
                )
            })
        }
        "find_tests" => {
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(100) as usize;
            let pattern = r"\b(def test_|func Test|@Test\b|it\s*\(|describe\s*\(|test\s*\()";
            search_for_pattern(&state.project, pattern, None, max_results, 0, 0).map(|value| {
                (
                    json!({
                        "tests": value,
                        "count": value.len()
                    }),
                    success_meta("filesystem", 0.97),
                )
            })
        }
        "get_complexity" => {
            let path = required_string(&arguments, "path")?;
            let symbol_name = arguments
                .get("symbol_name")
                .and_then(|value| value.as_str());
            let file_result = read_file(&state.project, path, None, None)?;
            let lines = file_result.content.lines().collect::<Vec<_>>();
            let symbols = state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
                .get_symbols_overview(path, 2)?;

            let functions = flatten_symbols(&symbols)
                .into_iter()
                .filter(|symbol| matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method))
                .filter(|symbol| symbol_name.is_none_or(|name| symbol.name == name))
                .map(|symbol| {
                    let start = symbol.line.saturating_sub(1).min(lines.len());
                    let end = (symbol.line + 50).min(lines.len());
                    let branches = count_branches(&lines[start..end]);
                    json!({
                        "name": symbol.name,
                        "kind": symbol.kind.kind_label(),
                        "file": symbol.file_path,
                        "line": symbol.line,
                        "branches": branches,
                        "complexity": 1 + branches
                    })
                })
                .collect::<Vec<_>>();

            let results = if functions.is_empty() {
                let branches = count_branches(&lines);
                vec![json!({
                    "name": path,
                    "branches": branches,
                    "complexity": 1 + branches
                })]
            } else {
                functions
            };

            let avg_complexity = if results.is_empty() {
                0.0
            } else {
                results
                    .iter()
                    .filter_map(|entry| entry.get("complexity").and_then(|value| value.as_i64()))
                    .map(|value| value as f64)
                    .sum::<f64>()
                    / results.len() as f64
            };

            Ok((
                json!({
                    "path": path,
                    "functions": results,
                    "count": results.len(),
                    "avg_complexity": avg_complexity
                }),
                success_meta("tree-sitter-cached", 0.89),
            ))
        }
        "get_changed_files" => {
            let git_ref = arguments.get("ref").and_then(|v| v.as_str());
            let include_untracked = arguments
                .get("include_untracked")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let changed = get_changed_files(&state.project, git_ref, include_untracked)?;
            let ref_label = git_ref.unwrap_or("HEAD");
            Ok((
                json!({
                    "ref": ref_label,
                    "files": changed,
                    "count": changed.len()
                }),
                success_meta("git", 0.95),
            ))
        }
        "get_diff_symbols" => {
            let git_ref = arguments.get("ref").and_then(|v| v.as_str());
            let diff_symbols = get_diff_symbols(&state.project, git_ref)?;
            let ref_label = git_ref.unwrap_or("HEAD");
            let enriched = diff_symbols
                .into_iter()
                .map(|entry| {
                    let symbol_count = state
                        .symbol_index
                        .lock()
                        .map(|mut index| {
                            index
                                .get_symbols_overview(&entry.file, 1)
                                .map(|s| s.len())
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);
                    json!({
                        "file": entry.file,
                        "status": entry.status,
                        "symbol_count": symbol_count
                    })
                })
                .collect::<Vec<_>>();
            Ok((
                json!({
                    "ref": ref_label,
                    "files": enriched,
                    "count": enriched.len()
                }),
                success_meta("git+tree-sitter", 0.9),
            ))
        }
        "get_blast_radius" => {
            let file_path = required_string(&arguments, "file_path")?;
            let max_depth = arguments
                .get("max_depth")
                .and_then(|value| value.as_u64())
                .unwrap_or(3) as usize;
            get_blast_radius(&state.project, file_path, max_depth).map(|value| {
                (
                    json!({
                        "file": file_path,
                        "affected_files": value,
                        "count": value.len()
                    }),
                    success_meta("import-graph", 0.86),
                )
            })
        }
        "get_impact_analysis" => {
            let file_path = required_string(&arguments, "file_path")?;
            let max_depth = arguments
                .get("max_depth")
                .and_then(|value| value.as_u64())
                .unwrap_or(3) as usize;

            // 1. Blast radius
            let blast = get_blast_radius(&state.project, file_path, max_depth).unwrap_or_default();

            // 2. Symbols in the target file
            let symbols = state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("lock poisoned"))?
                .get_symbols_overview(file_path, 1)
                .unwrap_or_default();
            let symbol_names: Vec<_> = flatten_symbols(&symbols)
                .iter()
                .map(|s| json!({"name": s.name, "kind": s.kind.as_label(), "line": s.line}))
                .collect();

            // 3. Importers (direct)
            let importers = get_importers(&state.project, file_path, 20).unwrap_or_default();

            // 4. Affected file summary with symbol counts
            let affected: Vec<_> = blast
                .iter()
                .map(|b| {
                    let sym_count = state
                        .symbol_index
                        .lock()
                        .ok()
                        .and_then(|mut idx| idx.get_symbols_overview(&b.file, 1).ok())
                        .map(|s| s.len())
                        .unwrap_or(0);
                    json!({"file": b.file, "depth": b.depth, "symbol_count": sym_count})
                })
                .collect();

            Ok((
                json!({
                    "file": file_path,
                    "symbols": symbol_names,
                    "symbol_count": symbol_names.len(),
                    "direct_importers": importers,
                    "blast_radius": affected,
                    "total_affected_files": affected.len(),
                }),
                success_meta("import-graph+tree-sitter", 0.85),
            ))
        }
        "find_importers" => {
            let file_path = required_string(&arguments, "file_path")?;
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;
            get_importers(&state.project, file_path, max_results).map(|value| {
                (
                    json!({
                        "file": file_path,
                        "importers": value,
                        "count": value.len()
                    }),
                    success_meta("import-graph", 0.87),
                )
            })
        }
        "get_symbol_importance" => {
            let top_n = arguments
                .get("top_n")
                .and_then(|value| value.as_u64())
                .unwrap_or(20) as usize;
            get_importance(&state.project, top_n).map(|value| {
                (
                    json!({
                        "ranking": value,
                        "count": value.len()
                    }),
                    success_meta("import-graph", 0.84),
                )
            })
        }
        "find_dead_code" => {
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;
            find_dead_code(&state.project, max_results).map(|value| {
                (
                    json!({
                        "dead_code": value,
                        "count": value.len()
                    }),
                    success_meta("import-graph", 0.83),
                )
            })
        }
        "find_dead_code_v2" => {
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;
            find_dead_code_v2(&state.project, max_results).map(|value| {
                (
                    json!({
                        "dead_code": value,
                        "count": value.len()
                    }),
                    success_meta("call-graph+import-graph", 0.82),
                )
            })
        }
        "get_symbols_overview" => {
            let path = required_string(&arguments, "path")?;
            let depth = arguments
                .get("depth")
                .and_then(|value| value.as_u64())
                .unwrap_or(1) as usize;
            state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
                .get_symbols_overview(path, depth)
                .map(|value| {
                    (
                        json!({ "symbols": value, "count": value.len() }),
                        success_meta("tree-sitter-cached", 0.93),
                    )
                })
        }
        "find_symbol" => {
            let symbol_id = arguments.get("symbol_id").and_then(|v| v.as_str());
            let name = symbol_id
                .or_else(|| arguments.get("name").and_then(|v| v.as_str()))
                .ok_or_else(|| anyhow::anyhow!("either 'symbol_id' or 'name' is required"))?;
            let file_path = arguments.get("file_path").and_then(|value| value.as_str());
            let include_body = arguments
                .get("include_body")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let exact_match = arguments
                .get("exact_match")
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            let max_matches = arguments
                .get("max_matches")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;
            state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
                .find_symbol(name, file_path, include_body, exact_match, max_matches)
                .map(|value| {
                    (
                        json!({ "symbols": value, "count": value.len() }),
                        success_meta("tree-sitter-cached", 0.93),
                    )
                })
        }
        "get_ranked_context" => {
            let query = required_string(&arguments, "query")?;
            let path = arguments.get("path").and_then(|value| value.as_str());
            let max_tokens = arguments
                .get("max_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(4000) as usize;
            let include_body = arguments
                .get("include_body")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            let depth = arguments
                .get("depth")
                .and_then(|value| value.as_u64())
                .unwrap_or(2) as usize;
            state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
                .get_ranked_context(query, path, max_tokens, include_body, depth)
                .map(|value| (json!(value), success_meta("tree-sitter-cached", 0.91)))
        }
        "refresh_symbol_index" => {
            let stats = state
                .symbol_index
                .lock()
                .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
                .refresh_all()?;
            Ok((json!(stats), success_meta("tree-sitter-cached", 0.95)))
        }
        "find_referencing_symbols" => {
            let file_path = required_string(&arguments, "file_path")?.to_owned();
            let symbol_name_param = arguments.get("symbol_name").and_then(|v| v.as_str());
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;

            // Fast path: symbol_name provided -> text-based search directly
            if let Some(sym_name) = symbol_name_param {
                find_referencing_symbols_via_text(
                    &state.project,
                    sym_name,
                    Some(&file_path),
                    max_results,
                )
                .map(|value| {
                    (
                        json!({ "references": value, "count": value.len() }),
                        success_meta("text_search", 0.80),
                    )
                })
            } else {
                // LSP path with text fallback
                let line = arguments
                    .get("line")
                    .and_then(|value| value.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing line or symbol_name"))?
                    as usize;
                let column = arguments
                    .get("column")
                    .and_then(|value| value.as_u64())
                    .ok_or_else(|| anyhow::anyhow!("Missing column or symbol_name"))?
                    as usize;
                let command = arguments
                    .get("command")
                    .and_then(|value| value.as_str())
                    .map(ToOwned::to_owned)
                    .or_else(|| default_lsp_command_for_path(&file_path));

                if let Some(command) = command {
                    let args = arguments
                        .get("args")
                        .and_then(|value| value.as_array())
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_else(|| default_lsp_args_for_command(&command));

                    let lsp_result = state
                        .lsp_pool
                        .lock()
                        .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                        .find_referencing_symbols(&LspRequest {
                            command,
                            args,
                            file_path: file_path.clone(),
                            line,
                            column,
                            max_results,
                        });

                    match lsp_result {
                        Ok(value) => Ok((
                            json!({ "references": value, "count": value.len() }),
                            success_meta("lsp_pooled", 0.9),
                        )),
                        Err(_) => {
                            // LSP failed -> text fallback
                            let word =
                                extract_word_at_position(&state.project, &file_path, line, column)?;
                            find_referencing_symbols_via_text(
                                &state.project,
                                &word,
                                Some(&file_path),
                                max_results,
                            )
                            .map(|value| {
                                (
                                    json!({ "references": value, "count": value.len() }),
                                    success_meta("text_fallback", 0.75),
                                )
                            })
                        }
                    }
                } else {
                    // No LSP command available -> text fallback directly
                    let word = extract_word_at_position(&state.project, &file_path, line, column)?;
                    find_referencing_symbols_via_text(
                        &state.project,
                        &word,
                        Some(&file_path),
                        max_results,
                    )
                    .map(|value| {
                        (
                            json!({ "references": value, "count": value.len() }),
                            success_meta("text_fallback", 0.75),
                        )
                    })
                }
            }
        }
        "get_file_diagnostics" => {
            let file_path = required_string(&arguments, "file_path")?.to_owned();
            let command = arguments
                .get("command")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| default_lsp_command_for_path(&file_path))
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing command and no default LSP mapping for file")
                })?;
            let args = arguments
                .get("args")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| default_lsp_args_for_command(&command));
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(200) as usize;

            state
                .lsp_pool
                .lock()
                .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                .get_diagnostics(&LspDiagnosticRequest {
                    command,
                    args,
                    file_path,
                    max_results,
                })
                .map(|value| {
                    (
                        json!({ "diagnostics": value, "count": value.len() }),
                        success_meta("lsp_pooled", 0.9),
                    )
                })
        }
        "search_workspace_symbols" => {
            let query = required_string(&arguments, "query")?.to_owned();
            let command = required_string(&arguments, "command")?.to_owned();
            let args = arguments
                .get("args")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| default_lsp_args_for_command(&command));
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;

            state
                .lsp_pool
                .lock()
                .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                .search_workspace_symbols(&LspWorkspaceSymbolRequest {
                    command,
                    args,
                    query,
                    max_results,
                })
                .map(|value| {
                    (
                        json!({ "symbols": value, "count": value.len() }),
                        success_meta("lsp_pooled", 0.88),
                    )
                })
        }
        "get_type_hierarchy" => {
            let query = arguments
                .get("name_path")
                .or_else(|| arguments.get("fully_qualified_name"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!("Either 'name_path' or 'fully_qualified_name' is required")
                })?
                .to_owned();
            let relative_path = arguments
                .get("relative_path")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let hierarchy_type = arguments
                .get("hierarchy_type")
                .and_then(|value| value.as_str())
                .unwrap_or("both")
                .to_owned();
            let depth = arguments
                .get("depth")
                .and_then(|value| value.as_u64())
                .unwrap_or(1) as usize;
            let command = arguments
                .get("command")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    relative_path
                        .as_deref()
                        .and_then(|p| default_lsp_command_for_path(p))
                });

            // Try LSP first, fall back to native tree-sitter
            if let Some(command) = command {
                let args = arguments
                    .get("args")
                    .and_then(|value| value.as_array())
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| default_lsp_args_for_command(&command));

                let lsp_result = state
                    .lsp_pool
                    .lock()
                    .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                    .get_type_hierarchy(&LspTypeHierarchyRequest {
                        command,
                        args,
                        query: query.clone(),
                        relative_path: relative_path.clone(),
                        hierarchy_type: hierarchy_type.clone(),
                        depth: if depth == 0 { 8 } else { depth },
                    });

                match lsp_result {
                    Ok(value) => Ok((json!(value), success_meta("lsp_pooled", 0.82))),
                    Err(_) => {
                        // LSP failed — fall back to native
                        get_type_hierarchy_native(
                            &state.project,
                            &query,
                            relative_path.as_deref(),
                            &hierarchy_type,
                            depth,
                        )
                        .map(|value| (json!(value), success_meta("tree-sitter-native", 0.80)))
                    }
                }
            } else {
                // No LSP command — use native directly
                get_type_hierarchy_native(
                    &state.project,
                    &query,
                    relative_path.as_deref(),
                    &hierarchy_type,
                    depth,
                )
                .map(|value| (json!(value), success_meta("tree-sitter-native", 0.80)))
            }
        }
        "plan_symbol_rename" => {
            let file_path = required_string(&arguments, "file_path")?.to_owned();
            let line = arguments
                .get("line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing line"))? as usize;
            let column = arguments
                .get("column")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing column"))?
                as usize;
            let new_name = arguments
                .get("new_name")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            let command = arguments
                .get("command")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned)
                .or_else(|| default_lsp_command_for_path(&file_path))
                .ok_or_else(|| {
                    anyhow::anyhow!("Missing command and no default LSP mapping for file")
                })?;
            let args = arguments
                .get("args")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| default_lsp_args_for_command(&command));

            state
                .lsp_pool
                .lock()
                .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                .get_rename_plan(&LspRenamePlanRequest {
                    command,
                    args,
                    file_path,
                    line,
                    column,
                    new_name,
                })
                .map(|value| (json!(value), success_meta("lsp_pooled", 0.86)))
        }
        "rename_symbol" => {
            let file_path = required_string(&arguments, "file_path")?;
            let symbol_name = arguments
                .get("symbol_name")
                .or_else(|| arguments.get("name"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing symbol_name or name"))?;
            let new_name = required_string(&arguments, "new_name")?;
            let name_path = arguments.get("name_path").and_then(|v| v.as_str());
            let scope = match arguments.get("scope").and_then(|v| v.as_str()) {
                Some("file") => rename::RenameScope::File,
                _ => rename::RenameScope::Project,
            };
            let dry_run = arguments
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            rename::rename_symbol(
                &state.project,
                file_path,
                symbol_name,
                new_name,
                name_path,
                scope,
                dry_run,
            )
            .map(|value| (json!(value), success_meta("tree-sitter+filesystem", 0.90)))
        }
        "create_text_file" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let content = required_string(&arguments, "content")?;
            let overwrite = arguments
                .get("overwrite")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            create_text_file(&state.project, relative_path, content, overwrite).map(|_| {
                (
                    json!({ "created": relative_path }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "delete_lines" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let start_line = arguments
                .get("start_line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing start_line"))?
                as usize;
            let end_line = arguments
                .get("end_line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing end_line"))?
                as usize;
            delete_lines(&state.project, relative_path, start_line, end_line).map(|content| {
                (
                    json!({ "content": content }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "insert_at_line" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let line = arguments
                .get("line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing line"))? as usize;
            let content = required_string(&arguments, "content")?;
            insert_at_line(&state.project, relative_path, line, content).map(|modified| {
                (
                    json!({ "content": modified }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "replace_lines" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let start_line = arguments
                .get("start_line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing start_line"))?
                as usize;
            let end_line = arguments
                .get("end_line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing end_line"))?
                as usize;
            let new_content = required_string(&arguments, "new_content")?;
            replace_lines(
                &state.project,
                relative_path,
                start_line,
                end_line,
                new_content,
            )
            .map(|content| {
                (
                    json!({ "content": content }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "replace_content" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let old_text = required_string(&arguments, "old_text")?;
            let new_text = required_string(&arguments, "new_text")?;
            let regex_mode = arguments
                .get("regex_mode")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            replace_content(
                &state.project,
                relative_path,
                old_text,
                new_text,
                regex_mode,
            )
            .map(|(content, count)| {
                (
                    json!({ "content": content, "replacements": count }),
                    success_meta("filesystem", 1.0),
                )
            })
        }
        "replace_symbol_body" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let symbol_name = required_string(&arguments, "symbol_name")?;
            let name_path = arguments.get("name_path").and_then(|v| v.as_str());
            let new_body = required_string(&arguments, "new_body")?;
            replace_symbol_body(
                &state.project,
                relative_path,
                symbol_name,
                name_path,
                new_body,
            )
            .map(|content| {
                (
                    json!({ "content": content }),
                    success_meta("tree-sitter+filesystem", 0.95),
                )
            })
        }
        "insert_before_symbol" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let symbol_name = required_string(&arguments, "symbol_name")?;
            let name_path = arguments.get("name_path").and_then(|v| v.as_str());
            let content = required_string(&arguments, "content")?;
            insert_before_symbol(
                &state.project,
                relative_path,
                symbol_name,
                name_path,
                content,
            )
            .map(|modified| {
                (
                    json!({ "content": modified }),
                    success_meta("tree-sitter+filesystem", 0.95),
                )
            })
        }
        "insert_after_symbol" => {
            let relative_path = required_string(&arguments, "relative_path")?;
            let symbol_name = required_string(&arguments, "symbol_name")?;
            let name_path = arguments.get("name_path").and_then(|v| v.as_str());
            let content = required_string(&arguments, "content")?;
            insert_after_symbol(
                &state.project,
                relative_path,
                symbol_name,
                name_path,
                content,
            )
            .map(|modified| {
                (
                    json!({ "content": modified }),
                    success_meta("tree-sitter+filesystem", 0.95),
                )
            })
        }
        "find_referencing_code_snippets" => {
            let symbol_name = required_string(&arguments, "symbol_name")?;
            let file_glob = arguments.get("file_glob").and_then(|v| v.as_str());
            let context_lines = arguments
                .get("context_lines")
                .and_then(|v| v.as_u64())
                .unwrap_or(2) as usize;
            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            search_for_pattern(
                &state.project,
                symbol_name,
                file_glob,
                max_results,
                context_lines,
                context_lines,
            )
            .map(|matches| {
                let snippets = matches
                    .iter()
                    .map(|m| {
                        let mut obj = json!({
                            "file_path": m.file_path,
                            "line": m.line,
                            "column": m.column,
                            "matched_text": m.matched_text,
                            "line_content": m.line_content,
                        });
                        if !m.context_before.is_empty() {
                            obj["context_before"] = json!(m.context_before);
                        }
                        if !m.context_after.is_empty() {
                            obj["context_after"] = json!(m.context_after);
                        }
                        obj
                    })
                    .collect::<Vec<_>>();
                (
                    json!({ "snippets": snippets, "count": snippets.len() }),
                    success_meta("filesystem", 0.92),
                )
            })
        }
        "find_scoped_references" => {
            let symbol_name = required_string(&arguments, "symbol_name")?;
            let file_path = arguments.get("file_path").and_then(|v| v.as_str());
            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            if let Some(fp) = file_path {
                find_scoped_references(&state.project, symbol_name, Some(fp), max_results)
            } else {
                find_scoped_references(&state.project, symbol_name, None, max_results)
            }
            .map(|refs| {
                (
                    json!({ "references": refs, "count": refs.len() }),
                    success_meta("tree-sitter-scope", 0.95),
                )
            })
        }
        "get_callers" => {
            let function_name = required_string(&arguments, "function_name")?;
            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            get_callers(&state.project, function_name, max_results).map(|value| {
                (
                    json!({
                        "function": function_name,
                        "callers": value,
                        "count": value.len()
                    }),
                    success_meta("call-graph", 0.85),
                )
            })
        }
        "get_callees" => {
            let function_name = required_string(&arguments, "function_name")?;
            let file_path = arguments.get("file_path").and_then(|v| v.as_str());
            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(50) as usize;
            get_callees(&state.project, function_name, file_path, max_results).map(|value| {
                (
                    json!({
                        "function": function_name,
                        "callees": value,
                        "count": value.len()
                    }),
                    success_meta("call-graph", 0.85),
                )
            })
        }
        "find_circular_dependencies" => {
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(50) as usize;
            find_circular_dependencies(&state.project, max_results).map(|value| {
                (
                    json!({
                        "cycles": value,
                        "count": value.len()
                    }),
                    success_meta("import-graph", 0.88),
                )
            })
        }
        "get_change_coupling" => {
            let months = arguments
                .get("months")
                .and_then(|value| value.as_u64())
                .unwrap_or(6) as usize;
            let min_strength = arguments
                .get("min_strength")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.3);
            let min_commits = arguments
                .get("min_commits")
                .and_then(|value| value.as_u64())
                .unwrap_or(3) as usize;
            let max_results = arguments
                .get("max_results")
                .and_then(|value| value.as_u64())
                .unwrap_or(30) as usize;
            get_change_coupling(
                &state.project,
                months,
                min_strength,
                min_commits,
                max_results,
            )
            .map(|value| {
                (
                    json!({
                        "coupling": value,
                        "count": value.len()
                    }),
                    success_meta("git", 0.85),
                )
            })
        }
        "check_lsp_status" => {
            let statuses = check_lsp_status();
            Ok((
                json!({ "servers": statuses, "count": statuses.len() }),
                success_meta("lsp", 1.0),
            ))
        }
        "get_lsp_recipe" => {
            let extension = required_string(&arguments, "extension")?;
            match get_lsp_recipe(extension) {
                Some(recipe) => Ok((json!(recipe), success_meta("lsp", 1.0))),
                None => Err(anyhow::anyhow!(
                    "No LSP recipe found for extension: {extension}"
                )),
            }
        }
        "search_symbols_fuzzy" => {
            let query = required_string(&arguments, "query")?;
            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(30) as usize;
            let fuzzy_threshold = arguments
                .get("fuzzy_threshold")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.6);
            search_symbols_hybrid(&state.project, query, max_results, fuzzy_threshold).map(
                |value| {
                    (
                        json!({ "results": value, "count": value.len() }),
                        success_meta("sqlite+fuzzy", 0.9),
                    )
                },
            )
        }
        // ── Auto-import tools ────────────────────────────────────────────
        "analyze_missing_imports" => {
            let file_path = required_string(&arguments, "file_path")?;
            analyze_missing_imports(&state.project, file_path)
                .map(|value| (json!(value), success_meta("tree-sitter+index", 0.85)))
        }
        "add_import" => {
            let file_path = required_string(&arguments, "file_path")?;
            let import_statement = required_string(&arguments, "import_statement")?;
            add_import(&state.project, file_path, import_statement)
                .map(|content| {
                    (
                        json!({"success": true, "file_path": file_path, "content_length": content.len()}),
                        success_meta("filesystem", 1.0),
                    )
                })
        }
        // ── Serena-compatible: no-op thinking/mode tools ────────────────
        "think_about_collected_information"
        | "think_about_task_adherence"
        | "think_about_whether_you_are_done" => Ok((json!(""), success_meta("noop", 1.0))),
        "switch_modes" => {
            let mode = arguments
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            Ok((
                json!({"status":"ok","mode":mode,"note":"Mode switching is a no-op in standalone mode"}),
                success_meta("noop", 1.0),
            ))
        }
        // ── Serena-compatible: memory tools (.serena/memories/) ─────────
        "list_memories" => {
            let topic = arguments.get("topic").and_then(|v| v.as_str());
            let names = list_memory_names(&state.memories_dir, topic);
            Ok((
                json!({"topic": topic, "count": names.len(), "memories": names.iter().map(|n| json!({"name": n, "path": format!(".serena/memories/{n}.md")})).collect::<Vec<_>>()}),
                success_meta("filesystem", 1.0),
            ))
        }
        "read_memory" => {
            let name = required_string(&arguments, "memory_name")?;
            let path = resolve_memory_path(&state.memories_dir, name)?;
            let content = std::fs::read_to_string(&path)
                .map_err(|_| anyhow::anyhow!("Memory not found: {name}"))?;
            Ok((
                json!({"memory_name": name, "content": content}),
                success_meta("filesystem", 1.0),
            ))
        }
        "write_memory" => {
            let name = required_string(&arguments, "memory_name")?;
            let content = required_string(&arguments, "content")?;
            let path = resolve_memory_path(&state.memories_dir, name)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, content)?;
            Ok((
                json!({"status":"ok","memory_name": name}),
                success_meta("filesystem", 1.0),
            ))
        }
        "delete_memory" => {
            let name = required_string(&arguments, "memory_name")?;
            let path = resolve_memory_path(&state.memories_dir, name)?;
            if !path.is_file() {
                return Err(anyhow::anyhow!("Memory not found: {name}"));
            }
            std::fs::remove_file(&path)?;
            Ok((
                json!({"status":"ok","memory_name": name}),
                success_meta("filesystem", 1.0),
            ))
        }
        "edit_memory" => {
            let name = required_string(&arguments, "memory_name")?;
            let content = required_string(&arguments, "content")?;
            let path = resolve_memory_path(&state.memories_dir, name)?;
            if !path.is_file() {
                return Err(anyhow::anyhow!("Memory not found: {name}"));
            }
            std::fs::write(&path, content)?;
            Ok((
                json!({"status":"ok","memory_name": name}),
                success_meta("filesystem", 1.0),
            ))
        }
        "rename_memory" => {
            let old_name = required_string(&arguments, "old_name")?;
            let new_name = required_string(&arguments, "new_name")?;
            let old_path = resolve_memory_path(&state.memories_dir, old_name)?;
            let new_path = resolve_memory_path(&state.memories_dir, new_name)?;
            if !old_path.is_file() {
                return Err(anyhow::anyhow!("Memory not found: {old_name}"));
            }
            if new_path.exists() {
                return Err(anyhow::anyhow!("Target already exists: {new_name}"));
            }
            if let Some(parent) = new_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&old_path, &new_path)?;
            Ok((
                json!({"status":"ok","old_name": old_name,"new_name": new_name}),
                success_meta("filesystem", 1.0),
            ))
        }
        // ── Serena-compatible: session/config tools ─────────────────────
        "activate_project" => {
            let project_name = state
                .project
                .as_path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let memory_count = list_memory_names(&state.memories_dir, None).len();
            Ok((
                json!({
                    "activated": true,
                    "project_name": project_name,
                    "project_base_path": state.project.as_path().to_string_lossy(),
                    "backend_id": "rust-core",
                    "memory_count": memory_count,
                    "serena_memories_dir": state.memories_dir.to_string_lossy()
                }),
                success_meta("session", 1.0),
            ))
        }
        "check_onboarding_performed" => {
            let required = [
                "project_overview",
                "style_and_conventions",
                "suggested_commands",
                "task_completion",
            ];
            let present = list_memory_names(&state.memories_dir, None);
            let missing: Vec<_> = required
                .iter()
                .filter(|r| !present.contains(&r.to_string()))
                .map(|s| *s)
                .collect();
            Ok((
                json!({
                    "onboarding_performed": missing.is_empty(),
                    "required_memories": required,
                    "present_memories": present,
                    "missing_memories": missing,
                    "serena_memories_dir": state.memories_dir.to_string_lossy(),
                    "backend_id": "rust-core"
                }),
                success_meta("session", 1.0),
            ))
        }
        "initial_instructions" => {
            let project_name = state
                .project
                .as_path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let memories = list_memory_names(&state.memories_dir, None);
            Ok((
                json!({
                    "project_name": project_name,
                    "project_base_path": state.project.as_path().to_string_lossy(),
                    "compatible_context": "standalone",
                    "backend_id": "rust-core",
                    "known_memories": memories,
                    "recommended_tools": [
                        "activate_project","get_current_config","check_onboarding_performed",
                        "list_memories","read_memory","write_memory",
                        "get_symbols_overview","find_symbol","find_referencing_symbols",
                        "search_for_pattern","get_type_hierarchy"
                    ]
                }),
                success_meta("session", 1.0),
            ))
        }
        "onboarding" => {
            let force = arguments
                .get("force")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !force {
                let existing = list_memory_names(&state.memories_dir, None);
                let required = [
                    "project_overview",
                    "style_and_conventions",
                    "suggested_commands",
                    "task_completion",
                ];
                if required.iter().all(|r| existing.contains(&r.to_string())) {
                    return Ok((
                        json!({"status":"already_onboarded","existing_memories": existing}),
                        success_meta("session", 1.0),
                    ));
                }
            }
            std::fs::create_dir_all(&state.memories_dir)?;
            let project_name = state
                .project
                .as_path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let defaults = [
                (
                    "project_overview",
                    format!(
                        "# Project: {project_name}\nBase path: {}\n",
                        state.project.as_path().display()
                    ),
                ),
                (
                    "style_and_conventions",
                    "# Style & Conventions\nTo be filled during onboarding.".to_string(),
                ),
                (
                    "suggested_commands",
                    "# Suggested Commands\n- cargo build\n- cargo test".to_string(),
                ),
                (
                    "task_completion",
                    "# Task Completion Checklist\n- Build passes\n- Tests pass\n- No regressions"
                        .to_string(),
                ),
            ];
            for (name, content) in &defaults {
                let path = state.memories_dir.join(format!("{name}.md"));
                if !path.exists() {
                    std::fs::write(&path, content)?;
                }
            }
            let created = list_memory_names(&state.memories_dir, None);
            Ok((
                json!({"status":"onboarded","project_name": project_name,"memories_created": created}),
                success_meta("session", 1.0),
            ))
        }
        "prepare_for_new_conversation" => {
            let project_name = state
                .project
                .as_path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Ok((
                json!({
                    "status":"ready",
                    "project_name": project_name,
                    "project_base_path": state.project.as_path().to_string_lossy(),
                    "backend_id": "rust-core",
                    "memory_count": list_memory_names(&state.memories_dir, None).len()
                }),
                success_meta("session", 1.0),
            ))
        }
        "summarize_changes" => Ok((
            json!({
                "instructions": "To summarize your changes:\n1. Use search_for_pattern to identify modified symbols\n2. Use get_symbols_overview to understand file structure\n3. Write a summary to memory using write_memory with name 'session_summary'",
                "project_name": state.project.as_path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            }),
            success_meta("session", 1.0),
        )),
        "list_queryable_projects" => {
            let project_name = state
                .project
                .as_path()
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let has_memories = state.memories_dir.is_dir();
            Ok((
                json!({
                    "projects": [{"name": project_name, "path": state.project.as_path().to_string_lossy(), "is_active": true, "has_memories": has_memories}],
                    "count": 1
                }),
                success_meta("session", 1.0),
            ))
        }
        // ── Agent high-level composite tools ─────────────────────────────
        "summarize_file" => {
            let file_path = required_string(&arguments, "file_path")?;
            let symbols = get_symbols_overview(&state.project, file_path, 2)?;
            let importers = get_importers(&state.project, file_path, 20).unwrap_or_default();
            let source =
                std::fs::read_to_string(state.project.resolve(file_path)?).unwrap_or_default();
            let line_count = source.lines().count();

            // Count functions/classes
            let mut functions = 0usize;
            let mut classes = 0usize;
            for sym in &symbols {
                match sym.kind {
                    SymbolKind::Function | SymbolKind::Method => functions += 1,
                    SymbolKind::Class | SymbolKind::Interface => classes += 1,
                    _ => {}
                }
                for child in &sym.children {
                    match child.kind {
                        SymbolKind::Function | SymbolKind::Method => functions += 1,
                        _ => {}
                    }
                }
            }

            Ok((
                json!({
                    "file_path": file_path,
                    "lines": line_count,
                    "classes": classes,
                    "functions": functions,
                    "symbols": symbols.iter().map(|s| json!({
                        "name": s.name, "kind": s.kind, "line": s.line,
                        "signature": s.signature, "id": s.id
                    })).collect::<Vec<_>>(),
                    "importers": importers.iter().map(|i| &i.file).collect::<Vec<_>>(),
                    "importer_count": importers.len(),
                }),
                success_meta("composite", 0.95),
            ))
        }
        "explain_code_flow" => {
            let function_name = required_string(&arguments, "function_name")?;
            let max_depth = arguments
                .get("max_depth")
                .and_then(|v| v.as_u64())
                .unwrap_or(3) as usize;
            let max_results = arguments
                .get("max_results")
                .and_then(|v| v.as_u64())
                .unwrap_or(20) as usize;

            let callers = get_callers(&state.project, function_name, max_results)?;
            let callees = get_callees(&state.project, function_name, None, max_results)?;

            Ok((
                json!({
                    "function": function_name,
                    "callers": callers.iter().map(|c| json!({
                        "name": c.function, "file": c.file, "line": c.line
                    })).collect::<Vec<_>>(),
                    "caller_count": callers.len(),
                    "callees": callees.iter().map(|c| json!({
                        "name": c.name, "line": c.line
                    })).collect::<Vec<_>>(),
                    "callee_count": callees.len(),
                    "flow_summary": format!(
                        "{} is called by {} function(s) and calls {} function(s)",
                        function_name, callers.len(), callees.len()
                    )
                }),
                success_meta("call-graph", 0.90),
            ))
        }
        "refactor_extract_function" => {
            let file_path = required_string(&arguments, "file_path")?;
            let start_line = arguments
                .get("start_line")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing start_line"))?
                as usize;
            let end_line = arguments
                .get("end_line")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing end_line"))?
                as usize;
            let new_name = required_string(&arguments, "new_name")?;
            let dry_run = arguments
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let resolved = state.project.resolve(file_path)?;
            let source = std::fs::read_to_string(&resolved)?;
            let lines: Vec<&str> = source.lines().collect();

            if start_line < 1 || end_line < start_line || end_line > lines.len() {
                return Err(anyhow::anyhow!(
                    "Invalid line range: {start_line}-{end_line} (file has {} lines)",
                    lines.len()
                ));
            }

            // Detect language for syntax
            let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");

            // Extract the selected lines
            let extracted: Vec<&str> = lines[(start_line - 1)..end_line].to_vec();
            let indent = extracted
                .first()
                .map(|l| {
                    let trimmed = l.trim_start();
                    &l[..l.len() - trimmed.len()]
                })
                .unwrap_or("");
            let body = extracted
                .iter()
                .map(|l| {
                    if l.len() > indent.len() && l.starts_with(indent) {
                        format!("    {}", &l[indent.len()..])
                    } else {
                        format!("    {}", l.trim())
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");

            // Generate function definition and call based on language
            let (func_def, func_call) = match ext {
                "py" => (
                    format!("def {new_name}():\n{body}\n"),
                    format!("{indent}{new_name}()"),
                ),
                "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => (
                    format!("function {new_name}() {{\n{body}\n}}\n"),
                    format!("{indent}{new_name}();"),
                ),
                "rs" => (
                    format!("fn {new_name}() {{\n{body}\n}}\n"),
                    format!("{indent}{new_name}();"),
                ),
                "go" => (
                    format!("func {new_name}() {{\n{body}\n}}\n"),
                    format!("{indent}{new_name}()"),
                ),
                "java" | "kt" => (
                    format!("private void {new_name}() {{\n{body}\n}}\n"),
                    format!("{indent}{new_name}();"),
                ),
                _ => (
                    format!("function {new_name}() {{\n{body}\n}}\n"),
                    format!("{indent}{new_name}();"),
                ),
            };

            if !dry_run {
                let mut new_lines = lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
                // Replace extracted range with call
                new_lines.drain((start_line - 1)..end_line);
                new_lines.insert(start_line - 1, func_call.clone());
                // Append function definition at end
                new_lines.push(String::new());
                new_lines.push(func_def.clone());
                let mut result = new_lines.join("\n");
                if source.ends_with('\n') && !result.ends_with('\n') {
                    result.push('\n');
                }
                std::fs::write(&resolved, &result)?;
            }

            Ok((
                json!({
                    "success": true,
                    "file_path": file_path,
                    "extracted_lines": format!("{start_line}-{end_line}"),
                    "new_function_name": new_name,
                    "function_definition": func_def,
                    "call_replacement": func_call,
                    "dry_run": dry_run
                }),
                success_meta("refactor", 0.90),
            ))
        }
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    })();

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

fn flatten_symbols(symbols: &[SymbolInfo]) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    let mut stack = symbols.iter().cloned().collect::<Vec<_>>();
    while let Some(mut symbol) = stack.pop() {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        stack.extend(children);
    }
    flat
}

fn count_branches(lines: &[&str]) -> i32 {
    lines.iter().map(|line| count_branches_in_line(line)).sum()
}

fn count_branches_in_line(line: &str) -> i32 {
    let mut count = 0i32;
    for token in [
        "if", "elif", "for", "while", "catch", "except", "case", "and", "or",
    ] {
        count += count_word_occurrences(line, token);
    }
    count += line.match_indices("&&").count() as i32;
    count += line.match_indices("||").count() as i32;
    if line.contains("else if") {
        count += 1;
    }
    count
}

fn count_word_occurrences(line: &str, needle: &str) -> i32 {
    line.match_indices(needle)
        .filter(|(index, _)| {
            let start_ok = *index == 0
                || !line[..*index]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let end = index + needle.len();
            let end_ok = end == line.len()
                || !line[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            start_ok && end_ok
        })
        .count() as i32
}

trait SymbolKindLabel {
    fn kind_label(&self) -> &'static str;
}

impl SymbolKindLabel for SymbolKind {
    fn kind_label(&self) -> &'static str {
        match self {
            SymbolKind::File => "file",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::Module => "module",
            SymbolKind::Method => "method",
            SymbolKind::Function => "function",
            SymbolKind::Property => "property",
            SymbolKind::Variable => "variable",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Unknown => "unknown",
        }
    }
}

fn success_meta(backend_used: &str, confidence: f64) -> ToolResponseMeta {
    ToolResponseMeta {
        backend_used: backend_used.to_owned(),
        confidence,
        degraded_reason: None,
    }
}

fn tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "get_current_config",
            "Return Rust core runtime information and symbol index stats.",
            json!({"type":"object","properties":{}}),
        ),
        Tool::new(
            "read_file",
            "Read the contents of a file with optional line range.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "start_line":{"type":"integer"},
                    "end_line":{"type":"integer"}
                },
                "required":["relative_path"]
            }),
        ),
        Tool::new(
            "list_dir",
            "List contents of a directory with optional recursive traversal.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "recursive":{"type":"boolean"}
                },
                "required":["relative_path"]
            }),
        ),
        Tool::new(
            "find_file",
            "Find files matching a wildcard pattern within the project or specified directory.",
            json!({
                "type":"object",
                "properties":{
                    "wildcard_pattern":{"type":"string"},
                    "relative_dir":{"type":"string"}
                },
                "required":["wildcard_pattern"]
            }),
        ),
        Tool::new(
            "search_for_pattern",
            "Search for a regex pattern across project files. Set smart=true to include enclosing symbol context (Smart Excerpt).",
            json!({
                "type":"object",
                "properties":{
                    "pattern":{"type":"string"},
                    "substring_pattern":{"type":"string"},
                    "file_glob":{"type":"string"},
                    "max_results":{"type":"integer"},
                    "smart":{"type":"boolean","description":"Include enclosing symbol context for each match"},
                    "context_lines":{"type":"integer","description":"Number of context lines before and after each match (default 0)"},
                    "context_lines_before":{"type":"integer","description":"Context lines before each match (overrides context_lines)"},
                    "context_lines_after":{"type":"integer","description":"Context lines after each match (overrides context_lines)"}
                }
            }),
        ),
        Tool::new(
            "find_annotations",
            "Find annotation comments such as TODO, FIXME, and HACK across the project.",
            json!({
                "type":"object",
                "properties":{
                    "tags":{"type":"string"},
                    "max_results":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "find_tests",
            "Find test functions or test blocks across the project using regex heuristics.",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "max_results":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "get_complexity",
            "Calculate approximate cyclomatic complexity for functions or methods in a file.",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "symbol_name":{"type":"string"}
                },
                "required":["path"]
            }),
        ),
        Tool::new(
            "get_changed_files",
            "Return files changed compared to a git ref, with symbol counts.",
            json!({
                "type":"object",
                "properties":{
                    "ref":{"type":"string"},
                    "include_untracked":{"type":"boolean"}
                }
            }),
        ),
        Tool::new(
            "get_diff_symbols",
            "Return changed files with symbol counts for each file, compared to a git ref.",
            json!({
                "type":"object",
                "properties":{
                    "ref":{"type":"string"}
                }
            }),
        ),
        Tool::new(
            "get_blast_radius",
            "Return files transitively affected by a file change for supported Python/JS/TS import graphs.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string"},
                    "max_depth":{"type":"integer"}
                },
                "required":["file_path"]
            }),
        ),
        Tool::new(
            "get_impact_analysis",
            "One-shot impact analysis: symbols in file + direct importers + blast radius with symbol counts. Replaces multiple sequential tool calls.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string"},
                    "max_depth":{"type":"integer"}
                },
                "required":["file_path"]
            }),
        ),
        Tool::new(
            "find_importers",
            "Find reverse import dependencies for supported Python/JS/TS import graphs.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string"},
                    "max_results":{"type":"integer"}
                },
                "required":["file_path"]
            }),
        ),
        Tool::new(
            "get_symbol_importance",
            "Return file importance ranking based on import-graph PageRank for supported Python/JS/TS projects.",
            json!({
                "type":"object",
                "properties":{
                    "top_n":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "find_dead_code",
            "Return dead-code file candidates for supported Python/JS/TS import-graph projects.",
            json!({
                "type":"object",
                "properties":{
                    "max_results":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "find_dead_code_v2",
            "Multi-pass dead code detection: unreferenced files (pass 1) and unreferenced symbols via call-graph analysis (pass 2), with entry-point and decorator exception filters.",
            json!({
                "type":"object",
                "properties":{
                    "max_results":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "get_symbols_overview",
            "Get an overview of code symbols in a file or directory.",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "depth":{"type":"integer"}
                },
                "required":["path"]
            }),
        ),
        Tool::new(
            "find_symbol",
            "Find a symbol by name or stable ID. Use symbol_id (e.g. 'src/main.py#function:Service/greet') for fastest exact lookup.",
            json!({
                "type":"object",
                "properties":{
                    "name":{"type":"string","description":"Symbol name to search for"},
                    "symbol_id":{"type":"string","description":"Stable symbol ID (file#kind:name_path). Overrides name."},
                    "file_path":{"type":"string"},
                    "include_body":{"type":"boolean"},
                    "exact_match":{"type":"boolean"},
                    "max_matches":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "get_ranked_context",
            "Return the most relevant symbols for a query within a token budget.",
            json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "path":{"type":"string"},
                    "max_tokens":{"type":"integer"},
                    "include_body":{"type":"boolean"},
                    "depth":{"type":"integer"}
                },
                "required":["query"]
            }),
        ),
        Tool::new(
            "refresh_symbol_index",
            "Rebuild the cached symbol index for the current project.",
            json!({
                "type":"object",
                "properties":{}
            }),
        ),
        Tool::new(
            "find_referencing_symbols",
            "Find references via LSP or text-based fallback. Provide symbol_name for direct text search without LSP, or line/column for LSP (with automatic text fallback on failure).",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string","description":"File containing or declaring the symbol"},
                    "symbol_name":{"type":"string","description":"Symbol name for text-based search (skips LSP)"},
                    "line":{"type":"integer","description":"Line number for LSP lookup"},
                    "column":{"type":"integer","description":"Column number for LSP lookup"},
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "max_results":{"type":"integer"}
                },
                "required":["file_path"]
            }),
        ),
        Tool::new(
            "get_file_diagnostics",
            "Get file diagnostics through a pooled stdio LSP server. command/args may be provided explicitly.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string"},
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "max_results":{"type":"integer"}
                },
                "required":["file_path"]
            }),
        ),
        Tool::new(
            "search_workspace_symbols",
            "Search workspace symbols through a pooled stdio LSP server. command is required because no file path is available for inference.",
            json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "max_results":{"type":"integer"}
                },
                "required":["query","command"]
            }),
        ),
        Tool::new(
            "get_type_hierarchy",
            "Get the type hierarchy through a pooled stdio LSP server. command is required because Rust does not infer a language server from a type name alone.",
            json!({
                "type":"object",
                "properties":{
                    "name_path":{"type":"string"},
                    "fully_qualified_name":{"type":"string"},
                    "relative_path":{"type":"string"},
                    "hierarchy_type":{"type":"string","enum":["super","sub","both"]},
                    "depth":{"type":"integer"},
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}}
                }
            }),
        ),
        Tool::new(
            "plan_symbol_rename",
            "Plan a safe symbol rename through pooled LSP prepareRename without applying edits.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string"},
                    "line":{"type":"integer"},
                    "column":{"type":"integer"},
                    "new_name":{"type":"string"},
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}}
                },
                "required":["file_path","line","column"]
            }),
        ),
        Tool::new(
            "rename_symbol",
            "Rename a symbol across one file (file scope) or the entire project. Supports dry_run for preview.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string","description":"File containing the symbol declaration"},
                    "symbol_name":{"type":"string","description":"Current symbol name"},
                    "name":{"type":"string","description":"Alias for symbol_name"},
                    "new_name":{"type":"string","description":"Desired new name"},
                    "name_path":{"type":"string","description":"Qualified name path (e.g. 'Class/method')"},
                    "scope":{"type":"string","enum":["file","project"],"description":"Rename scope (default: project)"},
                    "dry_run":{"type":"boolean","description":"Preview changes without modifying files"}
                },
                "required":["file_path","new_name"]
            }),
        ),
        Tool::new(
            "create_text_file",
            "Create a new file with the given content. If overwrite is false and the file already exists, returns an error.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "content":{"type":"string"},
                    "overwrite":{"type":"boolean"}
                },
                "required":["relative_path","content"]
            }),
        ),
        Tool::new(
            "delete_lines",
            "Delete lines [start_line, end_line) from a file (1-indexed, end exclusive). Returns the modified content.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "start_line":{"type":"integer"},
                    "end_line":{"type":"integer"}
                },
                "required":["relative_path","start_line","end_line"]
            }),
        ),
        Tool::new(
            "insert_at_line",
            "Insert content at a given line number (1-indexed) in a file. Returns the modified content.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "line":{"type":"integer"},
                    "content":{"type":"string"}
                },
                "required":["relative_path","line","content"]
            }),
        ),
        Tool::new(
            "replace_lines",
            "Replace lines [start_line, end_line) in a file with new_content (1-indexed, end exclusive). Returns the modified content.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "start_line":{"type":"integer"},
                    "end_line":{"type":"integer"},
                    "new_content":{"type":"string"}
                },
                "required":["relative_path","start_line","end_line","new_content"]
            }),
        ),
        Tool::new(
            "replace_content",
            "Replace old_text with new_text in a file, either literal or regex. Returns modified content and replacement count.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "old_text":{"type":"string"},
                    "new_text":{"type":"string"},
                    "regex_mode":{"type":"boolean"}
                },
                "required":["relative_path","old_text","new_text"]
            }),
        ),
        Tool::new(
            "replace_symbol_body",
            "Replace the entire body of a named symbol (function, class, etc.) in a file using tree-sitter byte offsets. Optionally disambiguate with name_path (e.g. 'ClassName/method').",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "symbol_name":{"type":"string"},
                    "name_path":{"type":"string"},
                    "new_body":{"type":"string"}
                },
                "required":["relative_path","symbol_name","new_body"]
            }),
        ),
        Tool::new(
            "insert_before_symbol",
            "Insert content immediately before a named symbol in a file using tree-sitter byte offsets. Optionally disambiguate with name_path.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "symbol_name":{"type":"string"},
                    "name_path":{"type":"string"},
                    "content":{"type":"string"}
                },
                "required":["relative_path","symbol_name","content"]
            }),
        ),
        Tool::new(
            "insert_after_symbol",
            "Insert content immediately after a named symbol in a file using tree-sitter byte offsets. Optionally disambiguate with name_path.",
            json!({
                "type":"object",
                "properties":{
                    "relative_path":{"type":"string"},
                    "symbol_name":{"type":"string"},
                    "name_path":{"type":"string"},
                    "content":{"type":"string"}
                },
                "required":["relative_path","symbol_name","content"]
            }),
        ),
        Tool::new(
            "find_referencing_code_snippets",
            "Find all code snippets that reference (use) a given symbol name across the project. Returns file, line, column, and matched line content.",
            json!({
                "type":"object",
                "properties":{
                    "symbol_name":{"type":"string"},
                    "file_glob":{"type":"string"},
                    "context_lines":{"type":"integer"},
                    "max_results":{"type":"integer"}
                },
                "required":["symbol_name"]
            }),
        ),
        Tool::new(
            "find_scoped_references",
            "Scope-aware reference search using tree-sitter AST. Classifies each reference as definition/read/write/import with enclosing scope context.",
            json!({
                "type":"object",
                "properties":{
                    "symbol_name":{"type":"string","description":"Symbol name to find references for"},
                    "file_path":{"type":"string","description":"Declaration file (for sorting, optional)"},
                    "max_results":{"type":"integer","description":"Max results (default 50)"}
                },
                "required":["symbol_name"]
            }),
        ),
        Tool::new(
            "get_callers",
            "Find all functions that call a given function across the project using tree-sitter call graph analysis.",
            json!({
                "type":"object",
                "properties":{
                    "function_name":{"type":"string"},
                    "max_results":{"type":"integer"}
                },
                "required":["function_name"]
            }),
        ),
        Tool::new(
            "get_callees",
            "Find all functions called by a given function using tree-sitter call graph analysis. Optionally scoped to a specific file.",
            json!({
                "type":"object",
                "properties":{
                    "function_name":{"type":"string"},
                    "file_path":{"type":"string"},
                    "max_results":{"type":"integer"}
                },
                "required":["function_name"]
            }),
        ),
        Tool::new(
            "find_circular_dependencies",
            "Detect circular import dependencies in the project using Tarjan SCC algorithm on the import graph.",
            json!({
                "type":"object",
                "properties":{
                    "max_results":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "get_change_coupling",
            "Analyze git history to find files that frequently change together (temporal coupling).",
            json!({
                "type":"object",
                "properties":{
                    "months":{"type":"integer"},
                    "min_strength":{"type":"number"},
                    "min_commits":{"type":"integer"},
                    "max_results":{"type":"integer"}
                }
            }),
        ),
        Tool::new(
            "check_lsp_status",
            "Check which LSP servers are installed on this machine and which are missing, with install commands.",
            json!({"type":"object","properties":{}}),
        ),
        Tool::new(
            "get_lsp_recipe",
            "Get the LSP server recipe (binary name, install command, args) for a given file extension.",
            json!({
                "type":"object",
                "properties":{
                    "extension":{"type":"string","description":"File extension without dot, e.g. 'py', 'ts', 'rs'"}
                },
                "required":["extension"]
            }),
        ),
        Tool::new(
            "search_symbols_fuzzy",
            "Hybrid symbol search: exact match (score 100), substring match (score 60), and fuzzy jaro_winkler match (score by similarity). Results deduplicated and sorted by score descending.",
            json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string","description":"Symbol name to search for"},
                    "max_results":{"type":"integer","description":"Maximum number of results to return (default 30)"},
                    "fuzzy_threshold":{"type":"number","description":"Minimum jaro_winkler similarity 0.0-1.0 for fuzzy matches (default 0.6)"}
                },
                "required":["query"]
            }),
        ),
        // ── Auto-import tools ────────────────────────────────────────────
        Tool::new(
            "analyze_missing_imports",
            "Detect unresolved symbols in a file and suggest import statements from the project's symbol index.",
            json!({"type":"object","properties":{"file_path":{"type":"string","description":"File to analyze"}},"required":["file_path"]}),
        ),
        Tool::new(
            "add_import",
            "Insert an import statement at the correct position in a file.",
            json!({"type":"object","properties":{"file_path":{"type":"string"},"import_statement":{"type":"string","description":"Import statement to add"}},"required":["file_path","import_statement"]}),
        ),
        // ── Agent high-level composite tools ─────────────────────────────
        Tool::new(
            "summarize_file",
            "Get a comprehensive summary of a file: structure, symbols, importers, line count — all in one call.",
            json!({"type":"object","properties":{"file_path":{"type":"string","description":"File to summarize"}},"required":["file_path"]}),
        ),
        Tool::new(
            "explain_code_flow",
            "Explain the call flow around a function: who calls it (callers) and what it calls (callees).",
            json!({"type":"object","properties":{"function_name":{"type":"string","description":"Function to trace"},"max_depth":{"type":"integer","description":"Max traversal depth (default 3)"},"max_results":{"type":"integer","description":"Max results per direction (default 20)"}},"required":["function_name"]}),
        ),
        Tool::new(
            "refactor_extract_function",
            "Extract a line range into a new function. Replaces the original lines with a function call. Use dry_run=true to preview.",
            json!({"type":"object","properties":{"file_path":{"type":"string"},"start_line":{"type":"integer"},"end_line":{"type":"integer"},"new_name":{"type":"string","description":"Name for the new function"},"dry_run":{"type":"boolean","description":"Preview without modifying (default true)"}},"required":["file_path","start_line","end_line","new_name"]}),
        ),
        // ── Serena-compatible: no-op thinking/mode tools ────────────────
        Tool::new("think_about_collected_information", "Thinking tool: review and reflect on collected information.", json!({"type":"object","properties":{}})),
        Tool::new("think_about_task_adherence", "Thinking tool: verify that planned actions adhere to the original task.", json!({"type":"object","properties":{}})),
        Tool::new("think_about_whether_you_are_done", "Thinking tool: assess whether the current task is truly complete.", json!({"type":"object","properties":{}})),
        Tool::new("switch_modes", "Switches the server operating mode (no-op in standalone mode).", json!({"type":"object","properties":{"mode":{"type":"string","description":"Target mode"}}})),
        // ── Serena-compatible: memory tools ──────────────────────────────
        Tool::new("list_memories", "Lists all memory files stored under .serena/memories.", json!({"type":"object","properties":{"topic":{"type":"string","description":"Optional topic to filter"}}})),
        Tool::new("read_memory", "Reads the content of a named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"}},"required":["memory_name"]})),
        Tool::new("write_memory", "Writes (creates or overwrites) a named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}},"required":["memory_name","content"]})),
        Tool::new("delete_memory", "Deletes a named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"}},"required":["memory_name"]})),
        Tool::new("edit_memory", "Replaces the content of an existing named memory file.", json!({"type":"object","properties":{"memory_name":{"type":"string"},"content":{"type":"string"}},"required":["memory_name","content"]})),
        Tool::new("rename_memory", "Renames a memory file.", json!({"type":"object","properties":{"old_name":{"type":"string"},"new_name":{"type":"string"}},"required":["old_name","new_name"]})),
        // ── Serena-compatible: session/config tools ──────────────────────
        Tool::new("activate_project", "Activates and validates the current project.", json!({"type":"object","properties":{"project":{"type":"string","description":"Optional project name or path"}}})),
        Tool::new("check_onboarding_performed", "Checks whether Serena onboarding memories are present.", json!({"type":"object","properties":{}})),
        Tool::new("initial_instructions", "Returns initial instructions for starting work.", json!({"type":"object","properties":{}})),
        Tool::new("onboarding", "Creates default .serena/memories onboarding files.", json!({"type":"object","properties":{"force":{"type":"boolean","description":"Re-create even if exists"}}})),
        Tool::new("prepare_for_new_conversation", "Returns project context for a new conversation.", json!({"type":"object","properties":{}})),
        Tool::new("summarize_changes", "Provides instructions for summarising recent changes.", json!({"type":"object","properties":{}})),
        Tool::new("list_queryable_projects", "Lists projects queryable by this server.", json!({"type":"object","properties":{}})),
    ]
}

// ── Serena memory helpers ────────────────────────────────────────────────

fn list_memory_names(memories_dir: &std::path::Path, topic: Option<&str>) -> Vec<String> {
    if !memories_dir.is_dir() {
        return Vec::new();
    }
    let mut names = Vec::new();
    collect_memory_files(memories_dir, memories_dir, &mut names);
    names.sort();
    if let Some(t) = topic {
        let t = t.trim().trim_matches('/');
        if !t.is_empty() {
            names.retain(|n| n == t || n.starts_with(&format!("{t}/")));
        }
    }
    names
}

fn collect_memory_files(base: &std::path::Path, dir: &std::path::Path, names: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_memory_files(base, &path, names);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            if let Ok(rel) = path.strip_prefix(base) {
                let name = rel
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_end_matches(".md")
                    .to_string();
                names.push(name);
            }
        }
    }
}

fn resolve_memory_path(
    memories_dir: &std::path::Path,
    name: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let normalized = name
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .trim_end_matches(".md")
        .to_string();
    if normalized.is_empty() {
        anyhow::bail!("Memory name must not be empty");
    }
    if normalized.contains("..") {
        anyhow::bail!("Memory path must not contain '..': {name}");
    }
    Ok(memories_dir.join(format!("{normalized}.md")))
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
            "arguments": [{
                "name": "file_path",
                "description": "File to review",
                "required": true
            }]
        }),
        json!({
            "name": "onboard-project",
            "description": "Get a comprehensive overview of the project for onboarding",
            "arguments": []
        }),
        json!({
            "name": "analyze-impact",
            "description": "Analyze the impact of modifying a specific file",
            "arguments": [{
                "name": "file_path",
                "description": "File to analyze",
                "required": true
            }]
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

fn default_lsp_command_for_path(file_path: &str) -> Option<String> {
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
        _ => None,
    }
}

fn default_lsp_args_for_command(command: &str) -> Vec<String> {
    match command {
        "pyright-langserver" => vec!["--stdio".to_owned()],
        "typescript-language-server" => vec!["--stdio".to_owned()],
        _ => Vec::new(),
    }
}

fn required_string<'a>(value: &'a serde_json::Value, key: &str) -> anyhow::Result<&'a str> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing {key}"))
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
        assert_eq!(tools().len(), 65);
        let encoded = serde_json::to_string(&response).expect("serialize");
        assert!(encoded.contains("read_file"));
    }

    #[test]
    fn reads_file_via_tool_call() {
        let project = project_root();
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"read_file",
                    "arguments":{"relative_path":"hello.txt"}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("filesystem"));
        assert_eq!(tool_payload["confidence"], json!(1.0));
        assert!(text.contains("hello from rust"));
    }

    #[test]
    fn returns_symbols_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "class Service:\n    def run(self):\n        return True\n",
        )
        .expect("write python");
        let state = super::AppState::new(project, super::ToolPreset::Full);

        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_symbols_overview",
                    "arguments":{"path":"main.py","depth":2}
                })),
            },
        );

        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("tree-sitter-cached"));
        assert_eq!(tool_payload["confidence"], json!(0.93));
        assert!(text.contains("Service"));
        assert!(text.contains("run"));
    }

    #[test]
    fn reports_symbol_index_stats() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "class Service:\n    def run(self):\n        return True\n",
        )
        .expect("write python");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"refresh_symbol_index",
                    "arguments":{}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("tree-sitter-cached"));
        assert_eq!(tool_payload["confidence"], json!(0.95));
        assert!(
            tool_payload["data"]["indexed_files"]
                .as_u64()
                .expect("indexed_files")
                >= 1
        );
    }

    #[test]
    fn returns_ranked_context_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "class Service:\n    def run(self):\n        return True\n\ndef greet():\n    return 1\n",
        )
        .expect("write python");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_ranked_context",
                    "arguments":{
                        "query":"greet",
                        "max_tokens":40,
                        "include_body":true
                    }
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("tree-sitter-cached"));
        assert_eq!(tool_payload["confidence"], json!(0.91));
        assert!(text.contains("\"query\":\"greet\""));
        assert!(text.contains("\"token_budget\":40"));
        assert!(text.contains("\"relevance_score\":100"));
    }

    #[test]
    fn returns_blast_radius_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        std::fs::write(
            project.as_path().join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        std::fs::write(
            project.as_path().join("models.py"),
            "class User:\n    pass\n",
        )
        .expect("write models");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_blast_radius",
                    "arguments":{"file_path":"models.py","max_depth":3}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("import-graph"));
        assert_eq!(tool_payload["confidence"], json!(0.86));
        assert!(text.contains("\"file\":\"utils.py\""));
        assert!(text.contains("\"depth\":2"));
    }

    #[test]
    fn returns_importers_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        std::fs::write(
            project.as_path().join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        std::fs::write(
            project.as_path().join("utils.py"),
            "def greet():\n    return 1\n",
        )
        .expect("write utils");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"find_importers",
                    "arguments":{"file_path":"utils.py","max_results":10}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("import-graph"));
        assert_eq!(tool_payload["confidence"], json!(0.87));
        assert!(text.contains("\"file\":\"main.py\""));
        assert!(text.contains("\"file\":\"worker.py\""));
    }

    #[test]
    fn returns_symbol_importance_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        std::fs::write(
            project.as_path().join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        std::fs::write(
            project.as_path().join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        std::fs::write(
            project.as_path().join("models.py"),
            "class User:\n    pass\n",
        )
        .expect("write models");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_symbol_importance",
                    "arguments":{"top_n":10}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("import-graph"));
        assert_eq!(tool_payload["confidence"], json!(0.84));
        assert!(text.contains("\"ranking\""));
        assert!(text.contains("\"file\":\"models.py\""));
    }

    #[test]
    fn returns_dead_code_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        std::fs::write(
            project.as_path().join("utils.py"),
            "def greet():\n    return 1\n",
        )
        .expect("write utils");
        std::fs::write(
            project.as_path().join("unused.py"),
            "def helper():\n    return 2\n",
        )
        .expect("write unused");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"find_dead_code",
                    "arguments":{"max_results":10}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("import-graph"));
        assert_eq!(tool_payload["confidence"], json!(0.83));
        assert!(text.contains("\"dead_code\""));
        assert!(text.contains("\"file\":\"unused.py\""));
    }

    #[test]
    fn returns_annotations_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("main.py"),
            "# TODO: wire this up\n\ndef main():\n    return 1\n",
        )
        .expect("write main");
        std::fs::write(
            project.as_path().join("worker.py"),
            "# FIXME handle retries\n\ndef run():\n    return 2\n",
        )
        .expect("write worker");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"find_annotations",
                    "arguments":{"tags":"TODO,FIXME","max_results":10}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("filesystem"));
        assert_eq!(tool_payload["confidence"], json!(0.97));
        assert!(text.contains("\"tags\""));
        assert!(text.contains("\"TODO\""));
        assert!(text.contains("\"FIXME\""));
        assert!(text.contains("\"file\":\"main.py\""));
        assert!(text.contains("\"file\":\"worker.py\""));
    }

    #[test]
    fn returns_tests_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("test_main.py"),
            "def test_greet():\n    assert True\n",
        )
        .expect("write test file");
        std::fs::write(
            project.as_path().join("spec.js"),
            "describe('suite', () => { test('works', () => {}) })\n",
        )
        .expect("write js test file");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"find_tests",
                    "arguments":{"max_results":10}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("filesystem"));
        assert_eq!(tool_payload["confidence"], json!(0.97));
        assert!(text.contains("\"tests\""));
        assert!(text.contains("\"file_path\":\"test_main.py\""));
        assert!(text.contains("\"file_path\":\"spec.js\""));
    }

    #[test]
    fn returns_complexity_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("sample.py"),
            "def greet(flag):\n    if flag:\n        return 1\n    return 0\n",
        )
        .expect("write sample");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_complexity",
                    "arguments":{"path":"sample.py"}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("tree-sitter-cached"));
        assert_eq!(tool_payload["confidence"], json!(0.89));
        assert!(text.contains("\"functions\""));
        assert!(text.contains("\"name\":\"greet\""));
        assert!(text.contains("\"complexity\":2"));
    }

    #[test]
    fn returns_changed_files_via_tool_call() {
        let project = project_root();
        run_git(&project, &["init"]);
        run_git(&project, &["config", "user.email", "codex@example.com"]);
        run_git(&project, &["config", "user.name", "Codex"]);
        std::fs::write(
            project.as_path().join("tracked.py"),
            "def greet():\n    return 1\n",
        )
        .expect("write tracked");
        run_git(&project, &["add", "tracked.py"]);
        run_git(&project, &["commit", "-m", "init"]);
        std::fs::write(
            project.as_path().join("tracked.py"),
            "def greet():\n    return 2\n",
        )
        .expect("modify tracked");
        std::fs::write(
            project.as_path().join("new_file.py"),
            "def helper():\n    return 3\n",
        )
        .expect("write untracked");

        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_changed_files",
                    "arguments":{"include_untracked":true}
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("git"));
        assert_eq!(tool_payload["confidence"], json!(0.95));
        assert!(text.contains("\"file\":\"tracked.py\""));
        assert!(text.contains("\"status\":\"M\""));
        assert!(text.contains("\"file\":\"new_file.py\""));
        assert!(text.contains("\"status\":\"?\""));
    }

    #[test]
    fn returns_lsp_references_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("sample.py"),
            "def greet():\n    return 1\n",
        )
        .expect("write python");
        let script = project.as_path().join("mock_lsp.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys
def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))
def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()
while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"referencesProvider":True}}})
    elif method == "textDocument/references":
        uri = message["params"]["textDocument"]["uri"]
        send({"jsonrpc":"2.0","id":message["id"],"result":[{"uri":uri,"range":{"start":{"line":0,"character":4},"end":{"line":0,"character":9}}}]})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("write mock lsp");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"find_referencing_symbols",
                    "arguments":{
                        "file_path":"sample.py",
                        "line":1,
                        "column":5,
                        "command":"python3",
                        "args":[script.display().to_string()],
                        "max_results":10
                    }
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("lsp_pooled"));
        assert_eq!(tool_payload["confidence"], json!(0.9));
        assert!(text.contains("\"file_path\":\"sample.py\""));
    }

    #[test]
    fn returns_lsp_diagnostics_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("sample.py"),
            "def greet(:\n    return 1\n",
        )
        .expect("write python");
        let script = project.as_path().join("mock_lsp.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys
def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))
def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()
while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"diagnosticProvider":True}}})
    elif method == "textDocument/diagnostic":
        uri = message["params"]["textDocument"]["uri"]
        send({"jsonrpc":"2.0","id":message["id"],"result":{"kind":"full","uri":uri,"items":[{"range":{"start":{"line":0,"character":10},"end":{"line":0,"character":11}},"severity":1,"code":"E999","source":"mock-lsp","message":"syntax error"}]}})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("write mock lsp");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_file_diagnostics",
                    "arguments":{
                        "file_path":"sample.py",
                        "command":"python3",
                        "args":[script.display().to_string()],
                        "max_results":10
                    }
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("lsp_pooled"));
        assert_eq!(tool_payload["confidence"], json!(0.9));
        assert!(text.contains("\"severity_label\":\"error\""));
        assert!(text.contains("\"message\":\"syntax error\""));
    }

    #[test]
    fn returns_workspace_symbols_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("sample.py"),
            "class Service:\n    pass\n",
        )
        .expect("write python");
        let script = project.as_path().join("mock_lsp.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys
from pathlib import Path
symbol_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd() / "sample.py"
def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))
def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()
while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"workspaceSymbolProvider":True}}})
    elif method == "workspace/symbol":
        query = message["params"]["query"]
        send({"jsonrpc":"2.0","id":message["id"],"result":[{"name":query,"kind":5,"containerName":"sample","location":{"uri":"file://" + str(symbol_path.resolve()),"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":13}}}}]})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("write mock lsp");
        let state = super::AppState::new(project.clone(), super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"search_workspace_symbols",
                    "arguments":{
                        "query":"Service",
                        "command":"python3",
                        "args":[script.display().to_string(), project.as_path().join("sample.py").display().to_string()],
                        "max_results":10
                    }
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("lsp_pooled"));
        assert_eq!(tool_payload["confidence"], json!(0.88));
        assert!(text.contains("\"name\":\"Service\""));
        assert!(text.contains("\"kind_label\":\"class\""));
    }

    #[test]
    fn returns_type_hierarchy_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("sample.py"),
            "class Service:\n    pass\n",
        )
        .expect("write python");
        let script = project.as_path().join("mock_lsp.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys
from pathlib import Path
symbol_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd() / "sample.py"
def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))
def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()
while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"workspaceSymbolProvider":True,"typeHierarchyProvider":True}}})
    elif method == "workspace/symbol":
        query = message["params"]["query"]
        send({"jsonrpc":"2.0","id":message["id"],"result":[{"name":query,"kind":5,"containerName":"sample","location":{"uri":"file://" + str(symbol_path.resolve()),"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":13}}}}]})
    elif method == "textDocument/prepareTypeHierarchy":
        uri = message["params"]["textDocument"]["uri"]
        send({"jsonrpc":"2.0","id":message["id"],"result":[{"name":"Service","kind":5,"detail":"sample.Service","uri":uri,"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":13}},"selectionRange":{"start":{"line":0,"character":6},"end":{"line":0,"character":13}},"data":{"name":"Service"}}]})
    elif method == "typeHierarchy/supertypes":
        item = message["params"]["item"]
        send({"jsonrpc":"2.0","id":message["id"],"result":[{"name":"BaseService","kind":5,"detail":"sample.BaseService","uri":item["uri"],"range":item["range"],"selectionRange":item["selectionRange"],"data":{"name":"BaseService"}}]})
    elif method == "typeHierarchy/subtypes":
        item = message["params"]["item"]
        send({"jsonrpc":"2.0","id":message["id"],"result":[{"name":"ServiceImpl","kind":5,"detail":"sample.ServiceImpl","uri":item["uri"],"range":item["range"],"selectionRange":item["selectionRange"],"data":{"name":"ServiceImpl"}}]})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("write mock lsp");
        let state = super::AppState::new(project.clone(), super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"get_type_hierarchy",
                    "arguments":{
                        "name_path":"Service",
                        "relative_path":"sample.py",
                        "hierarchy_type":"both",
                        "depth":1,
                        "command":"python3",
                        "args":[script.display().to_string(), project.as_path().join("sample.py").display().to_string()]
                    }
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("lsp_pooled"));
        assert_eq!(tool_payload["confidence"], json!(0.82));
        assert!(text.contains("\"class_name\":\"Service\""));
        assert!(text.contains("\"qualified_name\":\"sample.BaseService\""));
        assert!(text.contains("\"qualified_name\":\"sample.ServiceImpl\""));
    }

    #[test]
    fn returns_rename_plan_via_tool_call() {
        let project = project_root();
        std::fs::write(
            project.as_path().join("sample.py"),
            "class Service:\n    pass\n",
        )
        .expect("write python");
        let script = project.as_path().join("mock_lsp.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import json
import sys
def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))
def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()
while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"renameProvider":{"prepareProvider":True}}}})
    elif method == "textDocument/prepareRename":
        send({"jsonrpc":"2.0","id":message["id"],"result":{"range":{"start":{"line":0,"character":6},"end":{"line":0,"character":13}},"placeholder":"Service"}})
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#,
        )
        .expect("write mock lsp");
        let state = super::AppState::new(project, super::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/call".to_owned(),
                params: Some(json!({
                    "name":"plan_symbol_rename",
                    "arguments":{
                        "file_path":"sample.py",
                        "line":1,
                        "column":8,
                        "new_name":"RenamedService",
                        "command":"python3",
                        "args":[script.display().to_string()]
                    }
                })),
            },
        );
        let encoded = serde_json::to_value(&response).expect("serialize");
        let text = encoded["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text");
        let tool_payload = parse_tool_payload(text);
        assert_eq!(tool_payload["success"], json!(true));
        assert_eq!(tool_payload["backend_used"], json!("lsp_pooled"));
        assert_eq!(tool_payload["confidence"], json!(0.86));
        assert!(text.contains("\"current_name\":\"Service\""));
        assert!(text.contains("\"new_name\":\"RenamedService\""));
    }

    fn parse_tool_payload(text: &str) -> serde_json::Value {
        serde_json::from_str(text).expect("inner payload")
    }

    fn project_root() -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rust-mcp-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        fs::write(dir.join("hello.txt"), "hello from rust\n").expect("write fixture");
        ProjectRoot::new(dir).expect("project")
    }

    fn run_git(project: &ProjectRoot, args: &[&str]) {
        use std::process::Command;
        let status = Command::new("git")
            .args(args)
            .current_dir(project.as_path())
            .status()
            .expect("run git");
        assert!(status.success(), "git {:?} failed", args);
    }
}
