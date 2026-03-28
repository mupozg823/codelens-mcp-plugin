use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct CouplingEntry {
    pub file_a: String,
    pub file_b: String,
    pub co_changes: usize,
    pub total_changes_a: usize,
    pub total_changes_b: usize,
    pub strength: f64,
}

/// Analyze git history to find files that frequently change together.
pub fn get_change_coupling(
    project: &ProjectRoot,
    months: usize,
    min_strength: f64,
    min_commits: usize,
    max_results: usize,
) -> Result<Vec<CouplingEntry>> {
    let since = format!("{months} months ago");
    let output = Command::new("git")
        .args([
            "log",
            "--name-only",
            "--pretty=format:---COMMIT---",
            &format!("--since={since}"),
        ])
        .current_dir(project.as_path())
        .output();

    let output = match output {
        Ok(out) => out,
        Err(_) => return Ok(Vec::new()),
    };

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let commits = parse_commits(&text);

    // Count individual file changes and co-changes
    let mut total_changes: HashMap<String, usize> = HashMap::new();
    let mut co_changes: HashMap<(String, String), usize> = HashMap::new();

    for files in &commits {
        for file in files {
            *total_changes.entry(file.clone()).or_insert(0) += 1;
        }
        // For all pairs in this commit (sorted to avoid double-counting)
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                let a = files[i].clone();
                let b = files[j].clone();
                let key = if a <= b { (a, b) } else { (b, a) };
                *co_changes.entry(key).or_insert(0) += 1;
            }
        }
    }

    let mut entries: Vec<CouplingEntry> = co_changes
        .into_iter()
        .filter_map(|((file_a, file_b), count)| {
            if count < min_commits {
                return None;
            }
            let total_a = *total_changes.get(&file_a).unwrap_or(&1);
            let total_b = *total_changes.get(&file_b).unwrap_or(&1);
            let strength = count as f64 / total_a.max(total_b) as f64;
            if strength < min_strength {
                return None;
            }
            Some(CouplingEntry {
                file_a,
                file_b,
                co_changes: count,
                total_changes_a: total_a,
                total_changes_b: total_b,
                strength,
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.strength
            .total_cmp(&a.strength)
            .then(a.file_a.cmp(&b.file_a))
    });

    if max_results > 0 && entries.len() > max_results {
        entries.truncate(max_results);
    }

    Ok(entries)
}

fn parse_commits(text: &str) -> Vec<Vec<String>> {
    let mut commits = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "---COMMIT---" {
            if !current.is_empty() {
                commits.push(std::mem::take(&mut current));
            }
        } else if !trimmed.is_empty() {
            current.push(trimmed.to_owned());
        }
    }
    if !current.is_empty() {
        commits.push(current);
    }

    commits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commits_correctly() {
        let text = "---COMMIT---\nfoo.rs\nbar.rs\n---COMMIT---\nfoo.rs\nbaz.rs\n";
        let commits = parse_commits(text);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0], vec!["foo.rs", "bar.rs"]);
        assert_eq!(commits[1], vec!["foo.rs", "baz.rs"]);
    }

    #[test]
    fn empty_git_output_returns_empty() {
        let commits = parse_commits("");
        assert!(commits.is_empty());
    }

    #[test]
    fn calculates_coupling_from_parsed_commits() {
        // Simulate what get_change_coupling would calculate
        let commits = vec![
            vec!["a.rs".to_owned(), "b.rs".to_owned()],
            vec!["a.rs".to_owned(), "b.rs".to_owned()],
            vec!["a.rs".to_owned(), "b.rs".to_owned()],
            vec!["a.rs".to_owned(), "c.rs".to_owned()],
        ];

        let mut total_changes: HashMap<String, usize> = HashMap::new();
        let mut co_changes: HashMap<(String, String), usize> = HashMap::new();

        for files in &commits {
            for file in files {
                *total_changes.entry(file.clone()).or_insert(0) += 1;
            }
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let a = files[i].clone();
                    let b = files[j].clone();
                    let key = if a <= b { (a, b) } else { (b, a) };
                    *co_changes.entry(key).or_insert(0) += 1;
                }
            }
        }

        let ab_count = co_changes[&("a.rs".to_owned(), "b.rs".to_owned())];
        assert_eq!(ab_count, 3);
        assert_eq!(total_changes["a.rs"], 4);
        let strength = ab_count as f64 / total_changes["a.rs"].max(total_changes["b.rs"]) as f64;
        assert!((strength - 0.75).abs() < 1e-9);
    }
}
