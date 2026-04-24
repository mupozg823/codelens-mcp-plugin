#![recursion_limit = "256"]

mod agent_coordination;
mod analysis_handles;
mod analysis_queue;
mod artifact_store;
mod authority;
mod backend;
mod build_info;
mod cli;
mod client_profile;
mod dispatch;
mod env_compat;
mod error;
mod job_store;
mod mutation_audit;
mod mutation_gate;
mod operator;
mod preflight_store;
mod prompts;
mod protocol;
mod recent_buffer;
mod registry;
mod resource_analysis;
mod resource_catalog;
mod resource_context;
mod resource_profiles;
mod resources;
mod rule_corpus;
mod rule_retrieval;
mod runtime_types;
mod server;
mod session_context;
mod session_metrics_payload;
mod state;
mod surface_manifest;
mod symbol_corpus;
mod symbol_retrieval;
mod telemetry;
mod test_helpers;
mod tool_defs;
mod tool_evidence;
mod tool_runtime;
mod tools;

pub(crate) use state::AppState;

use anyhow::Result;
#[cfg(feature = "http")]
use cli::format_http_startup_banner;
use cli::{
    attach_host_arg, cli_option_value, is_attach_subcommand, is_detach_subcommand,
    is_doctor_subcommand, render_attach_instructions, resolve_startup_project, run_detach_command,
    run_doctor_command, select_startup_project_source,
};
use env_compat::dual_prefix_env;
use server::oneshot::run_oneshot;
use server::transport_stdio::run_stdio;
use state::RuntimeDaemonMode;
use std::sync::Arc;
use tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
};

// ── Tracing / OpenTelemetry initialisation ──────────────────────────

fn configured_log_filter() -> tracing_subscriber::EnvFilter {
    dual_prefix_env("CODELENS_LOG")
        .and_then(|value| tracing_subscriber::EnvFilter::try_new(value).ok())
        .unwrap_or_else(|| tracing_subscriber::EnvFilter::new("warn"))
}

fn configured_preset_env() -> Option<ToolPreset> {
    dual_prefix_env("CODELENS_PRESET").map(|value| ToolPreset::from_str(&value))
}

fn configured_profile_env() -> Option<ToolProfile> {
    dual_prefix_env("CODELENS_PROFILE").and_then(|value| ToolProfile::from_str(&value))
}

fn configured_daemon_mode_env() -> Option<RuntimeDaemonMode> {
    dual_prefix_env("CODELENS_DAEMON_MODE").map(|value| RuntimeDaemonMode::from_str(&value))
}

#[cfg(feature = "otel")]
fn configured_otel_endpoint() -> String {
    dual_prefix_env("CODELENS_OTEL_ENDPOINT").unwrap_or_default()
}

/// Stderr-only fmt subscriber (default, always present).
fn init_tracing_fmt_only() {
    tracing_subscriber::fmt()
        .with_env_filter(configured_log_filter())
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

/// When the `otel` feature is enabled AND `SYMBIOTE_OTEL_ENDPOINT` or
/// `CODELENS_OTEL_ENDPOINT` is set, build a layered subscriber:
/// fmt (stderr) + OpenTelemetry OTLP exporter. Otherwise fall back to
/// fmt-only.
#[cfg(feature = "otel")]
fn init_tracing() {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let endpoint = configured_otel_endpoint();
    if endpoint.is_empty() {
        init_tracing_fmt_only();
        return;
    }

    // Build OTLP exporter targeting the user-specified collector endpoint.
    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
    {
        Ok(exp) => exp,
        Err(e) => {
            eprintln!(
                "codelens: failed to create OTLP exporter ({e}), falling back to stderr-only tracing"
            );
            init_tracing_fmt_only();
            return;
        }
    };

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_service_name("codelens-mcp")
                .build(),
        )
        .build();

    let tracer = provider.tracer("codelens-mcp");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false);

    tracing_subscriber::registry()
        .with(configured_log_filter())
        .with(fmt_layer)
        .with(otel_layer)
        .init();

    eprintln!("codelens: OpenTelemetry OTLP exporter active → {endpoint}");
}

#[cfg(not(feature = "otel"))]
fn init_tracing() {
    init_tracing_fmt_only();
}

