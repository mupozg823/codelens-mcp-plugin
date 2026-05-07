use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const REQUIRED_ROOT_INSTRUCTION_FILES: &[(&str, &str)] =
    &[("AGENTS.md", "codex"), ("CLAUDE.md", "claude-code")];

const OPTIONAL_ROOT_INSTRUCTION_FILES: &[(&str, &str)] = &[
    ("CLAUDE.local.md", "claude-code-local"),
    (".claude.md", "claude-code"),
    (".claude.local.md", "claude-code-local"),
];

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

fn contains_stale_copy_paste_command(text: &str) -> bool {
    let mut in_fence = false;
    for line in text.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }

        let command = line.trim().trim_start_matches('$').trim_start();
        if command.starts_with("cargo test -p codelens-mcp --lib") {
            return true;
        }
    }
    false
}

fn grade(score: u64) -> &'static str {
    match score {
        90..=100 => "A",
        70..=89 => "B",
        50..=69 => "C",
        30..=49 => "D",
        _ => "F",
    }
}

fn score_commands(lower: &str) -> u64 {
    let has_build = contains_any(lower, &["cargo check", "cargo build", "npm run build"]);
    let has_test = contains_any(lower, &["cargo test", "nextest", "pytest", "npm test"]);
    let has_lint = contains_any(lower, &["cargo clippy", "cargo fmt", "lint"]);
    match (has_build, has_test, has_lint) {
        (true, true, true) => 20,
        (true, true, false) | (true, false, true) | (false, true, true) => 15,
        (true, false, false) | (false, true, false) | (false, false, true) => 10,
        _ if lower.contains("```") => 6,
        _ => 0,
    }
}

fn score_architecture(lower: &str) -> u64 {
    let has_map = contains_any(lower, &["architecture", "repository", "workspace", "crate"]);
    let has_paths = contains_any(lower, &["crates/", "src/", "docs/", "scripts/"]);
    let has_relationships = contains_any(lower, &["owns", "depends", "surface", "dispatch"]);
    match (has_map, has_paths, has_relationships) {
        (true, true, true) => 20,
        (true, true, false) | (true, false, true) => 15,
        (true, false, false) | (false, true, false) => 10,
        _ => 0,
    }
}

fn score_patterns(lower: &str) -> u64 {
    let signals = [
        "pitfall",
        "gotcha",
        "do not",
        "must",
        "warning",
        "routing",
        "mutation gate",
        "generated",
    ]
    .iter()
    .filter(|signal| lower.contains(**signal))
    .count();
    match signals {
        6.. => 15,
        4..=5 => 12,
        2..=3 => 8,
        1 => 4,
        _ => 0,
    }
}

fn score_conciseness(line_count: usize) -> u64 {
    match line_count {
        1..=220 => 15,
        221..=360 => 10,
        361..=520 => 6,
        521..=800 => 3,
        _ => 0,
    }
}

fn score_currency(text: &str, lower: &str) -> u64 {
    let has_current_verify = contains_any(
        lower,
        &["surface-manifest.py --check", "regen-tool-defs.py --check"],
    );
    let markers_balanced =
        count_occurrences(text, ":BEGIN -->") == count_occurrences(text, ":END -->");
    let avoids_known_bad = !contains_stale_copy_paste_command(text);
    let has_current_year = lower.contains("2026") || lower.contains("1.13.");
    let mut score = 0;
    if has_current_verify {
        score += 5;
    }
    if markers_balanced {
        score += 4;
    }
    if avoids_known_bad {
        score += 3;
    }
    if has_current_year {
        score += 3;
    }
    score
}

fn score_actionability(lower: &str) -> u64 {
    let has_code_blocks = lower.contains("```");
    let has_absolute_or_repo_paths =
        contains_any(lower, &["crates/", "scripts/", ".codelens/", "docs/"]);
    let has_decision_rules = contains_any(lower, &["before", "after", "prefer", "use ", "run "]);
    match (
        has_code_blocks,
        has_absolute_or_repo_paths,
        has_decision_rules,
    ) {
        (true, true, true) => 15,
        (true, true, false) | (true, false, true) | (false, true, true) => 10,
        (true, false, false) | (false, true, false) | (false, false, true) => 5,
        _ => 0,
    }
}

