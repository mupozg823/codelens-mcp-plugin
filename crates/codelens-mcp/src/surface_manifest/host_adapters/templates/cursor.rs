use super::super::overlays::HOST_ROUTING_INVARIANTS;
use serde_json::{Value, json};

const HOST: &str = "cursor";

fn routing_policy() -> String {
    format!(
        r#"---
description: CodeLens routing invariants and verification
alwaysApply: true
---

## CodeLens Routing

CodeLens is a code-evidence and analysis data plane. This host owns execution,
approval, and mutation; CodeLens owns the evidence those decisions rest on.

{HOST_ROUTING_INVARIANTS}

### Verify

- `codelens-mcp doctor cursor` — checks the MCP config entry and this rule file.
- `codelens-mcp attach cursor` — reprints the canonical rule file; re-sync after
  a CodeLens upgrade instead of hand-editing it.
- The project's own build, test, and lint commands remain the acceptance gate.
  CodeLens output is evidence, not a substitute for running them.
- Background agents do not share the foreground trust boundary: assume a
  localhost daemon is unreachable there until a probe says otherwise.
"#
    )
}

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
                "template": routing_policy()
            }
        ]
    })
}
