//! MCP resource definitions and handlers.

use crate::tool_defs::tools;
use crate::AppState;
use serde_json::json;

pub(crate) fn resources(state: &AppState) -> Vec<serde_json::Value> {
    let project_name = state
        .project()
        .as_path()
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    vec![
        json!({
            "uri": "codelens://project/overview",
            "name": format!("Project: {project_name}"),
            "description": "Project root path and symbol index statistics",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://symbols/index",
            "name": "Symbol Index",
            "description": "All indexed files and symbol counts",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list",
            "name": "Available Tools",
            "description": format!("List of all {} MCP tools with descriptions", tools().len()),
            "mimeType": "application/json"
        }),
    ]
}

pub(crate) fn read_resource(state: &AppState, uri: &str) -> serde_json::Value {
    match uri {
        "codelens://project/overview" => {
            let stats = state.symbol_index().stats().ok();
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "project_root": state.project().as_path().to_string_lossy(),
                        "symbol_index": stats,
                        "memories_dir": state.memories_dir().to_string_lossy(),
                        "tool_count": tools().len()
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://symbols/index" => {
            let stats = state.symbol_index().stats().ok();
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&json!({
                        "stats": stats
                    })).unwrap_or_default()
                }]
            })
        }
        "codelens://tools/list" => {
            let tool_names: Vec<&str> = tools().iter().map(|t| t.name).collect();
            json!({
                "contents": [{
                    "uri": uri,
                    "mimeType": "application/json",
                    "text": serde_json::to_string_pretty(&tool_names).unwrap_or_default()
                }]
            })
        }
        _ => json!({
            "contents": [{
                "uri": uri,
                "mimeType": "text/plain",
                "text": format!("Unknown resource: {uri}")
            }]
        }),
    }
}
