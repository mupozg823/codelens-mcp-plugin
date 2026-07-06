use super::super::{IndexDb, SymbolWithFile};
use anyhow::Result;

impl IndexDb {
    /// Get all symbols with byte offsets and file paths, ordered by file for batch processing.
    pub fn all_symbols_with_bytes(&self) -> Result<Vec<SymbolWithFile>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             ORDER BY s.file_id, s.start_byte",
        )?;
        let rows = stmt.query_map([], row_to_symbol_with_file)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Stream all symbols with bytes via callback.
    pub fn for_each_symbol_with_bytes<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(SymbolWithFile) -> Result<()>,
    {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             ORDER BY s.file_id, s.start_byte",
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0usize;
        while let Some(row) = rows.next()? {
            callback(row_to_symbol_with_file(row)?)?;
            count += 1;
        }
        Ok(count)
    }

    /// Stream symbols grouped by file path via callback.
    pub fn for_each_file_symbols_with_bytes<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(String, Vec<SymbolWithFile>) -> Result<()>,
    {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             ORDER BY f.relative_path, s.start_byte",
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0usize;
        let mut current_file: Option<String> = None;
        let mut current_symbols: Vec<SymbolWithFile> = Vec::new();

        while let Some(row) = rows.next()? {
            let symbol = row_to_symbol_with_file(row)?;
            if current_file.as_deref() != Some(symbol.file_path.as_str())
                && let Some(previous_file) = current_file.replace(symbol.file_path.clone())
            {
                callback(previous_file, std::mem::take(&mut current_symbols))?;
            }
            current_symbols.push(symbol);
            count += 1;
        }

        if let Some(file_path) = current_file {
            callback(file_path, current_symbols)?;
        }
        Ok(count)
    }

    /// Get symbols with bytes for specific files only.
    pub fn symbols_for_files(&self, file_paths: &[&str]) -> Result<Vec<SymbolWithFile>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (1..=file_paths.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE f.relative_path IN ({})
             ORDER BY s.file_id, s.start_byte",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), row_to_symbol_with_file)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }
}

fn row_to_symbol_with_file(row: &rusqlite::Row<'_>) -> rusqlite::Result<SymbolWithFile> {
    Ok(SymbolWithFile {
        name: row.get(0)?,
        kind: row.get(1)?,
        file_path: row.get(2)?,
        line: row.get(3)?,
        signature: row.get(4)?,
        name_path: row.get(5)?,
        start_byte: row.get(6)?,
        end_byte: row.get(7)?,
    })
}
