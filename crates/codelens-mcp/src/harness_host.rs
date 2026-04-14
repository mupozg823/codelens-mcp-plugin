use crate::client_profile::ClientProfile;
use crate::protocol::{
    OrchestrationContract, RecommendedNextStep, RecommendedNextStepKind, RoutingHint,
};
use crate::tool_defs::ToolSurface;
use serde_json::{Value, json};
use std::collections::HashMap;

pub(crate) const ORCHESTRATOR_SUPPORT_CONTRACT_VERSION: &str = "orchestrator-support/v1";
pub(crate) const SUPPORTING_MCP_SERVER_ROLE: &str = "supporting_mcp";
pub(crate) const HOST_ORCHESTRATION_OWNER: &str = "host";

pub(crate) fn base_orchestration_contract() -> OrchestrationContract {
    OrchestrationContract {
        contract_version: ORCHESTRATOR_SUPPORT_CONTRACT_VERSION.to_owned(),
        server_role: SUPPORTING_MCP_SERVER_ROLE.to_owned(),
        orchestration_owner: HOST_ORCHESTRATION_OWNER.to_owned(),
        retry_policy_owner: HOST_ORCHESTRATION_OWNER.to_owned(),
        execution_loop_owner: HOST_ORCHESTRATION_OWNER.to_owned(),
        host_id: None,
        integration_style: None,
        tool_role: String::new(),
        stage_hint: String::new(),
        active_surface: None,
        continue_in_host: None,
        interaction_mode: String::new(),
        preferred_client_behavior: None,
    }
}

fn default_surface_label(client: ClientProfile, indexed_files: Option<usize>) -> &'static str {
    client.advertised_default_surface(indexed_files).as_label()
}

fn host_id(client: ClientProfile) -> &'static str {
    match client {
        ClientProfile::Claude => "claude-code",
        ClientProfile::Codex => "codex",
        ClientProfile::Generic => "generic-mcp",
    }
}

fn integration_style(client: ClientProfile) -> &'static str {
    match client {
        ClientProfile::Claude => "interactive-query-orchestrator",
        ClientProfile::Codex => "tool-first-harness",
        ClientProfile::Generic => "generic-mcp-client",
    }
}

fn orchestrator_entrypoint(client: ClientProfile) -> &'static str {
    match client {
        ClientProfile::Claude => "QueryEngine/query",
        ClientProfile::Codex => "agent_loop",
        ClientProfile::Generic => "host_defined",
    }
}

fn tool_call_template(name: &str, arguments: Value) -> Value {
    json!({
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": arguments,
        }
    })
}

fn resource_read_template(uri: &str, extra_params: Value) -> Value {
    let mut params = serde_json::Map::new();
    params.insert("uri".to_owned(), json!(uri));
    if let Some(extra) = extra_params.as_object() {
        for (key, value) in extra {
            params.insert(key.clone(), value.clone());
        }
    }
    json!({
        "method": "resources/read",
        "params": params,
    })
}

fn request_template(
    tool: &str,
    routing: &str,
    when: &str,
    input_placeholders: &[&str],
    request_shape: Value,
) -> Value {
    json!({
        "tool": tool,
        "routing": routing,
        "when": when,
        "input_placeholders": input_placeholders,
        "request_shape": request_shape,
    })
}

