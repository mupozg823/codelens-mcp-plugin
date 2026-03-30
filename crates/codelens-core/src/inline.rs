use crate::project::ProjectRoot;
use crate::rename::{apply_edits, find_all_word_matches, RenameEdit};
use crate::symbols::{find_symbol, find_symbol_range};
use anyhow::{bail, Result};
use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, Serialize)]
pub struct InlineResult {
    pub success: bool,
    pub message: String,
    pub call_sites_inlined: usize,
    pub definition_removed: bool,
    pub modified_files: Vec<String>,
    pub edits: Vec<RenameEdit>,
}

/// Inline a function: replace all call sites with the function body, then remove the definition.
///
/// Supports single-expression and multi-statement bodies. For multi-statement bodies,
/// only single call-site inlining is supported (otherwise ambiguous).
pub fn inline_function(
    project: &ProjectRoot,
    file_path: &str,
    function_name: &str,
    name_path: Option<&str>,
    dry_run: bool,
) -> Result<InlineResult> {
    // 1. Find the function definition
    let symbols = find_symbol(project, function_name, Some(file_path), true, true, 1)?;
    let sym = symbols.first().ok_or_else(|| {
        anyhow::anyhow!("Function '{}' not found in '{}'", function_name, file_path)
    })?;

    let kind_str = format!("{:?}", sym.kind).to_lowercase();
    if kind_str != "function" && kind_str != "method" {
        bail!(
            "'{}' is a {}, not a function/method",
            function_name,
            kind_str
        );
    }

    let resolved = project.resolve(file_path)?;
    let source = fs::read_to_string(&resolved)?;

    // 2. Extract function body (between the symbol range)
    let (start_byte, end_byte) = find_symbol_range(project, file_path, function_name, name_path)?;
    let full_def = &source[start_byte..end_byte];

    // 3. Parse parameters and body from the definition
    let (params, body) = parse_function_parts(full_def, file_path)?;

    // 4. Find all call sites across the project
    let matches = find_all_word_matches(project, function_name)?;

    // Filter to actual call sites (followed by '(')
    let mut call_sites = Vec::new();
    for (rel_path, line, col) in &matches {
        // Skip the definition itself
        if rel_path == file_path && *line == sym.line {
            continue;
        }
        let call_file = project.resolve(rel_path)?;
        let call_source = match fs::read_to_string(&call_file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let lines: Vec<&str> = call_source.lines().collect();
        if *line == 0 || *line > lines.len() {
            continue;
        }
        let line_text = lines[*line - 1];
        let after_name = *col - 1 + function_name.len();
        let rest = &line_text[after_name..].trim_start();
        if rest.starts_with('(') {
            // Extract the arguments
            if let Some(args) = extract_call_args(line_text, *col - 1) {
                call_sites.push((rel_path.clone(), *line, *col, args));
            }
        }
    }

    if call_sites.is_empty() {
        return Ok(InlineResult {
            success: true,
            message: format!(
                "No call sites found for '{}'. Definition kept.",
                function_name
            ),
            call_sites_inlined: 0,
            definition_removed: false,
            modified_files: vec![],
            edits: vec![],
        });
    }

    // 5. Build edits for each call site
    let body_lines: Vec<&str> = body.lines().collect();
    let is_single_expression = body_lines.len() <= 1;

    if !is_single_expression && call_sites.len() > 1 {
        bail!(
            "Cannot inline multi-statement function '{}' with {} call sites. \
             Inline manually or reduce to a single expression.",
            function_name,
            call_sites.len()
        );
    }

    let mut edits = Vec::new();

    for (rel_path, line, col, args) in &call_sites {
        let call_file = project.resolve(rel_path)?;
        let call_source = fs::read_to_string(&call_file)?;
        let lines_vec: Vec<&str> = call_source.lines().collect();
        let line_text = lines_vec[*line - 1];

        // Find the full call expression span (name + args including parens)
        let call_start = *col - 1;
        let call_end = find_call_end(line_text, call_start)?;
        let call_text = &line_text[call_start..call_end];

        // Substitute parameters with arguments in the body
        let mut inlined_body = body.trim().to_string();
        for (i, param) in params.iter().enumerate() {
            if let Some(arg) = args.get(i) {
                let param_re = regex::Regex::new(&format!(r"\b{}\b", regex::escape(param)))?;
                inlined_body = param_re.replace_all(&inlined_body, arg.trim()).to_string();
            }
        }

        // For single-expression: strip return keyword if present
        let inlined_body = strip_return_keyword(&inlined_body);

        edits.push(RenameEdit {
            file_path: rel_path.clone(),
            line: *line,
            column: *col,
            old_text: call_text.to_string(),
            new_text: inlined_body,
        });
    }

    // 6. Add edit to remove the function definition
    let (start_byte_2, end_byte_2) = (start_byte, end_byte);
    let def_start_line = source[..start_byte_2].lines().count();
    let def_end_line = source[..end_byte_2].lines().count();

    let mut modified_files: Vec<String> = edits.iter().map(|e| e.file_path.clone()).collect();
    if !modified_files.contains(&file_path.to_string()) {
        modified_files.push(file_path.to_string());
    }
    modified_files.sort();
    modified_files.dedup();

    let result = InlineResult {
        success: true,
        message: format!(
            "Inlined '{}' at {} call site(s) and removed definition",
            function_name,
            call_sites.len()
        ),
        call_sites_inlined: call_sites.len(),
        definition_removed: true,
        modified_files,
        edits: edits.clone(),
    };

    if !dry_run {
        // Apply call site edits first
        apply_edits(project, &edits)?;

        // Remove the function definition lines
        let resolved = project.resolve(file_path)?;
        let content = fs::read_to_string(&resolved)?;
        let mut lines: Vec<String> = content.lines().map(String::from).collect();

        // Recalculate definition line range from bytes
        let start_line_idx = if def_start_line > 0 {
            def_start_line - 1
        } else {
            0
        };
        let end_line_idx = def_end_line.min(lines.len());

        // Remove preceding blank line if any
        let drain_start = if start_line_idx > 0 && lines[start_line_idx - 1].trim().is_empty() {
            start_line_idx - 1
        } else {
            start_line_idx
        };
        lines.drain(drain_start..end_line_idx);

        let mut result_text = lines.join("\n");
        if content.ends_with('\n') {
            result_text.push('\n');
        }
        fs::write(&resolved, &result_text)?;
    }

    Ok(result)
}

/// Parse function parameters and body from a function definition string.
fn parse_function_parts(def: &str, file_path: &str) -> Result<(Vec<String>, String)> {
    // Find parameter list between first ( and matching )
    let paren_start = def
        .find('(')
        .ok_or_else(|| anyhow::anyhow!("No parameter list found"))?;
    let paren_end = find_matching_paren(def, paren_start)?;

    let params_str = &def[paren_start + 1..paren_end];
    let params: Vec<String> = if params_str.trim().is_empty() {
        vec![]
    } else {
        parse_param_names(params_str, file_path)
    };

    // Find body: after '{' for brace languages, after ':' for Python
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let body = if ext == "py" {
        // Python: body is everything after the first colon+newline, de-indented
        let colon_pos = def[paren_end..].find(':').map(|p| p + paren_end);
        if let Some(cp) = colon_pos {
            let after_colon = &def[cp + 1..];
            dedent_body(after_colon.trim_start_matches([' ', '\t']))
        } else {
            String::new()
        }
    } else {
        // Brace languages: body is between first { and last }
        let brace_start = def[paren_end..].find('{').map(|p| p + paren_end);
        let brace_end = def.rfind('}');
        match (brace_start, brace_end) {
            (Some(bs), Some(be)) if be > bs => dedent_body(&def[bs + 1..be]),
            _ => String::new(),
        }
    };

    Ok((params, body))
}

/// Extract just parameter names from a parameter string, handling typed params.
fn parse_param_names(params_str: &str, file_path: &str) -> Vec<String> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    params_str
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() || p == "self" || p == "&self" || p == "&mut self" || p == "this" {
                return None;
            }
            // Remove default values
            let p = p.split('=').next().unwrap_or(p).trim();
            // Extract name based on language
            let name = match ext {
                "rs" => p.split(':').next().unwrap_or(p).trim(),
                "go" => p.split_whitespace().next().unwrap_or(p),
                "java" | "kt" | "ts" | "tsx" | "dart" | "cs" | "scala" | "swift" => {
                    // type name or name: type
                    if p.contains(':') {
                        p.split(':').next().unwrap_or(p).trim()
                    } else {
                        p.split_whitespace().last().unwrap_or(p)
                    }
                }
                "py" => {
                    if p.contains(':') {
                        p.split(':').next().unwrap_or(p).trim()
                    } else {
                        p.trim()
                    }
                }
                _ => {
                    if p.contains(':') {
                        p.split(':').next().unwrap_or(p).trim()
                    } else {
                        p.split_whitespace().last().unwrap_or(p)
                    }
                }
            };
            Some(name.to_string())
        })
        .collect()
}

