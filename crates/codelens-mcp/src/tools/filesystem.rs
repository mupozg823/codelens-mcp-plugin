use super::{
    optional_bool, optional_string, optional_usize, required_string, success_meta, AppState,
    ToolResult,
};
use crate::client_profile::ClientProfile;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    detect_frameworks, detect_workspace_packages, find_files, list_dir, read_file,
    search_for_pattern, search_for_pattern_smart,
};
use serde_json::json;

pub fn get_current_config(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let stats = state.symbol_index().stats()?;
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let surface = state.execution_surface(&session);
    let token_budget = state.execution_token_budget(&session);
    let client_profile = session
        .client_name
        .as_deref()
        .map(|name| ClientProfile::detect(Some(name)))
        .unwrap_or_else(|| state.client_profile());
    let frameworks = detect_frameworks(state.project().as_path());
    let workspace_packages = detect_workspace_packages(state.project().as_path());
    Ok((
        json!({
            "runtime": "rust-core",
            "project_root": state.project().as_path().display().to_string(),
            "editor_integration": false,
            "available_backends": ["filesystem", "tree-sitter-cached", "lsp_pooled"],
            "symbol_index": stats,
            "surface": surface.as_label(),
            "token_budget": token_budget,
            "tool_count": crate::tool_defs::visible_tools(surface).len(),
            "client_profile": client_profile.as_str(),
            "frameworks": frameworks,
            "workspace_packages": workspace_packages
        }),
        success_meta(BackendKind::Config, 1.0),
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
    Ok(read_file(&state.project(), path, start_line, end_line)
        .map(|value| (json!(value), success_meta(BackendKind::Filesystem, 1.0)))?)
}

pub fn list_dir_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "relative_path")?;
    let recursive = optional_bool(arguments, "recursive", false);
    Ok(list_dir(&state.project(), path, recursive).map(|value| {
        (
            json!({ "entries": value, "count": value.len() }),
            success_meta(BackendKind::Filesystem, 1.0),
        )
    })?)
}

pub fn find_file_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let pattern = required_string(arguments, "wildcard_pattern")?;
    let dir = optional_string(arguments, "relative_dir");
    Ok(find_files(&state.project(), pattern, dir).map(|value| {
        (
            json!({ "files": value, "count": value.len() }),
            success_meta(BackendKind::Filesystem, 1.0),
        )
    })?)
}

pub fn search_for_pattern_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let pattern = arguments
        .get("pattern")
        .or_else(|| arguments.get("substring_pattern"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("pattern".into()))?;
    let file_glob = optional_string(arguments, "file_glob");
    let max_results = optional_usize(arguments, "max_results", 50);
    let smart = optional_bool(arguments, "smart", false);
    let ctx_fallback = optional_usize(arguments, "context_lines", 0);
    let ctx_before = optional_usize(arguments, "context_lines_before", ctx_fallback);
    let ctx_after = optional_usize(arguments, "context_lines_after", ctx_fallback);

    if smart {
        Ok(search_for_pattern_smart(
            &state.project(),
            pattern,
            file_glob,
            max_results,
            ctx_before,
            ctx_after,
        )
        .map(|value| {
            (
                json!({ "matches": value, "count": value.len() }),
                success_meta(BackendKind::TreeSitter, 0.96),
            )
        })?)
    } else {
        Ok(search_for_pattern(
            &state.project(),
            pattern,
            file_glob,
            max_results,
            ctx_before,
            ctx_after,
        )
        .map(|value| {
            (
                json!({ "matches": value, "count": value.len() }),
                success_meta(BackendKind::Filesystem, 0.98),
            )
        })?)
    }
}

pub fn find_annotations(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let tags = optional_string(arguments, "tags").unwrap_or("TODO,FIXME,HACK,DEPRECATED,XXX,NOTE");
    let max_results = optional_usize(arguments, "max_results", 100);
    let tag_list = tags
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>();
    let pattern = format!(r"\b({})\b[:\s]*(.*)", tag_list.join("|"));
    Ok(
        search_for_pattern(&state.project(), &pattern, None, max_results, 0, 0).map(|value| {
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
                success_meta(BackendKind::Filesystem, 0.97),
            )
        })?,
    )
}

pub fn find_tests(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let max_results = optional_usize(arguments, "max_results", 100);
    let pattern = r"\b(def test_|func Test|@Test\b|it\s*\(|describe\s*\(|test\s*\()";
    Ok(
        search_for_pattern(&state.project(), pattern, None, max_results, 0, 0).map(|value| {
            (
                json!({ "tests": value, "count": value.len() }),
                success_meta(BackendKind::Filesystem, 0.97),
            )
        })?,
    )
}
