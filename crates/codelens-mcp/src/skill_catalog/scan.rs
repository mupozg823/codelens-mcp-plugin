use super::metadata::read_skill_metadata;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

const MAX_VISITED_DIRS: usize = 20_000;
const MAX_SKILL_FILES: usize = 10_000;

pub(super) fn summarize_skill_root(root: &Path, sample_limit: usize) -> Value {
    let scan = scan_skill_root(root, sample_limit);
    json!({
        "path": root.to_string_lossy(),
        "exists": root.is_dir(),
        "skill_count": scan.skill_count,
        "sample_skill_paths": scan.sample_paths,
        "sample_skills": scan.sample_skills,
        "visited_dir_count": scan.visited_dir_count,
        "truncated": scan.truncated,
    })
}

pub(super) fn collect_skill_paths(root: &Path) -> Vec<PathBuf> {
    scan_skill_root(root, usize::MAX).paths
}

struct SkillRootScan {
    paths: Vec<PathBuf>,
    skill_count: usize,
    sample_paths: Vec<String>,
    sample_skills: Vec<Value>,
    visited_dir_count: usize,
    truncated: bool,
}

fn scan_skill_root(root: &Path, sample_limit: usize) -> SkillRootScan {
    if !root.is_dir() {
        return SkillRootScan {
            paths: Vec::new(),
            skill_count: 0,
            sample_paths: Vec::new(),
            sample_skills: Vec::new(),
            visited_dir_count: 0,
            truncated: false,
        };
    }

    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut paths = Vec::new();
    let mut visited_dir_count = 0usize;
    let mut sample_paths = Vec::new();
    let mut sample_skills = Vec::new();
    let mut truncated = false;

    while let Some(dir) = queue.pop_front() {
        if visited_dir_count >= MAX_VISITED_DIRS || paths.len() >= MAX_SKILL_FILES {
            truncated = true;
            break;
        }
        visited_dir_count += 1;

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                queue.push_back(path);
                continue;
            }
            if file_type.is_file() && entry.file_name() == "SKILL.md" {
                record_skill(
                    root,
                    path,
                    sample_limit,
                    &mut paths,
                    &mut sample_paths,
                    &mut sample_skills,
                );
            }
        }
    }

    SkillRootScan {
        skill_count: paths.len(),
        paths,
        sample_paths,
        sample_skills,
        visited_dir_count,
        truncated,
    }
}

fn record_skill(
    root: &Path,
    path: PathBuf,
    sample_limit: usize,
    paths: &mut Vec<PathBuf>,
    sample_paths: &mut Vec<String>,
    sample_skills: &mut Vec<Value>,
) {
    if sample_paths.len() < sample_limit {
        sample_paths.push(path.to_string_lossy().to_string());
        if let Some(metadata) = read_skill_metadata(&path) {
            sample_skills.push(json!({
                "name": metadata.name,
                "path": path.to_string_lossy(),
                "source_root": root.to_string_lossy(),
                "description": metadata.description,
                "mtime_epoch_secs": metadata.mtime_epoch_secs,
                "content_hash": metadata.content_hash,
            }));
        }
    }
    paths.push(path);
}