/// Find matching closing parenthesis, handling nesting.
fn find_matching_paren(s: &str, open_pos: usize) -> Result<usize> {
    let mut depth = 0;
    for (i, ch) in s[open_pos..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(open_pos + i);
                }
            }
            _ => {}
        }
    }
    bail!("Unmatched parenthesis")
}

/// Extract arguments from a function call at the given position.
fn extract_call_args(line: &str, name_start: usize) -> Option<Vec<String>> {
    // Find the opening paren after the function name
    let rest = &line[name_start..];
    let paren_start = rest.find('(')?;
    let paren_end = find_matching_paren(rest, paren_start).ok()?;
    let args_str = &rest[paren_start + 1..paren_end];
    if args_str.trim().is_empty() {
        return Some(vec![]);
    }
    Some(split_args(args_str))
}

/// Split argument string by commas, respecting nested parens/brackets.
fn split_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut depth = 0;
    let mut current = String::new();
    for ch in s.chars() {
        match ch {
            '(' | '[' | '{' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                args.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        args.push(current.trim().to_string());
    }
    args
}

/// Find the end of a function call expression (past the closing paren).
fn find_call_end(line: &str, name_start: usize) -> Result<usize> {
    let rest = &line[name_start..];
    let paren_start = rest
        .find('(')
        .ok_or_else(|| anyhow::anyhow!("No opening paren"))?;
    let paren_end = find_matching_paren(rest, paren_start)?;
    Ok(name_start + paren_end + 1)
}

