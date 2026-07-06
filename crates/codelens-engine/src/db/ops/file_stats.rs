use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

use super::super::{DirStats, IndexDb};

/// Per-directory aggregate stats.
pub(crate) fn dir_stats(conn: &Connection) -> Result<Vec<DirStats>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.relative_path, COUNT(s.id) AS sym_count
         FROM files f LEFT JOIN symbols s ON s.file_id = f.id
         GROUP BY f.id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;

    let mut dir_map: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for row in rows {
        let (path, sym_count) = row?;
        let dir = match path.rfind('/') {
            Some(pos) => &path[..=pos],
            None => ".",
        };
        let entry = dir_map.entry(dir.to_owned()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += sym_count;
    }

    let mut result: Vec<DirStats> = dir_map
        .into_iter()
        .map(|(dir, (files, symbols))| DirStats {
            dir,
            files,
            symbols,
            imports_from_others: 0,
        })
        .collect();
    result.sort_by_key(|b| std::cmp::Reverse(b.symbols));
    Ok(result)
}

/// Return all indexed file paths.
pub(crate) fn all_file_paths(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT relative_path FROM files")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

/// Return file paths that contain symbols of the given kinds.
pub(crate) fn files_with_symbol_kinds(conn: &Connection, kinds: &[&str]) -> Result<Vec<String>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT DISTINCT f.relative_path FROM files f \
         JOIN symbols s ON s.file_id = f.id \
         WHERE s.kind IN ({placeholders})"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = kinds
        .iter()
        .map(|k| k as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = stmt.query_map(params.as_slice(), |row| row.get(0))?;
    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

impl IndexDb {
    /// Count indexed files.
    pub fn file_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Newest `indexed_at` epoch (seconds) across all files in the index.
    pub fn max_files_indexed_at(&self) -> Result<Option<i64>> {
        let row: Option<i64> = self
            .conn
            .query_row("SELECT MAX(indexed_at) FROM files", [], |row| row.get(0))
            .optional()?;
        Ok(row)
    }

    /// Per-language indexed-file counts, descending.
    pub fn language_file_counts(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT language, COUNT(*) FROM files \
             WHERE language IS NOT NULL GROUP BY language ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?;
        let mut counts = Vec::new();
        for row in rows {
            counts.push(row?);
        }
        Ok(counts)
    }

    /// Oldest `indexed_at` epoch (seconds) across all files in the index.
    pub fn min_files_indexed_at(&self) -> Result<Option<i64>> {
        let row: Option<i64> = self
            .conn
            .query_row("SELECT MIN(indexed_at) FROM files", [], |row| row.get(0))
            .optional()?;
        Ok(row)
    }

    /// Return all indexed file paths.
    pub fn all_file_paths(&self) -> Result<Vec<String>> {
        all_file_paths(&self.conn)
    }

    /// Return file paths containing symbols of given kinds.
    pub fn files_with_symbol_kinds(&self, kinds: &[&str]) -> Result<Vec<String>> {
        files_with_symbol_kinds(&self.conn, kinds)
    }

    pub fn dir_stats(&self) -> Result<Vec<DirStats>> {
        dir_stats(&self.conn)
    }
}
