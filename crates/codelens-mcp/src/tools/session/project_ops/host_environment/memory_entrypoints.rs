use serde_json::{Value, json};
use std::path::Path;

const MAX_MEMORY_ENTRYPOINTS: usize = 8;

const MEMORY_ENTRYPOINT_CANDIDATES: &[(&str, &str, &str)] = &[
    (
        "memory_summary.md",
        "summary",
        "Start here for compressed long-term memory before reading larger memory files.",
    ),
    (
        "MEMORY.md",
        "registry",
        "Use this as the memory registry before scanning rollout or skill memory folders.",
    ),
    (
        "CLAUDE.md",
        "host_policy",
        "Use this for Claude-specific memory and policy carried by the host.",
    ),
    (
        "AGENTS.md",
        "host_policy",
        "Use this for Codex-style memory and routing policy carried by the host.",
    ),
    (
        "skills",
        "skills_dir",
        "Use this directory only after metadata or task matching indicates a relevant skill.",
    ),
    (
        "rollout_summaries",
        "rollout_summaries_dir",
        "Use this directory for exact prior-run evidence after the summary or registry points there.",
    ),
    (
        "extensions/ad_hoc/notes",
        "ad_hoc_notes_dir",
        "Use this directory for recent memory updates before broad memory scans.",
    ),
];

pub(super) fn memory_entrypoints(memory_roots: &[String]) -> Vec<Value> {
    let mut entrypoints = Vec::new();
    for root in memory_roots {
        let root_path = Path::new(root);
        for (relative_path, kind, reason) in MEMORY_ENTRYPOINT_CANDIDATES {
            let candidate = root_path.join(relative_path);
            if candidate.exists() {
                entrypoints.push(json!({
                    "root": root,
                    "path": candidate.to_string_lossy(),
                    "relative_path": relative_path,
                    "kind": kind,
                    "reason": reason,
                }));
            }
            if entrypoints.len() >= MAX_MEMORY_ENTRYPOINTS {
                return entrypoints;
            }
        }
    }
    entrypoints
}