fn main() -> Result<()> {
    // Initialize tracing subscriber — output to stderr to avoid interfering with
    // stdio JSON-RPC transport on stdout. Controlled via SYMBIOTE_LOG or
    // CODELENS_LOG.
    //
    // When the `otel` feature is enabled and SYMBIOTE_OTEL_ENDPOINT or
    // CODELENS_OTEL_ENDPOINT is set, an OpenTelemetry OTLP exporter layer
    // is added so spans are shipped to an external collector (Jaeger,
    // Grafana Tempo, etc.).
    init_tracing();

    let args: Vec<String> = std::env::args().collect();
    if is_attach_subcommand(&args) {
        println!(
            "{}",
            render_attach_instructions(attach_host_arg(&args).as_deref())?
        );
        return Ok(());
    }
    if is_detach_subcommand(&args) {
        println!("{}", run_detach_command(&args)?);
        return Ok(());
    }
    if is_doctor_subcommand(&args) {
        println!("{}", run_doctor_command(&args)?);
        return Ok(());
    }

    let preset = args
        .iter()
        .position(|a| a == "--preset")
        .and_then(|i| args.get(i + 1))
        .map(|s| ToolPreset::from_str(s))
        .or_else(configured_preset_env)
        .unwrap_or_else(|| state::ClientProfile::detect(None).default_preset());
    let profile = cli_option_value(&args, "--profile")
        .as_deref()
        .and_then(ToolProfile::from_str)
        .or_else(configured_profile_env);
    let daemon_mode = cli_option_value(&args, "--daemon-mode")
        .as_deref()
        .map(RuntimeDaemonMode::from_str)
        .or_else(configured_daemon_mode_env)
        .unwrap_or(RuntimeDaemonMode::Standard);

    if args.iter().any(|arg| arg == "--print-surface-manifest") {
        let surface = profile
            .map(ToolSurface::Profile)
            .unwrap_or_else(|| ToolSurface::Preset(preset));
        let manifest = surface_manifest::build_surface_manifest(surface, daemon_mode);
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }

    // Project root resolution priority:
    // 1. Explicit path argument (if not ".")
    // 2. CLAUDE_PROJECT_DIR environment variable (set by Claude Code)
    // 3. MCP_PROJECT_DIR environment variable (generic)
    // 4. Current working directory with .git/.cargo marker detection
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
    let cmd_tool = cli_option_value(&args, "--cmd");

    let cmd_args = cli_option_value(&args, "--args");

    let transport = cli_option_value(&args, "--transport").unwrap_or_else(|| "stdio".to_owned());

    #[cfg(feature = "http")]
    let port: u16 = cli_option_value(&args, "--port")
        .and_then(|s| s.parse().ok())
        .unwrap_or(7837);

    let project = resolve_startup_project(&project_source)?;
    if !project_source.is_explicit() && project.as_path() == std::path::Path::new("/") {
        anyhow::bail!(
            "Refusing to start CodeLens on `/` without an explicit project root. Pass a path or set MCP_PROJECT_DIR/CLAUDE_PROJECT_DIR."
        );
    }

    // v1.5 Phase 2j MCP follow-up: auto-detect the dominant language so
    // `CODELENS_EMBED_HINT_AUTO=1` alone (without an explicit
    // `CODELENS_EMBED_HINT_AUTO_LANG`) becomes the v1.6.0 default flip
    // candidate. Applies to both one-shot CLI (`--cmd`) and stdio MCP.
    // `activate_project` calls the same helper for MCP-driven switches.
    crate::tools::session::auto_set_embed_hint_lang(project.as_path());

    let app_state = AppState::new(project, preset);
    app_state.configure_transport_mode(&transport);
    app_state.configure_daemon_mode(daemon_mode);
    if let Some(profile) = profile {
        app_state.set_surface(ToolSurface::Profile(profile));
        app_state.set_token_budget(default_budget_for_profile(profile));
    } else {
        app_state.set_surface(ToolSurface::Preset(preset));
        app_state.set_token_budget(default_budget_for_preset(preset));
    }

    #[cfg(feature = "http")]
    if transport == "http" {
        let startup_banner = format_http_startup_banner(
            app_state.project().as_path(),
            &project_source,
            app_state.surface().as_label(),
            app_state.token_budget(),
            app_state.daemon_mode(),
            port,
            app_state.daemon_started_at(),
        );
        // Intentionally `warn!`: the default CODELENS_LOG filter is `warn`,
        // so a session-start marker must be visible without requiring users
        // to opt into `info` logging. This gives appended daemon logs an
        // explicit boundary between historical noise and the current run.
        tracing::warn!("{startup_banner}");
    }

    // One-shot mode: run a single tool and exit
    if let Some(tool_name) = cmd_tool {
        let state = Arc::new(app_state);
        return run_oneshot(&state, &tool_name, cmd_args.as_deref());
    }

    match transport.as_str() {
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

#[path = "integration_tests/mod.rs"]
#[cfg(test)]
mod tests;

#[cfg(test)]
mod env_config_tests {
    use super::*;

    fn with_env(vars: &[(&str, Option<&str>)], f: impl FnOnce()) {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let previous = vars
            .iter()
            .map(|(name, _)| ((*name).to_owned(), std::env::var(name).ok()))
            .collect::<Vec<_>>();
        unsafe {
            for (name, value) in vars {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
        f();
        unsafe {
            for (name, value) in previous {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn symbiote_preset_profile_and_daemon_mode_override_codelens_values() {
        with_env(
            &[
                ("SYMBIOTE_PRESET", Some("minimal")),
                ("CODELENS_PRESET", Some("full")),
                ("SYMBIOTE_PROFILE", Some("builder-minimal")),
                ("CODELENS_PROFILE", Some("planner-readonly")),
                ("SYMBIOTE_DAEMON_MODE", Some("mutation-enabled")),
                ("CODELENS_DAEMON_MODE", Some("read-only")),
            ],
            || {
                assert_eq!(configured_preset_env(), Some(ToolPreset::Minimal));
                assert_eq!(configured_profile_env(), Some(ToolProfile::BuilderMinimal));
                assert_eq!(
                    configured_daemon_mode_env(),
                    Some(RuntimeDaemonMode::MutationEnabled)
                );
            },
        );
    }

    #[cfg(feature = "otel")]
    #[test]
    fn symbiote_otel_endpoint_overrides_codelens_endpoint() {
        with_env(
            &[
                ("SYMBIOTE_OTEL_ENDPOINT", Some("http://symbiote-collector")),
                ("CODELENS_OTEL_ENDPOINT", Some("http://codelens-collector")),
            ],
            || {
                assert_eq!(configured_otel_endpoint(), "http://symbiote-collector");
            },
        );
    }
}
