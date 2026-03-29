mod authority;
mod dispatch;
mod error;
mod prompts;
mod protocol;
mod resources;
mod server;
mod state;
mod tool_defs;
mod tools;

pub(crate) use state::AppState;

use anyhow::Result;
use codelens_core::ProjectRoot;
use server::oneshot::run_oneshot;
use server::transport_stdio::run_stdio;
use std::sync::Arc;
use tool_defs::ToolPreset;

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

    // Project root resolution priority:
    // 1. Explicit path argument (if not ".")
    // 2. CLAUDE_PROJECT_DIR environment variable (set by Claude Code)
    // 3. MCP_PROJECT_DIR environment variable (generic)
    // 4. Current working directory with .git/.cargo marker detection
    let effective_path = if project_arg != "." {
        project_arg.to_string()
    } else if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        dir
    } else if let Ok(dir) = std::env::var("MCP_PROJECT_DIR") {
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
    let app_state = AppState::new(project, preset);

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
            anyhow::bail!("HTTP transport requires the `http` feature. Rebuild with: cargo build --features http");
        }
        _ => run_stdio(Arc::new(app_state)),
    }
}

#[cfg(test)]
#[path = "integration_tests.rs"]
mod tests;
