use anyhow::Result;
use rusqlite::{Connection, params};

use super::super::{IndexDb, NewImport};

/// Bulk insert imports for a file.
pub(crate) fn insert_imports(conn: &Connection, file_id: i64, imports: &[NewImport]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO imports (source_file_id, target_path, raw_import)
         VALUES (?1, ?2, ?3)",
    )?;
    for imp in imports {
        stmt.execute(params![file_id, imp.target_path, imp.raw_import])?;
    }
    Ok(())
}

impl IndexDb {
    /// Bulk insert imports for a file.
    pub fn insert_imports(&self, file_id: i64, imports: &[NewImport]) -> Result<()> {
        insert_imports(&self.conn, file_id, imports)
    }

    /// Get files that import the given file path (reverse dependencies).
    pub fn get_importers(&self, target_path: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.relative_path FROM imports i
             JOIN files f ON i.source_file_id = f.id
             WHERE i.target_path = ?1
             ORDER BY f.relative_path",
        )?;
        let rows = stmt.query_map(params![target_path], |row| row.get(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get files that the given file imports (forward dependencies).
    pub fn get_imports_of(&self, relative_path: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT i.target_path FROM imports i
             JOIN files f ON i.source_file_id = f.id
             WHERE f.relative_path = ?1
             ORDER BY i.target_path",
        )?;
        let rows = stmt.query_map(params![relative_path], |row| row.get(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Build the full import graph from the database.
    #[allow(clippy::type_complexity)]
    pub fn build_import_graph(
        &self,
    ) -> Result<std::collections::HashMap<String, (Vec<String>, Vec<String>)>> {
        let mut graph = std::collections::HashMap::new();
        for path in self.all_file_paths()? {
            graph.insert(path, (Vec::new(), Vec::new()));
        }

        let mut stmt = self.conn.prepare_cached(
            "SELECT f.relative_path, i.target_path FROM imports i
             JOIN files f ON i.source_file_id = f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (source, target) = row?;
            if let Some(entry) = graph.get_mut(&source) {
                entry.0.push(target.clone());
            }
            if let Some(entry) = graph.get_mut(&target) {
                entry.1.push(source.clone());
            }
        }
        Ok(graph)
    }
}
