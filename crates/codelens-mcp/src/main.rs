mod analysis_queue;
mod artifact_store;
mod authority;
mod client_profile;
mod dispatch;
mod dispatch_access;
mod dispatch_response;
mod dispatch_response_support;
mod error;
mod job_store;
mod mutation_audit;
mod mutation_gate;
mod preflight_store;
mod prompts;
mod protocol;
mod resource_analysis;
mod resource_catalog;
mod resource_context;
mod resource_profiles;
mod resources;
mod runtime_types;
mod server;
mod session_context;
mod session_metrics_payload;
mod state;
mod telemetry;
mod tool_defs;
mod tool_runtime;
mod tools;

pub(crate) use state::AppState;

use anyhow::{Context, Result};
use codelens_core::ProjectRoot;
use server::oneshot::run_oneshot;
use server::transport_stdio::run_stdio;
use state::RuntimeDaemonMode;
use std::path::PathBuf;
use std::sync::Arc;
use tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum StartupProjectSource {
    Cli(String),
    ClaudeEnv(String),
    McpEnv(String),
    Cwd(PathBuf),
}

impl StartupProjectSource {
    fn is_explicit(&self) -> bool {
        !matches!(self, Self::Cwd(_))
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Cli(_) => "CLI path",
            Self::ClaudeEnv(_) => "CLAUDE_PROJECT_DIR",
            Self::McpEnv(_) => "MCP_PROJECT_DIR",
            Self::Cwd(_) => "current working directory",
        }
    }
}

fn flag_takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "--preset" | "--profile" | "--daemon-mode" | "--cmd" | "--args" | "--transport" | "--port"
    )
}

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

fn select_startup_project_source(
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

fn resolve_startup_project(source: &StartupProjectSource) -> Result<ProjectRoot> {
    match source {
        StartupProjectSource::Cli(path)
        | StartupProjectSource::ClaudeEnv(path)
        | StartupProjectSource::McpEnv(path) => ProjectRoot::new(path)
            .map_err(anyhow::Error::from)
            .with_context(|| {
                format!(
                    "failed to resolve explicit project root from {}",
                    source.label()
                )
            }),
        StartupProjectSource::Cwd(path) => ProjectRoot::new(path)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("failed to resolve project root from {}", path.display())),
    }
}

// ── Entry point ────────────────────────────────────────────────────────

fn main() -> Result<()> {
    // Initialize tracing subscriber — output to stderr to avoid interfering with
    // stdio JSON-RPC transport on stdout. Controlled via CODELENS_LOG env var.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CODELENS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let preset = args
        .iter()
        .position(|a| a == "--preset")
        .and_then(|i| args.get(i + 1))
        .map(|s| ToolPreset::from_str(s))
        .or_else(|| {
            std::env::var("CODELENS_PRESET")
                .ok()
                .map(|s| ToolPreset::from_str(&s))
        })
        .unwrap_or_else(|| state::ClientProfile::detect(None).default_preset());
    let profile = args
        .iter()
        .position(|a| a == "--profile")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| ToolProfile::from_str(s))
        .or_else(|| {
            std::env::var("CODELENS_PROFILE")
                .ok()
                .and_then(|s| ToolProfile::from_str(&s))
        });
    let daemon_mode = args
        .iter()
        .position(|a| a == "--daemon-mode")
        .and_then(|i| args.get(i + 1))
        .map(|s| RuntimeDaemonMode::from_str(s))
        .or_else(|| {
            std::env::var("CODELENS_DAEMON_MODE")
                .ok()
                .map(|s| RuntimeDaemonMode::from_str(&s))
        })
        .unwrap_or(RuntimeDaemonMode::Standard);

    let project_from_claude = std::env::var("CLAUDE_PROJECT_DIR").ok();
    let project_from_mcp = std::env::var("MCP_PROJECT_DIR").ok();
    let cwd = std::env::current_dir()?;
    let project_source = select_startup_project_source(
        &args,
        project_from_claude.clone(),
        project_from_mcp.clone(),
        cwd,
    );

    // One-shot CLI mode: --cmd <tool_name> [--args '<json>']
    let cmd_tool = args
        .iter()
        .position(|a| a == "--cmd")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let cmd_args = args
        .iter()
        .position(|a| a == "--args")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let transport = args
        .iter()
        .position(|a| a == "--transport")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("stdio");

    #[cfg(feature = "http")]
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(7837);

    let project = resolve_startup_project(&project_source)?;
    if !project_source.is_explicit() && project.as_path() == std::path::Path::new("/") {
        anyhow::bail!(
            "Refusing to start CodeLens on `/` without an explicit project root. Pass a path or set MCP_PROJECT_DIR/CLAUDE_PROJECT_DIR."
        );
    }
    let app_state = AppState::new(project, preset);
    app_state.configure_transport_mode(transport);
    app_state.configure_daemon_mode(daemon_mode);
    if let Some(profile) = profile {
        app_state.set_surface(ToolSurface::Profile(profile));
        app_state.set_token_budget(default_budget_for_profile(profile));
    } else {
        app_state.set_surface(ToolSurface::Preset(preset));
        app_state.set_token_budget(default_budget_for_preset(preset));
    }

    // One-shot mode: run a single tool and exit
    if let Some(tool_name) = cmd_tool {
        let state = Arc::new(app_state);
        return run_oneshot(&state, &tool_name, cmd_args.as_deref());
    }

    match transport {
        #[cfg(feature = "http")]
        "http" => {
            let state = Arc::new(app_state.with_session_store());
            server::transport_http::run_http(state, port)
        }
        #[cfg(not(feature = "http"))]
        "http" => {
            anyhow::bail!(
                "HTTP transport requires the `http` feature. Rebuild with: cargo build --features http"
            );
        }
        _ => run_stdio(Arc::new(app_state)),
    }
}

#[cfg(test)]
mod startup_tests {
    use super::{StartupProjectSource, parse_cli_project_arg, resolve_startup_project};

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
}

#[cfg(test)]
#[path = "integration_tests.rs"]
mod tests;
