//! CLI argument parsing + project-root resolution + HTTP startup banner.
//!
//! Extracted from `main.rs` as of v1.9.32 to keep the binary entry point
//! focused on bootstrap/dispatch. All parsing functions are pure and test
//! co-located below.

use crate::state::RuntimeDaemonMode;
use crate::surface_manifest::HOST_ADAPTER_HOSTS;
use anyhow::{Context, Result};
use codelens_engine::ProjectRoot;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the startup project root came from, in priority order. Used for
/// diagnostic banners and the "refusing to start on `/` without explicit
/// project root" guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StartupProjectSource {
    Cli(String),
    ClaudeEnv(String),
    McpEnv(String),
    Cwd(PathBuf),
}

impl StartupProjectSource {
    pub(crate) fn is_explicit(&self) -> bool {
        !matches!(self, Self::Cwd(_))
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Cli(_) => "CLI path",
            Self::ClaudeEnv(_) => "CLAUDE_PROJECT_DIR",
            Self::McpEnv(_) => "MCP_PROJECT_DIR",
            Self::Cwd(_) => "current working directory",
        }
    }
}

/// Flags that consume the next argument as their value. Used by the
/// positional-project-arg parser to skip over `--flag value` pairs without
/// treating `value` as the project path.
fn flag_takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "--preset" | "--profile" | "--daemon-mode" | "--cmd" | "--args" | "--transport" | "--port"
    )
}

pub(crate) fn is_attach_subcommand(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("attach"))
}

pub(crate) fn is_detach_subcommand(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("detach"))
}

pub(crate) fn attach_host_arg(args: &[String]) -> Option<String> {
    args.get(2).cloned()
}

fn canonical_attach_host(host: &str) -> Option<&'static str> {
    match host.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" | "claudecode" => Some("claude-code"),
        "codex" => Some("codex"),
        "cursor" => Some("cursor"),
        "cline" => Some("cline"),
        "windsurf" | "codeium" => Some("windsurf"),
        _ => None,
    }
}

fn supported_attach_hosts() -> &'static str {
    "claude-code, codex, cursor, cline, windsurf"
}

fn home_dir_from_env() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot resolve host-native config paths")
}

fn resolve_host_path(raw: &str, home: &Path, cwd: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        cwd.join(raw)
    }
}

fn json_string_list(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn render_template(template: &Value) -> Result<String> {
    if let Some(text) = template.as_str() {
        Ok(text.to_owned())
    } else {
        serde_json::to_string_pretty(template).context("failed to render template as JSON")
    }
}

fn normalize_text_for_compare(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

fn parse_json_route_from_template(template: &Value) -> Option<(Vec<String>, String)> {
    let object = template.as_object()?;
    if object
        .get("mcpServers")
        .and_then(Value::as_object)
        .is_some_and(|map| map.contains_key("codelens"))
    {
        return Some((vec!["mcpServers".to_owned()], "codelens".to_owned()));
    }
    if object
        .get("servers")
        .and_then(Value::as_object)
        .is_some_and(|map| map.contains_key("codelens"))
    {
        return Some((vec!["servers".to_owned()], "codelens".to_owned()));
    }
    if object.contains_key("codelens") {
        return Some((Vec::new(), "codelens".to_owned()));
    }
    None
}

fn remove_json_key(value: &mut Value, parent_path: &[String], key: &str) -> bool {
    if parent_path.is_empty() {
        return value
            .as_object_mut()
            .and_then(|map| map.remove(key))
            .is_some();
    }

    let mut current = value;
    for segment in parent_path {
        let Some(next) = current.get_mut(segment) else {
            return false;
        };
        current = next;
    }

    current
        .as_object_mut()
        .and_then(|map| map.remove(key))
        .is_some()
}

fn prune_empty_json(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let empty_keys = map
                .iter_mut()
                .filter_map(|(key, child)| prune_empty_json(child).then_some(key.clone()))
                .collect::<Vec<_>>();
            for key in empty_keys {
                map.remove(&key);
            }
            map.is_empty()
        }
        Value::Array(items) => {
            items.retain_mut(|item| !prune_empty_json(item));
            items.is_empty()
        }
        _ => false,
    }
}

