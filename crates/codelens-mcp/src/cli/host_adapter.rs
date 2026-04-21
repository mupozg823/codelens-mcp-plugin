//! Host adapter management — inspect / render / detach / doctor / attach.
//!
//! Reads `HOST_ADAPTER_HOSTS` from `surface_manifest` to generate,
//! verify, and remove the host-native config files for each supported
//! MCP client (Claude Code, Codex, Cursor, Cline, Windsurf).
//!
//! File formats handled:
//! - JSON  — `.cursor/mcp.json`, `~/.claude/mcp.json` etc.
//! - TOML  — `~/.codex/config.toml`
//! - text policy — markdown / mdc files with optional `<!-- CODELENS_HOST_ROUTING:BEGIN -->` blocks
//!
//! Extracted from `cli.rs` in v1.9.50 to separate CLI parsing from host
//! adapter management. External API (three `pub(super)` runners) is
//! re-exported via `cli/mod.rs`.

mod attach;
mod detach;
mod doctor;
mod inspect;
mod json_config;
mod render;
mod resolve;
mod text_policy;
mod toml_config;

pub(crate) use attach::render_attach_instructions;
pub(crate) use detach::run_detach_command;
pub(crate) use doctor::run_doctor_command;

#[cfg(test)]
use self::{
    detach::{parse_detach_hosts, render_detach_report},
    doctor::{parse_doctor_hosts, render_doctor_report},
    resolve::canonical_attach_host,
    text_policy::{inspect_text_policy_file, inspect_text_policy_file_json},
};

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-startup-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn attach_host_aliases_normalize_to_canonical_host_ids() {
        assert_eq!(canonical_attach_host("claude"), Some("claude-code"));
        assert_eq!(canonical_attach_host("claudecode"), Some("claude-code"));
        assert_eq!(canonical_attach_host("codeium"), Some("windsurf"));
    }

    #[test]
    fn render_attach_instructions_for_codex_emits_copy_ready_targets() {
        let rendered = render_attach_instructions(Some("codex")).expect("attach output");
        assert!(rendered.contains("CodeLens attach target: codex"));
        assert!(rendered.contains("Native host primitives:"));
        assert!(rendered.contains("Use CodeLens for:"));
        assert!(rendered.contains("Avoid:"));
        assert!(rendered.contains("Primary bootstrap sequence:"));
        assert!(rendered.contains("Delegate scaffold contract:"));
        assert!(rendered.contains("Compiled overlays:"));
        assert!(rendered.contains("## Compiled Routing Overlays"));
        assert!(rendered.contains("delegate_to_codex_builder"));
        assert!(rendered.contains("handoff_id"));
        assert!(rendered.contains("~/.codex/config.toml"));
        assert!(rendered.contains("AGENTS.md"));
        assert!(rendered.contains("<!-- CODELENS_HOST_ROUTING:BEGIN -->"));
        assert!(rendered.contains("<!-- CODELENS_HOST_ROUTING:END -->"));
        assert!(rendered.contains("worktrees"));
        assert!(rendered.contains("analysis jobs for CI-facing summaries"));
        assert!(
            rendered
                .contains("copying Claude-specific subagent topology into Codex worktree flows")
        );
        assert!(rendered.contains("Verify the host wiring with `codelens-mcp doctor codex`"));
        assert!(rendered.contains("builder-minimal / editing"));
        assert!(rendered.contains("builder-minimal"));
        assert!(rendered.contains("refactor-full"));
    }

    #[test]
    fn render_attach_instructions_for_cursor_surfaces_delegate_handoff_contract() {
        let rendered = render_attach_instructions(Some("cursor")).expect("attach output");
        assert!(rendered.contains("CodeLens attach target: cursor"));
        assert!(rendered.contains("Native host primitives:"));
        assert!(rendered.contains("background agents"));
        assert!(rendered.contains("Use CodeLens for:"));
        assert!(rendered.contains("analysis jobs for background-agent queues"));
        assert!(rendered.contains("Avoid:"));
        assert!(rendered.contains("shipping the full CodeLens surface into every mode"));
        assert!(rendered.contains("Primary bootstrap sequence:"));
        assert!(rendered.contains("Delegate scaffold contract:"));
        assert!(rendered.contains("Compiled overlays:"));
        assert!(rendered.contains("## Compiled Routing Overlays"));
        assert!(rendered.contains("delegate_to_codex_builder"));
        assert!(rendered.contains("handoff_id"));
        assert!(rendered.contains(".cursor/rules/codelens-routing.mdc"));
    }

    #[test]
    fn render_attach_instructions_accepts_windsurf_aliases() {
        let rendered = render_attach_instructions(Some("codeium")).expect("attach output");
        assert!(rendered.contains("CodeLens attach target: windsurf"));
        assert!(rendered.contains("Requested alias: codeium -> windsurf"));
        assert!(rendered.contains("~/.codeium/windsurf/mcp_config.json"));
    }

    #[test]
    fn render_attach_instructions_rejects_unknown_hosts() {
        let error =
            render_attach_instructions(Some("openhands")).expect_err("unknown host must fail");
        assert!(error.to_string().contains("unknown attach host"));
    }

    #[test]
    fn render_attach_instructions_reports_project_local_url_override() {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = temp_dir("attach-override");
        std::fs::create_dir_all(root.join(".codelens")).unwrap();
        std::fs::write(
            root.join(".codelens/config.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "host_attach": {
                    "per_host_urls": {
                        "cursor": "http://127.0.0.1:7839/mcp"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let rendered = render_attach_instructions(Some("cursor")).expect("attach output");
        std::env::set_current_dir(previous).unwrap();

        assert!(rendered.contains(
            "Project-local daemon URL override from `.codelens/config.json host_attach.per_host_urls.cursor`: `http://127.0.0.1:7839/mcp`."
        ));
        assert!(rendered.contains("\"url\": \"http://127.0.0.1:7839/mcp\""));
    }

    #[test]
    fn detach_report_removes_codelens_json_entry_and_keeps_other_servers() {
        let root = temp_dir("detach-json");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" },
                    "other": { "type": "http", "url": "http://127.0.0.1:9999/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let report = render_detach_report(&["cursor"], &home, &cwd, true).expect("detach report");
        let updated = std::fs::read_to_string(cwd.join(".cursor/mcp.json")).unwrap();
        assert!(report.contains("Adapter resource: codelens://host-adapters/cursor"));
        assert!(report.contains("Preferred profiles: planner-readonly, reviewer-graph, ci-audit"));
        assert!(report.contains("Host-native targets: .cursor/rules, AGENTS.md, .cursor/mcp.json, background-agent environment.json"));
        assert!(report.contains("removed CodeLens config entry"));
        assert!(!updated.contains("\"codelens\""));
        assert!(updated.contains("\"other\""));
    }

    #[test]
    fn detach_report_removes_codelens_toml_section_only() {
        let root = temp_dir("detach-toml");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            r#"[mcp_servers.codelens]
url = "http://127.0.0.1:7837/mcp"

[mcp_servers.other]
url = "http://127.0.0.1:9999/mcp"
"#,
        )
        .unwrap();

        let report = render_detach_report(&["codex"], &home, &cwd, true).expect("detach report");
        let updated = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(report.contains("Adapter resource: codelens://host-adapters/codex"));
        assert!(report.contains("Native host primitives: AGENTS.md, skills, worktrees, shared MCP config, CLI, app, and IDE continuity"));
        assert!(report.contains(
            "Host-native targets: AGENTS.md, ~/.codex/config.toml, repo-local skill files"
        ));
        assert!(report.contains("removed CodeLens TOML section"));
        assert!(!updated.contains("[mcp_servers.codelens]"));
        assert!(updated.contains("[mcp_servers.other]"));
    }

    #[test]
    fn detach_report_requires_manual_cleanup_for_modified_policy_file() {
        let root = temp_dir("detach-manual");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("AGENTS.md"), "# CodeLens Routing\n\ncustomized\n").unwrap();

        let report = render_detach_report(&["codex"], &home, &cwd, true).expect("detach report");
        assert!(report.contains("manual cleanup required"));
        assert!(cwd.join("AGENTS.md").exists());
    }

    #[test]
    fn detach_cli_accepts_all_flag() {
        let hosts = parse_detach_hosts(&[
            "codelens-mcp".to_owned(),
            "detach".to_owned(),
            "--all".to_owned(),
        ])
        .expect("detach hosts");

        assert!(hosts.contains(&"claude-code"));
        assert!(hosts.contains(&"codex"));
        assert!(hosts.contains(&"windsurf"));
    }

    #[test]
    fn doctor_report_detects_machine_attachment_and_customized_policy_file() {
        let root = temp_dir("doctor-host");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(cwd.join(".cursor/rules")).unwrap();
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" },
                    "other": { "type": "http", "url": "http://127.0.0.1:9999/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            cwd.join(".cursor/rules/codelens-routing.mdc"),
            "---\ndescription: Route CodeLens usage by task risk and phase\nalwaysApply: true\n---\n\nCustomized locally.\n",
        )
        .unwrap();

        let report =
            render_doctor_report("doctor", &["cursor"], &home, &cwd).expect("doctor report");
        assert!(report.contains("CodeLens doctor report"));
        assert!(report.contains("Adapter resource: codelens://host-adapters/cursor"));
        assert!(report.contains(".cursor/mcp.json [json]: attached (exact CodeLens entry)"));
        assert!(report.contains(".cursor/rules/codelens-routing.mdc [mdc]: present (customized)"));
        assert!(report.contains("Interpretation:"));
    }

    #[test]
    fn doctor_report_marks_missing_codex_files() {
        let root = temp_dir("doctor-missing");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        let report =
            render_doctor_report("doctor", &["codex"], &home, &cwd).expect("doctor report");
        assert!(report.contains(".codex/config.toml [toml]: missing"));
        assert!(report.contains("AGENTS.md [markdown]: missing"));
    }

    #[test]
    fn inspect_text_policy_file_treats_exact_managed_block_as_present_exact() {
        let root = temp_dir("doctor-managed-block-exact");
        let file = root.join("AGENTS.md");
        let expected = r#"<!-- CODELENS_HOST_ROUTING:BEGIN -->
## CodeLens Routing

- Native first.
<!-- CODELENS_HOST_ROUTING:END -->
"#;
        std::fs::write(
            &file,
            format!(
                "# Repo Notes\n\n{}\n## Local Notes\n\nKeep the rest of the file.\n",
                expected.trim_end()
            ),
        )
        .unwrap();

        let text_status = inspect_text_policy_file(&file, expected, "markdown");
        let json_status = inspect_text_policy_file_json(&file, expected, "markdown");

        assert!(text_status.contains("present (exact managed block)"));
        assert_eq!(json_status["status"], "present_exact");
        assert_eq!(json_status["message"], "present (exact managed block)");
    }

    #[test]
    fn inspect_text_policy_file_treats_modified_managed_block_as_present_customized() {
        let root = temp_dir("doctor-managed-block-customized");
        let file = root.join("CLAUDE.md");
        let expected = r#"<!-- CODELENS_HOST_ROUTING:BEGIN -->
## CodeLens Routing

- Native first.
<!-- CODELENS_HOST_ROUTING:END -->
"#;
        let customized = r#"<!-- CODELENS_HOST_ROUTING:BEGIN -->
## CodeLens Routing

- Native first, but customized locally.
<!-- CODELENS_HOST_ROUTING:END -->
"#;
        std::fs::write(
            &file,
            format!(
                "# Project Notes\n\n{}\n## Remaining Instructions\n\nLocal content.\n",
                customized.trim_end()
            ),
        )
        .unwrap();

        let text_status = inspect_text_policy_file(&file, expected, "markdown");
        let json_status = inspect_text_policy_file_json(&file, expected, "markdown");

        assert!(text_status.contains("present (customized managed block)"));
        assert_eq!(json_status["status"], "present_customized");
        assert_eq!(json_status["message"], "present (customized managed block)");
    }

    #[test]
    fn doctor_report_treats_matching_toml_section_as_exact_even_with_extra_sections() {
        let root = temp_dir("doctor-toml-section-exact");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            r#"sandbox_mode = "workspace-write"

[mcp_servers.codelens]
url = "http://127.0.0.1:7837/mcp"

[mcp_servers.other]
url = "http://127.0.0.1:9999/mcp"
"#,
        )
        .unwrap();

        let report =
            render_doctor_report("doctor", &["codex"], &home, &cwd).expect("doctor report");
        assert!(report.contains(".codex/config.toml [toml]: attached (exact generated file)"));
    }

    #[test]
    fn doctor_cli_accepts_all_flag() {
        let hosts = parse_doctor_hosts(&[
            "codelens-mcp".to_owned(),
            "doctor".to_owned(),
            "--all".to_owned(),
        ])
        .expect("doctor hosts");

        assert!(hosts.contains(&"claude-code"));
        assert!(hosts.contains(&"codex"));
        assert!(hosts.contains(&"windsurf"));
    }

    #[test]
    fn doctor_cli_accepts_json_flag_before_host() {
        let hosts = parse_doctor_hosts(&[
            "codelens-mcp".to_owned(),
            "doctor".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("doctor hosts");
        assert_eq!(hosts, vec!["cursor"]);
    }

    #[test]
    fn run_doctor_command_renders_json_report() {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = temp_dir("doctor-json");
        let home = root.join("home");
        let cwd = root.join("repo");
        let previous_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", &home);
        }
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        let rendered = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "doctor".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("doctor json");
        std::env::set_current_dir(previous).unwrap();
        unsafe {
            match previous_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }

        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("valid doctor json");
        assert_eq!(payload["command"], serde_json::json!("doctor"));
        assert_eq!(payload["hosts"][0]["host"], serde_json::json!("cursor"));
        assert_eq!(
            payload["hosts"][0]["metadata"]["resource_uri"],
            serde_json::json!("codelens://host-adapters/cursor")
        );
        assert_eq!(
            payload["hosts"][0]["files"][0]["status"],
            serde_json::json!("attached_exact")
        );
    }

    #[test]
    fn run_status_command_renders_status_alias_in_text_and_json() {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = temp_dir("status-json");
        let home = root.join("home");
        let cwd = root.join("repo");
        let previous_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", &home);
        }
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        let text = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "status".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("status text");
        let rendered = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "status".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("status json");
        std::env::set_current_dir(previous).unwrap();
        unsafe {
            match previous_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }

        assert!(text.contains("CodeLens status report"));
        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("valid status json");
        assert_eq!(payload["command"], serde_json::json!("status"));
    }

    #[test]
    fn run_status_command_honors_project_local_host_attach_override() {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = temp_dir("status-override");
        let home = root.join("home");
        let cwd = root.join("repo");
        let previous_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", &home);
        }
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::create_dir_all(cwd.join(".codelens")).unwrap();
        std::fs::write(
            cwd.join(".codelens/config.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "host_attach": {
                    "per_host_urls": {
                        "cursor": "http://127.0.0.1:7839/mcp"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7839/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        let rendered = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "status".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("status json");
        std::env::set_current_dir(previous).unwrap();
        unsafe {
            match previous_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }

        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("valid status json");
        assert_eq!(
            payload["hosts"][0]["files"][0]["status"],
            serde_json::json!("attached_exact")
        );
    }
}