fn stage_templates(client: ClientProfile, stage: &str) -> Vec<Value> {
    match (client, stage) {
        (ClientProfile::Claude, "session_bootstrap") => vec![request_template(
            "prepare_harness_session",
            "sync",
            "Start of a new Claude Code session before the main query loop widens.",
            &["project"],
            tool_call_template(
                "prepare_harness_session",
                json!({
                    "project": "<PROJECT_PATH>",
                }),
            ),
        )],
        (ClientProfile::Claude, "multi_file_reasoning") => vec![
            request_template(
                "explore_codebase",
                "sync",
                "First bounded pass over a broad task or question.",
                &["query"],
                tool_call_template(
                    "explore_codebase",
                    json!({
                        "query": "<TASK_OR_QUESTION>",
                    }),
                ),
            ),
            request_template(
                "get_ranked_context",
                "sync",
                "When the host needs small symbol/file context to feed its own loop.",
                &["query"],
                tool_call_template(
                    "get_ranked_context",
                    json!({
                        "query": "<TASK_OR_QUESTION>",
                    }),
                ),
            ),
            request_template(
                "analyze_change_impact",
                "sync",
                "When the host needs reviewer-style impact before edits.",
                &["path_or_changed_files"],
                tool_call_template(
                    "analyze_change_impact",
                    json!({
                        "path": "<PATH_OR_MODULE>",
                    }),
                ),
            ),
        ],
        (ClientProfile::Claude, "mutation_preflight") => vec![
            request_template(
                "verify_change_readiness",
                "sync",
                "Right before the host commits to a risky edit.",
                &["task"],
                tool_call_template(
                    "verify_change_readiness",
                    json!({
                        "task": "<PLANNED_CHANGE>",
                    }),
                ),
            ),
            request_template(
                "review_changes",
                "sync",
                "After the host has a concrete changed-file set to inspect.",
                &["changed_files"],
                tool_call_template(
                    "review_changes",
                    json!({
                        "changed_files": ["<CHANGED_FILE>"],
                    }),
                ),
            ),
        ],
        (ClientProfile::Claude, "async_analysis") => vec![
            request_template(
                "start_analysis_job",
                "async",
                "When the host wants expensive analysis without blocking the query loop.",
                &["task_or_path"],
                tool_call_template(
                    "start_analysis_job",
                    json!({
                        "kind": "impact_report",
                        "path": "<PATH_OR_MODULE>",
                    }),
                ),
            ),
            request_template(
                "get_analysis_job",
                "poll",
                "Poll the job handle returned by start_analysis_job.",
                &["job_id"],
                tool_call_template(
                    "get_analysis_job",
                    json!({
                        "job_id": "<JOB_ID>",
                    }),
                ),
            ),
            request_template(
                "get_analysis_section",
                "sync",
                "Read a specific section after the async job completes.",
                &["analysis_id", "section"],
                tool_call_template(
                    "get_analysis_section",
                    json!({
                        "analysis_id": "<ANALYSIS_ID>",
                        "section": "<SECTION_NAME>",
                    }),
                ),
            ),
        ],
        (ClientProfile::Codex, "session_bootstrap") => vec![request_template(
            "prepare_harness_session",
            "sync",
            "Bootstrap once before deciding whether CodeLens is needed for the task.",
            &["project"],
            tool_call_template(
                "prepare_harness_session",
                json!({
                    "project": "<PROJECT_PATH>",
                }),
            ),
        )],
        (ClientProfile::Codex, "local_lookup") => vec![
            request_template(
                "get_ranked_context",
                "sync",
                "When native shell lookup is insufficient and graph-aware context is needed.",
                &["query"],
                tool_call_template(
                    "get_ranked_context",
                    json!({
                        "query": "<TASK_OR_QUESTION>",
                    }),
                ),
            ),
            request_template(
                "find_symbol",
                "sync",
                "When the host already knows the target symbol name.",
                &["symbol_name"],
                tool_call_template(
                    "find_symbol",
                    json!({
                        "name": "<SYMBOL_NAME>",
                        "include_body": true,
                    }),
                ),
            ),
            request_template(
                "find_referencing_symbols",
                "sync",
                "When the host needs bounded reference impact for one symbol.",
                &["file_path", "symbol_name"],
                tool_call_template(
                    "find_referencing_symbols",
                    json!({
                        "file_path": "<FILE_PATH>",
                        "symbol_name": "<SYMBOL_NAME>",
                    }),
                ),
            ),
        ],
        (ClientProfile::Codex, "refactor_preflight") => vec![
            request_template(
                "plan_safe_refactor",
                "sync",
                "Before broad edits that may cross multiple files.",
                &["task_or_symbol"],
                tool_call_template(
                    "plan_safe_refactor",
                    json!({
                        "task": "<PLANNED_CHANGE>",
                    }),
                ),
            ),
            request_template(
                "verify_change_readiness",
                "sync",
                "Before mutating code when a verifier-style preflight is needed.",
                &["task"],
                tool_call_template(
                    "verify_change_readiness",
                    json!({
                        "task": "<PLANNED_CHANGE>",
                    }),
                ),
            ),
        ],
        (ClientProfile::Codex, "async_analysis") => vec![
            request_template(
                "start_analysis_job",
                "async",
                "When heavy analysis should run outside the main Codex editing flow.",
                &["task_or_path"],
                tool_call_template(
                    "start_analysis_job",
                    json!({
                        "kind": "impact_report",
                        "path": "<PATH_OR_MODULE>",
                    }),
                ),
            ),
            request_template(
                "get_analysis_job",
                "poll",
                "Poll the async handle instead of re-running the analysis.",
                &["job_id"],
                tool_call_template(
                    "get_analysis_job",
                    json!({
                        "job_id": "<JOB_ID>",
                    }),
                ),
            ),
        ],
        (ClientProfile::Generic, "bootstrap") => vec![request_template(
            "prepare_harness_session",
            "sync",
            "Initial bootstrap before the client selects a narrower path.",
            &["project"],
            tool_call_template(
                "prepare_harness_session",
                json!({
                    "project": "<PROJECT_PATH>",
                }),
            ),
        )],
        (ClientProfile::Generic, "analysis") => vec![
            request_template(
                "explore_codebase",
                "sync",
                "First broad pass for a generic client.",
                &["query"],
                tool_call_template(
                    "explore_codebase",
                    json!({
                        "query": "<TASK_OR_QUESTION>",
                    }),
                ),
            ),
            request_template(
                "analyze_change_impact",
                "sync",
                "When impact is needed before edits or review.",
                &["path_or_changed_files"],
                tool_call_template(
                    "analyze_change_impact",
                    json!({
                        "path": "<PATH_OR_MODULE>",
                    }),
                ),
            ),
        ],
        _ => Vec::new(),
    }
}

