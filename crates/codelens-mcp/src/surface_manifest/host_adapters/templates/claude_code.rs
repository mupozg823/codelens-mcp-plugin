use super::super::overlays::{HOST_ROUTING_INVARIANTS, managed_host_policy_block};
use serde_json::{Value, json};

const HOST: &str = "claude-code";

fn routing_policy() -> String {
    managed_host_policy_block(&format!(
        r#"## CodeLens Routing

CodeLens is a code-evidence and analysis data plane. This host owns execution,
approval, and mutation; CodeLens owns the evidence those decisions rest on.

{HOST_ROUTING_INVARIANTS}

### Claude Code host facts

- Claude Code ships a native `LSP` tool (off by default; one locally installed
  language server per language). Use it for go-to-definition and diagnostics
  on a single open file when a server is present. Use CodeLens `search`,
  `graph` and `review` for cross-file references, impact and architecture, and
  for any session without a local language server (remote or cloud).
- Bind with the host's real capabilities so routing is not computed from
  defaults: `prepare_harness_session(project=<root>, host_context="claude-code",
  host_capabilities={{native_tool_search: true, native_subagents: true,
  native_worktrees: true, native_edit: true, dynamic_tool_list: true,
  approval_or_elicitation: true}})`. Only the always-loaded core is preloaded;
  every other CodeLens tool loads through the host's tool search on demand.

### Verify

- `codelens-mcp doctor claude-code` — checks the MCP config entry and this block.
- `codelens-mcp attach claude-code` — reprints the canonical block; re-sync after
  a CodeLens upgrade instead of hand-editing inside the markers.
- The project's own build, test, and lint commands remain the acceptance gate.
  CodeLens output is evidence, not a substitute for running them.
"#
    ))
}

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
                            "url": "http://127.0.0.1:7838/mcp"
                        }
                    }
                }
            },
            {
                "path": "CLAUDE.md",
                "format": "markdown",
                "purpose": "Carry the routing policy into Claude's project instructions.",
                "template": routing_policy()
            }
        ]
    })
}
