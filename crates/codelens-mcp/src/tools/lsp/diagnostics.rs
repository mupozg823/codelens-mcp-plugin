use super::super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, optional_usize,
    parse_lsp_args, success_meta,
};
use super::shared::{enhance_lsp_error, insert_response_annotations, resolve_path_argument};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::LspDiagnosticRequest;
use serde_json::{Value, json};
use std::collections::HashSet;

fn postprocess_lsp_diagnostics(
    project: &codelens_engine::ProjectRoot,
    file_path: &str,
    diagnostics: Vec<codelens_engine::LspDiagnostic>,
) -> (Vec<Value>, Vec<Value>) {
    let source = DiagnosticSourceContext::load(project, file_path);
    let mut visible = Vec::new();
    let mut suppressed = Vec::new();

    for diagnostic in diagnostics {
        if let Some(reason) = pyright_source_suppression_reason(&source, &diagnostic) {
            suppressed.push(json!({
                "file_path": diagnostic.file_path,
                "line": diagnostic.line,
                "column": diagnostic.column,
                "code": diagnostic.code,
                "source": diagnostic.source,
                "message": diagnostic.message,
                "suppression": reason,
            }));
            continue;
        }

        let mut value = serde_json::to_value(&diagnostic).unwrap_or_else(|_| json!({}));
        if is_optional_import_diagnostic(&source, &diagnostic) {
            value["classification"] = json!("optional_dependency_import");
            value["actionability"] = json!("environmental_optional_dependency");
            value["recommended_action"] = json!(
                "Do not patch source solely for this diagnostic; install the optional extra or treat it as non-blocking when the import is guarded by ImportError."
            );
        }
        visible.push(value);
    }

    (visible, suppressed)
}

struct DiagnosticSourceContext {
    lines: Vec<String>,
    disabled_pyright_rules: HashSet<String>,
}

impl DiagnosticSourceContext {
    fn load(project: &codelens_engine::ProjectRoot, file_path: &str) -> Self {
        let source = project
            .resolve(file_path)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        let lines: Vec<String> = source.lines().map(ToOwned::to_owned).collect();
        let disabled_pyright_rules = lines
            .iter()
            .take(10)
            .flat_map(|line| disabled_pyright_rules_on_line(line))
            .collect();
        Self {
            lines,
            disabled_pyright_rules,
        }
    }

    fn line(&self, one_based_line: usize) -> Option<&str> {
        self.lines
            .get(one_based_line.saturating_sub(1))
            .map(String::as_str)
    }
}

fn pyright_source_suppression_reason(
    source: &DiagnosticSourceContext,
    diagnostic: &codelens_engine::LspDiagnostic,
) -> Option<&'static str> {
    let code = diagnostic.code.as_deref()?;
    if !is_pyright_diagnostic(diagnostic, code) {
        return None;
    }
    if source.disabled_pyright_rules.contains(code) {
        return Some("file_pyright_rule_disabled");
    }
    let line = source.line(diagnostic.line)?;
    if line_suppresses_pyright_rule(line, code) {
        return Some("line_pyright_ignore");
    }
    None
}

fn is_pyright_diagnostic(diagnostic: &codelens_engine::LspDiagnostic, code: &str) -> bool {
    diagnostic.source.as_deref() == Some("pyright") || code.starts_with("report")
}

fn disabled_pyright_rules_on_line(line: &str) -> Vec<String> {
    let normalized = normalize_pyright_directive(line);
    let Some((_, directive)) = normalized.split_once("#pyright:") else {
        return Vec::new();
    };
    directive
        .split(',')
        .filter_map(|entry| {
            let (rule, value) = entry.split_once('=')?;
            (value == "false").then(|| rule.to_owned())
        })
        .collect()
}

fn line_suppresses_pyright_rule(line: &str, code: &str) -> bool {
    let normalized = normalize_pyright_directive(line);
    if normalized.contains("#type:ignore") {
        return true;
    }
    if !normalized.contains("#pyright:ignore") {
        return false;
    }
    !normalized.contains('[') || normalized.contains(code)
}