fn bootstrap_step(
    kind: &str,
    name: &str,
    required_runtime_inputs: &[&str],
    request_shape: Value,
) -> Value {
    json!({
        "kind": kind,
        "name": name,
        "required_runtime_inputs": required_runtime_inputs,
        "request_shape": request_shape,
    })
}

fn stage_routes(client: ClientProfile) -> Vec<Value> {
    match client {
        ClientProfile::Claude => vec![
            json!({
                "host_stage": "session_bootstrap",
                "entrypoints": ["prepare_harness_session"],
                "follow_up": ["codelens://harness/host", "codelens://tools/list"],
                "reason": "Bootstrap once, then let Claude Code keep ownership of the query loop.",
                "request_templates": stage_templates(client, "session_bootstrap"),
            }),
            json!({
                "host_stage": "multi_file_reasoning",
                "entrypoints": ["explore_codebase", "get_ranked_context", "analyze_change_impact"],
                "follow_up": [],
                "reason": "Use workflow-first retrieval for bounded evidence before low-level expansion.",
                "request_templates": stage_templates(client, "multi_file_reasoning"),
            }),
            json!({
                "host_stage": "mutation_preflight",
                "entrypoints": ["verify_change_readiness", "review_changes"],
                "follow_up": [],
                "reason": "Keep mutation safety in CodeLens while the host runtime still decides whether to act.",
                "request_templates": stage_templates(client, "mutation_preflight"),
            }),
            json!({
                "host_stage": "async_analysis",
                "entrypoints": ["start_analysis_job", "get_analysis_job", "get_analysis_section"],
                "follow_up": [],
                "reason": "Offload expensive review or refactor analysis instead of blocking the interactive loop.",
                "request_templates": stage_templates(client, "async_analysis"),
            }),
        ],
        ClientProfile::Codex => vec![
            json!({
                "host_stage": "session_bootstrap",
                "entrypoints": ["prepare_harness_session"],
                "follow_up": ["codelens://harness/host", "codelens://tools/list"],
                "reason": "Bootstrap the recommended Codex workflow surface first, then expand only when the task broadens.",
                "request_templates": stage_templates(client, "session_bootstrap"),
            }),
            json!({
                "host_stage": "local_lookup",
                "entrypoints": ["get_ranked_context", "find_symbol", "find_referencing_symbols"],
                "follow_up": [],
                "reason": "Codex already has native shell and file primitives, so CodeLens should stay focused on graph-aware lookup.",
                "request_templates": stage_templates(client, "local_lookup"),
            }),
            json!({
                "host_stage": "refactor_preflight",
                "entrypoints": ["plan_safe_refactor", "verify_change_readiness"],
                "follow_up": [],
                "reason": "Use CodeLens for refactor safety and impact checks before broad edits.",
                "request_templates": stage_templates(client, "refactor_preflight"),
            }),
            json!({
                "host_stage": "async_analysis",
                "entrypoints": ["start_analysis_job", "get_analysis_job", "get_analysis_section"],
                "follow_up": [],
                "reason": "Use analysis handles for long-running review work instead of widening the recommended Codex surface.",
                "request_templates": stage_templates(client, "async_analysis"),
            }),
        ],
        ClientProfile::Generic => vec![
            json!({
                "host_stage": "bootstrap",
                "entrypoints": ["prepare_harness_session"],
                "follow_up": ["codelens://harness/host", "codelens://tools/list"],
                "reason": "Start with the bootstrap contract before selecting lower-level tools.",
                "request_templates": stage_templates(client, "bootstrap"),
            }),
            json!({
                "host_stage": "analysis",
                "entrypoints": ["explore_codebase", "get_ranked_context", "analyze_change_impact"],
                "follow_up": [],
                "reason": "Prefer workflow entrypoints until the host proves it needs primitive expansion.",
                "request_templates": stage_templates(client, "analysis"),
            }),
        ],
    }
}

