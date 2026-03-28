use super::{required_string, success_meta, AppState, ToolResult};
use codelens_core::{get_callees, get_callers, get_importers, get_symbols_overview, SymbolKind};
use serde_json::json;

pub fn summarize_file(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let symbols = get_symbols_overview(&state.project, file_path, 2)?;
    let importers = get_importers(&state.project, file_path, 20).unwrap_or_default();
    let source = std::fs::read_to_string(state.project.resolve(file_path)?).unwrap_or_default();
    let line_count = source.lines().count();

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

pub fn explain_code_flow(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let function_name = required_string(arguments, "function_name")?;
    let _max_depth = arguments
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

pub fn refactor_extract_function(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Missing start_line"))? as usize;
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Missing end_line"))? as usize;
    let new_name = required_string(arguments, "new_name")?;
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

    let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
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
        new_lines.drain((start_line - 1)..end_line);
        new_lines.insert(start_line - 1, func_call.clone());
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
