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
        "delegate_scaffold_rules": [
            "If CodeLens emits `delegate_to_codex_builder`, forward delegate_tool, delegate_arguments, carry_forward, and handoff_id to the builder lane.",
            "Do not regenerate builder arguments from prose when delegate_arguments are already present."
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
- If CodeLens emits `delegate_to_codex_builder`, pass `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` through to the builder lane instead of rewriting them from prose.
"#
            }
        ]
    })
}
