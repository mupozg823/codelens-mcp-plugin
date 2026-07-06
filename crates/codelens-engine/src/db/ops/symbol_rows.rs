use rusqlite::Row;

use super::super::SymbolRow;

pub(super) fn symbol_row_from_row(row: &Row<'_>) -> rusqlite::Result<SymbolRow> {
    Ok(SymbolRow {
        id: row.get(0)?,
        file_id: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        line: row.get(4)?,
        column_num: row.get(5)?,
        start_byte: row.get(6)?,
        end_byte: row.get(7)?,
        signature: row.get(8)?,
        name_path: row.get(9)?,
        parent_id: row.get(10)?,
    })
}
