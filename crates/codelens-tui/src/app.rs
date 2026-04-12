use codelens_engine::{SymbolIndex, SymbolInfo};
use crossterm::event::KeyCode;
use std::sync::Arc;

pub enum Action {
    Quit,
    Continue,
}

pub enum Panel {
    Tree,
    Symbols,
    Detail,
}

pub struct FileEntry {
    pub path: String,
    pub depth: usize,
}

pub struct App {
    pub index: Arc<SymbolIndex>,
    pub files: Vec<FileEntry>,
    pub file_cursor: usize,
    pub symbols: Vec<SymbolInfo>,
    pub symbol_cursor: usize,
    pub active_panel: Panel,
    pub project_name: String,
    pub total_indexed_files: usize,
}

impl App {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        use codelens_engine::ProjectRoot;

        // ProjectRoot::new auto-detects the actual root by walking up.
        let project = ProjectRoot::new(path)?;
        let project_name = project
            .as_path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // SymbolIndex::new takes ownership of ProjectRoot.
        let index = Arc::new(SymbolIndex::new(project));

        // Read stats to get indexed file count.
        let total_indexed_files = index.stats().map(|s| s.indexed_files).unwrap_or(0);

        let mut app = App {
            index,
            files: Vec::new(),
            file_cursor: 0,
            symbols: Vec::new(),
            symbol_cursor: 0,
            active_panel: Panel::Tree,
            project_name,
            total_indexed_files,
        };
        app.load_files();
        Ok(app)
    }

    fn load_files(&mut self) {
        // Collect indexed file paths from the DB.
        let mut paths: Vec<String> = self.index.db().all_file_paths().unwrap_or_default();
        paths.sort();

        self.files = paths
            .into_iter()
            .map(|p| {
                let depth = p.matches('/').count();
                FileEntry { path: p, depth }
            })
            .collect();

        if !self.files.is_empty() {
            self.load_symbols_for_current_file();
        }
    }

    fn load_symbols_for_current_file(&mut self) {
        if let Some(file) = self.files.get(self.file_cursor) {
            // Use the SymbolIndex method to get symbols for a specific file path.
            self.symbols = self
                .index
                .get_symbols_overview(&file.path, 2)
                .unwrap_or_default();
            self.symbol_cursor = 0;
        }
    }

    pub fn selected_symbol(&self) -> Option<&SymbolInfo> {
        self.symbols.get(self.symbol_cursor)
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Action {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Tab => {
                self.active_panel = match self.active_panel {
                    Panel::Tree => Panel::Symbols,
                    Panel::Symbols => Panel::Detail,
                    Panel::Detail => Panel::Tree,
                };
                Action::Continue
            }
            KeyCode::Up => {
                match self.active_panel {
                    Panel::Tree => {
                        self.file_cursor = self.file_cursor.saturating_sub(1);
                        self.load_symbols_for_current_file();
                    }
                    Panel::Symbols => {
                        self.symbol_cursor = self.symbol_cursor.saturating_sub(1);
                    }
                    Panel::Detail => {}
                }
                Action::Continue
            }
            KeyCode::Down => {
                match self.active_panel {
                    Panel::Tree => {
                        if self.file_cursor + 1 < self.files.len() {
                            self.file_cursor += 1;
                            self.load_symbols_for_current_file();
                        }
                    }
                    Panel::Symbols => {
                        if self.symbol_cursor + 1 < self.symbols.len() {
                            self.symbol_cursor += 1;
                        }
                    }
                    Panel::Detail => {}
                }
                Action::Continue
            }
            _ => Action::Continue,
        }
    }
}
