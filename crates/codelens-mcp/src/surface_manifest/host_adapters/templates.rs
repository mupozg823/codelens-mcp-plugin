//! Raw host-native adapter templates.

use super::overlays::{
    append_compiled_overlay_section, augment_host_adapter_bundle, managed_host_policy_block,
};
use serde_json::{Value, json};

pub(super) fn raw_host_adapter_bundle(host: &str) -> Option<Value> {
    let mut bundle = match host {
        "claude-code" => json!({
            "name": "claude-code",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "planner and reviewer orchestration with isolated research and explicit policy control",
            "recommended_modes": ["solo-local", "planner-builder", "reviewer-gate"],
            "preferred_profiles": ["planner-readonly", "reviewer-graph"],
            "native_primitives": [
                "CLAUDE.md",
                "subagents and agent teams",
                "hooks",
                "managed-mcp.json and .mcp.json",
                "subagent-scoped MCP servers"
            ],
            "preferred_codelens_use": [
                "bootstrap and bounded architecture review",
                "preflight before dispatching a builder",
                "planner-session audit and handoff artifact production"
            ],
            "routing_defaults": {
                "point_lookup": "native-first",
                "multi_file_review": "codelens-after-first-local-step",
                "builder_dispatch": "planner-builder-handoff-required",
                "long_running_eval": "analysis-job-first"
            },
            "delegate_scaffold_rules": [
                "If `delegate_to_codex_builder` appears in suggested_next_calls, preserve delegate_tool, delegate_arguments, carry_forward, and handoff_id verbatim.",
                "Do not rewrite the first delegated builder call from prose."
            ],
            "avoid": [
                "defaulting to live bidirectional chat between planner and builder",
                "exposing mutation-heavy surfaces to read-side sessions"
            ],
            "compiler_targets": [
                "CLAUDE.md",
                ".mcp.json",
                "managed-mcp.json",
                "subagent definitions"
            ],
            "native_files": [
                {
                    "path": ".mcp.json",
                    "format": "json",
                    "purpose": "Attach the read-only CodeLens daemon to the project by default.",
                    "template": {
                        "mcpServers": {
                            "codelens": {
                                "type": "http",
                                "url": "http://127.0.0.1:7837/mcp"
                            }
                        }
                    }
                },
                {
                    "path": "CLAUDE.md",
                    "format": "markdown",
                    "purpose": "Carry the routing policy into Claude's project instructions.",
                    "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Use native Read/Glob/Grep first for trivial point lookups and single-file edits.
- Escalate to CodeLens after the first local step for multi-file review, refactor preflight, or durable artifact generation.
- Default CodeLens profile for planning/review is `reviewer-graph`.
- Before dispatching a builder, run:
  1. `prepare_harness_session`
  2. `get_symbols_overview` per target file
  3. `get_file_diagnostics` per target file
  4. `verify_change_readiness`
- Prefer asymmetric handoff over live planner/builder chat.
- If `delegate_to_codex_builder` appears in `suggested_next_calls`, preserve `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` verbatim when dispatching the builder.
"#, host))
                }
            ]
        }),
        "codex" => json!({
            "name": "codex",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "builder and refactor execution, parallel worktree-based implementation, and automation",
            "recommended_modes": ["solo-local", "planner-builder", "batch-analysis"],
            "preferred_profiles": ["builder-minimal", "refactor-full", "ci-audit"],
            "native_primitives": [
                "AGENTS.md",
                "skills",
                "worktrees",
                "shared MCP config",
                "CLI, app, and IDE continuity"
            ],
            "preferred_codelens_use": [
                "bounded mutation after verify_change_readiness",
                "session-scoped builder audit",
                "analysis jobs for CI-facing summaries"
            ],
            "routing_defaults": {
                "point_lookup": "native-first",
                "multi_file_build": "builder-minimal-after-bootstrap",
                "rename_or_broad_refactor": "refactor-full-after-preflight",
                "ci_summary": "analysis-job-first"
            },
            "delegate_scaffold_rules": [
                "If the planner hands you `delegate_to_codex_builder`, replay delegate_tool plus delegate_arguments unchanged for the first builder-heavy call.",
                "Preserve handoff_id exactly so planner-side emission and builder-side execution stay correlatable."
            ],
            "avoid": [
                "forcing CodeLens into trivial single-file lookups",
                "copying Claude-specific subagent topology into Codex worktree flows"
            ],
            "compiler_targets": [
                "AGENTS.md",
                "~/.codex/config.toml",
                "repo-local skill files"
            ],
            "native_files": [
                {
                    "path": "~/.codex/config.toml",
                    "format": "toml",
                    "purpose": "Share one CodeLens MCP attachment between the Codex CLI and IDE extension.",
                    "template": r#"[mcp_servers.codelens]
url = "http://127.0.0.1:7837/mcp"
"#
                },
                {
                    "path": "AGENTS.md",
                    "format": "markdown",
                    "purpose": "Tell Codex when to stay native and when to escalate into CodeLens workflow tools.",
                    "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- Default execution profile: `builder-minimal`.
- Use `refactor-full` only after `verify_change_readiness`; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
- If the planner hands you `delegate_to_codex_builder`, replay the first delegated builder call with `delegate_tool` + `delegate_arguments` unchanged, including `handoff_id`.
"#, host))
                }
            ]
        }),
        "cursor" => json!({
            "name": "cursor",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "editor-local iteration with scoped rules plus asynchronous remote execution when needed",
            "recommended_modes": ["solo-local", "reviewer-gate", "batch-analysis"],
            "preferred_profiles": ["planner-readonly", "reviewer-graph", "ci-audit"],
            "native_primitives": [
                ".cursor/rules",
                "AGENTS.md",
                "custom modes",
                "background agents",
                "mcp.json"
            ],
            "preferred_codelens_use": [
                "architecture review and diff-aware signoff",
                "analysis jobs for background-agent queues",
                "minimal surface exposure through mode- or rule-specific routing"
            ],
            "routing_defaults": {
                "foreground_lookup": "native-first",
                "foreground_review": "codelens-after-first-local-step",
                "background_queue": "analysis-job-first",
                "wide_surface": "deferred-loading-required"
            },
            "delegate_scaffold_rules": [
                "If CodeLens emits `delegate_to_codex_builder`, forward delegate_tool, delegate_arguments, carry_forward, and handoff_id to the builder lane.",
                "Do not regenerate builder arguments from prose when delegate_arguments are already present."
            ],
            "avoid": [
                "assuming foreground and background agents share the same trust boundary",
                "shipping the full CodeLens surface into every mode"
            ],
            "compiler_targets": [
                ".cursor/rules",
                "AGENTS.md",
                ".cursor/mcp.json",
                "background-agent environment.json"
            ],
            "native_files": [
                {
                    "path": ".cursor/mcp.json",
                    "format": "json",
                    "purpose": "Attach CodeLens to Cursor with the smallest stable project-local config.",
                    "template": {
                        "mcpServers": {
                            "codelens": {
                                "type": "http",
                                "url": "http://127.0.0.1:7837/mcp"
                            }
                        }
                    }
                },
                {
                    "path": ".cursor/rules/codelens-routing.mdc",
                    "format": "mdc",
                    "purpose": "Scope CodeLens to review-heavy and artifact-worthy tasks instead of every edit.",
                    "template": append_compiled_overlay_section(r#"---
description: Route CodeLens usage by task risk and phase
alwaysApply: true
---

- Use native code search and local file reads first for trivial lookups and single-file edits.
- Escalate to CodeLens when the task becomes multi-file, reviewer-heavy, refactor-sensitive, or needs durable analysis artifacts.
- Prefer `reviewer-graph` for review/signoff and `ci-audit` for async analysis summaries.
- In background-agent flows, assume localhost CodeLens is unavailable unless the daemon is reachable from the remote machine.
- If CodeLens emits `delegate_to_codex_builder`, pass `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` through to the builder lane instead of rewriting them from prose.
"#, host)
                }
            ]
        }),
        "cline" => json!({
            "name": "cline",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "human-in-the-loop debugging and foreground execution with explicit approvals",
            "recommended_modes": ["solo-local", "planner-builder"],
            "preferred_profiles": ["builder-minimal", "reviewer-graph"],
            "native_primitives": [
                "interactive permissioned terminal execution",
                "browser loop",
                "workspace checkpoints",
                "MCP integrations"
            ],
            "preferred_codelens_use": [
                "review-heavy exploration before write passes",
                "session audit and handoff artifacts when a change must cross sessions"
            ],
            "routing_defaults": {
                "foreground_debug": "native-first-with-codelens-escalation",
                "write_pass": "builder-minimal-after-bootstrap",
                "handoff": "artifact-required"
            },
            "avoid": [
                "treating Cline as a headless CI runner",
                "relying on CodeLens where the foreground checkpoint loop already provides the needed safety"
            ],
            "compiler_targets": [
                "mcp_servers.json",
                ".clinerules",
                "repo instructions"
            ],
            "native_files": [
                {
                    "path": "mcp_servers.json",
                    "format": "json",
                    "purpose": "Attach CodeLens to Cline with an explicit project-local server entry.",
                    "template": {
                        "codelens": {
                            "type": "http",
                            "url": "http://127.0.0.1:7837/mcp"
                        }
                    }
                },
                {
                    "path": ".clinerules",
                    "format": "markdown",
                    "purpose": "Keep CodeLens for reviewer-heavy or handoff-heavy flows, not every approval cycle.",
                    "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Use Cline's normal foreground loop for local debugging, browser checks, and explicit command approvals.
- Bring in CodeLens after the first local step when the task spans multiple files or needs refactor preflight.
- Use `reviewer-graph` for exploration and `builder-minimal` for bounded write passes.
- If work crosses sessions, export an audit or handoff artifact instead of relying on chat history.
"#, host))
                }
            ]
        }),
        "windsurf" => json!({
            "name": "windsurf",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "editor-local implementation with a hard MCP tool cap and bounded foreground agent flows",
            "recommended_modes": ["solo-local", "reviewer-gate"],
            "preferred_profiles": ["builder-minimal", "planner-readonly"],
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
                "multi_file_edit": "builder-minimal-after-bootstrap",
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
                                "url": "http://127.0.0.1:7837/mcp"
                            }
                        }
                    }
                }
            ]
        }),
        _ => return None,
    };

    augment_host_adapter_bundle(host, &mut bundle);
    Some(bundle)
}
