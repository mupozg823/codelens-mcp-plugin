use super::super::overlays::{append_compiled_overlay_section, managed_host_policy_block};
use serde_json::{Value, json};

const HOST: &str = "cline";

pub(super) fn bundle() -> Value {
    json!({
        "name": HOST,
        "resource_uri": format!("codelens://host-adapters/{HOST}"),
        "best_fit": "human-in-the-loop debugging and foreground execution with explicit approvals",
        "recommended_modes": ["solo-local", "planner-builder"],
        "preferred_profiles": ["builder-minimal", "reviewer-graph"],
        "native_primitives": [
            "interactive permissioned terminal execution",
            "browser loop",
            "workspace checkpoints",
            "MCP integrations"
        ],
        "preferred_codelens_use": [
            "review-heavy exploration before write passes",
            "session audit and handoff artifacts when a change must cross sessions"
        ],
        "routing_defaults": {
            "foreground_debug": "native-first-with-codelens-escalation",
            "write_pass": "builder-minimal-after-bootstrap",
            "handoff": "artifact-required"
        },
        "avoid": [
            "treating Cline as a headless CI runner",
            "relying on CodeLens where the foreground checkpoint loop already provides the needed safety"
        ],
        "compiler_targets": [
            "mcp_servers.json",
            ".clinerules",
            "repo instructions"
        ],
        "native_files": [
            {
                "path": "mcp_servers.json",
                "format": "json",
                "purpose": "Attach CodeLens to Cline with an explicit project-local server entry.",
                "template": {
                    "codelens": {
                        "type": "http",
                        "url": "http://127.0.0.1:7837/mcp"
                    }
                }
            },
            {
                "path": ".clinerules",
                "format": "markdown",
                "purpose": "Keep CodeLens for reviewer-heavy or handoff-heavy flows, not every approval cycle.",
                "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Use Cline's normal foreground loop for local debugging, browser checks, and explicit command approvals.
- Bring in CodeLens after the first local step when the task spans multiple files or needs refactor preflight.
- Use `reviewer-graph` for exploration and `builder-minimal` for bounded write passes.
- If work crosses sessions, export an audit or handoff artifact instead of relying on chat history.
"#, HOST))
            }
        ]
    })
}
