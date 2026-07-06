use anyhow::Result;
use rusqlite::params;

use super::super::IndexDb;

impl IndexDb {
    /// Record an indexing failure for a file. Updates retry_count on conflict.
    pub fn record_index_failure(
        &self,
        file_path: &str,
        error_type: &str,
        error_message: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.conn.execute(
            "INSERT INTO index_failures (file_path, error_type, error_message, failed_at, retry_count)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(file_path) DO UPDATE SET
                error_type = excluded.error_type,
                error_message = excluded.error_message,
                failed_at = excluded.failed_at,
                retry_count = retry_count + 1",
            params![file_path, error_type, error_message, now],
        )?;
        Ok(())
    }

    /// Clear a failure record when a file is successfully indexed.
    pub fn clear_index_failure(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM index_failures WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(())
    }

    /// Invalidate FTS index cache so next search triggers a lazy rebuild.
    pub fn invalidate_fts(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM meta WHERE key = 'fts_symbol_count'", [])?;
        Ok(())
    }

    /// Get the number of files with indexing failures.
    pub fn index_failure_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM index_failures", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Remove failure records for files that no longer exist on disk.
    pub fn prune_missing_index_failures(&self, project_root: &std::path::Path) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT file_path FROM index_failures ORDER BY file_path")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut missing = Vec::new();
        for row in rows {
            let relative_path = row?;
            if !project_root.join(&relative_path).is_file() {
                missing.push(relative_path);
            }
        }
        for relative_path in &missing {
            self.clear_index_failure(relative_path)?;
        }
        Ok(missing.len())
    }

    /// Summarize unresolved index failures by recency and persistence.
    pub fn index_failure_summary(
        &self,
        recent_window_secs: i64,
    ) -> Result<crate::db::IndexFailureSummary> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let recent_cutoff = now.saturating_sub(recent_window_secs.max(0));

        let total_failures: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM index_failures", [], |row| row.get(0))?;
        let recent_failures: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM index_failures WHERE failed_at >= ?1",
            params![recent_cutoff],
            |row| row.get(0),
        )?;
        let persistent_failures: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM index_failures WHERE retry_count >= 3",
            [],
            |row| row.get(0),
        )?;

        Ok(crate::db::IndexFailureSummary {
            total_failures: total_failures as usize,
            recent_failures: recent_failures as usize,
            stale_failures: total_failures.saturating_sub(recent_failures) as usize,
            persistent_failures: persistent_failures as usize,
        })
    }

    /// Get files that have failed more than `min_retries` times.
    pub fn get_persistent_failures(&self, min_retries: i64) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT file_path, error_message, retry_count FROM index_failures WHERE retry_count >= ?1 ORDER BY retry_count DESC",
        )?;
        let mut rows = stmt.query(params![min_retries])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(results)
    }
}