fn remove_json_config_entry(
    path: &Path,
    parent_path: &[String],
    key: &str,
    summary: &str,
    apply_changes: bool,
) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display}: not present");
    };
    let Ok(mut payload) = serde_json::from_str::<Value>(&content) else {
        return format!("- {display}: manual cleanup required ({summary}; invalid JSON)");
    };
    if !remove_json_key(&mut payload, parent_path, key) {
        return format!("- {display}: no CodeLens entry found");
    }
    prune_empty_json(&mut payload);
    if payload.as_object().is_some_and(|map| map.is_empty()) {
        if !apply_changes {
            return format!("- {display}: would remove empty config file");
        }
        match fs::remove_file(path) {
            Ok(()) => format!("- {display}: removed empty config file"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    } else {
        if !apply_changes {
            return format!("- {display}: would remove CodeLens config entry");
        }
        match fs::write(
            path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_owned())
            ),
        ) {
            Ok(()) => format!("- {display}: removed CodeLens config entry"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    }
}

fn extract_toml_section_name(template: &str) -> Option<String> {
    template.lines().find_map(|line| {
        let trimmed = line.trim();
        (trimmed.starts_with('[') && trimmed.ends_with(']')).then(|| {
            trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned()
        })
    })
}

fn remove_toml_section(path: &Path, section: &str, apply_changes: bool) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display}: not present");
    };
    let header = format!("[{section}]");
    let mut removed = false;
    let mut output = String::new();
    let mut skipping = false;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if !skipping && trimmed == header {
            removed = true;
            skipping = true;
            continue;
        }
        if skipping {
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                skipping = false;
            } else {
                continue;
            }
        }
        output.push_str(line);
    }

    if !removed {
        return format!("- {display}: no CodeLens section found");
    }

    let cleaned = output.trim().to_owned();
    if cleaned.is_empty() {
        if !apply_changes {
            return format!("- {display}: would remove empty config file");
        }
        match fs::remove_file(path) {
            Ok(()) => format!("- {display}: removed empty config file"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    } else {
        if !apply_changes {
            return format!("- {display}: would remove CodeLens TOML section");
        }
        match fs::write(path, format!("{cleaned}\n")) {
            Ok(()) => format!("- {display}: removed CodeLens TOML section"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    }
}

fn remove_exact_text_file(path: &Path, expected: &str, label: &str, apply_changes: bool) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display}: not present");
    };
    if normalize_text_for_compare(&content) != normalize_text_for_compare(expected) {
        return format!(
            "- {display}: manual cleanup required ({label} was modified after generation)"
        );
    }
    if !apply_changes {
        return format!("- {display}: would remove generated {label}");
    }
    match fs::remove_file(path) {
        Ok(()) => format!("- {display}: removed generated {label}"),
        Err(err) => format!("- {display}: manual cleanup required ({err})"),
    }
}

fn detach_host_files(
    host: &str,
    home: &Path,
    cwd: &Path,
    apply_changes: bool,
) -> Result<Vec<String>> {
    let adapter = crate::surface_manifest::host_adapter_bundle(host)
        .context("missing host adapter bundle for detach target")?;
    let native_files = adapter
        .get("native_files")
        .and_then(Value::as_array)
        .context("host adapter bundle is missing native_files")?;

    let mut notes = vec![format!("{host}:")];
    for file in native_files {
        let raw_path = file
            .get("path")
            .and_then(Value::as_str)
            .context("native file entry is missing path")?;
        let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
        let path = resolve_host_path(raw_path, home, cwd);
        let template = file.get("template");

        let note = match format {
            "json" => match template {
                Some(template) => match parse_json_route_from_template(template) {
                    Some((parent_path, key)) => remove_json_config_entry(
                        &path,
                        &parent_path,
                        &key,
                        "unsupported JSON shape",
                        apply_changes,
                    ),
                    None => format!(
                        "- {}: manual cleanup required (unsupported JSON template shape)",
                        path.display()
                    ),
                },
                None => format!(
                    "- {}: manual cleanup required (missing template)",
                    path.display()
                ),
            },
            "toml" => match template
                .and_then(Value::as_str)
                .and_then(extract_toml_section_name)
            {
                Some(section) => remove_toml_section(&path, &section, apply_changes),
                None => format!(
                    "- {}: manual cleanup required (missing TOML section template)",
                    path.display()
                ),
            },
            "markdown" | "mdc" => match template.and_then(Value::as_str) {
                Some(expected) => remove_exact_text_file(&path, expected, format, apply_changes),
                None => format!(
                    "- {}: manual cleanup required (missing text template)",
                    path.display()
                ),
            },
            other => format!(
                "- {}: manual cleanup required (unsupported format `{other}`)",
                path.display()
            ),
        };
        notes.push(note);
    }

    Ok(notes)
}

