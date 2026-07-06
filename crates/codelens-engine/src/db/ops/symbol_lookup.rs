use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::super::{IndexDb, SymbolRow};
use super::symbol_fts;
use super::symbol_rows::symbol_row_from_row;

impl IndexDb {
    /// Query symbols by name (exact or substring match).
    pub fn find_symbols_by_name(
        &self,
        name: &str,
        file_path: Option<&str>,
        exact: bool,
        max_results: usize,
    ) -> Result<Vec<SymbolRow>> {
        let name = crate::unicode::nfc_identifier(name);
        let name = name.as_ref();
        let (sql, use_file_filter) = match (exact, file_path.is_some()) {
            (true, true) => (
                "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num, s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id
                 FROM symbols s JOIN files f ON s.file_id = f.id
                 WHERE s.name = ?1 AND f.relative_path = ?2
                 LIMIT ?3",
                true,
            ),
            (true, false) => (
                "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
                 FROM symbols WHERE name = ?1
                 LIMIT ?2",
                false,
            ),
            (false, true) => (
                "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num, s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id
                 FROM symbols s JOIN files f ON s.file_id = f.id
                 WHERE s.name LIKE '%' || ?1 || '%' AND f.relative_path = ?2
                 ORDER BY LENGTH(s.name), s.name
                 LIMIT ?3",
                true,
            ),
            (false, false) => (
                "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
                 FROM symbols WHERE name LIKE '%' || ?1 || '%'
                 ORDER BY LENGTH(name), name
                 LIMIT ?2",
                false,
            ),
        };

        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = if use_file_filter {
            stmt.query(params![name, file_path.unwrap_or(""), max_results as i64])?
        } else {
            stmt.query(params![name, max_results as i64])?
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push(symbol_row_from_row(row)?);
        }
        Ok(results)
    }

    /// Query symbols by exact `name_path` within one indexed file.
    pub fn find_symbols_by_name_path(
        &self,
        file_path: &str,
        name_path: &str,
        max_results: usize,
    ) -> Result<Vec<SymbolRow>> {
        let name_path = crate::unicode::nfc_identifier(name_path);
        let name_path = name_path.as_ref();
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num, s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE s.name_path = ?1 AND f.relative_path = ?2
             LIMIT ?3",
        )?;
        let mut rows = stmt.query(params![name_path, file_path, max_results as i64])?;

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push(symbol_row_from_row(row)?);
        }
        Ok(results)
    }

    /// Query symbols by name with file path resolved via JOIN.
    pub fn find_symbols_with_path(
        &self,
        name: &str,
        exact: bool,
        max_results: usize,
    ) -> Result<Vec<(SymbolRow, String)>> {
        let name = crate::unicode::nfc_identifier(name);
        let name = name.as_ref();
        let sql = if exact {
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE s.name = ?1
             LIMIT ?2"
        } else {
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE s.name LIKE '%' || ?1 || '%'
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = stmt.query(params![name, max_results as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((symbol_row_from_row(row)?, row.get::<_, String>(11)?));
        }
        Ok(results)
    }

    /// Get all symbols for a file, ordered by start_byte.
    pub fn get_file_symbols(&self, file_id: i64) -> Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
             FROM symbols WHERE file_id = ?1 ORDER BY start_byte",
        )?;
        let rows = stmt.query_map(params![file_id], symbol_row_from_row)?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Full-text search symbols via FTS5 index.
    pub fn search_symbols_fts(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<(SymbolRow, String, f64)>> {
        symbol_fts::search_symbols_fts(self, query, max_results)
    }

    /// Get all symbols for files under a directory prefix in a single JOIN query.
    pub fn get_symbols_for_directory(&self, prefix: &str) -> Result<Vec<(String, Vec<SymbolRow>)>> {
        let pattern = if prefix.is_empty() || prefix == "." {
            "%".to_owned()
        } else {
            format!("{prefix}%")
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path
             FROM symbols s
             JOIN files f ON s.file_id = f.id
             WHERE f.relative_path LIKE ?1
             ORDER BY s.file_id, s.start_byte",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((symbol_row_from_row(row)?, row.get::<_, String>(11)?))
        })?;

        let mut groups: Vec<(String, Vec<SymbolRow>)> = Vec::new();
        let mut current_path = String::new();
        for row in rows {
            let (sym, path) = row?;
            if path != current_path {
                current_path = path.clone();
                groups.push((path, Vec::new()));
            }
            if let Some((_, symbols)) = groups.last_mut() {
                symbols.push(sym);
            }
        }
        Ok(groups)
    }

    /// Return all symbols as (name, kind, file_path, line, signature, name_path).
    #[allow(clippy::type_complexity)]
    pub fn all_symbol_names(&self) -> Result<Vec<(String, String, String, i64, String, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path
             FROM symbols s JOIN files f ON s.file_id = f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get file path for a file_id.
    pub fn get_file_path(&self, file_id: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT relative_path FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .optional()
            .context("get_file_path query failed")
    }
}
