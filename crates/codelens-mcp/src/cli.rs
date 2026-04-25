//! CLI argument parsing + project-root resolution + HTTP startup banner.
//!
//! Extracted from `main.rs` as of v1.9.32 to keep the binary entry point
//! focused on bootstrap/dispatch. All parsing functions are pure and test
//! co-located below.

use crate::state::RuntimeDaemonMode;
use anyhow::{Context, Result};
use codelens_engine::ProjectRoot;
use std::path::PathBuf;

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
        "--preset"
            | "--profile"
            | "--daemon-mode"
            | "--cmd"
            | "--args"
            | "--transport"
            | "--listen"
            | "--port"
            | "--tls-cert"
            | "--tls-key"
            | "--auth"
            | "--auth-jwks-url"
            | "--auth-issuer"
            | "--auth-audience"
            | "--auth-scope"
            | "--compat"
    )
}

pub(crate) fn is_attach_subcommand(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("attach"))
}

pub(crate) fn is_detach_subcommand(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("detach"))
}

pub(crate) fn is_doctor_subcommand(args: &[String]) -> bool {
    matches!(
        args.get(1).map(String::as_str),
        Some("doctor") | Some("status")
    )
}

pub(crate) fn attach_host_arg(args: &[String]) -> Option<String> {
    args.get(2).cloned()
}

mod host;
#[cfg(test)]
pub(crate) use host::{
    canonical_attach_host, inspect_text_policy_file, inspect_text_policy_file_json,
    parse_detach_hosts, parse_doctor_hosts, render_detach_report, render_doctor_report,
};
pub(crate) use host::{render_attach_instructions, run_detach_command, run_doctor_command};

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
    transport_endpoint: (&str, u16),
    daemon_started_at: &str,
) -> String {
    let (transport, port) = transport_endpoint;
    let escaped_project_root = project_root.display().to_string().replace('"', "\\\"");
    format!(
        "CODELENS_SESSION_START pid={} transport={} port={} project_root=\"{}\" project_source=\"{}\" surface={} token_budget={} daemon_mode={} git_sha={} build_time={} daemon_started_at={} git_dirty={}",
        std::process::id(),
        transport,
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
mod startup_tests;
