use crate::project::ProjectRoot;
use crate::rename::{RenameEdit, apply_edits, find_all_word_matches};
use crate::symbols::{find_symbol, find_symbol_range};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamSpec {
    pub name: String,
    #[serde(rename = "type", default)]
    pub param_type: Option<String>,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeSignatureResult {
    pub success: bool,
    pub message: String,
    pub old_params: Vec<String>,
    pub new_params: Vec<String>,
    pub call_sites_updated: usize,
    pub modified_files: Vec<String>,
    pub edits: Vec<RenameEdit>,
}

/// Change a function's signature and update all call sites.
///
/// Parameters can be reordered, added (with defaults), or removed.
/// Matching is name-based: parameters with the same name are mapped,
/// new names are insertions, missing names are deletions.
pub fn change_signature(
    project: &ProjectRoot,
    file_path: &str,
    function_name: &str,
    name_path: Option<&str>,
    new_params: &[ParamSpec],
    dry_run: bool,
) -> Result<ChangeSignatureResult> {
    // 1. Find the function
    let symbols = find_symbol(project, function_name, Some(file_path), true, true, 1)?;
    let _sym = symbols.first().ok_or_else(|| {
        anyhow::anyhow!("Function '{}' not found in '{}'", function_name, file_path)
    })?;

    let resolved = project.resolve(file_path)?;
    let source = fs::read_to_string(&resolved)?;
    let (start_byte, end_byte) = find_symbol_range(project, file_path, function_name, name_path)?;
    let full_def = &source[start_byte..end_byte];

    // 2. Parse current parameter list
    let paren_start = full_def
        .find('(')
        .ok_or_else(|| anyhow::anyhow!("No parameter list found in function definition"))?;
    let paren_end = find_matching_paren(full_def, paren_start)?;
    let old_params_str = &full_def[paren_start + 1..paren_end];

    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let old_param_names = parse_param_names(old_params_str, ext);
    // Filter out self/this params for mapping
    let old_mappable: Vec<&str> = old_param_names
        .iter()
        .filter(|p| !is_self_param(p))
        .map(|s| s.as_str())
        .collect();

    // 3. Build new parameter string for the definition
    let new_param_string = build_new_param_string(new_params, ext, old_params_str);

    // 4. Build edit for the definition
    let abs_paren_start = start_byte + paren_start;
    let _abs_paren_end = start_byte + paren_end;

    // Calculate line/col for definition edit
    let def_line = source[..abs_paren_start + 1].lines().count();
    let def_line_start = source[..abs_paren_start + 1]
        .rfind('\n')
        .map_or(0, |p| p + 1);
    let def_col = abs_paren_start + 1 - def_line_start + 1;

    let old_params_text = old_params_str.to_string();

    let mut edits = vec![RenameEdit {
        file_path: file_path.to_string(),
        line: def_line,
        column: def_col,
        old_text: old_params_text.clone(),
        new_text: new_param_string.clone(),
    }];

    // 5. Build parameter index mapping: old position -> new position
    let param_mapping = build_param_mapping(&old_mappable, new_params);

    // 6. Find and update call sites
    let matches = find_all_word_matches(project, function_name)?;
    let sym_line = _sym.line;

    let mut call_sites_updated = 0;

    for (ref_file, line, col) in &matches {
        // Skip the definition itself
        if ref_file == file_path && *line == sym_line {
            continue;
        }

        let ref_resolved = match project.resolve(ref_file) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ref_content = match fs::read_to_string(&ref_resolved) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let ref_lines: Vec<&str> = ref_content.lines().collect();
        if *line == 0 || *line > ref_lines.len() {
            continue;
        }
        let line_text = ref_lines[*line - 1];

        // Check if this is a call site (followed by '(')
        let name_end = *col - 1 + function_name.len();
        if name_end >= line_text.len() {
            continue;
        }
        let after = line_text[name_end..].trim_start();
        if !after.starts_with('(') {
            continue;
        }

        // Extract call arguments
        let call_rest = &line_text[*col - 1..];
        let call_paren = match call_rest.find('(') {
            Some(p) => p,
            None => continue,
        };
        let call_paren_end = match find_matching_paren(call_rest, call_paren) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let args_str = &call_rest[call_paren + 1..call_paren_end];
        let old_args = split_args(args_str);

        // Build new arguments based on mapping
        let new_args = build_new_args(&old_args, &param_mapping, new_params);
        let new_args_str = new_args.join(", ");

        if args_str.trim() != new_args_str.trim() {
            let args_col = *col + call_paren + 1;
            edits.push(RenameEdit {
                file_path: ref_file.clone(),
                line: *line,
                column: args_col,
                old_text: args_str.to_string(),
                new_text: new_args_str,
            });
            call_sites_updated += 1;
        }
    }

    let mut modified_files: Vec<String> = edits.iter().map(|e| e.file_path.clone()).collect();
    modified_files.sort();
    modified_files.dedup();

    let result = ChangeSignatureResult {
        success: true,
        message: format!(
            "Changed signature of '{}': {} params → {}, updated {} call site(s)",
            function_name,
            old_mappable.len(),
            new_params.len(),
            call_sites_updated
        ),
        old_params: old_mappable.iter().map(|s| s.to_string()).collect(),
        new_params: new_params.iter().map(|p| p.name.clone()).collect(),
        call_sites_updated,
        modified_files,
        edits: edits.clone(),
    };

    if !dry_run {
        apply_edits(project, &edits)?;
    }

    Ok(result)
}

/// Parse parameter names from a parameter string.
fn parse_param_names(params_str: &str, ext: &str) -> Vec<String> {
    if params_str.trim().is_empty() {
        return vec![];
    }
    params_str
        .split(',')
        .map(|p| {
            let p = p.trim();
            // Remove default values
            let p = p.split('=').next().unwrap_or(p).trim();
            match ext {
                "rs" => p.split(':').next().unwrap_or(p).trim().to_string(),
                "go" => p.split_whitespace().next().unwrap_or(p).to_string(),
                "py" => {
                    if p.contains(':') {
                        p.split(':').next().unwrap_or(p).trim().to_string()
                    } else {
                        p.to_string()
                    }
                }
                _ => {
                    if p.contains(':') {
                        p.split(':').next().unwrap_or(p).trim().to_string()
                    } else {
                        p.split_whitespace().last().unwrap_or(p).to_string()
                    }
                }
            }
        })
        .collect()
}

fn is_self_param(name: &str) -> bool {
    matches!(name, "self" | "&self" | "&mut self" | "this")
}

/// Build parameter index mapping: for each new param, find its old index (if it existed).
fn build_param_mapping(old_params: &[&str], new_params: &[ParamSpec]) -> Vec<Option<usize>> {
    new_params
        .iter()
        .map(|np| old_params.iter().position(|&op| op == np.name))
        .collect()
}

/// Build new parameter string for the function definition.
fn build_new_param_string(new_params: &[ParamSpec], ext: &str, old_params_str: &str) -> String {
    // Preserve self parameter if present
    let old_parts: Vec<&str> = old_params_str.split(',').map(|p| p.trim()).collect();
    let has_self = old_parts
        .first()
        .is_some_and(|p| is_self_param(p.split(':').next().unwrap_or(p).trim()));

    let mut parts = Vec::new();
    if has_self {
        parts.push(old_parts[0].to_string());
    }

    for param in new_params {
        let part = match ext {
            "rs" => {
                if let Some(t) = &param.param_type {
                    format!("{}: {}", param.name, t)
                } else {
                    param.name.clone()
                }
            }
            "py" => {
                let mut s = param.name.clone();
                if let Some(t) = &param.param_type {
                    s = format!("{}: {}", s, t);
                }
                if let Some(d) = &param.default {
                    s = format!("{} = {}", s, d);
                }
                s
            }
            "go" => {
                if let Some(t) = &param.param_type {
                    format!("{} {}", param.name, t)
                } else {
                    param.name.clone()
                }
            }
            "ts" | "tsx" | "js" | "jsx" => {
                let mut s = param.name.clone();
                if let Some(t) = &param.param_type {
                    s = format!("{}: {}", s, t);
                }
                if let Some(d) = &param.default {
                    s = format!("{} = {}", s, d);
                }
                s
            }
            _ => {
                if let Some(t) = &param.param_type {
                    format!("{} {}", t, param.name)
                } else {
                    param.name.clone()
                }
            }
        };
        parts.push(part);
    }

    parts.join(", ")
}

/// Build new argument list for a call site based on the parameter mapping.
fn build_new_args(
    old_args: &[String],
    mapping: &[Option<usize>],
    new_params: &[ParamSpec],
) -> Vec<String> {
    mapping
        .iter()
        .zip(new_params.iter())
        .map(|(old_idx, param)| {
            if let Some(idx) = old_idx {
                // Existing parameter: use the old argument
                old_args.get(*idx).cloned().unwrap_or_else(|| {
                    param
                        .default
                        .clone()
                        .unwrap_or_else(|| format!("/* {} */", param.name))
                })
            } else {
                // New parameter: use default value or placeholder
                param
                    .default
                    .clone()
                    .unwrap_or_else(|| format!("/* {} */", param.name))
            }
        })
        .collect()
}

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

fn split_args(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return vec![];
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_param_names_rust() {
        let names = parse_param_names("a: i32, b: String, c: &str", "rs");
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_param_names_python() {
        let names = parse_param_names("self, x, y: int, z=10", "py");
        assert_eq!(names, vec!["self", "x", "y", "z"]);
    }

    #[test]
    fn test_parse_param_names_go() {
        let names = parse_param_names("x int, y string", "go");
        assert_eq!(names, vec!["x", "y"]);
    }

    #[test]
    fn test_build_param_mapping() {
        let old = vec!["a", "b", "c"];
        let new_params = vec![
            ParamSpec {
                name: "c".into(),
                param_type: None,
                default: None,
            },
            ParamSpec {
                name: "a".into(),
                param_type: None,
                default: None,
            },
            ParamSpec {
                name: "d".into(),
                param_type: None,
                default: Some("0".into()),
            },
        ];
        let mapping = build_param_mapping(&old, &new_params);
        assert_eq!(mapping, vec![Some(2), Some(0), None]);
    }

    #[test]
    fn test_build_new_args() {
        let old_args = vec!["1".into(), "2".into(), "3".into()];
        let new_params = vec![
            ParamSpec {
                name: "c".into(),
                param_type: None,
                default: None,
            },
            ParamSpec {
                name: "a".into(),
                param_type: None,
                default: None,
            },
            ParamSpec {
                name: "d".into(),
                param_type: None,
                default: Some("0".into()),
            },
        ];
        let mapping = vec![Some(2), Some(0), None];
        let result = build_new_args(&old_args, &mapping, &new_params);
        assert_eq!(result, vec!["3", "1", "0"]);
    }

    #[test]
    fn test_build_new_param_string_rust() {
        let params = vec![
            ParamSpec {
                name: "x".into(),
                param_type: Some("i32".into()),
                default: None,
            },
            ParamSpec {
                name: "y".into(),
                param_type: Some("i32".into()),
                default: None,
            },
        ];
        let result = build_new_param_string(&params, "rs", "a: i32");
        assert_eq!(result, "x: i32, y: i32");
    }

    #[test]
    fn test_build_new_param_string_preserves_self() {
        let params = vec![ParamSpec {
            name: "x".into(),
            param_type: Some("i32".into()),
            default: None,
        }];
        let result = build_new_param_string(&params, "rs", "&self, a: i32");
        assert_eq!(result, "&self, x: i32");
    }

    #[test]
    fn test_build_new_param_string_python() {
        let params = vec![
            ParamSpec {
                name: "x".into(),
                param_type: Some("int".into()),
                default: None,
            },
            ParamSpec {
                name: "y".into(),
                param_type: None,
                default: Some("0".into()),
            },
        ];
        let result = build_new_param_string(&params, "py", "a, b");
        assert_eq!(result, "x: int, y = 0");
    }
}
