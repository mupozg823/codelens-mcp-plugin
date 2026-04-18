//! Live MCP observer — `codelens-tui watch` subcommand.
//!
//! Attaches to the same telemetry JSONL the MCP server writes when
//! `CODELENS_TELEMETRY_ENABLED=1` (or `CODELENS_TELEMETRY_PATH=…`) and
//! renders a three-panel real-time view:
//!
//!   ┌ Live timeline (recent tool calls)
//!   ├ Current tool (last call: args, result, latency)
//!   └ Session metrics summary (counts, total tokens, avg latency)
//!
//! Implements ADR-0007 path A: file-tail observability. No MCP
//! protocol change. Works with stdio AND HTTP servers. Host agents
//! (Claude Code / Codex / Cursor) keep their JSON-RPC channel intact;
//! the TUI is a sidecar reader.

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const DEFAULT_TRACE_PATH: &str = ".codelens/telemetry/tool_usage.jsonl";
const TIMELINE_CAP: usize = 24;
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// One persisted tool-call event. Mirrors the server's `PersistedEvent`
/// struct so deserialization is tolerant of future additions (serde
/// ignores unknown fields by default).
#[derive(Debug, Deserialize, Clone)]
struct ToolEvent {
    #[serde(default)]
    #[allow(dead_code)]
    timestamp_ms: u64,
    tool: String,
    surface: String,
    elapsed_ms: u64,
    #[serde(default)]
    tokens: usize,
    success: bool,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    target_paths: Option<Vec<String>>,
}

struct WatchState {
    trace_path: PathBuf,
    events: Vec<ToolEvent>,
    file: Option<File>,
    position: u64,
    last_error: Option<String>,
    paused: bool,
    last_update: Instant,
}

impl WatchState {
    fn new(trace_path: PathBuf) -> Self {
        Self {
            trace_path,
            events: Vec::with_capacity(TIMELINE_CAP * 2),
            file: None,
            position: 0,
            last_error: None,
            paused: false,
            last_update: Instant::now(),
        }
    }

    fn poll(&mut self) {
        if self.paused {
            return;
        }
        if self.file.is_none() {
            match File::open(&self.trace_path) {
                Ok(mut f) => {
                    // Start at end of file so we only render NEW events.
                    if let Ok(end) = f.seek(SeekFrom::End(0)) {
                        self.position = end;
                    }
                    self.file = Some(f);
                    self.last_error = None;
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    self.last_error = Some(format!(
                        "trace file not found at {} — set CODELENS_TELEMETRY_ENABLED=1 on the server",
                        self.trace_path.display()
                    ));
                    return;
                }
                Err(err) => {
                    self.last_error = Some(format!("{err}"));
                    return;
                }
            }
        }

        let file = self.file.as_mut().expect("file checked above");
        if let Err(err) = file.seek(SeekFrom::Start(self.position)) {
            self.last_error = Some(format!("seek: {err}"));
            return;
        }
        let mut reader = BufReader::new(file);
        let mut buf = String::new();
        loop {
            buf.clear();
            let read = match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) => {
                    self.last_error = Some(format!("read: {err}"));
                    break;
                }
            };
            self.position += read as u64;
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<ToolEvent>(trimmed) {
                Ok(event) => {
                    self.events.push(event);
                    if self.events.len() > TIMELINE_CAP * 4 {
                        let excess = self.events.len() - TIMELINE_CAP * 4;
                        self.events.drain(..excess);
                    }
                }
                Err(_) => {
                    // Skip malformed lines; server emits stable JSON so
                    // a malformed line is almost certainly a partial
                    // write caught mid-flush.
                }
            }
        }
        self.last_update = Instant::now();
    }

    fn timeline_items(&self) -> Vec<ListItem<'_>> {
        self.events
            .iter()
            .rev()
            .take(TIMELINE_CAP)
            .map(|event| {
                let status = if event.success { "✓" } else { "✗" };
                let phase = event.phase.as_deref().unwrap_or("-");
                let session = event
                    .session_id
                    .as_deref()
                    .map(|s| &s[..s.len().min(12)])
                    .unwrap_or("");
                let spans = vec![
                    Span::styled(format!("{status} "), status_style(event.success)),
                    Span::styled(format!("{:24} ", event.tool), Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!("{:10} ", event.surface),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:>6} ", phase),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::raw(format!("{:>5}ms ", event.elapsed_ms)),
                    Span::styled(
                        format!("{:>5}tok ", event.tokens),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::styled(
                        session.to_string(),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::DIM),
                    ),
                ];
                ListItem::new(Line::from(spans))
            })
            .collect()
    }

    fn current_paragraph(&self) -> Paragraph<'_> {
        match self.events.last() {
            None => Paragraph::new("(no events yet — waiting for the server to emit)")
                .style(Style::default().fg(Color::DarkGray))
                .wrap(Wrap { trim: true }),
            Some(event) => {
                let mut lines = vec![
                    Line::from(vec![
                        Span::raw("tool     : "),
                        Span::styled(event.tool.clone(), Style::default().fg(Color::Cyan)),
                        Span::raw(if event.truncated { "  [truncated]" } else { "" }),
                    ]),
                    Line::from(format!(
                        "surface  : {}    phase: {}",
                        event.surface,
                        event.phase.as_deref().unwrap_or("-"),
                    )),
                    Line::from(format!(
                        "status   : {}    elapsed: {}ms    tokens: {}",
                        if event.success { "success" } else { "error" },
                        event.elapsed_ms,
                        event.tokens,
                    )),
                    Line::from(format!(
                        "session  : {}",
                        event.session_id.as_deref().unwrap_or("(local)"),
                    )),
                ];
                if let Some(paths) = event.target_paths.as_ref()
                    && !paths.is_empty()
                {
                    lines.push(Line::from(format!(
                        "targets  : {}",
                        paths.join(", ")
                    )));
                }
                Paragraph::new(lines)
                    .style(Style::default())
                    .wrap(Wrap { trim: false })
            }
        }
    }

    fn summary_paragraph(&self) -> Paragraph<'_> {
        if self.events.is_empty() {
            return Paragraph::new("(no events)").style(Style::default().fg(Color::DarkGray));
        }
        let mut counts: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
        let mut total_events = 0u64;
        let mut total_tokens = 0u64;
        let mut total_ms = 0u64;
        let mut failures = 0u64;
        for event in &self.events {
            let entry = counts
                .entry(event.tool.as_str())
                .or_insert((0, 0, 0));
            entry.0 += 1;
            entry.1 += event.elapsed_ms;
            entry.2 += event.tokens as u64;
            total_events += 1;
            total_tokens += event.tokens as u64;
            total_ms += event.elapsed_ms;
            if !event.success {
                failures += 1;
            }
        }
        let avg_ms = if total_events == 0 {
            0
        } else {
            total_ms / total_events
        };
        let mut lines = vec![
            Line::from(vec![
                Span::raw("observed : "),
                Span::styled(
                    format!("{total_events}"),
                    Style::default().fg(Color::Green),
                ),
                Span::raw("  failures: "),
                Span::styled(
                    format!("{failures}"),
                    if failures > 0 {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::raw(format!("  avg_ms: {avg_ms}  total_tok: {total_tokens}")),
            ]),
            Line::from("top tools:"),
        ];
        let mut top: Vec<_> = counts.into_iter().collect();
        top.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (name, (n, ms, tok)) in top.iter().take(6) {
            lines.push(Line::from(format!(
                "  {:24} × {:>3}    avg_ms: {:>5}    tok: {:>5}",
                name,
                n,
                if *n == 0 { 0 } else { ms / n },
                tok,
            )));
        }
        if self.paused {
            lines.push(Line::from(Span::styled(
                "(paused — press p to resume)",
                Style::default().fg(Color::Yellow),
            )));
        }
        if let Some(err) = &self.last_error {
            lines.push(Line::from(Span::styled(
                format!("error: {err}"),
                Style::default().fg(Color::Red),
            )));
        }
        Paragraph::new(lines).wrap(Wrap { trim: false })
    }
}

