use super::*;

pub(super) fn host_context_for_adapter(host: &str) -> Option<HostContext> {
    match host {
        "claude-code" => Some(HostContext::ClaudeCode),
        "codex" => Some(HostContext::Codex),
        "cursor" => Some(HostContext::Cursor),
        "cline" => Some(HostContext::Cline),
        "windsurf" => Some(HostContext::Windsurf),
        _ => None,
    }
}

fn overlay_specs_for_host(host: &str) -> Vec<(ToolProfile, TaskOverlay)> {
    match host {
        "claude-code" => vec![
            (ToolProfile::PlannerReadonly, TaskOverlay::Planning),
            (ToolProfile::ReviewerGraph, TaskOverlay::Review),
            (ToolProfile::PlannerReadonly, TaskOverlay::Onboarding),
        ],
        "codex" => vec![
            (ToolProfile::BuilderMinimal, TaskOverlay::Editing),
            (ToolProfile::RefactorFull, TaskOverlay::Review),
            (ToolProfile::CiAudit, TaskOverlay::BatchAnalysis),
        ],
        "cursor" => vec![
            (ToolProfile::ReviewerGraph, TaskOverlay::Review),
            (ToolProfile::PlannerReadonly, TaskOverlay::Planning),
            (ToolProfile::CiAudit, TaskOverlay::BatchAnalysis),
        ],
        "cline" => vec![
            (ToolProfile::BuilderMinimal, TaskOverlay::Editing),
            (ToolProfile::ReviewerGraph, TaskOverlay::Review),
        ],
        "windsurf" => vec![
            (ToolProfile::BuilderMinimal, TaskOverlay::Editing),
            (ToolProfile::PlannerReadonly, TaskOverlay::Interactive),
        ],
        _ => Vec::new(),
    }
}

fn compiled_overlay_preview(
    profile: ToolProfile,
    host_context: HostContext,
    task_overlay: TaskOverlay,
) -> Value {
    let surface = ToolSurface::Profile(profile);
    let plan = compile_surface_overlay(surface, Some(host_context), Some(task_overlay));
    let mut bootstrap_sequence = vec!["prepare_harness_session".to_owned()];
    for tool in &plan.preferred_entrypoints {
        if !bootstrap_sequence.iter().any(|item| item == tool) {
            bootstrap_sequence.push((*tool).to_owned());
        }
    }

    json!({
        "host_context": host_context.as_str(),
        "profile": profile.as_str(),
        "surface": surface.as_label(),
        "task_overlay": task_overlay.as_str(),
        "preferred_executor_bias": plan.preferred_executor_bias,
        "bootstrap_sequence": bootstrap_sequence,
        "preferred_entrypoints": plan.preferred_entrypoints,
        "emphasized_tools": plan.emphasized_tools,
        "avoid_tools": plan.avoid_tools,
        "routing_notes": plan.routing_notes,
    })
}

pub(super) fn overlay_previews_for_host(host: &str) -> Vec<Value> {
    let Some(host_context) = host_context_for_adapter(host) else {
        return Vec::new();
    };
    overlay_specs_for_host(host)
        .into_iter()
        .map(|(profile, task_overlay)| {
            compiled_overlay_preview(profile, host_context, task_overlay)
        })
        .collect()
}

pub(super) fn primary_bootstrap_sequence_for_host(host: &str) -> Vec<String> {
    overlay_previews_for_host(host)
        .into_iter()
        .next()
        .and_then(|value| {
            value.get("bootstrap_sequence").and_then(|items| {
                items.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
            })
        })
        .unwrap_or_else(|| vec!["prepare_harness_session".to_owned()])
}

fn compiled_overlay_markdown_section(host: &str) -> String {
    let previews = overlay_previews_for_host(host);
    if previews.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## Compiled Routing Overlays".to_owned(),
        String::new(),
        format!(
            "- Primary bootstrap sequence: `{}`",
            primary_bootstrap_sequence_for_host(host).join("` -> `")
        ),
    ];

    for preview in previews {
        let profile = preview
            .get("profile")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown-profile");
        let task_overlay = preview
            .get("task_overlay")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown-overlay");
        let preferred_executor_bias = preview
            .get("preferred_executor_bias")
            .and_then(|value| value.as_str())
            .unwrap_or("any");
        let bootstrap_sequence = preview
            .get("bootstrap_sequence")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>();
        let avoid_tools = preview
            .get("avoid_tools")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>();

        let mut line = format!(
            "- `{profile}` + `{task_overlay}` [bias: `{preferred_executor_bias}`]: `{}`",
            bootstrap_sequence.join("` -> `")
        );
        if !avoid_tools.is_empty() {
            line.push_str(&format!(" | avoid: `{}`", avoid_tools.join("`, `")));
        }
        lines.push(line);
    }

    lines.join("\n")
}

pub(super) fn append_compiled_overlay_section(base: &str, host: &str) -> String {
    let compiled = compiled_overlay_markdown_section(host);
    let mut text = base.trim_end().to_owned();
    if !compiled.is_empty() {
        text.push_str("\n\n");
        text.push_str(&compiled);
    }
    text.push('\n');
    text
}

pub(super) fn managed_host_policy_block(body: &str) -> String {
    format!(
        "<!-- CODELENS_HOST_ROUTING:BEGIN -->\n{}\n<!-- CODELENS_HOST_ROUTING:END -->\n",
        body.trim_end()
    )
}
