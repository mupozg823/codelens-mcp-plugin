use super::{required_string, success_meta, AppState, ToolResult};
use codelens_core::{
    detect_frameworks, detect_workspace_packages, find_files, list_dir, read_file,
    search_for_pattern, search_for_pattern_smart,
};
use serde_json::json;

pub fn get_current_config(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let stats = state
        .symbol_index
        .lock()
        .map_err(|_| anyhow::anyhow!("symbol index lock poisoned"))?
        .stats()?;
    let preset = *state
        .preset
        .lock()
        .map_err(|_| anyhow::anyhow!("preset lock poisoned"))?;
    let frameworks = detect_frameworks(state.project.as_path());
    let workspace_packages = detect_workspace_packages(state.project.as_path());
    Ok((
        json!({
            "runtime": "rust-core",
            "project_root": state.project.as_path().display().to_string(),
            "editor_integration": false,
            "available_backends": ["filesystem", "tree-sitter-cached", "lsp_pooled"],
            "symbol_index": stats,
            "preset": format!("{preset:?}"),
            "tool_count": crate::tools().len(),
            "frameworks": frameworks,
            "workspace_packages": workspace_packages
        }),
        success_meta("rust-core", 1.0),
    ))
}

pub fn read_file_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "relative_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    read_file(&state.project, path, start_line, end_line)
        .map(|value| (json!(value), success_meta("filesystem", 1.0)))
}

pub fn list_dir_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "relative_path")?;
    let recursive = arguments
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    list_dir(&state.project, path, recursive).map(|value| {
        (
            json!({ "entries": value, "count": value.len() }),
            success_meta("filesystem", 1.0),
        )
    })
}

pub fn find_file_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let pattern = required_string(arguments, "wildcard_pattern")?;
    let dir = arguments.get("relative_dir").and_then(|v| v.as_str());
    find_files(&state.project, pattern, dir).map(|value| {
        (
            json!({ "files": value, "count": value.len() }),
            success_meta("filesystem", 1.0),
        )
    })
}

pub fn search_for_pattern_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let pattern = arguments
        .get("pattern")
        .or_else(|| arguments.get("substring_pattern"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing pattern"))?;
    let file_glob = arguments.get("file_glob").and_then(|v| v.as_str());
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    let smart = arguments
        .get("smart")
        .and_then(|v| v.as_bool())
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

pub fn find_annotations(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let tags = arguments
        .get("tags")
        .and_then(|v| v.as_str())
        .unwrap_or("TODO,FIXME,HACK,DEPRECATED,XXX,NOTE");
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
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
            json!({ "tags": grouped, "total": value.len() }),
            success_meta("filesystem", 0.97),
        )
    })
}

pub fn find_tests(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(100) as usize;
    let pattern = r"\b(def test_|func Test|@Test\b|it\s*\(|describe\s*\(|test\s*\()";
    search_for_pattern(&state.project, pattern, None, max_results, 0, 0).map(|value| {
        (
            json!({ "tests": value, "count": value.len() }),
            success_meta("filesystem", 0.97),
        )
    })
}
