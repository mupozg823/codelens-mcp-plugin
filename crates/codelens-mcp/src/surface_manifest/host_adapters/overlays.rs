//! Host routing overlay compilation for host adapter bundles.

use super::overlay_specs::{host_context_for_adapter, overlay_specs_for_host};
use crate::tool_defs::{
    AgentRole, HostContext, TaskOverlay, ToolProfile, ToolSurface,
    compile_surface_overlay_for_agent,
};
use serde_json::{Value, json};

fn compiled_overlay_preview(
    profile: ToolProfile,
    host_context: HostContext,
    task_overlay: TaskOverlay,
    agent_role: AgentRole,
) -> Value {
    let surface = ToolSurface::Profile(profile);
    let plan = compile_surface_overlay_for_agent(
        surface,
        Some(host_context),
        Some(task_overlay),
        Some(agent_role),
    );
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
        "agent_role": agent_role.as_str(),
        "bootstrap_sequence": bootstrap_sequence,
        "preferred_entrypoints": plan.preferred_entrypoints,
        "emphasized_tools": plan.emphasized_tools,
        "avoid_tools": plan.avoid_tools,
        "routing_notes": plan.routing_notes,
    })
}

fn compiled_overlays_for_host(host: &str) -> Vec<Value> {
    let Some(host_context) = host_context_for_adapter(host) else {
        return Vec::new();
    };
    overlay_specs_for_host(host)
        .into_iter()
        .map(|spec| {
            compiled_overlay_preview(
                spec.profile,
                host_context,
                spec.task_overlay,
                spec.agent_role,
            )
        })
        .collect()
}

fn primary_bootstrap_sequence_for_host(host: &str) -> Vec<String> {
    compiled_overlays_for_host(host)
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

pub(super) fn managed_host_policy_block(body: &str) -> String {
    format!(
        "<!-- CODELENS_HOST_ROUTING:BEGIN -->\n{}\n<!-- CODELENS_HOST_ROUTING:END -->\n",
        body.trim_end()
    )
}

pub(super) fn augment_host_adapter_bundle(host: &str, bundle: &mut Value) {
    let primary_overlay = compiled_overlays_for_host(host).into_iter().next();
    let primary_bootstrap_sequence = primary_bootstrap_sequence_for_host(host);

    if let Some(object) = bundle.as_object_mut() {
        object.insert(
            "host_context".to_owned(),
            json!(host_context_for_adapter(host).map(|value| value.as_str())),
        );
        object.insert(
            "primary_bootstrap_sequence".to_owned(),
            json!(primary_bootstrap_sequence),
        );
        object.insert(
            "default_profile".to_owned(),
            primary_overlay
                .as_ref()
                .and_then(|value| value.get("profile"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        object.insert(
            "default_agent_role".to_owned(),
            primary_overlay
                .as_ref()
                .and_then(|value| value.get("agent_role"))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
}

#[cfg(test)]
mod overlay_surface_invariant_tests {
    //! F4 regression guard: a routing overlay must never point a host at a tool
    //! that is not callable in that overlay's own surface context. The judgment
    //! reuses `is_tool_callable_in_surface` — the exact gate that
    //! `dispatch/access.rs::validate_tool_access` enforces at runtime — so the
    //! advertised routing and the dispatcher cannot drift.

    use super::*;
    use crate::surface_manifest::HOST_ADAPTER_HOSTS;
    use crate::tool_defs::{ToolPreset, is_tool_callable_in_surface};

    // Every profile-labeled overlay may only reference tools callable in that
    // overlay's own profile execution surface (dispatch-only tools the surface
    // whitelists are allowed). reviewer-graph's review overlay, for instance, is
    // legitimate as long as each referenced tool is callable once the session is
    // on the reviewer-graph profile.
    #[test]
    fn compiled_overlays_only_reference_tools_callable_in_their_profile_surface() {
        for host in HOST_ADAPTER_HOSTS {
            let Some(host_context) = host_context_for_adapter(host) else {
                continue;
            };
            for spec in overlay_specs_for_host(host) {
                let surface = ToolSurface::Profile(spec.profile);
                let plan = compile_surface_overlay_for_agent(
                    surface,
                    Some(host_context),
                    Some(spec.task_overlay),
                    Some(spec.agent_role),
                );

                // The bootstrap prefix plus the compiled preferred entrypoints
                // and emphasized tools are everything the overlay tells the host
                // to reach for.
                let mut referenced: Vec<&str> = vec!["prepare_harness_session"];
                for tool in plan
                    .preferred_entrypoints
                    .iter()
                    .chain(plan.emphasized_tools.iter())
                {
                    if !referenced.contains(tool) {
                        referenced.push(tool);
                    }
                }

                for tool in referenced {
                    assert!(
                        is_tool_callable_in_surface(tool, surface),
                        "host `{host}` overlay `{}`+`{}` references `{tool}`, which is not callable in its `{}` profile surface",
                        spec.profile.as_str(),
                        spec.task_overlay.as_str(),
                        spec.profile.as_str(),
                    );
                }
            }
        }
    }

    // The unlabeled primary/bootstrap sequence is what the repo's own default
    // session executes. This repo binds `claude-code` and sends no
    // `x-codelens-profile` header, so the active surface is the Balanced preset;
    // every tool the primary sequence names must therefore be callable in
    // Balanced. This is the direct F4 guard: a Balanced-excluded tool (e.g.
    // audit_planner_session, which is legitimate only inside the reviewer-graph
    // profile overlay) must never surface in the default primary sequence.
    #[test]
    fn primary_bootstrap_sequence_is_callable_in_default_balanced_preset() {
        let balanced = ToolSurface::Preset(ToolPreset::Balanced);
        for tool in primary_bootstrap_sequence_for_host("claude-code") {
            assert!(
                is_tool_callable_in_surface(&tool, balanced),
                "claude-code primary bootstrap sequence references `{tool}`, which is not callable in the default Balanced preset",
            );
        }
    }
}
