use anyhow::Result;
use std::collections::HashSet;

use super::super::EmbeddingEngine;
use super::super::prompt::{extract_leading_doc, is_test_only_symbol, split_identifier};
use crate::db::IndexDb;
use crate::project::ProjectRoot;

impl EmbeddingEngine {
    pub fn generate_bridge_candidates(
        &self,
        project: &ProjectRoot,
    ) -> Result<Vec<(String, String)>> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let mut bridges: Vec<(String, String)> = Vec::new();
        let mut seen_nl = HashSet::new();

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
            for sym in &symbols {
                if is_test_only_symbol(sym, source.as_deref()) {
                    continue;
                }
                let doc = source.as_deref().and_then(|src| {
                    extract_leading_doc(src, sym.start_byte as usize, sym.end_byte as usize)
                });
                let doc = match doc {
                    Some(d) if !d.is_empty() => d,
                    _ => continue,
                };

                let split = split_identifier(&sym.name);
                let code_term = if split != sym.name {
                    format!("{} {}", sym.name, split)
                } else {
                    sym.name.clone()
                };

                let first_line = doc.lines().next().unwrap_or("").trim().to_lowercase();
                let clean = first_line.trim_end_matches(|c: char| c.is_ascii_punctuation());
                let words: Vec<&str> = clean.split_whitespace().collect();
                if words.len() < 2 {
                    continue;
                }

                for window in 2..=words.len().min(4) {
                    let key = words[..window].join(" ");
                    if key.len() < 5 || key.len() > 60 {
                        continue;
                    }
                    if seen_nl.insert(key.clone()) {
                        bridges.push((key, code_term.clone()));
                    }
                }

                if split != sym.name && !seen_nl.contains(&split.to_lowercase()) {
                    let lowered = split.to_lowercase();
                    if lowered.split_whitespace().count() >= 2 && seen_nl.insert(lowered.clone()) {
                        bridges.push((lowered, code_term.clone()));
                    }
                }
            }
            Ok(())
        })?;

        Ok(bridges)
    }
}
