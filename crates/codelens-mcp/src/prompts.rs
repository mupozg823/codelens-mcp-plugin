//! MCP prompt definitions and handlers.

use crate::AppState;
use serde_json::json;

pub(crate) fn prompts() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "review-file",
            "description": "Review a file for code quality, bugs, and improvements",
            "arguments": [{ "name": "file_path", "description": "File to review", "required": true }]
        }),
        json!({
            "name": "onboard-project",
            "description": "Get a comprehensive overview of the project for onboarding",
            "arguments": []
        }),
        json!({
            "name": "analyze-impact",
            "description": "Analyze the impact of modifying a specific file",
            "arguments": [{ "name": "file_path", "description": "File to analyze", "required": true }]
        }),
    ]
}

pub(crate) fn get_prompt(
    state: &AppState,
    name: &str,
    args: &serde_json::Value,
) -> serde_json::Value {
    let project_root = state.project().as_path().to_string_lossy().to_string();
    match name {
        "review-file" => {
            let file_path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Please review the file `{file_path}` in the project at `{project_root}`.\n\n\
                            Use these tools to analyze:\n\
                            1. `get_symbols_overview` to understand the file structure\n\
                            2. `find_scoped_references` to check how symbols are used\n\
                            3. `get_complexity` to identify complex functions\n\
                            4. `analyze_missing_imports` to find import issues\n\n\
                            Focus on: bugs, performance, readability, and missing error handling."
                        )
                    }
                }]
            })
        }
        "onboard-project" => {
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "I'm new to the project at `{project_root}`. Help me understand it.\n\n\
                            Use these tools:\n\
                            1. `get_symbols_overview` on the root to see top-level structure\n\
                            2. `get_symbol_importance` to find the most important files\n\
                            3. `find_circular_dependencies` to understand architecture issues\n\
                            4. `search_for_pattern` for key patterns (main entry, config, tests)\n\n\
                            Give me: architecture overview, key files, entry points, and test strategy."
                        )
                    }
                }]
            })
        }
        "analyze-impact" => {
            let file_path = args
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or(".");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Analyze the impact of modifying `{file_path}` in `{project_root}`.\n\n\
                            Use these tools:\n\
                            1. `get_impact_analysis` for symbols + importers + blast radius\n\
                            2. `get_symbols_overview` to understand what's in the file\n\
                            3. `find_scoped_references` for each exported symbol\n\n\
                            Assess: risk level, affected modules, required test coverage."
                        )
                    }
                }]
            })
        }
        _ => json!({
            "messages": [{
                "role": "user",
                "content": {
                    "type": "text",
                    "text": format!("Unknown prompt: {name}")
                }
            }]
        }),
    }
}
