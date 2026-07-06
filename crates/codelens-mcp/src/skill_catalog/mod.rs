mod metadata;
mod recommend;
mod scan;
#[cfg(test)]
mod tests;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const CODEX_SKILL_CATALOG_SCHEMA_VERSION: &str = "codelens-codex-skill-catalog-v1";
const DEFAULT_SAMPLE_LIMIT: usize = 24;

const DEFAULT_CODEX_SKILL_ROOTS: &[&str] =
    &[".codex/skills", ".agents/skills", ".codex/plugins/cache"];

pub(crate) const CODEX_SKILL_CATALOG_RESOURCE_URI: &str =
    "codelens://host-adapters/codex/skill-catalog";

pub(crate) fn codex_skill_binding_contract() -> Value {
    json!({
        "schema_version": CODEX_SKILL_CATALOG_SCHEMA_VERSION,
        "resource_uri": CODEX_SKILL_CATALOG_RESOURCE_URI,
        "target_host": "codex",
        "purpose": "Bind Codex to the user's installed skills without injecting every SKILL.md into the startup prompt.",
        "default_roots": DEFAULT_CODEX_SKILL_ROOTS
            .iter()
            .map(|root| format!("$HOME/{root}"))
            .collect::<Vec<_>>(),
        "index_policy": {
            "bootstrap": "metadata_only",
            "metadata_fields": ["name", "description", "path", "source_root", "mtime_epoch_secs", "content_hash"],
            "body_loading": "load only the selected SKILL.md after task matching",
            "reference_loading": "follow the selected skill's own progressive-disclosure instructions"
        },
        "binding_policy": [
            "Use the runtime skill catalog to shortlist 1-3 candidate skills for non-trivial Codex tasks.",
            "Prefer repo-local and user-authored skills over broad plugin-cache skills when confidence is similar.",
            "Do not inject full skill bodies into AGENTS.md or prepare_harness_session; emit compact hints and paths.",
            "Treat strict enforcement as opt-in; default Codex binding is advisory."
        ],
        "codex_native_targets": ["AGENTS.md", "~/.codex/config.toml", "repo-local skill files"],
    })
}

pub(crate) fn codex_skill_catalog_resource() -> Value {
    let roots = codex_default_skill_roots();
    codex_skill_catalog_for_roots(&roots, DEFAULT_SAMPLE_LIMIT)
}

pub(crate) fn codex_prepare_skill_hints(task: Option<&str>, file_path: Option<&str>) -> Value {
    let roots = codex_default_skill_roots();
    codex_prepare_skill_hints_for_roots(task, file_path, &roots)
}

pub(crate) fn codex_prepare_skill_hints_for_roots(
    task: Option<&str>,
    file_path: Option<&str>,
    roots: &[PathBuf],
) -> Value {
    let catalog = codex_skill_catalog_for_roots(roots, DEFAULT_SAMPLE_LIMIT);
    let compact_roots = catalog
        .get("roots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|root| {
            json!({
                "path": root.get("path").cloned().unwrap_or(Value::Null),
                "exists": root.get("exists").cloned().unwrap_or(Value::Null),
                "skill_count": root.get("skill_count").cloned().unwrap_or(Value::Null),
                "truncated": root.get("truncated").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();

    json!({
        "target_host": "codex",
        "catalog_resource": CODEX_SKILL_CATALOG_RESOURCE_URI,
        "total_skill_count": catalog.get("total_skill_count").cloned().unwrap_or(Value::Null),
        "roots": compact_roots,
        "selection_limit": 3,
        "load_policy": "shortlist from metadata first, then read only selected SKILL.md files",
        "candidate_skills": recommend::recommend_codex_skills_for_roots(
            roots,
            task,
            file_path,
            3,
        ),
        "recommended_sequence": [
            "Read the runtime skill catalog only for non-trivial tasks.",
            "Match task, files, language, and repo instructions against skill metadata.",
            "Load at most the selected SKILL.md files before acting."
        ],
    })
}

fn codex_skill_catalog_for_roots(roots: &[PathBuf], sample_limit: usize) -> Value {
    let root_summaries = roots
        .iter()
        .map(|root| scan::summarize_skill_root(root, sample_limit))
        .collect::<Vec<_>>();
    let total_skill_count = root_summaries
        .iter()
        .filter_map(|summary| summary.get("skill_count").and_then(Value::as_u64))
        .sum::<u64>();
    let truncated = root_summaries.iter().any(|summary| {
        summary
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    });

    json!({
        "schema_version": CODEX_SKILL_CATALOG_SCHEMA_VERSION,
        "resource_uri": CODEX_SKILL_CATALOG_RESOURCE_URI,
        "target_host": "codex",
        "scan_policy": "metadata path scan only; SKILL.md bodies are not loaded by this resource",
        "roots": root_summaries,
        "total_skill_count": total_skill_count,
        "truncated": truncated,
        "next_step": "Use these locations to build a task-specific skill shortlist, then read only the selected SKILL.md files before acting.",
    })
}

pub(crate) fn codex_default_skill_roots() -> Vec<PathBuf> {
    match std::env::var_os("HOME") {
        Some(home) => discover_codex_skill_roots_from_home(Path::new(&home)),
        None => Vec::new(),
    }
}

fn discover_codex_skill_roots_from_home(home: &Path) -> Vec<PathBuf> {
    DEFAULT_CODEX_SKILL_ROOTS
        .iter()
        .map(|relative| home.join(relative))
        .collect()
}