fn guardrails(client: ClientProfile) -> Vec<&'static str> {
    match client {
        ClientProfile::Claude => vec![
            "CodeLens does not own the conversation loop or retry policy.",
            "Use workflow-first entrypoints before primitive symbol or file tools.",
            "Use async analysis handles for expensive review or refactor tasks.",
        ],
        ClientProfile::Codex => vec![
            "Keep the default Codex surface workflow-oriented and expand on demand.",
            "Let Codex keep native shell and edit orchestration ownership.",
            "Use CodeLens for graph-aware retrieval, preflight, and async analysis.",
        ],
        ClientProfile::Generic => vec![
            "Bootstrap first, then narrow the surface according to observed task shape.",
            "Prefer workflow entrypoints before primitive expansion.",
        ],
    }
}

pub(crate) fn requested_host_profile(
    params: Option<&Value>,
    fallback: ClientProfile,
) -> (ClientProfile, &'static str) {
    let explicit = params
        .and_then(|value| value.get("host"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_ascii_lowercase());
    match explicit.as_deref() {
        Some("claude") | Some("claude-code") => (ClientProfile::Claude, "param"),
        Some("codex") => (ClientProfile::Codex, "param"),
        Some("generic") | Some("generic-mcp") => (ClientProfile::Generic, "param"),
        _ => (fallback, "session"),
    }
}

pub(crate) fn host_runtime_contract(
    client: ClientProfile,
    active_surface: ToolSurface,
    indexed_files: Option<usize>,
) -> Value {
    json!({
        "host_id": host_id(client),
        "runtime_role": "host-orchestrator",
        "integration_style": integration_style(client),
        "orchestrator_entrypoint": orchestrator_entrypoint(client),
        "orchestration_owner": "host",
        "integration_boundary": "bounded-evidence-preflight-async-analysis",
        "client_default_surface": default_surface_label(client, indexed_files),
        "active_surface": active_surface.as_label(),
        "default_contract_mode": client.default_tool_contract_mode(),
        "bootstrap_sequence": [
            bootstrap_step(
                "tool",
                "activate_project",
                &["project"],
                tool_call_template(
                    "activate_project",
                    json!({
                        "project": "<PROJECT_PATH>",
                    }),
                ),
            ),
            bootstrap_step(
                "tool",
                "prepare_harness_session",
                &["project"],
                tool_call_template(
                    "prepare_harness_session",
                    json!({
                        "project": "<PROJECT_PATH>",
                    }),
                ),
            ),
            bootstrap_step(
                "resource",
                "codelens://harness/host",
                &["host"],
                resource_read_template(
                    "codelens://harness/host",
                    json!({
                        "host": host_id(client),
                    }),
                ),
            ),
            bootstrap_step(
                "resource",
                "codelens://tools/list",
                &[],
                resource_read_template("codelens://tools/list", json!({})),
            ),
            bootstrap_step(
                "resource",
                "codelens://project/architecture",
                &[],
                resource_read_template("codelens://project/architecture", json!({})),
            )
        ],
        "task_stages": stage_routes(client),
        "guardrails": guardrails(client),
    })
}

pub(crate) fn supported_host_summaries(indexed_files: Option<usize>) -> Value {
    json!([
        {
            "host_id": host_id(ClientProfile::Claude),
            "integration_style": integration_style(ClientProfile::Claude),
            "orchestrator_entrypoint": orchestrator_entrypoint(ClientProfile::Claude),
            "client_default_surface": default_surface_label(ClientProfile::Claude, indexed_files),
            "default_contract_mode": ClientProfile::Claude.default_tool_contract_mode(),
        },
        {
            "host_id": host_id(ClientProfile::Codex),
            "integration_style": integration_style(ClientProfile::Codex),
            "orchestrator_entrypoint": orchestrator_entrypoint(ClientProfile::Codex),
            "client_default_surface": default_surface_label(ClientProfile::Codex, indexed_files),
            "default_contract_mode": ClientProfile::Codex.default_tool_contract_mode(),
        },
        {
            "host_id": host_id(ClientProfile::Generic),
            "integration_style": integration_style(ClientProfile::Generic),
            "orchestrator_entrypoint": orchestrator_entrypoint(ClientProfile::Generic),
            "client_default_surface": default_surface_label(ClientProfile::Generic, indexed_files),
            "default_contract_mode": ClientProfile::Generic.default_tool_contract_mode(),
        },
    ])
}

fn tool_stage(name: &str, routing_hint: RoutingHint) -> &'static str {
    if matches!(routing_hint, RoutingHint::Async) {
        return "async_analysis";
    }
    match name {
        "activate_project"
        | "prepare_harness_session"
        | "get_current_config"
        | "get_capabilities" => "session_bootstrap",
        "verify_change_readiness"
        | "safe_rename_report"
        | "unresolved_reference_check"
        | "plan_safe_refactor"
        | "assess_change_readiness" => "mutation_preflight",
        "start_analysis_job" | "get_analysis_job" | "get_analysis_section" => "async_analysis",
        "explore_codebase"
        | "review_architecture"
        | "analyze_change_impact"
        | "review_changes"
        | "analyze_change_request"
        | "find_minimal_context_for_change"
        | "trace_request_path"
        | "diagnose_issues" => "multi_file_reasoning",
        _ => "bounded_lookup",
    }
}