fn push_finding(
    findings: &mut Vec<Value>,
    code: &str,
    severity: &str,
    message: &str,
    action: &str,
) {
    findings.push(json!({
        "code": code,
        "severity": severity,
        "message": message,
        "recommended_action": action,
    }));
}

fn audit_existing_file(path: PathBuf, host: &str, text: String) -> Value {
    let lower = text.to_ascii_lowercase();
    let line_count = text.lines().count();
    let byte_count = text.len();
    let command_score = score_commands(&lower);
    let architecture_score = score_architecture(&lower);
    let pattern_score = score_patterns(&lower);
    let conciseness_score = score_conciseness(line_count);
    let currency_score = score_currency(&text, &lower);
    let actionability_score = score_actionability(&lower);
    let total_score = command_score
        + architecture_score
        + pattern_score
        + conciseness_score
        + currency_score
        + actionability_score;

    let mut findings = Vec::new();
    if line_count > 360 {
        push_finding(
            &mut findings,
            "manifest_too_long",
            "warn",
            "Instruction manifest is large enough to become a startup-token tax and may bury routing-critical rules.",
            "Move stable reference material into docs/resources and keep the manifest focused on commands, architecture, pitfalls, and routing thresholds.",
        );
    }
    if command_score < 15 {
        push_finding(
            &mut findings,
            "weak_verify_commands",
            "warn",
            "Build/test/lint commands are incomplete or hard to detect.",
            "Document copy-ready build, test, lint, and codegen drift commands.",
        );
    }
    if architecture_score < 15 {
        push_finding(
            &mut findings,
            "weak_architecture_map",
            "warn",
            "Architecture map lacks enough path or ownership signal.",
            "Name the key packages/directories and explain which runtime responsibility each owns.",
        );
    }
    if count_occurrences(&text, ":BEGIN -->") != count_occurrences(&text, ":END -->") {
        push_finding(
            &mut findings,
            "generated_block_unbalanced",
            "fail",
            "Generated marker blocks are unbalanced.",
            "Run `python3 scripts/surface-manifest.py --write` and avoid hand-editing generated regions.",
        );
    }
    if contains_stale_copy_paste_command(&text) {
        push_finding(
            &mut findings,
            "stale_codelens_mcp_lib_command",
            "fail",
            "Manifest exposes copy-pasteable `cargo test -p codelens-mcp --lib`, but the package has no lib target.",
            "Replace it with `cargo test -p codelens-mcp --bin codelens-mcp` or `cargo test -p codelens-mcp`.",
        );
    }

    json!({
        "path": path.to_string_lossy(),
        "host_target": host,
        "exists": true,
        "line_count": line_count,
        "byte_count": byte_count,
        "score": total_score,
        "grade": grade(total_score),
        "criteria": {
            "commands_workflows": command_score,
            "architecture_clarity": architecture_score,
            "non_obvious_patterns": pattern_score,
            "conciseness": conciseness_score,
            "currency": currency_score,
            "actionability": actionability_score,
        },
        "findings": findings,
    })
}

fn audit_missing_file(path: PathBuf, host: &str) -> Value {
    json!({
        "path": path.to_string_lossy(),
        "host_target": host,
        "exists": false,
        "score": 0,
        "grade": "F",
        "criteria": {
            "commands_workflows": 0,
            "architecture_clarity": 0,
            "non_obvious_patterns": 0,
            "conciseness": 0,
            "currency": 0,
            "actionability": 0,
        },
        "findings": [{
            "code": "missing_manifest",
            "severity": "info",
            "message": "Optional host instruction manifest is not present.",
            "recommended_action": "Add only if this host is supported by the project workflow; do not create duplicate policy files by default."
        }],
    })
}

fn instruction_file_candidates(project_root: &Path) -> Vec<(&'static str, &'static str)> {
    REQUIRED_ROOT_INSTRUCTION_FILES
        .iter()
        .copied()
        .chain(
            OPTIONAL_ROOT_INSTRUCTION_FILES
                .iter()
                .copied()
                .filter(|(relative, _)| project_root.join(relative).exists()),
        )
        .collect()
}

