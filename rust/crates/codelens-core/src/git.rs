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
    if include_untracked {
        if let Ok(output) = run_git(project, &["ls-files", "--others", "--exclude-standard"]) {
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
    }

    Ok(dedup_files(all_files))
}

pub fn get_diff_symbols(project: &ProjectRoot, git_ref: Option<&str>) -> Result<Vec<DiffSymbol>> {
    let changed = get_changed_files(project, git_ref, false)?;
    let result = changed
        .into_iter()
        .map(|cf| DiffSymbol {
            file: cf.file,
            status: cf.status,
            symbols: Vec::new(),
        })
        .collect();
    Ok(result)
}
