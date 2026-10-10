//! Giving back the space deleted index rows leave behind.
//!
//! The index connections asked for `auto_vacuum = INCREMENTAL` after
//! `journal_mode = WAL`. Switching to WAL writes the database header, which
//! fixes the auto-vacuum mode, so every index (new ones too) was created with
//! auto-vacuum off, and nothing called `incremental_vacuum` either. Rows a
//! refresh removed stayed as free pages: on 2026-10-10 drawboard's symbol
//! index was 166.8 MB with 149.7 MB free and SignatureStudio's 73.2 MB with
//! 47.6 MB free, after the gitignore and nested-worktree cleanups.

use anyhow::Result;
use rusqlite::Connection;

/// Free space worth giving back: a quarter of the file and at least 16 MiB.
pub(crate) const MIN_FREE_RATIO: f64 = 0.25;
pub(crate) const MIN_FREE_BYTES: i64 = 16 * 1024 * 1024;

const AUTO_VACUUM_INCREMENTAL: i64 = 2;

/// What a compaction gave back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactReport {
    pub bytes_before: i64,
    pub bytes_after: i64,
    /// The file had auto-vacuum off and was rebuilt once with `VACUUM`, which
    /// also switched it to incremental; later compactions free pages in place.
    pub converted: bool,
}

/// Give back free pages when at least `min_ratio` of the file, and
/// `min_bytes`, is free. Runs outside any transaction, on the connection of
/// the process that holds the project's writer lease.
pub(crate) fn compact_if_mostly_free(
    conn: &Connection,
    min_ratio: f64,
    min_bytes: i64,
) -> Result<Option<CompactReport>> {
    let page_size: i64 = conn.pragma_query_value(None, "page_size", |row| row.get(0))?;
    let page_count: i64 = conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
    let free_pages: i64 = conn.pragma_query_value(None, "freelist_count", |row| row.get(0))?;
    if page_count == 0
        || free_pages * page_size < min_bytes
        || (free_pages as f64) < min_ratio * page_count as f64
    {
        return Ok(None);
    }
    let auto_vacuum: i64 = conn.pragma_query_value(None, "auto_vacuum", |row| row.get(0))?;
    let converted = auto_vacuum != AUTO_VACUUM_INCREMENTAL;
    if converted {
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL; VACUUM;")?;
    } else {
        // Each step frees one page, so `execute_batch` (one step) would give
        // back a single page; drain the statement.
        let mut statement = conn.prepare("PRAGMA incremental_vacuum")?;
        let mut rows = statement.query([])?;
        while rows.next()?.is_some() {}
    }
    // In WAL mode the file itself shrinks when the WAL is checkpointed. A
    // reader holding an old snapshot only delays that; it is not an error.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let page_count_after: i64 = conn.pragma_query_value(None, "page_count", |row| row.get(0))?;
    Ok(Some(CompactReport {
        bytes_before: page_count * page_size,
        bytes_after: page_count_after * page_size,
        converted,
    }))
}
