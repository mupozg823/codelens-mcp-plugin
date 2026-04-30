use codelens_engine::{
    SymbolIndex, SymbolInfo, configured_embedding_model_name, configured_embedding_runtime_info,
    embedding_model_assets_available,
};
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

pub struct AppHealth {
    pub project_root: String,
    pub indexed_files: usize,
    pub supported_files: usize,
    pub stale_files: usize,
    pub semantic_assets_available: bool,
    pub embedding_model: String,
    pub embedding_runtime_preference: String,
    pub embedding_runtime_backend: String,
    pub embedding_threads: usize,
    pub embedding_max_length: usize,
    pub warnings: Vec<String>,
}

impl AppHealth {
    fn new(
        project_root: String,
        indexed_files: usize,
        supported_files: usize,
        stale_files: usize,
    ) -> Self {
        let runtime = configured_embedding_runtime_info();
        let semantic_assets = embedding_model_assets_available();
        let mut warnings = Vec::new();
        if supported_files == 0 {
            warnings.push("no supported source files detected".to_string());
        }
        if indexed_files == 0 {
            warnings.push("index is empty".to_string());
        }
        if supported_files > 0 && indexed_files < supported_files {
            warnings.push(format!(
                "index coverage incomplete ({indexed_files}/{supported_files})"
            ));
        }
        if stale_files > 0 {
            warnings.push(format!("{stale_files} indexed files are stale"));
        }
        if !semantic_assets {
            warnings.push("semantic model assets unavailable".to_string());
        }

        Self {
            project_root,
            indexed_files,
            supported_files,
            stale_files,
            semantic_assets_available: semantic_assets,
            embedding_model: configured_embedding_model_name(),
            embedding_runtime_preference: runtime.runtime_preference,
            embedding_runtime_backend: runtime.backend,
            embedding_threads: runtime.threads,
            embedding_max_length: runtime.max_length,
            warnings,
        }
    }

    pub fn status_label(&self) -> &'static str {
        if self.warnings.is_empty() {
            "OK"
        } else {
            "WARN"
        }
    }

    pub fn coverage_percent(&self) -> usize {
        self.indexed_files
            .checked_mul(100)
            .and_then(|v| v.checked_div(self.supported_files))
            .unwrap_or(0)
    }
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
    pub health: AppHealth,
    pub search_mode: bool,
    pub search_query: String,
    pub symbol_search_mode: bool,
    pub symbol_search_query: String,
}

impl App {
    pub fn new(path: &str) -> anyhow::Result<Self> {
        use codelens_engine::ProjectRoot;

        // ProjectRoot::new auto-detects the actual root by walking up.
        let project = ProjectRoot::new(path)?;
        let project_root = project.as_path().display().to_string();
        let project_name = project
            .as_path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // SymbolIndex::new takes ownership of ProjectRoot.
        let index = Arc::new(SymbolIndex::new(project));

        // Read stats to get indexed file count and operator-facing health data.
        let stats = index.stats().ok();
        let total_indexed_files = stats.as_ref().map_or(0, |s| s.indexed_files);
        let health = AppHealth::new(
            project_root,
            stats.as_ref().map_or(0, |s| s.indexed_files),
            stats.as_ref().map_or(0, |s| s.supported_files),
            stats.as_ref().map_or(0, |s| s.stale_files),
        );

        let mut app = App {
            index,
            files: Vec::new(),
            file_cursor: 0,
            symbols: Vec::new(),
            symbol_cursor: 0,
            active_panel: Panel::Tree,
            project_name,
            total_indexed_files,
            health,
            search_mode: false,
            search_query: String::new(),
            symbol_search_mode: false,
            symbol_search_query: String::new(),
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
                .map_or(0, |v| v.len())
        } else {
            0
        }
    }

    /// Files that import the currently selected file (who depends on us).
    pub fn current_file_importers(&self) -> Vec<String> {
        if let Some(file) = self.files.get(self.file_cursor) {
            self.index
                .db()
                .get_importers(&file.path)
                .unwrap_or_default()
        } else {
            vec![]
        }
    }

