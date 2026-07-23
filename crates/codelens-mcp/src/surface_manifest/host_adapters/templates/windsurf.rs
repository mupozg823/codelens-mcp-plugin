use serde_json::{Value, json};

const HOST: &str = "windsurf";

pub(super) fn bundle() -> Value {
    json!({
        "name": HOST,
        "resource_uri": format!("codelens://host-adapters/{HOST}"),
        "best_fit": "editor-local implementation with a hard MCP tool cap and bounded foreground agent flows",
        "recommended_modes": ["solo-local", "reviewer-gate"],
        "preferred_profiles": ["builder", "readonly"],
        "native_primitives": [
            "global MCP config",
            "foreground agent loop",
            "workspace-local editing",
            "100-tool cap across MCP servers"
        ],
        "preferred_codelens_use": [
            "bounded builder execution under a small visible surface",
            "compressed planning when the task escapes single-file scope"
        ],
        "routing_defaults": {
            "foreground_lookup": "native-first",
            "multi_file_edit": "builder-after-bootstrap",
            "wide_surface": "deferred-loading-required",
            "tool_cap": "keep-profile-bounded"
        },
        "avoid": [
            "attaching the full CodeLens surface alongside many other MCP servers",
            "using reviewer-heavy profiles as the default editing surface"
        ],
        "compiler_targets": [
            "~/.codeium/windsurf/mcp_config.json"
        ],
        "native_files": [
            {
                "path": "~/.codeium/windsurf/mcp_config.json",
                "format": "json",
                "purpose": "Attach CodeLens to Windsurf with the smallest stable config that respects the host-wide MCP tool cap.",
                "template": {
                    "mcpServers": {
                        "codelens": {
                            "type": "http",
                            "url": "http://127.0.0.1:7838/mcp"
                        }
                    }
                }
            }
        ]
    })
}
