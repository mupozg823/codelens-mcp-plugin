use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::super::{IndexDb, SymbolRow};
use super::symbol_rows::symbol_row_from_row;

pub(super) fn search_symbols_fts(
    db: &IndexDb,
    query: &str,
    max_results: usize,
) -> Result<Vec<(SymbolRow, String, f64)>> {
    let fts_exists: bool = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='symbols_fts'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !fts_exists {
        return db
            .find_symbols_with_path(query, false, max_results)
            .map(|rows| rows.into_iter().map(|(r, p)| (r, p, 0.0)).collect());
    }

    rebuild_fts_if_stale(db)?;
    let fts_query = fts5_escape(query);
    let mut stmt = db.conn.prepare_cached(
        "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                f.relative_path, rank
         FROM symbols_fts
         JOIN symbols s ON symbols_fts.rowid = s.id
         JOIN files f ON s.file_id = f.id
         WHERE symbols_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;

    let mut rows = stmt.query(params![fts_query, max_results as i64])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push((
            symbol_row_from_row(row)?,
            row.get::<_, String>(11)?,
            row.get::<_, f64>(12)?,
        ));
    }
    Ok(results)
}

/// Build FTS5 query: split into tokens, add prefix matching (*), join with OR.
fn fts5_escape(query: &str) -> String {
    let tokens: Vec<String> = query
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|t| !t.is_empty())
        .map(|token| {
            let escaped = token.replace('"', "\"\"");
            format!("{escaped}*")
        })
        .collect();
    if tokens.is_empty() {
        let escaped = query.replace('"', "\"\"");
        return format!("{escaped}*");
    }
    tokens.join(" OR ")
}

fn rebuild_fts_if_stale(db: &IndexDb) -> Result<()> {
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let last_rebuild_ts: i64 = db
        .conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'fts_rebuild_ts'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);

    if now_secs - last_rebuild_ts <= 30 {
        return Ok(());
    }

    let fts_fresh: bool = db
        .conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'fts_symbol_count'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.parse::<i64>().ok())
        .map(|cached_count| {
            let current: i64 = db
                .conn
                .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
                .unwrap_or(0);
            cached_count == current
        })
        .unwrap_or(false);

    if !fts_fresh {
        let sym_count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
            .unwrap_or(0);
        if sym_count > 0 {
            let _ = db
                .conn
                .execute_batch("INSERT INTO symbols_fts(symbols_fts) VALUES('rebuild')");
            let _ = db.conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('fts_symbol_count', ?1)",
                params![sym_count.to_string()],
            );
        }
    }
    let _ = db.conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('fts_rebuild_ts', ?1)",
        params![now_secs.to_string()],
    );
    Ok(())
}
