mod authority;
mod dispatch;
mod error;
mod prompts;
mod protocol;
mod resources;
mod server;
mod state;
mod telemetry;
mod tool_defs;
mod tools;

pub(crate) use state::AppState;

use anyhow::Result;
use codelens_core::ProjectRoot;
use server::oneshot::run_oneshot;
use server::transport_stdio::run_stdio;
use state::RuntimeDaemonMode;
use std::sync::Arc;
use tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
};

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
    let project_arg = args.get(1).map(|s| s.as_str()).unwrap_or(".");
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
        .unwrap_or(ToolPreset::Balanced);
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

    // Project root resolution priority:
    // 1. Explicit path argument (if not ".")
    // 2. CLAUDE_PROJECT_DIR environment variable (set by Claude Code)
    // 3. MCP_PROJECT_DIR environment variable (generic)
    // 4. Current working directory with .git/.cargo marker detection
    let project_from_cli = project_arg != ".";
    let project_from_claude = std::env::var("CLAUDE_PROJECT_DIR").ok();
    let project_from_mcp = std::env::var("MCP_PROJECT_DIR").ok();

    let effective_path = if project_from_cli {
        project_arg.to_string()
    } else if let Some(dir) = project_from_claude.clone() {
        dir
    } else if let Some(dir) = project_from_mcp.clone() {
        dir
    } else {
        ".".to_string()
    };

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

    let project = ProjectRoot::new(&effective_path)?;
    if !project_from_cli
        && project_from_claude.is_none()
        && project_from_mcp.is_none()
        && project.as_path() == std::path::Path::new("/")
    {
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
#[path = "integration_tests.rs"]
mod tests;
