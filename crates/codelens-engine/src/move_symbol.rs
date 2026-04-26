use crate::edit_transaction::{apply_full_writes_with_evidence, ApplyEvidence};
use crate::project::ProjectRoot;
use crate::rename::{apply_edits, find_all_word_matches, RenameEdit};
use crate::symbols::{find_symbol, find_symbol_range};
use anyhow::{bail, Result};
use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, Serialize)]
pub struct MoveResult {
    pub success: bool,
    pub message: String,
    pub source_file: String,
    pub target_file: String,
    pub symbol_name: String,
    pub import_updates: usize,
    pub edits: Vec<MoveEdit>,
    /// G7b — `ApplyEvidence` for the source+target atomic transaction.
    /// `None` on dry runs (no on-disk write attempted) and on the
    /// pre-substrate validation failures that bail before the
    /// transaction begins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_evidence: Option<ApplyEvidence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MoveEdit {
    pub file_path: String,
    pub action: MoveAction,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveAction {
    RemoveFromSource,
    AddToTarget,
    UpdateImport,
}

/// Move a symbol from one file to another, updating imports across the project.
pub fn move_symbol(
    project: &ProjectRoot,
    file_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    target_file: &str,
    dry_run: bool,
) -> Result<MoveResult> {
    if file_path == target_file {
        bail!("Source and target files are the same");
    }

    // 1. Find the symbol
    let symbols = find_symbol(project, symbol_name, Some(file_path), true, true, 1)?;
    let _sym = symbols
        .first()
        .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in '{}'", symbol_name, file_path))?;

    // 2. Extract the full symbol text
    let resolved_source = project.resolve(file_path)?;
    let source_content = fs::read_to_string(&resolved_source)?;
    let (start_byte, end_byte) = find_symbol_range(project, file_path, symbol_name, name_path)?;
    let symbol_text = source_content[start_byte..end_byte].to_string();

    // 3. Determine the line range of the symbol for removal
    let start_line = source_content[..start_byte].lines().count();
    let end_line = source_content[..end_byte].lines().count();

    // 4. Build edits
    let mut edits = Vec::new();

    // Edit 1: Remove from source
    edits.push(MoveEdit {
        file_path: file_path.to_string(),
        action: MoveAction::RemoveFromSource,
        content: symbol_text.clone(),
    });

    // Edit 2: Add to target
    edits.push(MoveEdit {
        file_path: target_file.to_string(),
        action: MoveAction::AddToTarget,
        content: symbol_text.clone(),
    });

    // 5. Find import references to update
    let matches = find_all_word_matches(project, symbol_name)?;
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let source_module = file_path_to_module(file_path, ext);
    let target_module = file_path_to_module(target_file, ext);

    let mut import_edits: Vec<RenameEdit> = Vec::new();

    for (ref_file, line, _col) in &matches {
        if ref_file == file_path || ref_file == target_file {
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

        // Check if this line is an import/from statement referencing the source module
        if is_import_line(line_text, &source_module, ext) {
            let new_line = line_text.replace(&source_module, &target_module);
            if new_line != line_text {
                import_edits.push(RenameEdit {
                    file_path: ref_file.clone(),
                    line: *line,
                    column: 1,
                    old_text: line_text.to_string(),
                    new_text: new_line,
                });

                edits.push(MoveEdit {
                    file_path: ref_file.clone(),
                    action: MoveAction::UpdateImport,
                    content: format!("{} → {}", source_module, target_module),
                });
            }
        }
    }

    let import_updates = edits
        .iter()
        .filter(|e| matches!(e.action, MoveAction::UpdateImport))
        .count();

    let mut result = MoveResult {
        success: true,
        message: format!(
            "Moved '{}' from '{}' to '{}', updated {} import(s)",
            symbol_name, file_path, target_file, import_updates
        ),
        source_file: file_path.to_string(),
        target_file: target_file.to_string(),
        symbol_name: symbol_name.to_string(),
        import_updates,
        edits,
        apply_evidence: None,
    };

    if !dry_run {
        // Compute the post-move source content (symbol removed).
        let source_lines: Vec<String> = source_content.lines().map(String::from).collect();
        let start_idx = if start_line > 0 { start_line - 1 } else { 0 };
        let end_idx = end_line.min(source_lines.len());
        let mut new_lines: Vec<String> = Vec::new();
        for (i, line) in source_lines.iter().enumerate() {
            if i < start_idx || i >= end_idx {
                new_lines.push(line.clone());
            }
        }
        // Remove trailing blank line if the symbol was followed by one
        if start_idx > 0
            && start_idx < new_lines.len()
            && new_lines[start_idx].trim().is_empty()
            && (start_idx == 0 || new_lines[start_idx - 1].trim().is_empty())
        {
            new_lines.remove(start_idx);
        }
        let mut new_source = new_lines.join("\n");
        if source_content.ends_with('\n') {
            new_source.push('\n');
        }

        // Compute the post-move target content (symbol appended).
        let resolved_target = project.resolve(target_file)?;
        let mut target_content = if resolved_target.exists() {
            fs::read_to_string(&resolved_target)?
        } else {
            String::new()
        };
        if !target_content.is_empty() && !target_content.ends_with('\n') {
            target_content.push('\n');
        }
        if !target_content.is_empty() {
            target_content.push('\n');
        }
        target_content.push_str(&symbol_text);
        target_content.push('\n');

        // G7b — atomic 2-file substrate. If the target write fails, the
        // source write is rolled back to its pre-move bytes so the
        // symbol never disappears mid-flight.
        let writes: Vec<(&str, &str)> = vec![
            (file_path, new_source.as_str()),
            (target_file, target_content.as_str()),
        ];
        match apply_full_writes_with_evidence(project, &writes) {
            Ok(evidence) => {
                result.apply_evidence = Some(evidence);
            }
            Err(err) => {
                // Surface the substrate's evidence on rollback so callers
                // can confirm the disk state is back to its pre-move
                // shape. Other errors (PreReadFailed,
                // PreApplyHashMismatch) propagate as-is.
                if let crate::edit_transaction::ApplyError::ApplyFailed { evidence, source } = err {
                    result.apply_evidence = Some(evidence);
                    result.success = false;
                    result.message = format!(
                        "move '{symbol_name}' failed mid-write: {source}; source+target rolled back"
                    );
                    return Ok(result);
                }
                return Err(anyhow::anyhow!(err));
            }
        }

        // Update imports across the project. These edits operate on
        // files outside the source/target pair so a failure here does
        // not undo the move itself; the import-update step is best-
        // effort the same way it was pre-G7b.
        if !import_edits.is_empty() {
            apply_edits(project, &import_edits)?;
        }
    }

    Ok(result)
}

/// Convert a file path to a module path based on language conventions.
fn file_path_to_module(path: &str, ext: &str) -> String {
    let without_ext = path.strip_suffix(&format!(".{}", ext)).unwrap_or(path);

    match ext {
        "py" => without_ext.replace(['/', '\\'], "."),
        "js" | "ts" | "tsx" | "jsx" => {
            let clean = without_ext.strip_suffix("/index").unwrap_or(without_ext);
            format!("./{}", clean)
        }
        "go" => {
            // Go uses directory-based packages
            std::path::Path::new(without_ext)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string())
        }
        "java" | "kt" | "scala" => without_ext.replace(['/', '\\'], "."),
        _ => without_ext.replace(['/', '\\'], "."),
    }
}

/// Check if a line is an import statement referencing the given module.
fn is_import_line(line: &str, module: &str, ext: &str) -> bool {
    let trimmed = line.trim();
    match ext {
        "py" => {
            (trimmed.starts_with("from ") || trimmed.starts_with("import "))
                && trimmed.contains(module)
        }
        "js" | "ts" | "tsx" | "jsx" => {
            (trimmed.starts_with("import ") || trimmed.contains("require("))
                && trimmed.contains(module)
        }
        "go" => trimmed.contains(module) && trimmed.contains('"'),
        "java" | "kt" | "scala" => trimmed.starts_with("import ") && trimmed.contains(module),
        "rs" => trimmed.starts_with("use ") && trimmed.contains(module),
        _ => trimmed.starts_with("import ") && trimmed.contains(module),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;
    use std::fs;

    fn make_fixture() -> (std::path::PathBuf, ProjectRoot) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-move-fixture-{}",
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
    fn test_file_path_to_module_python() {
        assert_eq!(
            file_path_to_module("utils/helpers.py", "py"),
            "utils.helpers"
        );
    }

    #[test]
    fn test_file_path_to_module_js() {
        assert_eq!(
            file_path_to_module("utils/helpers.js", "js"),
            "./utils/helpers"
        );
    }

    #[test]
    fn test_is_import_line_python() {
        assert!(is_import_line(
            "from utils.helpers import foo",
            "utils.helpers",
            "py"
        ));
        assert!(!is_import_line("x = helpers.foo()", "utils.helpers", "py"));
    }

    #[test]
    fn test_is_import_line_js() {
        assert!(is_import_line(
            "import { foo } from './utils/helpers';",
            "./utils/helpers",
            "js"
        ));
    }

    #[test]
    fn test_same_file_error() {
        let (_dir, project) = make_fixture();
        let result = move_symbol(&project, "a.py", "foo", None, "a.py", true);
        assert!(result.is_err());
    }

    #[test]
    fn test_move_dry_run() {
        let (dir, project) = make_fixture();

        let source = "def foo():\n    return 42\n\ndef bar():\n    return foo()\n";
        fs::write(dir.join("source.py"), source).unwrap();
        fs::write(dir.join("target.py"), "# target\n").unwrap();

        let result = move_symbol(&project, "source.py", "foo", None, "target.py", true).unwrap();
        assert!(result.success);
        assert_eq!(result.symbol_name, "foo");

        // Dry run: files unchanged
        let after = fs::read_to_string(dir.join("source.py")).unwrap();
        assert_eq!(after, source);

        fs::remove_dir_all(&dir).ok();
    }
}
