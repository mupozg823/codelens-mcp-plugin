use crate::AppState;
use crate::error::CodeLensError;
use crate::tool_defs::deprecated_workflow_alias;
use crate::tool_runtime::{ToolHandler, ToolResult};
use serde_json::{Value, json};

#[cfg(feature = "semantic")]
use crate::protocol::BackendKind;
#[cfg(feature = "semantic")]
use crate::tool_runtime::success_meta;
#[cfg(feature = "semantic")]
use codelens_engine::{ProjectRoot, embedding::DuplicatePair};

#[cfg(feature = "semantic")]
struct DuplicateFilterOutcome {
    pairs: Vec<DuplicatePair>,
    suppressed_config_code_pairs: usize,
}

fn attach_workflow_metadata(workflow: &str, delegated_tool: &str, payload: Value) -> Value {
    let deprecation = deprecated_workflow_alias(workflow);
    match payload {
        Value::Object(mut map) => {
            map.insert("workflow".to_owned(), json!(workflow));
            map.insert("delegated_tool".to_owned(), json!(delegated_tool));
            if let Some((replacement_tool, removal_target)) = deprecation {
                map.insert("deprecated".to_owned(), json!(true));
                map.insert("replacement_tool".to_owned(), json!(replacement_tool));
                map.insert("removal_target".to_owned(), json!(removal_target));
            }
            Value::Object(map)
        }
        other => {
            let mut map = serde_json::Map::new();
            map.insert("workflow".to_owned(), json!(workflow));
            map.insert("delegated_tool".to_owned(), json!(delegated_tool));
            if let Some((replacement_tool, removal_target)) = deprecation {
                map.insert("deprecated".to_owned(), json!(true));
                map.insert("replacement_tool".to_owned(), json!(replacement_tool));
                map.insert("removal_target".to_owned(), json!(removal_target));
            }
            map.insert("result".to_owned(), other);
            Value::Object(map)
        }
    }
}

#[cfg(feature = "semantic")]
fn normalize_duplicate_scope(project: &ProjectRoot, scope: Option<&str>) -> Option<String> {
    let raw = scope?.trim();
    if raw.is_empty() || raw == "." {
        return None;
    }
    let resolved = project.resolve(raw).ok()?;
    let relative = project.to_relative(resolved);
    if relative.is_empty() || relative == "." {
        None
    } else {
        Some(relative.trim_end_matches('/').to_owned())
    }
}

#[cfg(feature = "semantic")]
fn file_in_duplicate_scope(scope: &str, file: &str) -> bool {
    let file = file.trim_start_matches("./");
    file == scope || file.starts_with(&format!("{scope}/"))
}

#[cfg(feature = "semantic")]
fn duplicate_pair_in_scope(scope: &str, pair: &DuplicatePair) -> bool {
    file_in_duplicate_scope(scope, &pair.file_a) || file_in_duplicate_scope(scope, &pair.file_b)
}

#[cfg(feature = "semantic")]
fn symbol_name_for_duplicate_side<'a>(rendered_symbol: &'a str, file: &str) -> &'a str {
    rendered_symbol
        .strip_prefix(&format!("{file}:"))
        .unwrap_or(rendered_symbol)
}

#[cfg(feature = "semantic")]
fn is_config_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".toml")
        || lower.ends_with(".json")
        || lower.ends_with(".jsonc")
}