fn tool_role(name: &str, routing_hint: RoutingHint) -> &'static str {
    if matches!(routing_hint, RoutingHint::Async) {
        return "analysis_handle";
    }
    match tool_stage(name, routing_hint) {
        "session_bootstrap" => "bootstrap_contract",
        "mutation_preflight" => "preflight_guard",
        "async_analysis" => "async_entrypoint",
        "multi_file_reasoning" => "workflow_entrypoint",
        _ => "bounded_evidence",
    }
}

pub(crate) fn preferred_client_behavior_for_stage(stage: &str) -> &'static str {
    match stage {
        "async_analysis" => "start once, then poll or read sections instead of re-running",
        "mutation_preflight" => "use as guard before edits; host still decides whether to mutate",
        "session_bootstrap" => {
            "bootstrap once, then let the host keep query orchestration ownership"
        }
        _ => "treat as bounded evidence inside the host query plan",
    }
}

pub(crate) fn response_orchestration_contract(
    client: ClientProfile,
    active_surface: ToolSurface,
    tool_name: &str,
    routing_hint: RoutingHint,
) -> OrchestrationContract {
    let stage = tool_stage(tool_name, routing_hint);
    let mut contract = base_orchestration_contract();
    contract.host_id = Some(host_id(client).to_owned());
    contract.integration_style = Some(integration_style(client).to_owned());
    contract.tool_role = tool_role(tool_name, routing_hint).to_owned();
    contract.stage_hint = stage.to_owned();
    contract.active_surface = Some(active_surface.as_label().to_owned());
    contract.continue_in_host = Some(true);
    contract.interaction_mode = match routing_hint {
        RoutingHint::Async => "handle_then_poll",
        RoutingHint::Cached => "reuse_cached_result",
        RoutingHint::Sync => "inline_bounded_call",
    }
    .to_owned();
    contract.preferred_client_behavior =
        Some(preferred_client_behavior_for_stage(stage).to_owned());
    contract
}