    /// Files that the currently selected file imports (what we depend on).
    pub fn current_file_imports(&self) -> Vec<String> {
        if let Some(file) = self.files.get(self.file_cursor) {
            self.index
                .db()
                .get_imports_of(&file.path)
                .unwrap_or_default()
        } else {
            vec![]
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

    pub fn search_symbols(&self) -> Vec<SymbolInfo> {
        if self.symbol_search_query.is_empty() {
            return vec![];
        }
        self.index
            .find_symbol(&self.symbol_search_query, None, false, false, 20)
            .unwrap_or_default()
    }

    pub fn check_payload(&self) -> serde_json::Value {
        let filtered = self.filtered_files();
        serde_json::json!({
            "status": self.health.status_label(),
            "project_name": self.project_name,
            "project_root": self.health.project_root,
            "indexed_files": self.health.indexed_files,
            "supported_files": self.health.supported_files,
            "stale_files": self.health.stale_files,
            "coverage_percent": self.health.coverage_percent(),
            "visible_files": filtered.len(),
            "semantic_assets_available": self.health.semantic_assets_available,
            "embedding_model": self.health.embedding_model,
            "embedding_runtime_preference": self.health.embedding_runtime_preference,
            "embedding_runtime_backend": self.health.embedding_runtime_backend,
            "embedding_threads": self.health.embedding_threads,
            "embedding_max_length": self.health.embedding_max_length,
            "warning_count": self.health.warnings.len(),
            "warnings": self.health.warnings,
            "first_file": filtered.first().map(|(_, first)| first.path.clone()),
            "first_symbol": self.symbols.first().map(|sym| {
                serde_json::json!({
                    "name": sym.name,
                    "kind": format!("{:?}", sym.kind),
                    "line": sym.line,
                    "file": sym.file_path,
                })
            }),
        })
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Action {
        // Handle symbol search mode input first.
        if self.symbol_search_mode {
            match key {
                KeyCode::Esc => {
                    self.symbol_search_mode = false;
                }
                KeyCode::Enter => {
                    self.symbol_search_mode = false;
                    let results = self.search_symbols();
                    if let Some(first) = results.first() {
                        let file_path = first.file_path.clone();
                        if let Some(pos) = self.files.iter().position(|f| f.path == file_path) {
                            self.file_cursor = pos;
                            self.load_symbols_for_current_file();
                        }
                    }
                }
                KeyCode::Backspace => {
                    self.symbol_search_query.pop();
                }
                KeyCode::Char(c) => {
                    self.symbol_search_query.push(c);
                }
                _ => {}
            }
            return Action::Continue;
        }

        // Handle file search mode input.
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
            KeyCode::Char('s') => {
                self.symbol_search_mode = true;
                self.symbol_search_query.clear();
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
            KeyCode::Enter => {
                if let Panel::Symbols = self.active_panel
                    && let Some(sym) = self.selected_symbol()
                {
                    let target = sym.file_path.clone();
                    if let Some(pos) = self.files.iter().position(|f| f.path == target) {
                        self.file_cursor = pos;
                        self.load_symbols_for_current_file();
                        self.active_panel = Panel::Tree;
                    }
                }
                Action::Continue
            }
            _ => Action::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppHealth;

    #[test]
    fn app_health_is_ok_when_index_and_semantic_state_are_clean() {
        let health = AppHealth {
            project_root: "/tmp/project".to_string(),
            indexed_files: 10,
            supported_files: 10,
            stale_files: 0,
            semantic_assets_available: true,
            embedding_model: "MiniLM".to_string(),
            embedding_runtime_preference: "cpu".to_string(),
            embedding_runtime_backend: "cpu".to_string(),
            embedding_threads: 4,
            embedding_max_length: 256,
            warnings: vec![],
        };
        assert_eq!(health.status_label(), "OK");
        assert_eq!(health.coverage_percent(), 100);
    }

    #[test]
    fn app_health_warns_when_coverage_is_incomplete() {
        let health = AppHealth::new("/tmp/project".to_string(), 5, 10, 2);
        assert_eq!(health.status_label(), "WARN");
        assert!(health.warnings.iter().any(|w| w.contains("coverage")));
        assert!(health.warnings.iter().any(|w| w.contains("stale")));
    }
}