fn benchmark_mapping() -> Value {
    json!([
        {
            "reference": "Session Report",
            "absorbed_as": "get_tool_metrics.token_bill and codelens://stats/token-efficiency.token_bill",
            "status": "implemented",
            "remaining_gap": "Persisted transcript import with USD pricing remains outside the MCP response path."
        },
        {
            "reference": "CLAUDE.md Management",
            "absorbed_as": "codelens://host-instructions/audit",
            "status": "implemented_in_this_resource",
            "remaining_gap": "The resource audits and recommends; it does not rewrite manifests automatically."
        },
        {
            "reference": "Serena MCP",
            "absorbed_as": "project memory registry, rule retrieval, and host-instruction audit",
            "status": "partial",
            "remaining_gap": "Memory entries are not yet promoted from session audit findings automatically."
        },
        {
            "reference": "Hookify",
            "absorbed_as": "recommended_hook_exports and hook_settings_templates",
            "status": "implemented_in_this_resource",
            "remaining_gap": "Templates are exported as candidate settings fragments; CodeLens does not install or trust-enable host hooks automatically."
        }
    ])
}

fn upper_compatible_layers() -> Value {
    json!([
        {
            "layer": "session_economics",
            "reference_tools": ["Session Report", "ccusage"],
            "reference_pattern": "Offline transcript aggregation: per-session token and cost summaries after the run.",
            "codelens_current": [
                "Live MCP telemetry per logical session",
                "token_bill.top_token_tools",
                "token_bill.waste_signals",
                "workflow follow-through and cache-hit KPIs"
            ],
            "upper_compatible_delta": "CodeLens can guide the next tool call while the session is still running; transcript-cost import is the remaining offline reporting gap."
        },
        {
            "layer": "instruction_memory",
            "reference_tools": ["CLAUDE.md Management"],
            "reference_pattern": "Score CLAUDE.md quality and propose approved updates from session learnings.",
            "codelens_current": [
                "AGENTS.md + CLAUDE.md scoring",
                "staleness and duplicate-manifest findings",
                "host-specific guidance without rewriting files by default"
            ],
            "upper_compatible_delta": "CodeLens keeps host-neutral policy evidence in resources so Codex, Claude Code, Cursor, and similar hosts can consume one audit surface."
        },
        {
            "layer": "semantic_code_intelligence",
            "reference_tools": ["Serena MCP"],
            "reference_pattern": "Symbol-level LSP retrieval/editing, project activation, and persistent memory.",
            "codelens_current": [
                "Hybrid tree-sitter + SCIP + LSP + embedding retrieval",
                "Verifier-gated mutation preflight",
                "durable analysis handles and bounded reports"
            ],
            "upper_compatible_delta": "CodeLens is already stronger as a harness-control layer; Serena still leads broad IDE-grade backend coverage until CodeLens completes active backend routing."
        },
        {
            "layer": "behavior_guardrails",
            "reference_tools": ["Hookify", "Claude Code hooks"],
            "reference_pattern": "Rules and hooks block or warn on unsafe commands, edits, prompts, and session stops.",
            "codelens_current": [
                "mutation gate",
                "recommended_hook_exports",
                "hook_settings_templates"
            ],
            "upper_compatible_delta": "CodeLens exports host-hook candidates from the same audit evidence used by MCP tools, reducing drift between prompt policy and runtime gates."
        }
    ])
}

fn recommended_hook_exports() -> Value {
    json!([
        {
            "event": "Stop",
            "matcher": null,
            "purpose": "Block premature completion when code changed but build/test evidence is absent.",
            "status": "candidate",
            "code_lens_source": "token_bill + builder/planner audits"
        },
        {
            "event": "PreToolUse",
            "matcher": "Bash",
            "purpose": "Deny destructive or policy-violating shell commands before execution.",
            "status": "candidate",
            "code_lens_source": "mutation gate and repo instruction audit"
        },
        {
            "event": "SessionStart",
            "matcher": "startup",
            "purpose": "Load compact host routing and recent audit findings without forcing full tools/list.",
            "status": "candidate",
            "code_lens_source": "prepare_harness_session + codelens://host-instructions/audit"
        }
    ])
}

