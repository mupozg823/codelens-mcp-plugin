use crate::surface_manifest::{HARNESS_HOST_COMPAT_RESOURCE_URI, HOST_ADAPTER_HOSTS};
use serde_json::{Value, json};

pub(crate) fn static_resource_entries(project_name: &str) -> Vec<Value> {
    let mut items = vec![
        json!({
            "uri": "codelens://project/overview",
            "name": format!("Project: {project_name}"),
            "description": "Compressed project overview with active surface and index status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://project/architecture",
            "name": "Project Architecture",
            "description": "High-level architecture summary for harness planning",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list",
            "name": "Visible Tool Surface",
            "description": "Compressed role-aware tool surface summary",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://tools/list/full",
            "name": "Visible Tool Surface (Full)",
            "description": "Expanded role-aware tool surface with descriptions",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://surface/manifest",
            "name": "Surface Manifest",
            "description": "Canonical runtime and documentation surface manifest",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://surface/overlay",
            "name": "Surface Overlay Preview",
            "description": "Runtime preview of the (profile × host_context × task_overlay) compiled plan — query with ?host=<id>&task=<id>",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://backend/capabilities",
            "name": "Semantic Backend Capabilities",
            "description": "Passive capability map for the Rust engine, LSP bridge, and SCIP bridge backends — lists which capability each backend claims to fulfil",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://registry/projects",
            "name": "Project Registry",
            "description": "Active project plus registered secondary projects with memory availability, without requiring a tool call",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://registry/memory-scopes",
            "name": "Memory Scope Registry",
            "description": "Declared memory scopes (project + global) with current paths and mutation-wiring status",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://operator/dashboard",
            "name": "Operator Dashboard",
            "description": "Point-in-time operator snapshot — project + surface + index health + job queue + analysis summary + backends + memory scopes, aggregated from existing telemetry",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/modes",
            "name": "Harness Modes",
            "description": "Canonical harness-mode topology and communication policy",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/spec",
            "name": "Harness Spec",
            "description": "Portable harness contract with preflight, coordination, audit, and handoff templates",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://harness/host-adapters",
            "name": "Host Adapter Spec",
            "description": "Portable host-adaptation guidance for Claude Code, Codex, Cursor, Cline, Windsurf, and similar agent hosts",
            "mimeType": "application/json"
        }),
        json!({
            "uri": HARNESS_HOST_COMPAT_RESOURCE_URI,
            "name": "Resolved Harness Host",
            "description": "Compatibility summary for hosts that expect one resolved harness-host contract resource",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://host-instructions/audit",
            "name": "Host Instruction Audit",
            "description": "CLAUDE.md / AGENTS.md quality, staleness, duplication, and hook-export readiness audit",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://benchmarks/host-plugin-stack",
            "name": "Host Plugin Stack Benchmark",
            "description": "Upper-compatible benchmark against Session Report, CLAUDE.md Management, Serena MCP, and Hookify patterns",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://design/agent-experience",
            "name": "Agent Experience Spec",
            "description": "Portable UX, user-flow, agent-flow, tool-flow, and harness-flow contract",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://schemas/handoff-artifact/v1",
            "name": "Handoff Artifact Schema v1",
            "description": "JSON schema for planner -> builder -> reviewer handoff artifacts",
            "mimeType": "application/schema+json"
        }),
        json!({
            "uri": "codelens://stats/token-efficiency",
            "name": "Token Efficiency Stats",
            "description": "Session-level token, chain, and handle reuse metrics",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://session/http",
            "name": "HTTP Session Runtime",
            "description": "Shared daemon session counts, timeout, and resume support",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "codelens://activity/current",
            "name": "Current Agent Activity",
            "description": "Active agent registrations, advisory claims, and recent per-session activity",
            "mimeType": "application/json"
        }),
    ];
    items.extend(HOST_ADAPTER_HOSTS.iter().map(|host| {
        json!({
            "uri": format!("codelens://host-adapters/{host}"),
            "name": format!("Host Adapter: {host}"),
            "description": "Concrete host-native routing and template bundle",
            "mimeType": "application/json"
        })
    }));
    items
}
