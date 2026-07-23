use super::super::overlays::managed_host_policy_block;
use serde_json::{Value, json};

const HOST: &str = "claude-code";

pub(super) fn bundle() -> Value {
    json!({
        "name": HOST,
        "resource_uri": format!("codelens://host-adapters/{HOST}"),
        "best_fit": "planner and reviewer orchestration with isolated research and explicit policy control",
        "recommended_modes": ["solo-local", "planner-builder", "reviewer-gate"],
        "preferred_profiles": ["readonly", "review"],
        "native_primitives": [
            "CLAUDE.md",
            "subagents and agent teams",
            "hooks",
            "managed-mcp.json and .mcp.json",
            "subagent-scoped MCP servers"
        ],
        "preferred_codelens_use": [
            "bootstrap and bounded architecture review",
            "preflight before dispatching a builder",
            "planner-session audit and handoff artifact production"
        ],
        "routing_defaults": {
            "point_lookup": "native-first",
            "multi_file_review": "codelens-after-first-local-step",
            "builder_dispatch": "planner-builder-handoff-required",
            "long_running_eval": "analysis-job-first"
        },
        "execution_rules": [
            "Treat suggested_next_calls as host-neutral follow-up or mutation intent; the host chooses the native executor.",
            "Preserve concrete suggested arguments and apply normal approval, preflight, and mutation gates before execution."
        ],
        "avoid": [
            "defaulting to live bidirectional chat between planner and builder",
            "exposing mutation-heavy surfaces to read-side sessions"
        ],
        "compiler_targets": [
            "CLAUDE.md",
            ".mcp.json",
            "managed-mcp.json",
            "subagent definitions"
        ],
        "native_files": [
            {
                "path": ".mcp.json",
                "format": "json",
                "purpose": "Attach the read-only CodeLens daemon to the project by default.",
                "template": {
                    "mcpServers": {
                        "codelens": {
                            "type": "http",
                            "url": "http://127.0.0.1:7837/mcp"
                        }
                    }
                }
            },
            {
                "path": "CLAUDE.md",
                "format": "markdown",
                "purpose": "Carry the routing policy into Claude's project instructions.",
                "template": managed_host_policy_block(r#"## CodeLens Routing

- Use native Read/Glob/Grep first for trivial point lookups and single-file edits.
- Escalate to CodeLens after the first local step for multi-file review, refactor preflight, or durable artifact generation.
- Default CodeLens profile for planning/review is `review`.
- Main sessions call `prepare_harness_session` with `agent_role="main"`; delegated research/build workers call it with `agent_role="subagent"` and a narrow task overlay.
- If the host can observe orchestration capabilities, MCP server/tool names, memory roots, or subagent-scoped MCP config, pass only those facts/names/roots as `host_capabilities`, `available_mcp_servers`, `available_mcp_tools`, `memory_roots`, and `host_setting_keys`; never pass secret values.
- Before dispatching a builder, run:
  1. `prepare_harness_session`
  2. `get_symbols_overview` per target file
  3. `get_file_diagnostics` per target file
  4. `verify_change_readiness`
- Prefer asymmetric handoff over live planner/builder chat.
- Treat `suggested_next_calls` as host-neutral follow-up or mutation intent; choose the native executor in the host and preserve concrete arguments through normal approval and mutation gates.
"#)
            }
        ]
    })
}
