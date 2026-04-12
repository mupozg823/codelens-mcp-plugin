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
    pub search_mode: bool,
    pub search_query: String,
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
            search_mode: false,
            search_query: String::new(),
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

    /// Get importer count for the currently selected file.
    pub fn current_file_importer_count(&self) -> usize {
        if let Some(file) = self.files.get(self.file_cursor) {
            self.index
                .db()
                .get_importers(&file.path)
                .map(|v| v.len())
                .unwrap_or(0)
        } else {
            0
        }
    }

    pub fn filtered_files(&self) -> Vec<(usize, &FileEntry)> {
        if self.search_query.is_empty() {
            return self.files.iter().enumerate().collect();
        }
        let q = self.search_query.to_lowercase();
        self.files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.path.to_lowercase().contains(&q))
            .collect()
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Action {
        // Handle search mode input first.
        if self.search_mode {
            match key {
                KeyCode::Esc | KeyCode::Enter => {
                    self.search_mode = false;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                }
                _ => {}
            }
            return Action::Continue;
        }

        match key {
            KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
            KeyCode::Char('/') => {
                self.search_mode = true;
                self.search_query.clear();
                Action::Continue
            }
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
                        let filtered = self.filtered_files();
                        if let Some(pos) = filtered
                            .iter()
                            .position(|(i, _)| *i == self.file_cursor)
                            .filter(|&p| p > 0)
                        {
                            self.file_cursor = filtered[pos - 1].0;
                            self.load_symbols_for_current_file();
                        }
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
                        let filtered = self.filtered_files();
                        if let Some(pos) = filtered.iter().position(|(i, _)| *i == self.file_cursor)
                        {
                            if pos + 1 < filtered.len() {
                                self.file_cursor = filtered[pos + 1].0;
                                self.load_symbols_for_current_file();
                            }
                        } else if let Some((first_idx, _)) = filtered.first() {
                            // Cursor is not in filtered results — snap to first match
                            self.file_cursor = *first_idx;
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
