use super::{AppState, optional_string};
use crate::error::CodeLensError;
use codelens_engine::SymbolInfo;

pub(crate) fn code_action_range(
    state: &AppState,
    arguments: &serde_json::Value,
    file_path: &str,
    operation: &str,
) -> Result<(usize, usize, usize, usize, &'static str), CodeLensError> {
    if let Some(start_line) = arguments.get("start_line").and_then(|value| value.as_u64()) {
        let end_line = arguments
            .get("end_line")
            .and_then(|value| value.as_u64())
            .unwrap_or(start_line) as usize;
        let start_line = start_line as usize;
        let start_column = arguments
            .get("start_column")
            .or_else(|| arguments.get("column"))
            .and_then(|value| value.as_u64())
            .unwrap_or(1) as usize;
        let end_column = arguments
            .get("end_column")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| default_end_column(state, file_path, end_line));
        return Ok((
            start_line,
            start_column,
            end_line,
            end_column,
            "explicit_range",
        ));
    }

    let (symbol_name, name_path) = symbol_for_operation(arguments, operation)?;
    let (line, column) = symbol_position(state, arguments, file_path, &symbol_name, name_path)?;
    Ok((line, column, line, column, position_source(arguments)))
}

pub(crate) fn code_action_kinds(
    arguments: &serde_json::Value,
    default_kinds: &[&str],
) -> Vec<String> {
    if let Some(kind) = optional_string(arguments, "code_action_kind") {
        return vec![kind.to_owned()];
    }
    if let Some(items) = arguments
        .get("code_action_kinds")
        .and_then(|value| value.as_array())
    {
        let parsed = items
            .iter()
            .filter_map(|item| item.as_str().map(ToOwned::to_owned))
            .collect::<Vec<_>>();
        if !parsed.is_empty() {
            return parsed;
        }
    }
    default_kinds
        .iter()
        .map(|kind| (*kind).to_owned())
        .collect()
}

pub(crate) fn language_for_file(file_path: &str) -> &'static str {
    match std::path::Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "java" => "java",
        _ => "unknown",
    }
}

pub(crate) fn position_source(arguments: &serde_json::Value) -> &'static str {
    match (
        arguments.get("line").and_then(|value| value.as_u64()),
        arguments.get("column").and_then(|value| value.as_u64()),
    ) {
        (Some(_), Some(_)) => "explicit",
        _ => "symbol_index",
    }
}

pub(crate) fn symbol_position(
    state: &AppState,
    arguments: &serde_json::Value,
    file_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
) -> Result<(usize, usize), CodeLensError> {
    match (
        arguments.get("line").and_then(|value| value.as_u64()),
        arguments.get("column").and_then(|value| value.as_u64()),
    ) {
        (Some(line), Some(column)) => return Ok((line as usize, column as usize)),
        (None, None) => {}
        _ => {
            return Err(CodeLensError::MissingParam(
                "line and column must be provided together".into(),
            ));
        }
    }

    let symbols = codelens_engine::get_symbols_overview(&state.project(), file_path, 0)
        .map_err(CodeLensError::Internal)?;
    let flat = flatten_symbols(symbols);
    flat.iter()
        .find(|symbol| {
            if let Some(name_path) = name_path {
                symbol.name_path == name_path
            } else {
                symbol.name == symbol_name
            }
        })
        .map(|symbol| (symbol.line, symbol.column))
        .ok_or_else(|| {
            CodeLensError::NotFound(format!(
                "symbol `{symbol_name}` not found in {file_path}; provide line and column for LSP rename"
            ))
        })
}

fn symbol_for_operation<'a>(
    arguments: &'a serde_json::Value,
    operation: &str,
) -> Result<(String, Option<&'a str>), CodeLensError> {
    let name_path = arguments.get("name_path").and_then(|value| value.as_str());
    let key = match operation {
        "move_symbol" => "symbol_name",
        "inline_function" | "change_signature" => "function_name",
        _ => "symbol_name",
    };
    let symbol_name = arguments
        .get(key)
        .or_else(|| arguments.get("symbol_name"))
        .or_else(|| arguments.get("function_name"))
        .or_else(|| arguments.get("name"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| CodeLensError::MissingParam(key.into()))?
        .to_owned();
    Ok((symbol_name, name_path))
}

fn default_end_column(state: &AppState, file_path: &str, line: usize) -> usize {
    state
        .project()
        .resolve(file_path)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|source| {
            source
                .lines()
                .nth(line.saturating_sub(1))
                .map(|text| text.len() + 1)
        })
        .unwrap_or(1)
}

