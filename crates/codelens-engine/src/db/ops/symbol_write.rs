use anyhow::Result;
use rusqlite::{Connection, params};

use super::super::{IndexDb, NewSymbol};

/// Bulk insert symbols for a file. Returns the inserted symbol ids.
pub(crate) fn insert_symbols(
    conn: &Connection,
    file_id: i64,
    symbols: &[NewSymbol<'_>],
) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(symbols.len());
    let mut stmt = conn.prepare_cached(
        "INSERT INTO symbols (file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for sym in symbols {
        // #349: the `name`/`name_path` columns hold NFC.
        let name = crate::unicode::nfc_identifier(sym.name);
        let name_path = crate::unicode::nfc_identifier(sym.name_path);
        stmt.execute(params![
            file_id,
            name.as_ref(),
            sym.kind,
            sym.line,
            sym.column_num,
            sym.start_byte,
            sym.end_byte,
            sym.signature,
            name_path.as_ref(),
            sym.parent_id,
        ])?;
        ids.push(conn.last_insert_rowid());
    }
    Ok(ids)
}

impl IndexDb {
    /// Bulk insert symbols for a file. Returns the inserted symbol ids.
    pub fn insert_symbols(&self, file_id: i64, symbols: &[NewSymbol<'_>]) -> Result<Vec<i64>> {
        insert_symbols(&self.conn, file_id, symbols)
    }
}
