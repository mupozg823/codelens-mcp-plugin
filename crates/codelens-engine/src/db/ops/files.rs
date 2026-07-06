use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use super::super::{FileRow, IndexDb};

/// Returns the file row if it exists and is fresh (same mtime and hash).
pub(crate) fn get_fresh_file(
    conn: &Connection,
    relative_path: &str,
    mtime_ms: i64,
    content_hash: &str,
) -> Result<Option<FileRow>> {
    conn.query_row(
        "SELECT id, relative_path, mtime_ms, content_hash, size_bytes, language
         FROM files WHERE relative_path = ?1 AND mtime_ms = ?2 AND content_hash = ?3",
        params![relative_path, mtime_ms, content_hash],
        |row| {
            Ok(FileRow {
                id: row.get(0)?,
                relative_path: row.get(1)?,
                mtime_ms: row.get(2)?,
                content_hash: row.get(3)?,
                size_bytes: row.get(4)?,
                language: row.get(5)?,
            })
        },
    )
    .optional()
    .context("get_fresh_file query failed")
}

/// Upsert a file record. Returns the file id. Deletes old symbols/imports on update.
pub(crate) fn upsert_file(
    conn: &Connection,
    relative_path: &str,
    mtime_ms: i64,
    content_hash: &str,
    size_bytes: i64,
    language: Option<&str>,
) -> Result<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let id: i64 = conn.query_row(
        "INSERT INTO files (relative_path, mtime_ms, content_hash, size_bytes, language, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(relative_path) DO UPDATE SET
            mtime_ms = excluded.mtime_ms,
            content_hash = excluded.content_hash,
            size_bytes = excluded.size_bytes,
            language = excluded.language,
            indexed_at = excluded.indexed_at
         RETURNING id",
        params![relative_path, mtime_ms, content_hash, size_bytes, language, now],
        |row| row.get(0),
    )?;

    conn.execute("DELETE FROM symbols WHERE file_id = ?1", params![id])?;
    conn.execute("DELETE FROM imports WHERE source_file_id = ?1", params![id])?;
    conn.execute("DELETE FROM calls WHERE caller_file_id = ?1", params![id])?;

    Ok(id)
}

/// Delete a file and its associated symbols/imports.
pub(crate) fn delete_file(conn: &Connection, relative_path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM files WHERE relative_path = ?1",
        params![relative_path],
    )?;
    Ok(())
}

/// Clear all symbol-index content in bulk.
pub(crate) fn clear_symbol_index(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM symbols;
         DELETE FROM imports;
         DELETE FROM calls;
         DELETE FROM files;",
    )?;
    Ok(())
}

impl IndexDb {
    /// Fast mtime-only freshness check. Avoids content hashing entirely.
    pub fn get_fresh_file_by_mtime(
        &self,
        relative_path: &str,
        mtime_ms: i64,
    ) -> Result<Option<FileRow>> {
        self.conn
            .query_row(
                "SELECT id, relative_path, mtime_ms, content_hash, size_bytes, language
                 FROM files WHERE relative_path = ?1 AND mtime_ms = ?2",
                params![relative_path, mtime_ms],
                |row| {
                    Ok(FileRow {
                        id: row.get(0)?,
                        relative_path: row.get(1)?,
                        mtime_ms: row.get(2)?,
                        content_hash: row.get(3)?,
                        size_bytes: row.get(4)?,
                        language: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("get_fresh_file_by_mtime query failed")
    }

    /// Returns the file row if it exists and is fresh (same mtime and hash).
    pub fn get_fresh_file(
        &self,
        relative_path: &str,
        mtime_ms: i64,
        content_hash: &str,
    ) -> Result<Option<FileRow>> {
        get_fresh_file(&self.conn, relative_path, mtime_ms, content_hash)
    }

    /// Returns the file row by path (regardless of freshness).
    pub fn get_file(&self, relative_path: &str) -> Result<Option<FileRow>> {
        self.conn
            .query_row(
                "SELECT id, relative_path, mtime_ms, content_hash, size_bytes, language
                 FROM files WHERE relative_path = ?1",
                params![relative_path],
                |row| {
                    Ok(FileRow {
                        id: row.get(0)?,
                        relative_path: row.get(1)?,
                        mtime_ms: row.get(2)?,
                        content_hash: row.get(3)?,
                        size_bytes: row.get(4)?,
                        language: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("get_file query failed")
    }

    /// Upsert a file record. Returns the file id. Deletes old symbols/imports on update.
    pub fn upsert_file(
        &self,
        relative_path: &str,
        mtime_ms: i64,
        content_hash: &str,
        size_bytes: i64,
        language: Option<&str>,
    ) -> Result<i64> {
        upsert_file(
            &self.conn,
            relative_path,
            mtime_ms,
            content_hash,
            size_bytes,
            language,
        )
    }

    /// Delete a file and its associated symbols/imports.
    pub fn delete_file(&self, relative_path: &str) -> Result<()> {
        delete_file(&self.conn, relative_path)
    }
}
