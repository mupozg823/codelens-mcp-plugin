use super::super::overlays::managed_host_policy_block;
use serde_json::{Value, json};

const HOST: &str = "codex";

pub(super) fn bundle() -> Value {
    json!({
        "name": HOST,
        "resource_uri": format!("codelens://host-adapters/{HOST}"),
        "best_fit": "builder and refactor execution, parallel worktree-based implementation, and automation",
        "recommended_modes": ["solo-local", "planner-builder", "batch-analysis"],
        "preferred_profiles": ["builder", "review"],
        "native_primitives": [
            "AGENTS.md",
            "skills",
            "worktrees",
            "shared MCP config",
            "CLI, app, and IDE continuity"
        ],
        "preferred_codelens_use": [
            "bounded mutation after verify_change_readiness",
            "session-scoped builder audit",
            "analysis jobs for CI-facing summaries"
        ],
        "routing_defaults": {
            "point_lookup": "native-first",
            "multi_file_build": "builder-after-bootstrap",
            "rename_or_broad_refactor": "builder-after-preflight",
            "ci_summary": "analysis-job-first"
        },
        "delegate_scaffold_rules": [
            "If the planner hands you `delegate_to_codex_builder`, replay delegate_tool plus delegate_arguments unchanged for the first builder-heavy call.",
            "Preserve handoff_id exactly so planner-side emission and builder-side execution stay correlatable."
        ],
        "skill_binding": crate::skill_catalog::codex_skill_binding_contract(),
        "avoid": [
            "forcing CodeLens into trivial single-file lookups",
            "copying Claude-specific subagent topology into Codex worktree flows"
        ],
        "compiler_targets": [
            "AGENTS.md",
            "~/.codex/config.toml",
            "repo-local skill files"
        ],
        "native_files": [
            {
                "path": "~/.codex/config.toml",
                "format": "toml",
                "purpose": "Share one CodeLens MCP attachment between the Codex CLI and IDE extension.",
                "template": r#"[mcp_servers.codelens]
url = "http://127.0.0.1:7838/mcp"
"#
            },
            {
                "path": "AGENTS.md",
                "format": "markdown",
                "purpose": "Tell Codex when to stay native and when to escalate into CodeLens workflow tools.",
                "template": managed_host_policy_block(r#"## CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- Main Codex sessions call `prepare_harness_session` with `agent_role="main"`; delegated worker sessions call it with `agent_role="subagent"` so routing favors bounded context, diagnostics, and evidence return.
- When available, pass host-observed `available_mcp_servers`, `available_mcp_tools`, `skill_roots`, `memory_roots`, `host_setting_keys`, and `harness_profile`; send names, paths, and key names only, never secret values.
- If `get_current_config.project_root` is not the intended workspace, call `prepare_harness_session` or `activate_project` with `project=<absolute repo path>` and continue with CodeLens; do not fall back to native tools solely because the active project was stale.
- Default execution profile: `builder`.
- Run `verify_change_readiness` before broad refactors; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
- If the planner hands you `delegate_to_codex_builder`, replay the first delegated builder call with `delegate_tool` + `delegate_arguments` unchanged, including `handoff_id`.
- For non-trivial tasks, let `prepare_harness_session` compile skill hints from observed `skill_roots`; if more inventory is needed, inspect `codelens://host-adapters/codex/skill-catalog`, then read only the selected `SKILL.md` files before acting.
"#)
            }
        ]
    })
}
