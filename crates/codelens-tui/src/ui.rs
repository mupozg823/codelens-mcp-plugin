use crate::app::{App, Panel};
use codelens_engine::SymbolInfo;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

pub fn draw(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(f.area());

    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(main_chunks[0]);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(top_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(top_chunks[1]);

    draw_file_tree(f, app, left_chunks[0]);
    draw_symbol_list(f, app, right_chunks[0]);
    draw_detail(f, app, left_chunks[1]);
    draw_metrics(f, app, right_chunks[1]);
    draw_status_bar(f, app, main_chunks[1]);
}

fn panel_style(active: bool) -> Style {
    if active {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn draw_file_tree(f: &mut Frame, app: &App, area: Rect) {
    let active = matches!(app.active_panel, Panel::Tree);
    let filtered = app.filtered_files();
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|(orig_idx, entry)| {
            let indent = "  ".repeat(entry.depth);
            let label = entry.path.rsplit('/').next().unwrap_or(&entry.path);
            let style = if *orig_idx == app.file_cursor {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("{indent}  {label}")).style(style)
        })
        .collect();

    let title = if app.search_mode {
        format!(" Files [/{}] ", app.search_query)
    } else {
        " Files ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(panel_style(active));

    let cursor_pos = filtered
        .iter()
        .position(|(i, _)| *i == app.file_cursor)
        .unwrap_or(0);
    let mut state = ListState::default();
    state.select(Some(cursor_pos));

    let list = List::new(items).block(block).highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_symbol_list(f: &mut Frame, app: &App, area: Rect) {
    let active = matches!(app.active_panel, Panel::Symbols);

    let search_results = if app.symbol_search_mode {
        app.search_symbols()
    } else {
        vec![]
    };

    let sym_iter: Box<dyn Iterator<Item = (usize, &SymbolInfo)>> = if app.symbol_search_mode {
        Box::new(search_results.iter().enumerate())
    } else {
        Box::new(app.symbols.iter().enumerate())
    };

    let items: Vec<ListItem> = sym_iter
        .map(|(i, sym)| {
            let kind_icon = match sym.kind {
                codelens_engine::SymbolKind::Function | codelens_engine::SymbolKind::Method => "fn",
                codelens_engine::SymbolKind::Class => "cl",
                codelens_engine::SymbolKind::Interface => "if",
                codelens_engine::SymbolKind::Variable => "va",
                codelens_engine::SymbolKind::Module => "md",
                codelens_engine::SymbolKind::Enum => "en",
                codelens_engine::SymbolKind::TypeAlias => "ty",
                codelens_engine::SymbolKind::Property => "pr",
                _ => "  ",
            };
            let style = if i == app.symbol_cursor {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!(" {kind_icon}  {}  :{}", sym.name, sym.line)).style(style)
        })
        .collect();

    let title = if app.symbol_search_mode {
        format!(" Symbols [s/{}] ", app.symbol_search_query)
    } else if let Some(file) = app.files.get(app.file_cursor) {
        format!(
            " Symbols — {} ",
            file.path.rsplit('/').next().unwrap_or(&file.path)
        )
    } else {
        " Symbols ".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(panel_style(active));

    let mut state = ListState::default();
    state.select(Some(app.symbol_cursor));

    let list = List::new(items).block(block).highlight_symbol("▶ ");
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let active = matches!(app.active_panel, Panel::Detail);

    let importers = app.current_file_importers();
    let imports = app.current_file_imports();

    let mut lines = vec![];

    // Symbol detail (if selected)
    if let Some(sym) = app.selected_symbol() {
        lines.push(format!(" {} {:?} :{}", sym.name, sym.kind, sym.line));
        lines.push(format!(" {}", sym.signature));
        lines.push(String::new());
    }

    // Upstream (who imports us)
    lines.push(format!(" \u{25b2} Imported by ({}):", importers.len()));
    for imp in importers.iter().take(8) {
        lines.push(format!("   {}", imp));
    }
    if importers.len() > 8 {
        lines.push(format!("   ... +{} more", importers.len() - 8));
    }

    // Downstream (what we import) - if available
    if !imports.is_empty() {
        lines.push(String::new());
        lines.push(format!(" \u{25bc} Imports ({}):", imports.len()));
        for dep in imports.iter().take(5) {
            lines.push(format!("   {}", dep));
        }
    }

    let text = lines.join("\n");
    let block = Block::default()
        .title(" Detail + Imports ")
        .borders(Borders::ALL)
        .border_style(panel_style(active));
    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn draw_metrics(f: &mut Frame, app: &App, area: Rect) {
    let sym_count = app.symbols.len();
    let file_name = app
        .files
        .get(app.file_cursor)
        .map(|f| f.path.as_str())
        .unwrap_or("-");

    let child_total: usize = app.symbols.iter().map(|s| s.children.len()).sum();

    let importer_count = app.current_file_importer_count();

    let text = format!(
        " File:      {}\n Symbols:   {}\n Children:  {}\n Importers: {}\n Total:     {} indexed files",
        file_name, sym_count, child_total, importer_count, app.total_indexed_files,
    );

    let block = Block::default()
        .title(" File Metrics ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let prefix = if app.symbol_search_mode {
        format!(" Symbol search: {} |", app.symbol_search_query)
    } else if app.search_mode {
        format!(" Search: {} |", app.search_query)
    } else {
        format!(" {} |", app.project_name)
    };
    let status = format!(
        "{} {} indexed files | [q]Quit [Tab]Panel [↑↓]Nav [/]Search [s]Symbol",
        prefix, app.total_indexed_files,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(status)
        .block(block)
        .style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);
}