fn parse_detach_hosts(args: &[String]) -> Result<Vec<&'static str>> {
    let tail = &args[2..];
    if tail.is_empty() {
        anyhow::bail!(
            "usage: codelens-mcp detach <host>\n       codelens-mcp detach --all\nsupported hosts: {}",
            supported_attach_hosts()
        );
    }
    if tail.iter().any(|arg| arg == "--all" || arg == "all") {
        return Ok(HOST_ADAPTER_HOSTS.into_iter().collect());
    }

    let requested = tail[0].as_str();
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown detach host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    Ok(vec![canonical])
}

fn detach_is_dry_run(args: &[String]) -> bool {
    args[2..].iter().any(|arg| arg == "--dry-run")
}

fn render_detach_report(
    hosts: &[&str],
    home: &Path,
    cwd: &Path,
    apply_changes: bool,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("CodeLens detach report\n");
    if apply_changes {
        out.push_str("Machine-editable config files are cleaned automatically.\n");
    } else {
        out.push_str("Dry run only. No files were modified.\n");
    }
    out.push_str(
        "Modified policy markdown files are left in place and reported for manual cleanup.\n\n",
    );

    for (index, host) in hosts.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        for line in detach_host_files(host, home, cwd, apply_changes)? {
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.push_str("\nManual follow-up:\n");
    out.push_str("- Stop any running `codelens-mcp --transport http` daemons if you no longer want the shared server.\n");
    out.push_str(
        "- Remove repo-local `.codelens/` only if you also want to discard cached runtime state.\n",
    );
    out.push_str("- Remove the binary with your install channel: `brew uninstall codelens-mcp`, `cargo uninstall codelens-mcp`, or delete the installed executable path.\n");
    Ok(out)
}

pub(crate) fn run_detach_command(args: &[String]) -> Result<String> {
    let hosts = parse_detach_hosts(args)?;
    let home = home_dir_from_env()?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    render_detach_report(&hosts, &home, &cwd, !detach_is_dry_run(args))
}

pub(crate) fn render_attach_instructions(host: Option<&str>) -> Result<String> {
    let requested = host.context(format!(
        "usage: codelens-mcp attach <host>\nsupported hosts: {}",
        supported_attach_hosts()
    ))?;
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown attach host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    let adapter = crate::surface_manifest::host_adapter_bundle(canonical)
        .context("missing host adapter bundle for attach target")?;

    let resource_uri = adapter
        .get("resource_uri")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let best_fit = adapter
        .get("best_fit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let recommended_modes = json_string_list(&adapter, "recommended_modes");
    let preferred_profiles = json_string_list(&adapter, "preferred_profiles");
    let compiler_targets = json_string_list(&adapter, "compiler_targets");

    let routing_defaults = adapter
        .get("routing_defaults")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let native_files = adapter
        .get("native_files")
        .and_then(Value::as_array)
        .context("host adapter bundle is missing native_files")?;

    let mut out = String::new();
    out.push_str(&format!("CodeLens attach target: {canonical}\n"));
    if requested != canonical {
        out.push_str(&format!("Requested alias: {requested} -> {canonical}\n"));
    }
    if !resource_uri.is_empty() {
        out.push_str(&format!("Adapter resource: {resource_uri}\n"));
    }
    if !best_fit.is_empty() {
        out.push_str(&format!("Best fit: {best_fit}\n"));
    }
    if !recommended_modes.is_empty() {
        out.push_str(&format!(
            "Recommended modes: {}\n",
            recommended_modes.join(", ")
        ));
    }
    if !preferred_profiles.is_empty() {
        out.push_str(&format!(
            "Preferred profiles: {}\n",
            preferred_profiles.join(", ")
        ));
    }
    if !compiler_targets.is_empty() {
        out.push_str(&format!(
            "Host-native targets: {}\n",
            compiler_targets.join(", ")
        ));
    }

    if !routing_defaults.is_empty() {
        out.push_str("Routing defaults:\n");
        for (key, value) in routing_defaults {
            let value = value.as_str().unwrap_or("<non-string-routing-default>");
            out.push_str(&format!("- {key}: {value}\n"));
        }
    }

    out.push_str("\nCopy the following templates into the listed host-native files.\n");
    out.push_str("The default daemon URL assumes `http://127.0.0.1:7837/mcp`.\n");

    for file in native_files {
        let path = file
            .get("path")
            .and_then(Value::as_str)
            .context("native file entry is missing path")?;
        let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
        let purpose = file
            .get("purpose")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let template = file
            .get("template")
            .context("native file entry is missing template")?;

        out.push_str(&format!("\nPath: {path}\n"));
        out.push_str(&format!("Format: {format}\n"));
        if !purpose.is_empty() {
            out.push_str(&format!("Purpose: {purpose}\n"));
        }
        out.push_str(&format!(
            "```{format}\n{}\n```\n",
            render_template(template)?
        ));
    }

    Ok(out)
}

/// Locate the positional project argument, skipping known `--flag value`
/// pairs and `--flag=value` forms. `--` terminates flag parsing.
pub(crate) fn parse_cli_project_arg(args: &[String]) -> Option<String> {
    let mut skip_next = false;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let value = arg.as_str();
        if skip_next {
            skip_next = false;
            continue;
        }
        if value == "--" {
            return iter.next().map(|entry| entry.to_string());
        }
        if let Some((flag, _)) = value.split_once('=')
            && flag_takes_value(flag)
        {
            continue;
        }
        if flag_takes_value(value) {
            skip_next = true;
            continue;
        }
        if value.starts_with('-') {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

/// Resolve the authoritative project-root *source* in the documented
/// priority order: explicit CLI arg → `CLAUDE_PROJECT_DIR` →
/// `MCP_PROJECT_DIR` → current working directory.
pub(crate) fn select_startup_project_source(
    args: &[String],
    claude_project_dir: Option<String>,
    mcp_project_dir: Option<String>,
    cwd: PathBuf,
) -> StartupProjectSource {
    if let Some(path) = parse_cli_project_arg(args) {
        StartupProjectSource::Cli(path)
    } else if let Some(path) = claude_project_dir {
        StartupProjectSource::ClaudeEnv(path)
    } else if let Some(path) = mcp_project_dir {
        StartupProjectSource::McpEnv(path)
    } else {
        StartupProjectSource::Cwd(cwd)
    }
}

/// Resolve a [`StartupProjectSource`] into a concrete [`ProjectRoot`]. Fails
/// closed when an explicit source points at a path that cannot be resolved.
pub(crate) fn resolve_startup_project(source: &StartupProjectSource) -> Result<ProjectRoot> {
    match source {
        StartupProjectSource::Cli(path)
        | StartupProjectSource::ClaudeEnv(path)
        | StartupProjectSource::McpEnv(path) => ProjectRoot::new(path).with_context(|| {
            format!(
                "failed to resolve explicit project root from {}",
                source.label()
            )
        }),
        StartupProjectSource::Cwd(path) => ProjectRoot::new(path)
            .with_context(|| format!("failed to resolve project root from {}", path.display())),
    }
}

/// Extract the value of `--flag <value>` or `--flag=<value>` from an argv
/// slice. `--` terminates flag scanning. Returns `None` if the flag is
/// absent, or when `--flag` appears as the last argument without a value.
pub(crate) fn cli_option_value(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
            return Some(value.to_owned());
        }
        if arg == flag {
            return iter.next().cloned();
        }
    }
    None
}

/// Phase 4c (§observability): emit a single-line startup marker at
/// `warn` level so append-only log files (e.g. launchd's
/// `~/.codex/codelens-http.log`) have an explicit session boundary
/// between historical noise and the current run. Includes every
/// identity field a debugger might want: `pid`, `transport`, `port`,
/// `project_root`, `project_source` (CLI path / env var / cwd),
/// `surface`, `token_budget`, `daemon_mode`, and the build-time
/// identity fields introduced in Phase 4b (`git_sha`, `build_time`,
/// `git_dirty`) plus the wall-clock `daemon_started_at`.
///
/// `warn!` level is intentional: the default `CODELENS_LOG` filter
/// is `warn`, so session-start markers are visible without users
/// having to opt into `info` logging.
#[cfg_attr(not(feature = "http"), allow(dead_code))]
pub(crate) fn format_http_startup_banner(
    project_root: &std::path::Path,
    project_source: &StartupProjectSource,
    surface_label: &str,
    token_budget: usize,
    daemon_mode: RuntimeDaemonMode,
    port: u16,
    daemon_started_at: &str,
) -> String {
    let escaped_project_root = project_root.display().to_string().replace('"', "\\\"");
    format!(
        "CODELENS_SESSION_START pid={} transport=http port={} project_root=\"{}\" project_source=\"{}\" surface={} token_budget={} daemon_mode={} git_sha={} build_time={} daemon_started_at={} git_dirty={}",
        std::process::id(),
        port,
        escaped_project_root,
        project_source.label(),
        surface_label,
        token_budget,
        daemon_mode.as_str(),
        crate::build_info::BUILD_GIT_SHA,
        crate::build_info::BUILD_TIME,
        daemon_started_at,
        crate::build_info::build_git_dirty()
    )
}

#[cfg(test)]
mod startup_tests {
    use super::{
        StartupProjectSource, canonical_attach_host, parse_cli_project_arg, parse_detach_hosts,
        render_attach_instructions, render_detach_report, resolve_startup_project,
    };

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
    fn cli_project_arg_skips_flag_values() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--transport".to_owned(),
            "http".to_owned(),
            "--profile".to_owned(),
            "reviewer-graph".to_owned(),
            "/tmp/repo".to_owned(),
        ];
        assert_eq!(parse_cli_project_arg(&args).as_deref(), Some("/tmp/repo"));
    }

    #[test]
    fn cli_project_arg_honors_double_dash_separator() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--transport".to_owned(),
            "http".to_owned(),
            "--".to_owned(),
            ".".to_owned(),
        ];
        assert_eq!(parse_cli_project_arg(&args).as_deref(), Some("."));
    }

    #[test]
    fn cli_project_arg_skips_equals_syntax_flags() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--transport=http".to_owned(),
            "--port=7842".to_owned(),
            "/tmp/repo".to_owned(),
        ];
        assert_eq!(parse_cli_project_arg(&args).as_deref(), Some("/tmp/repo"));
    }

    #[test]
    fn explicit_project_resolution_fails_closed() {
        let missing = temp_dir("missing-parent").join("does-not-exist");
        let source = StartupProjectSource::Cli(missing.to_string_lossy().to_string());
        let error = resolve_startup_project(&source).expect_err("missing explicit path must fail");
        assert!(
            error
                .to_string()
                .contains("failed to resolve explicit project root")
        );
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
        assert!(rendered.contains("~/.codex/config.toml"));
        assert!(rendered.contains("AGENTS.md"));
        assert!(rendered.contains("builder-minimal"));
        assert!(rendered.contains("refactor-full"));
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

    /// Phase 4c (§observability): the startup banner must carry
    /// every identity field a debugger might want in a single line,
    /// so append-only log tails can pinpoint "which build, which
    /// process, which project" without cross-referencing other
    /// state. Guards the format string against accidental field
    /// removal.
    #[test]
    fn http_startup_banner_includes_runtime_identity_fields() {
        let banner = super::format_http_startup_banner(
            std::path::Path::new("/tmp/repo"),
            &StartupProjectSource::McpEnv("/tmp/repo".to_owned()),
            "builder-minimal",
            2400,
            crate::state::RuntimeDaemonMode::Standard,
            7837,
            "2026-04-11T19:49:55Z",
        );
        assert!(banner.starts_with("CODELENS_SESSION_START pid="));
        assert!(banner.contains("transport=http"));
        assert!(banner.contains("port=7837"));
        assert!(banner.contains("project_root=\"/tmp/repo\""));
        assert!(banner.contains("project_source=\"MCP_PROJECT_DIR\""));
        assert!(banner.contains("surface=builder-minimal"));
        assert!(banner.contains("token_budget=2400"));
        assert!(banner.contains("daemon_mode=standard"));
        assert!(banner.contains("daemon_started_at=2026-04-11T19:49:55Z"));
        assert!(banner.contains("git_sha="));
        assert!(banner.contains("build_time="));
        assert!(banner.contains("git_dirty="));
    }
}