#[cfg(feature = "semantic")]
fn is_code_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    [
        ".rs", ".py", ".js", ".jsx", ".ts", ".tsx", ".go", ".java", ".kt", ".kts", ".swift", ".rb",
        ".php", ".c", ".h", ".cpp", ".hpp", ".cs", ".scala", ".dart", ".lua", ".ex", ".exs",
        ".erl", ".hrl", ".zig",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

#[cfg(feature = "semantic")]
fn is_structural_config_symbol(symbol: &str) -> bool {
    let normalized = symbol
        .trim()
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`')
        .to_ascii_lowercase()
        .replace('-', "_");
    matches!(
        normalized.as_str(),
        "name"
            | "on"
            | "env"
            | "jobs"
            | "job"
            | "steps"
            | "step"
            | "uses"
            | "with"
            | "run"
            | "needs"
            | "permissions"
            | "strategy"
            | "matrix"
            | "workflow_dispatch"
            | "push"
            | "pull_request"
            | "schedule"
            | "branches"
            | "paths"
            | "timeout_minutes"
            | "runs_on"
    )
}

#[cfg(feature = "semantic")]
fn is_config_code_duplicate_noise(pair: &DuplicatePair) -> bool {
    let left_config = is_config_file(&pair.file_a);
    let right_config = is_config_file(&pair.file_b);
    if left_config == right_config {
        return false;
    }

    let left_code = is_code_file(&pair.file_a);
    let right_code = is_code_file(&pair.file_b);
    if !(left_code || right_code) {
        return false;
    }

    if left_config {
        is_structural_config_symbol(symbol_name_for_duplicate_side(&pair.symbol_a, &pair.file_a))
    } else {
        is_structural_config_symbol(symbol_name_for_duplicate_side(&pair.symbol_b, &pair.file_b))
    }
}

#[cfg(feature = "semantic")]
fn duplicate_quality_scan_limit(include_config_code_pairs: bool, max_pairs: usize) -> usize {
    if include_config_code_pairs {
        max_pairs
    } else {
        max_pairs.saturating_mul(8).clamp(max_pairs, 2048)
    }
}

#[cfg(feature = "semantic")]
fn filter_duplicate_pairs_for_cleanup(
    project: &ProjectRoot,
    scope: Option<&str>,
    pairs: Vec<DuplicatePair>,
    max_pairs: usize,
    include_config_code_pairs: bool,
) -> DuplicateFilterOutcome {
    let normalized_scope = normalize_duplicate_scope(project, scope);
    let mut suppressed_config_code_pairs = 0usize;
    let pairs = pairs
        .into_iter()
        .filter(|pair| {
            normalized_scope
                .as_deref()
                .is_none_or(|scope| duplicate_pair_in_scope(scope, pair))
        })
        .filter(|pair| {
            if include_config_code_pairs || !is_config_code_duplicate_noise(pair) {
                return true;
            }
            suppressed_config_code_pairs += 1;
            false
        })
        .take(max_pairs)
        .collect();

    DuplicateFilterOutcome {
        pairs,
        suppressed_config_code_pairs,
    }
}

fn delegate_workflow(
    state: &AppState,
    workflow: &str,
    delegated_tool: &str,
    delegated_args: Value,
    handler: ToolHandler,
) -> ToolResult {
    let (payload, meta) = handler(state, &delegated_args)?;
    Ok((
        attach_workflow_metadata(workflow, delegated_tool, payload),
        meta,
    ))
}

pub fn explore_codebase(state: &AppState, arguments: &Value) -> ToolResult {
    if let Some(query) = arguments.get("query").and_then(|value| value.as_str()) {
        return delegate_workflow(
            state,
            "explore_codebase",
            "get_ranked_context",
            json!({
                "query": query,
                "path": arguments.get("path").and_then(|value| value.as_str()),
                "max_tokens": arguments.get("max_tokens").and_then(|value| value.as_u64()),
                "include_body": arguments.get("include_body").and_then(|value| value.as_bool()),
                "depth": arguments.get("depth").and_then(|value| value.as_u64()),
                "disable_semantic": arguments.get("disable_semantic").and_then(|value| value.as_bool()),
            }),
            crate::tools::symbols::get_ranked_context,
        );
    }

    delegate_workflow(
        state,
        "explore_codebase",
        "onboard_project",
        json!({
            "project_root": arguments.get("project_root").and_then(|value| value.as_str()),
        }),
        crate::tools::composite::onboard_project,
    )
}

pub fn trace_request_path(state: &AppState, arguments: &Value) -> ToolResult {
    let function_name = arguments
        .get("function_name")
        .or_else(|| arguments.get("symbol"))
        .or_else(|| arguments.get("entrypoint"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("function_name".to_owned()))?;

    delegate_workflow(
        state,
        "trace_request_path",
        "explain_code_flow",
        json!({
            "function_name": function_name,
            "max_depth": arguments.get("max_depth").and_then(|value| value.as_u64()),
            "max_results": arguments.get("max_results").and_then(|value| value.as_u64()),
        }),
        crate::tools::composite::explain_code_flow,
    )
}

pub fn review_architecture(state: &AppState, arguments: &Value) -> ToolResult {
    let include_diagram = arguments
        .get("include_diagram")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if let Some(path) = arguments.get("path").and_then(|value| value.as_str()) {
        if include_diagram {
            return delegate_workflow(
                state,
                "review_architecture",
                "mermaid_module_graph",
                json!({
                    "path": path,
                    "max_nodes": arguments.get("max_nodes").and_then(|value| value.as_u64()),
                }),
                crate::tools::reports::mermaid_module_graph,
            );
        }

        return delegate_workflow(
            state,
            "review_architecture",
            "module_boundary_report",
            json!({ "path": path }),
            crate::tools::reports::module_boundary_report,
        );
    }

    delegate_workflow(
        state,
        "review_architecture",
        "onboard_project",
        json!({
            "project_root": arguments.get("project_root").and_then(|value| value.as_str()),
        }),
        crate::tools::composite::onboard_project,
    )
}

pub fn plan_safe_refactor(state: &AppState, arguments: &Value) -> ToolResult {
    if let (Some(file_path), Some(symbol)) = (
        arguments.get("file_path").and_then(|value| value.as_str()),
        arguments.get("symbol").and_then(|value| value.as_str()),
    ) {
        return delegate_workflow(
            state,
            "plan_safe_refactor",
            "safe_rename_report",
            json!({
                "file_path": file_path,
                "symbol": symbol,
                "new_name": arguments.get("new_name").and_then(|value| value.as_str()),
            }),
            crate::tools::reports::safe_rename_report,
        );
    }

    delegate_workflow(
        state,
        "plan_safe_refactor",
        "refactor_safety_report",
        json!({
            "task": arguments.get("task").and_then(|value| value.as_str()),
            "symbol": arguments.get("symbol").and_then(|value| value.as_str()),
            "path": arguments.get("path").and_then(|value| value.as_str()),
            "file_path": arguments.get("file_path").and_then(|value| value.as_str()),
        }),
        crate::tools::reports::refactor_safety_report,
    )
}

pub fn review_changes(state: &AppState, arguments: &Value) -> ToolResult {
    if arguments
        .get("changed_files")
        .and_then(|v| v.as_array())
        .is_some()
    {
        return delegate_workflow(
            state,
            "review_changes",
            "diff_aware_references",
            arguments.clone(),
            crate::tools::reports::diff_aware_references,
        );
    }

    delegate_workflow(
        state,
        "review_changes",
        "impact_report",
        arguments.clone(),
        crate::tools::reports::impact_report,
    )
}

pub fn diagnose_issues(state: &AppState, arguments: &Value) -> ToolResult {
    if let Some(path_str) = arguments
        .get("path")
        .or_else(|| arguments.get("file_path"))
        .and_then(|v| v.as_str())
    {
        // diagnose_issues delegates to get_file_diagnostics, which routes
        // through the LSP recipe table keyed by file extension. A directory
        // path bypasses recipe selection and surfaces as the misleading
        // "no default LSP mapping for file" error (see #207-C-2). Detect a
        // directory up front and return an actionable Validation error so
        // callers know to expand the path before retry.
        let project_relative = state.project().as_path().join(path_str);
        if project_relative.is_dir() || std::path::Path::new(path_str).is_dir() {
            return Err(crate::error::CodeLensError::Validation(format!(
                "diagnose_issues received a directory path `{path_str}`; pass a single file path instead. Directory-scope diagnostics are not yet supported — expand the directory via list_dir or get_changed_files and call diagnose_issues per file."
            )));
        }
        return delegate_workflow(
            state,
            "diagnose_issues",
            "get_file_diagnostics",
            json!({ "file_path": path_str }),
            crate::tools::lsp::get_file_diagnostics,
        );
    }

    delegate_workflow(
        state,
        "diagnose_issues",
        "unresolved_reference_check",
        json!({
            "file_path": arguments.get("file_path").and_then(|v| v.as_str()),
            "symbol": arguments.get("symbol").and_then(|v| v.as_str()),
            "changed_files": arguments.get("changed_files"),
        }),
        crate::tools::reports::unresolved_reference_check,
    )
}

pub fn cleanup_duplicate_logic(state: &AppState, arguments: &Value) -> ToolResult {
    #[cfg(feature = "semantic")]
    {
        let threshold = arguments
            .get("threshold")
            .and_then(|value| value.as_f64())
            .unwrap_or(0.85);
        let max_pairs = arguments
            .get("max_pairs")
            .and_then(|value| value.as_u64())
            .unwrap_or(20) as usize;
        let scope = arguments.get("scope").and_then(|value| value.as_str());
        let include_config_code_pairs = arguments
            .get("include_config_code_pairs")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let guard = state.embedding_engine();
        if let Some(engine) = guard.as_ref() {
            let normalized_scope = normalize_duplicate_scope(&state.project(), scope);
            let scan_limit = duplicate_quality_scan_limit(include_config_code_pairs, max_pairs);
            let pairs = engine.find_duplicates_in_scope(
                threshold,
                scan_limit,
                normalized_scope.as_deref(),
            )?;
            let filtered = filter_duplicate_pairs_for_cleanup(
                &state.project(),
                scope,
                pairs,
                max_pairs,
                include_config_code_pairs,
            );
            let count = filtered.pairs.len();
            let suppressed_config_code_pairs = filtered.suppressed_config_code_pairs;
            let payload = json!({
                "threshold": threshold,
                "scope": scope,
                "include_config_code_pairs": include_config_code_pairs,
                "quality_filters": {
                    "config_code_pairs": if include_config_code_pairs { "included" } else { "suppressed_by_default" },
                    "suppressed_config_code_pairs": suppressed_config_code_pairs,
                },
                "duplicates": filtered.pairs,
                "count": count,
            });
            return Ok((
                attach_workflow_metadata(
                    "cleanup_duplicate_logic",
                    "find_code_duplicates",
                    payload,
                ),
                success_meta(BackendKind::Semantic, 0.80),
            ));
        }
    }

    delegate_workflow(
        state,
        "cleanup_duplicate_logic",
        "dead_code_report",
        json!({
            "scope": arguments.get("scope").and_then(|value| value.as_str()),
            "max_results": arguments.get("max_results").and_then(|value| value.as_u64()),
        }),
        crate::tools::reports::dead_code_report,
    )
}

#[cfg(all(test, feature = "semantic"))]
mod tests {
    use super::*;
    use codelens_engine::ProjectRoot;
    use codelens_engine::embedding::DuplicatePair;

    fn duplicate_pair_with_symbols(
        file_a: &str,
        symbol_a: &str,
        file_b: &str,
        symbol_b: &str,
    ) -> DuplicatePair {
        DuplicatePair {
            symbol_a: format!("{file_a}:{symbol_a}"),
            symbol_b: format!("{file_b}:{symbol_b}"),
            file_a: file_a.to_owned(),
            file_b: file_b.to_owned(),
            line_a: 1,
            line_b: 1,
            similarity: 0.99,
        }
    }

    fn duplicate_pair(file_a: &str, file_b: &str) -> DuplicatePair {
        duplicate_pair_with_symbols(file_a, "a", file_b, "b")
    }

    fn temp_project() -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-workflow-scope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("crates")).unwrap();
        ProjectRoot::new_exact(&dir).unwrap()
    }

    #[test]
    fn duplicate_scope_filter_drops_pairs_fully_outside_scope() {
        let project = temp_project();
        let pairs = vec![
            duplicate_pair(
                ".github/workflows/benchmark.yml",
                ".github/workflows/build.yml",
            ),
            duplicate_pair(
                "crates/codelens-mcp/src/tools/workflows.rs",
                ".github/workflows/build.yml",
            ),
        ];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            false,
        );

        assert_eq!(filtered.pairs.len(), 1);
        assert_eq!(
            filtered.pairs[0].file_a,
            "crates/codelens-mcp/src/tools/workflows.rs"
        );
    }

    #[test]
    fn duplicate_quality_filter_suppresses_workflow_key_code_pairs_by_default() {
        let project = temp_project();
        let pairs = vec![
            duplicate_pair_with_symbols(
                ".github/workflows/pages.yml",
                "workflow_dispatch",
                "crates/codelens-mcp/src/integration_tests/workflow/mod.rs",
                "dispatch",
            ),
            duplicate_pair_with_symbols(
                "crates/codelens-mcp/src/tools/workflows.rs",
                "cleanup_duplicate_logic",
                "crates/codelens-mcp/src/tools/mod.rs",
                "cleanup_duplicate_logic",
            ),
        ];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            false,
        );

        assert_eq!(filtered.suppressed_config_code_pairs, 1);
        assert_eq!(filtered.pairs.len(), 1);
        assert_eq!(
            filtered.pairs[0].file_a,
            "crates/codelens-mcp/src/tools/workflows.rs"
        );
    }

    #[test]
    fn duplicate_quality_filter_can_include_config_code_pairs() {
        let project = temp_project();
        let pairs = vec![duplicate_pair_with_symbols(
            ".github/workflows/pages.yml",
            "workflow_dispatch",
            "crates/codelens-mcp/src/integration_tests/workflow/mod.rs",
            "dispatch",
        )];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            true,
        );

        assert_eq!(filtered.suppressed_config_code_pairs, 0);
        assert_eq!(filtered.pairs.len(), 1);
    }
}
