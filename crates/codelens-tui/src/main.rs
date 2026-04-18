mod app;
mod ui;
mod watch;

use anyhow::Result;
use app::App;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::io;

struct CliArgs {
    project_path: String,
    check: bool,
    json: bool,
    strict: bool,
}

fn parse_cli_args(argv: &[String]) -> CliArgs {
    let mut project_path = ".".to_string();
    let mut check = false;
    let mut json = false;
    let mut strict = false;

    for arg in argv.iter().skip(1) {
        match arg.as_str() {
            "--check" => check = true,
            "--json" => json = true,
            "--strict" => strict = true,
            _ if !arg.starts_with('-') => project_path = arg.clone(),
            _ => {}
        }
    }

    CliArgs {
        project_path,
        check,
        json,
        strict,
    }
}

fn main() -> Result<()> {
    let argv: Vec<String> = std::env::args().collect();

    // Subcommand: `codelens-tui watch [--trace <path>] [project_path]`
    // Live observer over the server's telemetry JSONL stream.
    if argv.get(1).map(String::as_str) == Some("watch") {
        let rest: Vec<String> = argv.iter().skip(2).cloned().collect();
        let mut trace_path: Option<String> = None;
        let mut project_path = ".".to_string();
        let mut iter = rest.iter();
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--trace" => {
                    if let Some(value) = iter.next() {
                        trace_path = Some(value.clone());
                    }
                }
                _ if !arg.starts_with('-') => project_path = arg.clone(),
                _ => {}
            }
        }
        let project_root = std::path::PathBuf::from(&project_path);
        return watch::run(trace_path.as_deref(), &project_root);
    }

    let args = parse_cli_args(&argv);

    // --check mode: non-interactive verification of index health
    if args.check {
        return run_check(&args.project_path, args.json, args.strict);
    }

    // Build index before entering TUI (shows progress on stderr)
    eprintln!("CodeLens TUI: indexing {}...", args.project_path);
    let mut app = App::new(&args.project_path)?;
    eprintln!(
        "Indexed {} files, {} symbols. Launching dashboard...",
        app.total_indexed_files,
        app.symbols.len()
    );

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match app.handle_key(key.code) {
                app::Action::Quit => break,
                app::Action::Continue => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

/// Non-interactive check: build index, print stats, exit.
fn run_check(project_path: &str, json_output: bool, strict: bool) -> Result<()> {
    let app = App::new(project_path)?;
    let payload = app.check_payload();
    if json_output {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("CodeLens TUI Check");
        println!("  Project:   {}", app.project_name);
        println!("  Root:      {}", app.health.project_root);
        println!(
            "  Index:     {}/{} ({}%)",
            app.health.indexed_files,
            app.health.supported_files,
            app.health.coverage_percent()
        );
        println!("  Stale:     {}", app.health.stale_files);
        println!(
            "  Semantic:  {}",
            if app.health.semantic_assets_available {
                "ready"
            } else {
                "missing"
            }
        );
        println!("  Model:     {}", app.health.embedding_model);
        println!(
            "  Runtime:   {} / {} threads / max {}",
            app.health.embedding_runtime_backend,
            app.health.embedding_threads,
            app.health.embedding_max_length
        );
        if let Some(first) = payload.get("first_file").and_then(|v| v.as_str()) {
            println!("  First:     {first}");
        }
        if let Some(sym) = payload.get("first_symbol") {
            println!("  Symbol:    {}", sym);
        }
        for warning in &app.health.warnings {
            println!("  Warning:   {warning}");
        }
        println!("  Status:    {}", app.health.status_label());
    }
    if strict && !app.health.warnings.is_empty() {
        anyhow::bail!("TUI health check reported warnings");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_cli_args;

    #[test]
    fn parse_cli_args_collects_check_flags_and_path() {
        let argv = vec![
            "codelens-tui".to_string(),
            "--check".to_string(),
            "--json".to_string(),
            "--strict".to_string(),
            "/tmp/project".to_string(),
        ];
        let args = parse_cli_args(&argv);
        assert!(args.check);
        assert!(args.json);
        assert!(args.strict);
        assert_eq!(args.project_path, "/tmp/project");
    }
}
