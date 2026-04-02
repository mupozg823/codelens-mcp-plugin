//! MCP prompt definitions and handlers.

use crate::AppState;
use serde_json::json;

pub(crate) fn prompts() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "review-file",
            "description": "Review one file with compressed, graph-aware evidence instead of raw tool chaining",
            "arguments": [{ "name": "file_path", "description": "File to review", "required": true }]
        }),
        json!({
            "name": "onboard-project",
            "description": "Get a harness-oriented project overview with the smallest useful context",
            "arguments": []
        }),
        json!({
            "name": "analyze-impact",
            "description": "Assess change surface and test risk for one file",
            "arguments": [{ "name": "file_path", "description": "File to analyze", "required": true }]
        }),
        json!({
            "name": "planner-readonly-guide",
            "description": "Recommended question pattern for planner-readonly",
            "arguments": [{ "name": "task", "description": "Change request to compress", "required": true }]
        }),
        json!({
            "name": "reviewer-graph-guide",
            "description": "Recommended question pattern for reviewer-graph",
            "arguments": [{ "name": "path", "description": "Path or module to inspect", "required": true }]
        }),
        json!({
            "name": "refactor-full-guide",
            "description": "Recommended preview-first pattern for refactor-full",
            "arguments": [{ "name": "path", "description": "Path to refactor", "required": true }]
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
                            "Review `{file_path}` in `{project_root}` using bounded evidence. Prefer `impact_report`, `module_boundary_report`, and `diff_aware_references`, then expand one section at a time with `get_analysis_section`. Focus on bug risk, structural coupling, and missing tests."
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
                            "Onboard me to `{project_root}` as a harness designer. Start with `onboard_project` or `find_minimal_context_for_change`, keep the context compressed, and call out the best planner/reviewer profiles for this repo."
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
                            "Assess the change surface for `{file_path}` in `{project_root}`. Prefer `impact_report` and `summarize_symbol_impact`, then expand individual sections only if the first report is insufficient."
                        )
                    }
                }]
            })
        }
        "planner-readonly-guide" => {
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .unwrap_or("planned change");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "For planner-readonly, compress the request `{task}` in `{project_root}` into the smallest useful context. Start with `analyze_change_request`; only open `get_analysis_section` for ranked_files or changed_files if needed."
                        )
                    }
                }]
            })
        }
        "reviewer-graph-guide" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "For reviewer-graph, inspect `{path}` in `{project_root}` with graph-aware evidence. Start with `impact_report`, `module_boundary_report`, and `dead_code_report`, then open individual sections only if the summary is insufficient."
                        )
                    }
                }]
            })
        }
        "refactor-full-guide" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "For refactor-full, treat `{path}` in `{project_root}` as preview-first. Start with `refactor_safety_report` or `safe_rename_report`, poll any durable jobs with `get_analysis_job`, and only use mutation tools after the bounded report shows low risk."
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