fn normalize_pyright_directive(line: &str) -> String {
    line.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn is_optional_import_diagnostic(
    source: &DiagnosticSourceContext,
    diagnostic: &codelens_engine::LspDiagnostic,
) -> bool {
    let Some(code) = diagnostic.code.as_deref() else {
        return false;
    };
    if !matches!(code, "reportMissingImports" | "reportMissingModuleSource") {
        return false;
    }
    import_line_is_guarded_by_import_error(source, diagnostic.line)
}

fn import_line_is_guarded_by_import_error(
    source: &DiagnosticSourceContext,
    one_based_line: usize,
) -> bool {
    let Some(import_line) = source.line(one_based_line) else {
        return false;
    };
    if !import_line.contains("import ") {
        return false;
    }
    let line_index = one_based_line.saturating_sub(1);
    let start = line_index.saturating_sub(4);
    let end = (line_index + 8).min(source.lines.len().saturating_sub(1));
    let has_try = source.lines[start..=line_index]
        .iter()
        .any(|line| line.trim_start().starts_with("try:"));
    let has_import_error_handler = source.lines[line_index..=end]
        .iter()
        .any(|line| line.trim_start().starts_with("except ImportError"));
    has_try && has_import_error_handler
}

pub fn get_file_diagnostics(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    const FILE_DIAGNOSTICS_KNOWN_ARGS: &[&str] = &[
        "path",
        "file_path",
        "relative_path",
        "command",
        "args",
        "max_results",
    ];
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let file_path = file_path_arg.to_owned();
    let max_results = optional_usize(arguments, "max_results", 200);
    let unknown_args =
        crate::tool_runtime::collect_unknown_args(arguments, FILE_DIAGNOSTICS_KNOWN_ARGS);

    // Try SCIP diagnostics first (if available).
    #[cfg(feature = "scip-backend")]
    if let Some(backend) = state.scip() {
        use codelens_engine::PreciseBackend as _;
        if let Ok(scip_diags) = backend.diagnostics(&file_path)
            && !scip_diags.is_empty()
        {
            let limited: Vec<_> = scip_diags.into_iter().take(max_results).collect();
            let count = limited.len();
            let diags_json: Vec<serde_json::Value> = limited
                .iter()
                .map(|d| {
                    json!({
                        "file_path": d.file_path,
                        "line": d.line,
                        "column": d.column,
                        "severity": format!("{:?}", d.severity),
                        "message": d.message,
                        "source": "scip",
                        "code": d.code,
                    })
                })
                .collect();
            let mut payload = json!({
                "diagnostics": diags_json,
                "count": count,
                "backend": "scip",
            });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            return Ok((payload, success_meta(BackendKind::Scip, 0.95)));
        }
    }

    // Fall back to LSP diagnostics.
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);

    let command_ref = command.clone();
    let request_file_path = file_path.clone();
    state
        .lsp_pool()
        .get_diagnostics(&LspDiagnosticRequest {
            command,
            args,
            file_path,
            max_results,
        })
        .map_err(|e| enhance_lsp_error(e, &command_ref))
        .map(|value| {
            let (diagnostics, suppressed) =
                postprocess_lsp_diagnostics(&state.project(), &request_file_path, value);
            let mut payload = json!({
                "diagnostics": diagnostics,
                "count": diagnostics.len(),
                "backend": "lsp",
            });
            if !suppressed.is_empty() {
                payload["suppressed_diagnostics"] = json!(suppressed);
                payload["suppressed_diagnostics_count"] = json!(
                    payload["suppressed_diagnostics"]
                        .as_array()
                        .map_or(0, Vec::len)
                );
            }
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            (payload, success_meta(BackendKind::Lsp, 0.9))
        })
}

/// D1 (#346 Phase 4): `get_file_diagnostics` narrowed to one symbol's
/// span. The span comes from the symbol index (definition line + body
/// line count), the diagnostics from the same SCIP→LSP pipeline as the
/// file-level tool — so classification/suppression behavior is
/// identical, just filtered. LSP-absent degrades to a successful empty
/// result with `degraded_reason` + `fallback_hint` (read-surface
/// contract shared with the navigation pair).
pub fn get_diagnostics_for_symbol(state: &AppState, arguments: &Value) -> ToolResult {
    const SYMBOL_DIAGNOSTICS_KNOWN_ARGS: &[&str] = &[
        "path",
        "file_path",
        "relative_path",
        "symbol_name",
        "command",
        "args",
        "max_results",
    ];
    let (file_path_arg, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args =
        crate::tool_runtime::collect_unknown_args(arguments, SYMBOL_DIAGNOSTICS_KNOWN_ARGS);
    let file_path = file_path_arg.to_owned();
    let symbol_name = crate::tool_runtime::required_string(arguments, "symbol_name")?.to_owned();

    let symbols = state
        .symbol_index()
        .find_symbol(&symbol_name, Some(&file_path), true, true, 1)
        .map_err(|err| {
            CodeLensError::Internal(err.context(
                "get_diagnostics_for_symbol: symbol index lookup failed (is the file indexed in this project?)",
            ))
        })?;
    let Some(symbol) = symbols.first() else {
        return Err(CodeLensError::Validation(format!(
            "symbol '{symbol_name}' not found in {file_path} — run refresh_symbol_index if it was just added"
        )));
    };
    let start_line = symbol.line;
    let end_line = symbol
        .body
        .as_ref()
        .map(|body| start_line + body.lines().count().saturating_sub(1))
        .unwrap_or(start_line);
    let symbol_summary = json!({
        "name": symbol.name,
        "kind": symbol.kind.as_label(),
        "file_path": symbol.file_path,
        "span": {"start_line": start_line, "end_line": end_line},
    });

    match get_file_diagnostics(state, arguments) {
        Ok((file_payload, _meta)) => {
            let all = file_payload["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let file_count = all.len();
            let diagnostics: Vec<Value> = all
                .into_iter()
                .filter(|diag| {
                    diag["line"]
                        .as_u64()
                        .map(|line| {
                            let line = line as usize;
                            line >= start_line && line <= end_line
                        })
                        .unwrap_or(false)
                })
                .collect();
            let mut payload = json!({
                "success": true,
                "symbol": symbol_summary,
                "diagnostics": diagnostics,
                "count": diagnostics.len(),
                "file_diagnostics_count": file_count,
                "backend": file_payload["backend"],
            });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            Ok((payload, success_meta(BackendKind::Lsp, 0.9)))
        }
        Err(error) => {
            let reason = format!("LSP unavailable for symbol diagnostics: {error}");
            let mut payload = json!({
                "success": true,
                "symbol": symbol_summary,
                "diagnostics": [],
                "count": 0,
                "degraded_reason": reason,
                "fallback_hint": super::navigation::LSP_READ_FALLBACK_HINTS,
            });
            insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);
            Ok((
                payload,
                crate::tool_runtime::degraded_meta(BackendKind::Lsp, 0.3, &reason),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_pyright_rules_on_line_parses_multiple_rules() {
        assert_eq!(
            disabled_pyright_rules_on_line(
                "# pyright: reportMissingImports=false, reportCallIssue=false"
            ),
            vec![
                "reportMissingImports".to_owned(),
                "reportCallIssue".to_owned()
            ]
        );
    }
}