pub(crate) fn response_next_steps(
    client: ClientProfile,
    tool_name: &str,
    routing_hint: RoutingHint,
    suggested_next_tools: &[String],
    suggestion_reasons: Option<&HashMap<String, String>>,
) -> Vec<RecommendedNextStep> {
    let mut steps = Vec::new();
    let push_tool_steps = |steps: &mut Vec<RecommendedNextStep>| {
        for tool in suggested_next_tools.iter().take(3) {
            let reason = suggestion_reasons
                .and_then(|map| map.get(tool))
                .cloned()
                .unwrap_or_else(|| {
                    "Recommended follow-up in the current workflow stage".to_owned()
                });
            steps.push(RecommendedNextStep {
                kind: RecommendedNextStepKind::Tool,
                target: tool.clone(),
                reason,
            });
        }
    };

    if matches!(tool_name, "prepare_harness_session") {
        push_tool_steps(&mut steps);
        steps.push(RecommendedNextStep {
            kind: RecommendedNextStepKind::Resource,
            target: "codelens://project/architecture".to_owned(),
            reason:
                "Load the bounded architecture contract only if the workflow entrypoint needs wider structural context."
                    .to_owned(),
        });
    } else if tool_name == "activate_project" {
        steps.push(RecommendedNextStep {
            kind: RecommendedNextStepKind::Resource,
            target: "codelens://tools/list".to_owned(),
            reason: "Read the active tool surface before selecting lower-level tools.".to_owned(),
        });
        steps.push(RecommendedNextStep {
            kind: RecommendedNextStepKind::Resource,
            target: "codelens://project/architecture".to_owned(),
            reason:
                "Load the bounded project contract once, then let the host keep the query loop."
                    .to_owned(),
        });
    }
    if tool_name != "prepare_harness_session" {
        push_tool_steps(&mut steps);
    }

    if steps.is_empty() && matches!(routing_hint, RoutingHint::Async) {
        steps.push(RecommendedNextStep {
            kind: RecommendedNextStepKind::Tool,
            target: "get_analysis_job".to_owned(),
            reason: "Poll the analysis handle instead of re-running heavy work inline.".to_owned(),
        });
    }

    steps.push(RecommendedNextStep {
        kind: RecommendedNextStepKind::Handoff,
        target: "host_orchestrator".to_owned(),
        reason: match client {
            ClientProfile::Claude =>
                "Claude Code QueryEngine/query() remains the orchestrator; CodeLens only returns bounded evidence, preflight, or analysis handles.",
            ClientProfile::Codex =>
                "Codex remains responsible for shell/edit orchestration; CodeLens only contributes graph-aware evidence and safety contracts.",
            ClientProfile::Generic =>
                "The host remains responsible for retries, branching, and execution policy.",
        }
        .to_owned(),
    });

    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_defs::{ToolProfile, ToolSurface};

    #[test]
    fn codex_host_runtime_contract_uses_repo_scaled_default_surface() {
        let payload = host_runtime_contract(
            ClientProfile::Codex,
            ToolSurface::Profile(ToolProfile::WorkflowFirst),
            Some(12),
        );
        assert_eq!(payload["client_default_surface"], json!("workflow-first"));
        assert_eq!(payload["active_surface"], json!("workflow-first"));
        assert_eq!(
            payload["task_stages"][0]["reason"],
            json!(
                "Bootstrap the recommended Codex workflow surface first, then expand only when the task broadens."
            )
        );
    }

    #[test]
    fn supported_host_summaries_keep_claude_balanced_and_scale_codex() {
        let payload = supported_host_summaries(Some(12));
        let items = payload.as_array().expect("host summary array");
        let claude = items
            .iter()
            .find(|item| item["host_id"] == json!("claude-code"))
            .expect("claude summary");
        let codex = items
            .iter()
            .find(|item| item["host_id"] == json!("codex"))
            .expect("codex summary");
        assert_eq!(claude["client_default_surface"], json!("preset:balanced"));
        assert_eq!(codex["client_default_surface"], json!("workflow-first"));
    }
}