fn hook_settings_templates() -> Value {
    json!({
        "schema_version": "codelens-hook-settings-templates-v1",
        "target_files": [".claude/settings.json", ".claude/settings.local.json"],
        "status": "candidate_templates",
        "install_policy": "Never auto-install; host hooks should be reviewed and enabled by the user or repository owner.",
        "templates": [
            {
                "name": "stop_evidence_gate",
                "purpose": "Warn or block premature completion when code changed but no build/test evidence is present.",
                "event": "Stop",
                "matcher": "",
                "settings_fragment": {
                    "hooks": {
                        "Stop": [
                            {
                                "matcher": "",
                                "hooks": [
                                    {
                                        "type": "prompt",
                                        "timeout": 30,
                                        "prompt": "You are a strict completion gate. Inspect this Stop hook input and allow completion only when changed-code work has explicit build/test evidence or the user explicitly waived verification. Return JSON with decision allow|block and a concise reason. Input: $ARGUMENTS"
                                    }
                                ]
                            }
                        ]
                    }
                },
                "codelens_signal_source": ["token_bill", "audit_builder_session", "audit_planner_session"]
            },
            {
                "name": "pretool_destructive_bash_gate",
                "purpose": "Warn before destructive shell commands that can erase user work or bypass mutation gates.",
                "event": "PreToolUse",
                "matcher": "Bash",
                "settings_fragment": {
                    "hooks": {
                        "PreToolUse": [
                            {
                                "matcher": "Bash",
                                "hooks": [
                                    {
                                        "type": "prompt",
                                        "timeout": 15,
                                        "prompt": "Review the Bash command in this hook input. Block commands that delete, reset, overwrite, chmod/chown recursively, or rewrite git history unless the user explicitly requested that exact destructive action. Return JSON with decision allow|block and reason. Input: $ARGUMENTS"
                                    }
                                ]
                            }
                        ]
                    }
                },
                "codelens_signal_source": ["mutation_gate", "instruction_manifest_audit"]
            },
            {
                "name": "sessionstart_compact_bootstrap",
                "purpose": "Load compact CodeLens routing evidence at session start without forcing full tools/list.",
                "event": "SessionStart",
                "matcher": "startup",
                "settings_fragment": {
                    "hooks": {
                        "SessionStart": [
                            {
                                "matcher": "startup",
                                "hooks": [
                                    {
                                        "type": "mcp_tool",
                                        "server": "codelens",
                                        "tool": "prepare_harness_session",
                                        "input": {
                                            "host_context": "claude-code",
                                            "task_overlay": "interactive",
                                            "detail": "compact"
                                        }
                                    }
                                ]
                            }
                        ]
                    }
                },
                "codelens_signal_source": ["prepare_harness_session", "surface_overlay"]
            }
        ]
    })
}

pub(crate) fn host_plugin_stack_benchmark(project_root: &Path) -> Value {
    json!({
        "schema_version": "codelens-host-plugin-stack-benchmark-v1",
        "project_root": project_root.to_string_lossy(),
        "reference_scope": [
            "Session Report / ccusage style token reports",
            "CLAUDE.md Management",
            "Serena MCP",
            "Hookify / Claude Code hooks"
        ],
        "positioning": "CodeLens should remain the host-neutral harness-control layer above plugin-specific tools: observe cost, audit instructions, route semantic work, gate mutation, and export hook candidates from one evidence model.",
        "upper_compatible_layers": upper_compatible_layers(),
        "benchmark_mapping": benchmark_mapping(),
        "recommended_hook_exports": recommended_hook_exports(),
        "hook_settings_templates": hook_settings_templates(),
        "cherry_pick_backlog": [
            {
                "source": "Session Report / ccusage",
                "candidate": "Import Claude Code JSONL transcripts into token_bill for 7d/30d cost reports.",
                "priority": "P2",
                "why_not_now": "Current CodeLens telemetry is live MCP response telemetry; transcript pricing requires a separate local data reader and pricing table."
            },
            {
                "source": "CLAUDE.md Management",
                "candidate": "Generate approved manifest patch plans from audit findings.",
                "priority": "P1",
                "why_not_now": "This resource intentionally audits first; file mutation should go through normal mutation gates."
            },
            {
                "source": "Serena MCP",
                "candidate": "Finish active backend routing for move/inline/change-signature only where LSP/IDE returns inspectable WorkspaceEdit.",
                "priority": "P1",
                "why_not_now": "Requires per-language fixture gates before claiming authoritative edit coverage."
            },
            {
                "source": "Hookify",
                "candidate": "Promote recurring audit findings into hook rule suggestions with de-duplication and confidence.",
                "priority": "P1",
                "why_not_now": "Templates now exist; automatic promotion needs false-positive controls."
            }
        ]
    })
}

