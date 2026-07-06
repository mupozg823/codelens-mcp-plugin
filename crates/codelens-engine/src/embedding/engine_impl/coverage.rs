#[path = "coverage_existing_index.rs"]
mod existing_index;

use anyhow::Result;
use std::collections::{HashMap, HashSet};

use super::super::cache::{
    ReusableEmbeddingKey, reusable_embedding_key_for_chunk, reusable_embedding_key_for_symbol,
};
use super::super::prompt::{build_embedding_text, is_test_only_symbol};
use super::super::{
    EmbeddingCoverageReport, EmbeddingEngine, EmbeddingIndexInfo, EmbeddingStaleFileReason,
    EmbeddingStaleReason,
};
use super::git_sha::current_git_sha;
use crate::db::IndexDb;
use crate::project::ProjectRoot;
use existing_index::{
    count_query, meta_value, open_existing_index_connection, read_existing_embedding_keys,
    valid_schema,
};

const STALE_FILE_REASON_LIMIT: usize = 20;

impl EmbeddingEngine {
    pub fn coverage_report(&self, project: &ProjectRoot) -> Result<EmbeddingCoverageReport> {
        let mut existing_embeddings: HashMap<String, HashSet<ReusableEmbeddingKey>> =
            HashMap::new();
        self.store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                existing_embeddings.insert(
                    file_path,
                    chunks
                        .into_iter()
                        .map(|chunk| reusable_embedding_key_for_chunk(&chunk))
                        .collect(),
                );
                Ok(())
            })?;

        coverage_report_from_keys(
            project,
            self.model_name.clone(),
            self.store.count().unwrap_or(0),
            self.store.file_count().unwrap_or(0),
            self.store.meta_value("last_index_sha").ok().flatten(),
            existing_embeddings,
        )
    }

    pub fn index_info(&self) -> EmbeddingIndexInfo {
        EmbeddingIndexInfo {
            model_name: self.model_name.clone(),
            indexed_symbols: self.store.count().unwrap_or(0),
            indexed_files: self.store.file_count().unwrap_or(0),
            query_cache_entries: self.store.query_cache_count().unwrap_or(0),
            last_index_sha: self.store.meta_value("last_index_sha").ok().flatten(),
        }
    }

    pub fn inspect_existing_index(project: &ProjectRoot) -> Result<Option<EmbeddingIndexInfo>> {
        let Some(conn) = open_existing_index_connection(project)? else {
            return Ok(None);
        };
        if !valid_schema(&conn) {
            return Ok(None);
        }
        let Some(model_name) = meta_value(&conn, "model") else {
            return Ok(None);
        };

        Ok(Some(EmbeddingIndexInfo {
            model_name,
            indexed_symbols: count_query(&conn, "SELECT COUNT(*) FROM symbols"),
            indexed_files: count_query(&conn, "SELECT COUNT(DISTINCT file_path) FROM symbols"),
            query_cache_entries: count_query(&conn, "SELECT COUNT(*) FROM query_embeddings"),
            last_index_sha: meta_value(&conn, "last_index_sha"),
        }))
    }

    pub fn inspect_existing_coverage(
        project: &ProjectRoot,
    ) -> Result<Option<EmbeddingCoverageReport>> {
        let Some(conn) = open_existing_index_connection(project)? else {
            return Ok(None);
        };
        if !valid_schema(&conn) {
            return Ok(None);
        }
        let Some(model_name) = meta_value(&conn, "model") else {
            return Ok(None);
        };
        let indexed_symbols = count_query(&conn, "SELECT COUNT(*) FROM symbols");
        let indexed_files = count_query(&conn, "SELECT COUNT(DISTINCT file_path) FROM symbols");
        let last_index_sha = meta_value(&conn, "last_index_sha");
        let existing_embeddings = read_existing_embedding_keys(&conn)?;

        coverage_report_from_keys(
            project,
            model_name,
            indexed_symbols,
            indexed_files,
            last_index_sha,
            existing_embeddings,
        )
        .map(Some)
    }
}

fn coverage_report_from_keys(
    project: &ProjectRoot,
    model_name: String,
    indexed_symbols: usize,
    indexed_files: usize,
    last_index_sha: Option<String>,
    mut existing_embeddings: HashMap<String, HashSet<ReusableEmbeddingKey>>,
) -> Result<EmbeddingCoverageReport> {
    let db_path = crate::db::index_db_path(project.as_path());
    let symbol_db = IndexDb::open(&db_path)?;
    let mut report = EmbeddingCoverageReport {
        model_name,
        indexed_symbols,
        indexed_files,
        current_git_sha: current_git_sha(project),
        last_index_sha,
        ..EmbeddingCoverageReport::default()
    };

    symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
        report.checked_files += 1;
        let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
        let relevant_symbols: Vec<_> = symbols
            .into_iter()
            .filter(|sym| !is_test_only_symbol(sym, source.as_deref()))
            .collect();
        let Some(existing_for_file) = existing_embeddings.remove(&file_path) else {
            if !relevant_symbols.is_empty() {
                report.missing_files += 1;
                report.skipped_new_files += 1;
                record_stale_file(
                    &mut report,
                    &file_path,
                    EmbeddingStaleReason::MissingEmbeddings,
                );
            } else {
                report.ready_files += 1;
            }
            return Ok(());
        };
        if relevant_symbols.is_empty() {
            if !existing_for_file.is_empty() {
                report.stale_files += 1;
                record_stale_file(
                    &mut report,
                    &file_path,
                    EmbeddingStaleReason::OrphanedEmbeddings,
                );
            } else {
                report.ready_files += 1;
            }
            return Ok(());
        }

        let current_keys = relevant_symbols
            .iter()
            .map(|sym| {
                let text = build_embedding_text(sym, source.as_deref());
                reusable_embedding_key_for_symbol(sym, &text)
            })
            .collect::<HashSet<_>>();
        if current_keys == existing_for_file {
            report.unchanged_files += 1;
            report.ready_files += 1;
        } else {
            report.stale_files += 1;
            record_stale_file(
                &mut report,
                &file_path,
                EmbeddingStaleReason::EmbeddingKeysChanged,
            );
        }
        Ok(())
    })?;
    report.extra_files = existing_embeddings.len();
    let mut extra_file_paths = existing_embeddings.into_keys().collect::<Vec<_>>();
    extra_file_paths.sort();
    for file_path in extra_file_paths {
        record_stale_file(
            &mut report,
            &file_path,
            EmbeddingStaleReason::OrphanedEmbeddings,
        );
    }
    report.readiness_percent = readiness_percent(report.ready_files, report.checked_files);
    Ok(report)
}

fn record_stale_file(
    report: &mut EmbeddingCoverageReport,
    file_path: &str,
    reason: EmbeddingStaleReason,
) {
    if report.stale_file_reasons.len() < STALE_FILE_REASON_LIMIT {
        report.stale_file_reasons.push(EmbeddingStaleFileReason {
            file_path: file_path.to_owned(),
            reason,
        });
    } else {
        report.stale_file_reasons_omitted += 1;
    }
}

fn readiness_percent(ready_files: usize, checked_files: usize) -> u8 {
    if checked_files == 0 {
        return 0;
    }
    let percent = ready_files.saturating_mul(100) / checked_files;
    u8::try_from(percent.min(100)).unwrap_or(100)
}
