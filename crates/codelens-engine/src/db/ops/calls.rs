use anyhow::Result;
use rusqlite::{Connection, params};

use super::super::{IndexDb, NewCall};

/// Bulk insert call edges for a file (clears old edges first).
pub(crate) fn insert_calls(conn: &Connection, file_id: i64, calls: &[NewCall]) -> Result<()> {
    conn.execute(
        "DELETE FROM calls WHERE caller_file_id = ?1",
        params![file_id],
    )?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO calls (caller_file_id, caller_name, callee_name, line)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for call in calls {
        // #353: call-edge names are stored as NFC.
        let caller_name = crate::unicode::nfc_identifier(&call.caller_name);
        let callee_name = crate::unicode::nfc_identifier(&call.callee_name);
        stmt.execute(params![
            file_id,
            caller_name.as_ref(),
            callee_name.as_ref(),
            call.line
        ])?;
    }
    Ok(())
}

impl IndexDb {
    /// Bulk insert call edges for a file (clears old edges first).
    pub fn insert_calls(&self, file_id: i64, calls: &[NewCall]) -> Result<()> {
        insert_calls(&self.conn, file_id, calls)
    }

    /// Find all callers of a function name (from DB cache).
    pub fn get_callers_cached(
        &self,
        callee_name: &str,
        max_results: usize,
    ) -> Result<Vec<(String, String, i64)>> {
        let callee_name = crate::unicode::nfc_identifier(callee_name);
        let callee_name = callee_name.as_ref();
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.relative_path, c.caller_name, c.line FROM calls c
             JOIN files f ON c.caller_file_id = f.id
             WHERE c.callee_name = ?1
             ORDER BY f.relative_path, c.line
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![callee_name, max_results as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(results)
    }

    /// Find all callees of a function name (from DB cache).
    pub fn get_callees_cached(
        &self,
        caller_name: &str,
        file_path: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<(String, i64)>> {
        let caller_name = crate::unicode::nfc_identifier(caller_name);
        let caller_name = caller_name.as_ref();
        let (sql, use_file) = match file_path {
            Some(_) => (
                "SELECT c.callee_name, c.line FROM calls c
                 JOIN files f ON c.caller_file_id = f.id
                 WHERE c.caller_name = ?1 AND f.relative_path = ?2
                 ORDER BY c.line LIMIT ?3",
                true,
            ),
            None => (
                "SELECT c.callee_name, c.line FROM calls c
                 WHERE c.caller_name = ?1
                 ORDER BY c.line LIMIT ?2",
                false,
            ),
        };
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = if use_file {
            stmt.query(params![
                caller_name,
                file_path.unwrap_or(""),
                max_results as i64
            ])?
        } else {
            stmt.query(params![caller_name, max_results as i64])?
        };
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((row.get(0)?, row.get(1)?));
        }
        Ok(results)
    }

    /// Check if calls table has any data.
    pub fn has_call_data(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM calls", [], |row| row.get(0))?;
        Ok(count > 0)
    }
}
