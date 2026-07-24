use super::super::overlays::{HOST_ROUTING_INVARIANTS, managed_host_policy_block};
use serde_json::{Value, json};

const HOST: &str = "codex";

fn routing_policy() -> String {
    managed_host_policy_block(&format!(
        r#"## CodeLens Routing

CodeLens is a code-evidence and analysis data plane. This host owns execution,
approval, and mutation; CodeLens owns the evidence those decisions rest on.

{HOST_ROUTING_INVARIANTS}

### Verify

- `codelens-mcp doctor codex` — checks the MCP config entry and this block.
- `codelens-mcp attach codex` — reprints the canonical block; re-sync after a
  CodeLens upgrade instead of hand-editing inside the markers.
- The project's own build, test, and lint commands remain the acceptance gate.
  CodeLens output is evidence, not a substitute for running them.
- Skill inventory, when needed, comes from `codelens://host-adapters/codex/skill-catalog`;
  read only the SKILL.md files that shortlist selects.
"#
    ))
}

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
        "execution_rules": [
            "Treat suggested_next_calls as host-neutral follow-up or mutation intent; the host chooses the native executor.",
            "Preserve concrete suggested arguments and apply normal approval, preflight, and mutation gates before execution."
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
                "template": routing_policy()
            }
        ]
    })
}
