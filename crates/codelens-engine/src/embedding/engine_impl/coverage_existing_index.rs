use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

use super::super::super::cache::{ReusableEmbeddingKey, reusable_embedding_key};
use super::super::super::vec_store::EMBEDDING_STORE_SCHEMA_VERSION;
use crate::project::ProjectRoot;

pub(super) fn open_existing_index_connection(project: &ProjectRoot) -> Result<Option<Connection>> {
    let db_path = project.as_path().join(".codelens/index/embeddings.db");
    if !db_path.exists() {
        return Ok(None);
    }

    crate::db::open_derived_sqlite_with_recovery(&db_path, "embedding index", || {
        super::super::super::ffi::register_sqlite_vec()?;
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "PRAGMA busy_timeout = 5000; PRAGMA mmap_size = 67108864; PRAGMA cache_size = -16000;",
        )?;
        conn.query_row("PRAGMA schema_version", [], |_row| Ok(()))?;
        Ok(conn)
    })
    .map(Some)
}

pub(super) fn valid_schema(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version' LIMIT 1",
        [],
        |row| row.get::<_, i64>(0),
    )
    .ok()
        == Some(EMBEDDING_STORE_SCHEMA_VERSION)
}

pub(super) fn meta_value(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = ?1 LIMIT 1",
        rusqlite::params![key],
        |row| row.get(0),
    )
    .ok()
}

pub(super) fn count_query(conn: &Connection, sql: &str) -> usize {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .map(|count| count.max(0) as usize)
        .unwrap_or(0)
}

pub(super) fn read_existing_embedding_keys(
    conn: &Connection,
) -> Result<HashMap<String, HashSet<ReusableEmbeddingKey>>> {
    let mut stmt = conn.prepare(
        "SELECT file_path, symbol_name, kind, signature, name_path, text
         FROM symbols
         ORDER BY file_path, id",
    )?;
    let mut rows = stmt.query([])?;
    let mut keys_by_file: HashMap<String, HashSet<ReusableEmbeddingKey>> = HashMap::new();

    while let Some(row) = rows.next()? {
        let file_path: String = row.get(0)?;
        let symbol_name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        let signature: String = row.get(3)?;
        let name_path: String = row.get(4)?;
        let text: String = row.get(5)?;
        keys_by_file
            .entry(file_path.clone())
            .or_default()
            .insert(reusable_embedding_key(
                &file_path,
                &symbol_name,
                &kind,
                &signature,
                &name_path,
                &text,
            ));
    }

    Ok(keys_by_file)
}