fn flatten_symbols(symbols: Vec<SymbolInfo>) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    for mut symbol in symbols {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        flat.extend(flatten_symbols(children));
    }
    flat
}

#[cfg(test)]
mod tests {
    use super::*;
    use codelens_engine::{SymbolKind, SymbolProvenance};
    use serde_json::json;

    #[test]
    fn language_for_file_maps_extensions() {
        assert_eq!(language_for_file("main.rs"), "rust");
        assert_eq!(language_for_file("app.ts"), "typescript");
        assert_eq!(language_for_file("app.tsx"), "typescript");
        assert_eq!(language_for_file("lib.js"), "javascript");
        assert_eq!(language_for_file("Foo.java"), "java");
        assert_eq!(language_for_file("README.md"), "unknown");
    }

    #[test]
    fn position_source_explicit_when_line_and_column_present() {
        assert_eq!(
            position_source(&json!({"line": 1, "column": 2})),
            "explicit"
        );
    }

    #[test]
    fn position_source_symbol_index_when_missing() {
        assert_eq!(position_source(&json!({})), "symbol_index");
        assert_eq!(position_source(&json!({"line": 1})), "symbol_index");
    }

    #[test]
    fn code_action_kinds_prefers_single_kind() {
        let args = json!({"code_action_kind": "quickfix"});
        assert_eq!(code_action_kinds(&args, &["refactor"]), vec!["quickfix"]);
    }

    #[test]
    fn code_action_kinds_falls_back_to_array() {
        let args = json!({"code_action_kinds": ["quickfix", "source"]});
        assert_eq!(
            code_action_kinds(&args, &["refactor"]),
            vec!["quickfix", "source"]
        );
    }

    #[test]
    fn code_action_kinds_falls_back_to_defaults() {
        let args = json!({});
        assert_eq!(
            code_action_kinds(&args, &["refactor", "quickfix"]),
            vec!["refactor", "quickfix"]
        );
    }

    #[test]
    fn symbol_for_operation_move_symbol() {
        let args = json!({"symbol_name": "Foo"});
        let (name, path) = symbol_for_operation(&args, "move_symbol").unwrap();
        assert_eq!(name, "Foo");
        assert_eq!(path, None);
    }

    #[test]
    fn symbol_for_operation_inline_function() {
        let args = json!({"function_name": "bar"});
        let (name, _) = symbol_for_operation(&args, "inline_function").unwrap();
        assert_eq!(name, "bar");
    }

    #[test]
    fn symbol_for_operation_missing_param_errors() {
        let args = json!({});
        assert!(symbol_for_operation(&args, "move_symbol").is_err());
    }

    #[test]
    fn flatten_symbols_recursive() {
        let child = SymbolInfo {
            name: "child".to_owned(),
            name_path: "child".to_owned(),
            kind: SymbolKind::Function,
            line: 2,
            column: 0,
            file_path: "a.rs".to_owned(),
            signature: String::new(),
            id: String::new(),
            provenance: SymbolProvenance::default(),
            body: None,
            children: vec![],
            start_byte: 0,
            end_byte: 0,
        };
        let parent = SymbolInfo {
            name: "parent".to_owned(),
            name_path: "parent".to_owned(),
            kind: SymbolKind::Module,
            line: 1,
            column: 0,
            file_path: "a.rs".to_owned(),
            signature: String::new(),
            id: String::new(),
            provenance: SymbolProvenance::default(),
            body: None,
            children: vec![child],
            start_byte: 0,
            end_byte: 0,
        };
        let flat = flatten_symbols(vec![parent]);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].name, "parent");
        assert_eq!(flat[1].name, "child");
    }
}
