mod app;
mod ui;

use anyhow::Result;
use app::App;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::prelude::*;
use std::io;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    // --check mode: non-interactive verification of index health
    if args.iter().any(|a| a == "--check") {
        let project_path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
        return run_check(project_path);
    }

    let project_path = args.get(1).map(|s| s.as_str()).unwrap_or(".");

    // Build index before entering TUI (shows progress on stderr)
    eprintln!("CodeLens TUI: indexing {}...", project_path);
    let mut app = App::new(project_path)?;
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
fn run_check(project_path: &str) -> Result<()> {
    let app = App::new(project_path)?;
    let filtered = app.filtered_files();
    println!("CodeLens TUI Check");
    println!("  Project:  {}", app.project_name);
    println!("  Files:    {}", app.total_indexed_files);
    println!("  Visible:  {}", filtered.len());
    if let Some((_, first)) = filtered.first() {
        println!("  First:    {}", first.path);
    }
    if let Some(sym) = app.symbols.first() {
        println!(
            "  Symbol:   {} ({:?}, line {})",
            sym.name, sym.kind, sym.line
        );
    }
    println!("  Status:   OK");
    Ok(())
}
