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

/// E6.3 — the host-neutral core of every generated routing block.
///
/// Host adapters wrap this with their own heading, one-line framing, and
/// verification commands, so the contract text itself cannot drift between
/// `CLAUDE.md`, `AGENTS.md`, and `.cursor/rules/`. Deliberately free of role,
/// lane, agent-topology, and model assignments (ADR-0015): the block states
/// what is always true about the data plane and how to verify the wiring, and
/// leaves executor choice entirely to the host.
pub(super) const HOST_ROUTING_INVARIANTS: &str = r#"### Invariants

- Native file reads and text search stay first for point lookups and single-file
  edits. Escalate to CodeLens once a task spans multiple files, needs reference
  or impact evidence, or has to leave a durable artifact.
- Bind the workspace before the first analysis call: `prepare_harness_session`
  with an absolute project path. `get_current_config` reports the binding that
  is actually in effect; a stale binding is a reason to rebind, not a reason to
  abandon the index.
- Analysis answers are index reads, not file reads. They are only as fresh as
  the committed index generation, so a result that contradicts an edit you just
  made is stale rather than authoritative.
- Pin a multi-call read to a single index snapshot, and retry the call unchanged
  when the server reports that the generation moved underneath it.
- One writable runtime per project. A second writer is rejected outright and is
  never silently downgraded to a read-only fallback — surface the rejection.
- Follow-up suggestions in a response are intent, not execution. The host picks
  the executor and applies its own approval and mutation gates.
- Report observable host facts through `host_capabilities` and its sibling
  inputs: capability flags, MCP server and tool names, roots, and setting key
  names. Names, paths, and flags only — never secret values.
- Mutation is gated: run `verify_change_readiness` on the target paths, clear
  the blockers it reports, then re-run `diagnose` on those paths afterwards.
- An unreachable or failing daemon falls back to native tools. Nothing in this
  contract may block work on CodeLens being available.

### Default calls

- Find code — `search` (mode=symbol|refs|defn|impl|semantic|ranked)
- Read structure — `overview` (mode=file|explore)
- Relationships and blast radius — `graph` (mode=callers|callees|impact|trace)
- Health — `diagnose` (mode=file|symbol|unresolved)
- Reports — `review` (mode=architecture|changes|dead|dupes)
- Whole-repo work — `start_analysis_job`, poll `get_analysis_job`, then expand
  only the sections you need with `get_analysis_section`"#;

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