/// Strip leading 'return ' keyword from a body string.
fn strip_return_keyword(body: &str) -> String {
    let trimmed = body.trim();
    if let Some(rest) = trimmed.strip_prefix("return ") {
        rest.trim_end_matches(';').to_string()
    } else {
        trimmed.trim_end_matches(';').to_string()
    }
}

/// Remove common leading whitespace from a body string.
fn dedent_body(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let non_empty: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    if non_empty.is_empty() {
        return String::new();
    }
    let min_indent = non_empty
        .iter()
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.len() >= min_indent {
                &l[min_indent..]
            } else {
                l.trim()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;
    use std::fs;

    fn make_fixture() -> (std::path::PathBuf, ProjectRoot) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-inline-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let project = ProjectRoot::new(dir.clone()).unwrap();
        (dir, project)
    }

    #[test]
    fn test_parse_function_parts_js() {
        let def = "function add(a, b) {\n  return a + b;\n}";
        let (params, body) = parse_function_parts(def, "test.js").unwrap();
        assert_eq!(params, vec!["a", "b"]);
        assert!(body.contains("return a + b"));
    }

    #[test]
    fn test_parse_function_parts_python() {
        let def = "def add(x, y):\n    return x + y";
        let (params, body) = parse_function_parts(def, "test.py").unwrap();
        assert_eq!(params, vec!["x", "y"]);
        assert!(body.contains("return x + y"));
    }

    #[test]
    fn test_parse_function_parts_rust() {
        let def = "fn add(a: i32, b: i32) -> i32 {\n    a + b\n}";
        let (params, body) = parse_function_parts(def, "test.rs").unwrap();
        assert_eq!(params, vec!["a", "b"]);
        assert!(body.contains("a + b"));
    }

    #[test]
    fn test_extract_call_args() {
        let line = "let result = add(1, 2);";
        let args = extract_call_args(line, 13).unwrap();
        assert_eq!(args, vec!["1", "2"]);
    }

    #[test]
    fn test_extract_call_args_nested() {
        let line = "let result = add(foo(1), bar(2, 3));";
        let args = extract_call_args(line, 13).unwrap();
        assert_eq!(args, vec!["foo(1)", "bar(2, 3)"]);
    }

    #[test]
    fn test_strip_return_keyword() {
        assert_eq!(strip_return_keyword("return x + y;"), "x + y");
        assert_eq!(strip_return_keyword("x + y"), "x + y");
    }

    #[test]
    fn test_dedent_body() {
        let body = "    let x = 1;\n    let y = 2;\n    x + y";
        let result = dedent_body(body);
        assert_eq!(result, "let x = 1;\nlet y = 2;\nx + y");
    }

    #[test]
    fn test_inline_dry_run() {
        let (dir, project) = make_fixture();

        let main_content = r#"function greet(name) {
    return "Hello, " + name;
}

let msg = greet("World");
console.log(greet("Rust"));
"#;
        fs::write(dir.join("main.js"), main_content).unwrap();

        let result = inline_function(&project, "main.js", "greet", None, true).unwrap();
        assert!(result.success);
        assert_eq!(result.call_sites_inlined, 2);
        assert!(result.definition_removed);

        // Dry run: file should be unchanged
        let after = fs::read_to_string(dir.join("main.js")).unwrap();
        assert_eq!(after, main_content);

        fs::remove_dir_all(&dir).ok();
    }
}
