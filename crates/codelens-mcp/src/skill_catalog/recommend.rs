use super::metadata::read_skill_metadata;
use super::scan::collect_skill_paths;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(super) fn recommend_codex_skills_for_roots(
    roots: &[PathBuf],
    task: Option<&str>,
    file_path: Option<&str>,
    limit: usize,
) -> Vec<Value> {
    let terms = query_terms(task, file_path);
    if terms.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    for root in roots {
        for path in collect_skill_paths(root) {
            if let Some(candidate) = score_skill_candidate(root, &path, &terms) {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    candidates
        .into_iter()
        .take(limit)
        .map(SkillCandidate::into_json)
        .collect()
}

struct SkillCandidate {
    name: String,
    path: String,
    source_root: String,
    score: usize,
    matched_terms: Vec<String>,
    description: String,
    mtime_epoch_secs: u64,
    content_hash: String,
}

impl SkillCandidate {
    fn into_json(self) -> Value {
        json!({
            "name": self.name,
            "path": self.path,
            "source_root": self.source_root,
            "score": self.score,
            "matched_terms": self.matched_terms,
            "description": self.description,
            "mtime_epoch_secs": self.mtime_epoch_secs,
            "content_hash": self.content_hash,
            "load_policy": "read this SKILL.md before acting only if selected",
        })
    }
}

fn score_skill_candidate(root: &Path, path: &Path, terms: &[String]) -> Option<SkillCandidate> {
    let metadata = read_skill_metadata(path)?;
    let haystack_name = metadata.name.to_ascii_lowercase();
    let haystack_description = metadata.description.to_ascii_lowercase();
    let haystack_path = path.to_string_lossy().to_ascii_lowercase();
    let mut score = 0usize;
    let mut matched_terms = Vec::new();

    for term in terms {
        let mut matched = false;
        if haystack_name.contains(term) {
            score += 5;
            matched = true;
        }
        if haystack_description.contains(term) {
            score += 3;
            matched = true;
        }
        if haystack_path.contains(term) {
            score += 2;
            matched = true;
        }
        if matched && !matched_terms.iter().any(|existing| existing == term) {
            matched_terms.push(term.clone());
        }
    }

    (score > 0).then(|| SkillCandidate {
        name: metadata.name,
        path: path.to_string_lossy().to_string(),
        source_root: root.to_string_lossy().to_string(),
        score,
        matched_terms,
        description: metadata.description,
        mtime_epoch_secs: metadata.mtime_epoch_secs,
        content_hash: metadata.content_hash,
    })
}

fn query_terms(task: Option<&str>, file_path: Option<&str>) -> Vec<String> {
    let mut source = String::new();
    if let Some(task) = task {
        source.push_str(task);
        source.push(' ');
    }
    if let Some(file_path) = file_path {
        source.push_str(file_path);
        source.push(' ');
        if file_path.ends_with(".rs") || file_path.ends_with("Cargo.toml") {
            source.push_str(" rust cargo ");
        }
        if file_path.ends_with(".ts") || file_path.ends_with(".tsx") {
            source.push_str(" typescript frontend ");
        }
        if file_path.ends_with(".py") {
            source.push_str(" python ");
        }
    }
    let lower = source.to_ascii_lowercase();
    let mut terms = lower
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .filter(|term| term.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    push_alias_terms(&lower, &mut terms);
    terms.sort();
    terms.dedup();
    terms
}

fn push_alias_terms(lower: &str, terms: &mut Vec<String>) {
    for (needle, alias) in [
        ("러스트", "rust"),
        ("코덱스", "codex"),
        ("클로드", "claude"),
        ("엠씨피", "mcp"),
        ("스킬", "skill"),
        ("프론트", "frontend"),
    ] {
        if lower.contains(needle) && !terms.iter().any(|term| term == alias) {
            terms.push(alias.to_owned());
        }
    }
}
