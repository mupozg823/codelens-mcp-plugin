use anyhow::Result;
use rusqlite::params;

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

    let Some(fts_query) = fts5_escape(query) else {
        return Ok(Vec::new());
    };
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
///
/// Splits on every non-alphanumeric character, the same rule the `unicode61`
/// index tokenizer applies, so `next.js` or `std::fs` reach the indexed
/// tokens instead of failing FTS5 parsing. Each token is quoted so words such
/// as `OR` or `NOT` stay terms rather than operators. Stopwords are dropped
/// unless nothing else is left. `None` when the query has no token to match.
fn fts5_escape(query: &str) -> Option<String> {
    let tokens: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let terms: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|token| !crate::symbols::scoring::is_query_stopword(token))
        .collect();
    let terms = if terms.is_empty() { tokens } else { terms };
    (!terms.is_empty()).then(|| {
        terms
            .iter()
            .map(|term| format!("\"{term}\"*"))
            .collect::<Vec<_>>()
            .join(" OR ")
    })
}
