mod protocol;

use anyhow::Result;
use codelens_core::{
    create_text_file, delete_lines, find_circular_dependencies, find_dead_code, find_files,
    get_blast_radius, get_callees, get_callers, get_change_coupling, get_changed_files,
    get_diff_symbols, get_importance, get_importers, insert_after_symbol, insert_at_line,
    insert_before_symbol, list_dir, read_file, replace_content, replace_lines, replace_symbol_body,
    search_for_pattern, search_for_pattern_smart, LspDiagnosticRequest, LspRenamePlanRequest,
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
        Self {
            project,
            symbol_index: Mutex::new(symbol_index),
            lsp_pool: Mutex::new(lsp_pool),
            preset,
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
    let project = ProjectRoot::new(project_arg)?;
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
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "codelens-rust",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
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
            if smart {
                search_for_pattern_smart(&state.project, pattern, file_glob, max_results).map(
                    |value| {
                        (
                            json!({ "matches": value, "count": value.len() }),
                            success_meta("tree-sitter+filesystem", 0.96),
                        )
                    },
                )
            } else {
                search_for_pattern(&state.project, pattern, file_glob, max_results).map(|value| {
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
            search_for_pattern(&state.project, &pattern, None, max_results).map(|value| {
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
            search_for_pattern(&state.project, pattern, None, max_results).map(|value| {
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
            let name = required_string(&arguments, "name")?;
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
            let line = arguments
                .get("line")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing line"))? as usize;
            let column = arguments
                .get("column")
                .and_then(|value| value.as_u64())
                .ok_or_else(|| anyhow::anyhow!("Missing column"))?
                as usize;
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
                .unwrap_or(50) as usize;

            state
                .lsp_pool
                .lock()
                .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                .find_referencing_symbols(&LspRequest {
                    command,
                    args,
                    file_path,
                    line,
                    column,
                    max_results,
                })
                .map(|value| {
                    (
                        json!({ "references": value, "count": value.len() }),
                        success_meta("lsp_pooled", 0.9),
                    )
                })
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

            state
                .lsp_pool
                .lock()
                .map_err(|_| anyhow::anyhow!("lsp pool lock poisoned"))?
                .get_type_hierarchy(&LspTypeHierarchyRequest {
                    command,
                    args,
                    query,
                    relative_path,
                    hierarchy_type,
                    depth: if depth == 0 { 8 } else { depth },
                })
                .map(|value| (json!(value), success_meta("lsp_pooled", 0.82)))
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
            search_for_pattern(&state.project, symbol_name, file_glob, max_results).map(|matches| {
                let snippets = matches
                    .iter()
                    .map(|m| {
                        json!({
                            "file_path": m.file_path,
                            "line": m.line,
                            "column": m.column,
                            "matched_text": m.matched_text,
                            "line_content": m.line_content,
                            "context_lines": context_lines
                        })
                    })
                    .collect::<Vec<_>>();
                (
                    json!({ "snippets": snippets, "count": snippets.len() }),
                    success_meta("filesystem", 0.92),
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
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    })();

    match result {
        Ok((payload, meta)) => {
            let hints = navigation_hints(name);
            let response = if hints.is_empty() {
                ToolCallResponse::success(payload, meta)
            } else {
                ToolCallResponse::success_with_hints(payload, meta, hints)
            };
            JsonRpcResponse::result(
                id,
                json!({
                    "content": [
                        {
                            "type": "text",
                            "text": serde_json::to_string(&response)
                                .unwrap_or_else(|_| "{\"success\":false,\"error\":\"serialization failed\"}".to_owned())
                        }
                    ]
                }),
            )
        }
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

fn navigation_hints(tool_name: &str) -> Vec<String> {
    match tool_name {
        "find_symbol" => vec![
            "get_callers: find functions that call this symbol".into(),
            "get_callees: find functions called by this symbol".into(),
            "find_referencing_symbols: trace all references".into(),
            "get_blast_radius: estimate change impact".into(),
            "get_complexity: check cyclomatic complexity".into(),
        ],
        "get_symbols_overview" => vec![
            "find_symbol: search for a specific symbol with body".into(),
            "get_ranked_context: get token-budget-aware context".into(),
        ],
        "get_blast_radius" => vec![
            "find_circular_dependencies: check for circular imports".into(),
            "get_change_coupling: find historically co-changed files".into(),
            "find_dead_code: detect unreferenced files".into(),
        ],
        "find_importers" => vec![
            "get_blast_radius: full transitive impact analysis".into(),
            "get_symbol_importance: PageRank file ranking".into(),
        ],
        "get_callers" => vec![
            "get_callees: reverse direction — what does this function call?".into(),
            "find_referencing_symbols: LSP-backed reference tracing".into(),
        ],
        "get_callees" => vec![
            "get_callers: reverse direction — who calls this function?".into(),
            "get_complexity: check callee complexity".into(),
        ],
        "get_changed_files" => vec![
            "get_diff_symbols: see symbols in changed files".into(),
            "get_change_coupling: find historically co-changed files".into(),
        ],
        "get_change_coupling" => vec![
            "find_circular_dependencies: check circular imports in coupled files".into(),
            "get_blast_radius: estimate change impact for coupled files".into(),
        ],
        "find_circular_dependencies" => vec![
            "get_blast_radius: estimate impact of breaking the cycle".into(),
            "find_importers: trace specific import chains".into(),
        ],
        "search_for_pattern" => vec![
            "find_symbol: structured symbol search (faster for known names)".into(),
            "find_referencing_code_snippets: search with context lines".into(),
        ],
        "get_complexity" => vec![
            "find_symbol: read the function body".into(),
            "get_callers: see who calls this complex function".into(),
        ],
        _ => vec![],
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
                    "smart":{"type":"boolean","description":"Include enclosing symbol context for each match"}
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
            "Find a symbol by name with optional body retrieval.",
            json!({
                "type":"object",
                "properties":{
                    "name":{"type":"string"},
                    "file_path":{"type":"string"},
                    "include_body":{"type":"boolean"},
                    "exact_match":{"type":"boolean"},
                    "max_matches":{"type":"integer"}
                },
                "required":["name"]
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
            "Find references through a stdio LSP server. Requires file position; command/args may be provided explicitly.",
            json!({
                "type":"object",
                "properties":{
                    "file_path":{"type":"string"},
                    "line":{"type":"integer"},
                    "column":{"type":"integer"},
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "max_results":{"type":"integer"}
                },
                "required":["file_path","line","column"]
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
    ]
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
        assert_eq!(tools().len(), 36);
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