pub(crate) fn instruction_manifest_audit(project_root: &Path) -> Value {
    let files = instruction_file_candidates(project_root)
        .into_iter()
        .map(|(relative, host)| {
            let path = project_root.join(relative);
            match std::fs::read_to_string(&path) {
                Ok(text) => audit_existing_file(path, host, text),
                Err(_) => audit_missing_file(path, host),
            }
        })
        .collect::<Vec<_>>();

    let present = files
        .iter()
        .filter(|file| file["exists"].as_bool().unwrap_or(false))
        .count();
    let average_score = if present > 0 {
        files
            .iter()
            .filter(|file| file["exists"].as_bool().unwrap_or(false))
            .filter_map(|file| file["score"].as_u64())
            .sum::<u64>() as f64
            / present as f64
    } else {
        0.0
    };
    let blocking_findings = files
        .iter()
        .flat_map(|file| {
            file["findings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|finding| finding["severity"] == "fail")
        })
        .count();
    let warning_findings = files
        .iter()
        .flat_map(|file| {
            file["findings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|finding| finding["severity"] == "warn")
        })
        .count();

    json!({
        "scope": "project_root",
        "project_root": project_root.to_string_lossy(),
        "present_manifest_count": present,
        "average_score": average_score,
        "grade": grade(average_score.round() as u64),
        "blocking_findings": blocking_findings,
        "warning_findings": warning_findings,
        "files": files,
        "benchmark_mapping": benchmark_mapping(),
        "upper_compatible_layers": upper_compatible_layers(),
        "recommended_hook_exports": recommended_hook_exports(),
        "hook_settings_templates": hook_settings_templates(),
        "next_actions": [
            "Keep generated CodeLens routing blocks authoritative via `python3 scripts/surface-manifest.py --check`.",
            "Keep root manifests concise; move stable explanations to resources/docs and link them from manifests.",
            "Promote recurring session findings into memory or host-hook candidates only when they reduce repeated agent mistakes."
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_boundaries_are_stable() {
        assert_eq!(grade(100), "A");
        assert_eq!(grade(89), "B");
        assert_eq!(grade(69), "C");
        assert_eq!(grade(49), "D");
        assert_eq!(grade(29), "F");
    }

    #[test]
    fn stale_codelens_lib_command_is_blocking() {
        let report = audit_existing_file(
            PathBuf::from("CLAUDE.md"),
            "claude-code",
            "```bash\ncargo test -p codelens-mcp --lib\n```\n".to_owned(),
        );
        assert!(
            report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(
                    |finding| finding["code"] == "stale_codelens_mcp_lib_command"
                        && finding["severity"] == "fail"
                )
        );
    }

    #[test]
    fn stale_codelens_lib_antipattern_note_is_not_blocking() {
        let report = audit_existing_file(
            PathBuf::from("CLAUDE.md"),
            "claude-code",
            "The bin target is `codelens-mcp`; lib target does not exist - `cargo test -p codelens-mcp --lib` fails.\n".to_owned(),
        );
        assert!(
            !report["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| finding["code"] == "stale_codelens_mcp_lib_command")
        );
    }
}
