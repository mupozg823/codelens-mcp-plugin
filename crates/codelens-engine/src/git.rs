use crate::project::ProjectRoot;
use anyhow::{Result, bail};
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct ChangedFile {
    pub file: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffSymbol {
    pub file: String,
    pub status: String,
    pub symbols: Vec<DiffSymbolEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffSymbolEntry {
    pub name: String,
    pub kind: String,
    pub line: usize,
}

fn run_git(project: &ProjectRoot, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project.as_path())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") || stderr.contains("fatal:") {
            bail!("not a git repository: {}", project.as_path().display());
        }
        bail!("git error: {}", stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_name_status(output: &str) -> Vec<ChangedFile> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let status = parts.next()?.trim().to_owned();
            let file = parts.next()?.trim().to_owned();
            if status.is_empty() || file.is_empty() {
                return None;
            }
            // For renames (R100\told\tnew), take just the status prefix letter
            let status_char = status.chars().next()?.to_string();
            Some(ChangedFile {
                file,
                status: status_char,
            })
        })
        .collect()
}

fn dedup_files(files: Vec<ChangedFile>) -> Vec<ChangedFile> {
    let mut seen = std::collections::HashSet::new();
    files
        .into_iter()
        .filter(|f| seen.insert(f.file.clone()))
        .collect()
}

pub fn get_changed_files(
    project: &ProjectRoot,
    git_ref: Option<&str>,
    include_untracked: bool,
) -> Result<Vec<ChangedFile>> {
    // Verify it's a git repo first
    run_git(project, &["rev-parse", "--git-dir"])?;

    let ref_target = git_ref.unwrap_or("HEAD");
    let mut all_files: Vec<ChangedFile> = Vec::new();

    // Files changed relative to git_ref (committed diff)
    match run_git(project, &["diff", "--name-status", ref_target]) {
        Ok(output) => all_files.extend(parse_name_status(&output)),
        Err(e) => {
            // If HEAD doesn't exist yet (empty repo), ignore
            let msg = e.to_string();
            if !msg.contains("unknown revision") && !msg.contains("ambiguous argument") {
                return Err(e);
            }
        }
    }

    // Unstaged changes (working tree vs index)
    if let Ok(output) = run_git(project, &["diff", "--name-status"]) {
        all_files.extend(parse_name_status(&output));
    }

    // Staged changes (index vs HEAD)
    if let Ok(output) = run_git(project, &["diff", "--name-status", "--cached"]) {
        all_files.extend(parse_name_status(&output));
    }

    // Untracked files
    if include_untracked
        && let Ok(output) = run_git(project, &["ls-files", "--others", "--exclude-standard"])
    {
        for line in output.lines() {
            let file = line.trim().to_owned();
            if !file.is_empty() {
                all_files.push(ChangedFile {
                    file,
                    status: "?".to_owned(),
                });
            }
        }
    }

    Ok(dedup_files(all_files))
}

/// Check whether the diff for a single file is additive-only (no deleted lines).
/// Returns `"additive"` if the file has 0 deleted lines (new exports, new code),
/// `"breaking"` if it was deleted, or `"mixed"` otherwise.
pub fn classify_change_kind(project: &ProjectRoot, file_path: &str) -> String {
    // New/untracked files are always additive
    let status = run_git(project, &["status", "--porcelain", "--", file_path]).unwrap_or_default();
    let status_char = status.trim().chars().next().unwrap_or('M');
    if status_char == '?' || status_char == 'A' {
        return "additive".to_owned();
    }
    if status_char == 'D' {
        return "breaking".to_owned();
    }
    // For modified files: check numstat (additions/deletions)
    let numstat =
        run_git(project, &["diff", "--numstat", "HEAD", "--", file_path]).unwrap_or_default();
    if let Some(line) = numstat.lines().next() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            let deletions: u64 = parts[1].parse().unwrap_or(1);
            if deletions == 0 {
                return "additive".to_owned();
            }
        }
    }
    "mixed".to_owned()
}

pub fn get_diff_symbols(project: &ProjectRoot, git_ref: Option<&str>) -> Result<Vec<DiffSymbol>> {
    use crate::symbols::{SymbolKind, get_symbols_overview};

    let changed = get_changed_files(project, git_ref, false)?;
    let mut result = Vec::new();

    for cf in changed {
        // Skip deleted files — no symbols to parse
        if cf.status == "D" {
            result.push(DiffSymbol {
                file: cf.file,
                status: cf.status,
                symbols: Vec::new(),
            });
            continue;
        }

        // Parse symbols from the changed file
        let symbols = match get_symbols_overview(project, &cf.file, 2) {
            Ok(syms) => syms
                .into_iter()
                .filter(|s| !matches!(s.kind, SymbolKind::File | SymbolKind::Variable))
                .map(|s| DiffSymbolEntry {
                    name: s.name,
                    kind: s.kind.as_label().to_owned(),
                    line: s.line,
                })
                .collect(),
            Err(_) => Vec::new(),
        };

        result.push(DiffSymbol {
            file: cf.file,
            status: cf.status,
            symbols,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_name_status_basic() {
        let output = "M\tsrc/main.py\nA\tsrc/utils.py\nD\told.py\n";
        let files = parse_name_status(output);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].file, "src/main.py");
        assert_eq!(files[0].status, "M");
        assert_eq!(files[1].status, "A");
        assert_eq!(files[2].status, "D");
    }

    #[test]
    fn parse_name_status_rename() {
        let output = "R100\told_name.py\n";
        let files = parse_name_status(output);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, "R");
        assert_eq!(files[0].file, "old_name.py");
    }

    #[test]
    fn parse_name_status_empty() {
        assert!(parse_name_status("").is_empty());
        assert!(parse_name_status("\n\n").is_empty());
    }

    fn git_init_with_file(dir: &std::path::Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
        std::process::Command::new("git")
            .args(["add", name])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "init", "--allow-empty-message"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    #[test]
    fn classify_change_kind_additive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(dir)
            .output()
            .unwrap();
        git_init_with_file(dir, "lib.py", "def hello(): pass\n");
        // Append-only change → additive
        std::fs::write(dir.join("lib.py"), "def hello(): pass\ndef world(): pass\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        assert_eq!(classify_change_kind(&project, "lib.py"), "additive");
    }

    #[test]
    fn classify_change_kind_mixed() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(dir)
            .output()
            .unwrap();
        git_init_with_file(dir, "lib.py", "def hello(): pass\n");
        // Replace line → mixed (has deletions)
        std::fs::write(dir.join("lib.py"), "def goodbye(): pass\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        assert_eq!(classify_change_kind(&project, "lib.py"), "mixed");
    }

    #[test]
    fn classify_change_kind_untracked() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        // Untracked file → additive
        std::fs::write(dir.join("new.py"), "x = 1\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        assert_eq!(classify_change_kind(&project, "new.py"), "additive");
    }

    #[test]
    fn dedup_files_removes_duplicates() {
        let files = vec![
            ChangedFile {
                file: "a.py".into(),
                status: "M".into(),
            },
            ChangedFile {
                file: "b.py".into(),
                status: "A".into(),
            },
            ChangedFile {
                file: "a.py".into(),
                status: "D".into(),
            },
        ];
        let deduped = dedup_files(files);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].file, "a.py");
        assert_eq!(deduped[0].status, "M"); // first occurrence kept
        assert_eq!(deduped[1].file, "b.py");
    }
}
