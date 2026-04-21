//! MCP prompt definitions and handlers.

use crate::AppState;
use crate::tool_defs::{ToolProfile, ToolSurface};
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

fn prompt_tool_names(names: &[&str]) -> Vec<&'static str> {
    crate::tool_defs::canonical_tool_names(names)
}

fn prompt_surface_tool_names(surface: ToolSurface, names: &[&str]) -> Vec<&'static str> {
    crate::tool_defs::canonical_surface_tool_names(surface, names)
}

fn quoted_tool(name: &str) -> String {
    format!("`{name}`")
}

fn format_tool_list(names: &[&str], conjunction: &str, fallback: &str) -> String {
    match names {
        [] => fallback.to_owned(),
        [one] => quoted_tool(one),
        [first, second] => format!(
            "{} {conjunction} {}",
            quoted_tool(first),
            quoted_tool(second)
        ),
        _ => {
            let head = names[..names.len() - 1]
                .iter()
                .map(|name| quoted_tool(name))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{head}, {conjunction} {}",
                quoted_tool(names[names.len() - 1])
            )
        }
    }
}

fn first_tool_or_fallback(names: &[&str], fallback: &str) -> String {
    names
        .first()
        .map(|name| quoted_tool(name))
        .unwrap_or_else(|| fallback.to_owned())
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
            let review_tools = prompt_tool_names(&[
                "impact_report",
                "module_boundary_report",
                "diff_aware_references",
            ]);
            let expansion_tool = prompt_tool_names(&["get_analysis_section"]);
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Review `{file_path}` in `{project_root}` using bounded evidence. Prefer {}, then expand one section at a time with {}. Focus on bug risk, structural coupling, and missing tests.",
                            format_tool_list(&review_tools, "and", "the canonical review workflows"),
                            first_tool_or_fallback(&expansion_tool, "the section-expansion workflow"),
                        )
                    }
                }]
            })
        }
        "onboard-project" => {
            let onboarding_tools =
                prompt_tool_names(&["onboard_project", "find_minimal_context_for_change"]);
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Onboard me to `{project_root}` as a harness designer. Start with {}, keep the context compressed, and call out the best planner/reviewer profiles for this repo.",
                            format_tool_list(&onboarding_tools, "or", "the canonical onboarding workflows"),
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
            let impact_tools = prompt_tool_names(&["impact_report", "summarize_symbol_impact"]);
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Assess the change surface for `{file_path}` in `{project_root}`. Prefer {}, then expand individual sections only if the first report is insufficient.",
                            format_tool_list(&impact_tools, "and", "the canonical impact workflows"),
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
            let surface = ToolSurface::Profile(ToolProfile::PlannerReadonly);
            let start_tools = prompt_surface_tool_names(surface, &["analyze_change_request"]);
            let expansion_tools = prompt_surface_tool_names(surface, &["get_analysis_section"]);
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "For planner-readonly, compress the request `{task}` in `{project_root}` into the smallest useful context. Start with {}; only open {} for ranked_files or changed_files if needed.",
                            first_tool_or_fallback(&start_tools, "the planner-readonly bootstrap workflow"),
                            first_tool_or_fallback(&expansion_tools, "the section-expansion workflow"),
                        )
                    }
                }]
            })
        }
        "reviewer-graph-guide" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let surface = ToolSurface::Profile(ToolProfile::ReviewerGraph);
            let review_tools = prompt_surface_tool_names(
                surface,
                &[
                    "impact_report",
                    "module_boundary_report",
                    "dead_code_report",
                ],
            );
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "For reviewer-graph, inspect `{path}` in `{project_root}` with graph-aware evidence. Start with {}, then open individual sections only if the summary is insufficient.",
                            format_tool_list(&review_tools, "and", "the canonical reviewer workflows"),
                        )
                    }
                }]
            })
        }
        "refactor-full-guide" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            let surface = ToolSurface::Profile(ToolProfile::RefactorFull);
            let preview_tools = prompt_surface_tool_names(
                surface,
                &["refactor_safety_report", "safe_rename_report"],
            );
            let poll_tool = prompt_surface_tool_names(surface, &["get_analysis_job"]);
            json!({
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "For refactor-full, treat `{path}` in `{project_root}` as preview-first. Start with {}, poll any durable jobs with {}, and only use mutation tools after the bounded report shows low risk.",
                            format_tool_list(&preview_tools, "or", "the canonical refactor preview workflows"),
                            first_tool_or_fallback(&poll_tool, "the analysis-job poller"),
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

#[cfg(test)]
mod tests {
    use super::{prompt_surface_tool_names, prompt_tool_names};
    use crate::tool_defs::{ToolProfile, ToolSurface};

    #[test]
    fn prompt_tool_references_resolve_through_registry() {
        let tool_groups = [
            prompt_tool_names(&[
                "impact_report",
                "module_boundary_report",
                "diff_aware_references",
                "get_analysis_section",
            ]),
            prompt_tool_names(&["onboard_project", "find_minimal_context_for_change"]),
            prompt_tool_names(&["impact_report", "summarize_symbol_impact"]),
            prompt_surface_tool_names(
                ToolSurface::Profile(ToolProfile::PlannerReadonly),
                &["analyze_change_request", "get_analysis_section"],
            ),
            prompt_surface_tool_names(
                ToolSurface::Profile(ToolProfile::ReviewerGraph),
                &[
                    "impact_report",
                    "module_boundary_report",
                    "dead_code_report",
                ],
            ),
            prompt_surface_tool_names(
                ToolSurface::Profile(ToolProfile::RefactorFull),
                &[
                    "refactor_safety_report",
                    "safe_rename_report",
                    "get_analysis_job",
                ],
            ),
        ];

        for tools in tool_groups {
            assert!(!tools.is_empty(), "prompt tool group unexpectedly empty");
            for tool in tools {
                assert!(
                    crate::tool_defs::tool_definition(tool).is_some(),
                    "prompt referenced unknown tool `{tool}`"
                );
            }
        }
    }

    #[test]
    fn reviewer_graph_prompt_filters_out_invisible_tools() {
        let tools = prompt_surface_tool_names(
            ToolSurface::Profile(ToolProfile::ReviewerGraph),
            &[
                "impact_report",
                "module_boundary_report",
                "dead_code_report",
            ],
        );
        assert_eq!(tools, vec!["impact_report", "module_boundary_report"]);
    }
}