fn status_style(success: bool) -> Style {
    if success {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::Red)
    }
}

fn resolve_trace_path(explicit: Option<&str>) -> PathBuf {
    if let Some(explicit) = explicit {
        return PathBuf::from(explicit);
    }
    if let Ok(env_override) = std::env::var("SYMBIOTE_TELEMETRY_PATH")
        && !env_override.is_empty()
    {
        return PathBuf::from(env_override);
    }
    if let Ok(env_override) = std::env::var("CODELENS_TELEMETRY_PATH")
        && !env_override.is_empty()
    {
        return PathBuf::from(env_override);
    }
    PathBuf::from(DEFAULT_TRACE_PATH)
}

pub fn run(trace_arg: Option<&str>, project_root: &Path) -> Result<()> {
    let mut trace_path = resolve_trace_path(trace_arg);
    if trace_path.is_relative() {
        trace_path = project_root.join(trace_path);
    }
    let mut state = WatchState::new(trace_path.clone());
    state.poll();

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alt screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;

    let exit_result = loop {
        state.poll();

        let draw_result = terminal.draw(|frame| {
            let size = frame.area();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(8),
                    Constraint::Length(8),
                    Constraint::Length(10),
                ])
                .split(size);

            let timeline_items = state.timeline_items();
            let timeline = List::new(timeline_items).block(
                Block::default()
                    .title(format!(
                        "Live tool timeline · trace={}{}",
                        state.trace_path.display(),
                        if state.paused { " · paused" } else { "" }
                    ))
                    .borders(Borders::ALL),
            );
            frame.render_widget(timeline, chunks[0]);

            let current = state.current_paragraph().block(
                Block::default()
                    .title("Current tool call")
                    .borders(Borders::ALL),
            );
            frame.render_widget(current, chunks[1]);

            let summary = state.summary_paragraph().block(
                Block::default()
                    .title("Session summary · q quit · p pause/resume · c clear")
                    .borders(Borders::ALL),
            );
            frame.render_widget(summary, chunks[2]);
        });
        if let Err(err) = draw_result {
            break Err::<(), anyhow::Error>(err.into());
        }

        if event::poll(POLL_INTERVAL)? {
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                    KeyCode::Char('p') => state.paused = !state.paused,
                    KeyCode::Char('c') => state.events.clear(),
                    _ => {}
                }
            }
        }
    };

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    exit_result
}
