use serde_json::{Value, json};

const HOST: &str = "cursor";

pub(super) fn bundle() -> Value {
    json!({
        "name": HOST,
        "resource_uri": format!("codelens://host-adapters/{HOST}"),
        "best_fit": "editor-local iteration with scoped rules plus asynchronous remote execution when needed",
        "recommended_modes": ["solo-local", "reviewer-gate", "batch-analysis"],
        "preferred_profiles": ["readonly", "review"],
        "native_primitives": [
            ".cursor/rules",
            "AGENTS.md",
            "custom modes",
            "background agents",
            "mcp.json"
        ],
        "preferred_codelens_use": [
            "architecture review and diff-aware signoff",
            "analysis jobs for background-agent queues",
            "minimal surface exposure through mode- or rule-specific routing"
        ],
        "routing_defaults": {
            "foreground_lookup": "native-first",
            "foreground_review": "codelens-after-first-local-step",
            "background_queue": "analysis-job-first",
            "wide_surface": "deferred-loading-required"
        },
        "execution_rules": [
            "Treat suggested_next_calls as host-neutral follow-up or mutation intent; the host chooses the native executor.",
            "Preserve concrete suggested arguments and apply normal approval, preflight, and mutation gates before execution."
        ],
        "avoid": [
            "assuming foreground and background agents share the same trust boundary",
            "shipping the full CodeLens surface into every mode"
        ],
        "compiler_targets": [
            ".cursor/rules",
            "AGENTS.md",
            ".cursor/mcp.json",
            "background-agent environment.json"
        ],
        "native_files": [
            {
                "path": ".cursor/mcp.json",
                "format": "json",
                "purpose": "Attach CodeLens to Cursor with the smallest stable project-local config.",
                "template": {
                    "mcpServers": {
                        "codelens": {
                            "type": "http",
                            "url": "http://127.0.0.1:7838/mcp"
                        }
                    }
                }
            },
            {
                "path": ".cursor/rules/codelens-routing.mdc",
                "format": "mdc",
                "purpose": "Scope CodeLens to review-heavy and artifact-worthy tasks instead of every edit.",
                "template": r#"---
description: Route CodeLens usage by task risk and phase
alwaysApply: true
---

- Use native code search and local file reads first for trivial lookups and single-file edits.
- Escalate to CodeLens when the task becomes multi-file, reviewer-heavy, refactor-sensitive, or needs durable analysis artifacts.
- Prefer `review` for review/signoff and durable async analysis summaries.
- In background-agent flows, assume localhost CodeLens is unavailable unless the daemon is reachable from the remote machine.
- Pass `host_capabilities` only when the host can report concrete native tool-search, subagent, worktree, edit, task, dynamic-tool, workspace-binding, or approval support.
- Treat `suggested_next_calls` as host-neutral follow-up or mutation intent; choose the native executor in the host and preserve concrete arguments through normal approval and mutation gates.
"#
            }
        ]
    })
}
