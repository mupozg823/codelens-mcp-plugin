//! SCIP (Source Code Intelligence Protocol) backend providing precise navigation.
//!
//! Loads a SCIP index file (`index.scip`) and provides type-aware definitions,
//! references, hover docs, and diagnostics. This is the "precise path" that
//! complements tree-sitter's "fast path".
//!
//! Gated behind the `scip-backend` feature.
//!
//! Split layout (P2-2, 2026-05-19):
//! - `mod.rs` — `ScipBackend` struct + `load` / `detect` / `*_count`.
//! - `parse.rs` — occurrence + symbol parse helpers (no state).
//! - `call_graph.rs` — `find_callees` / `find_callers` enclosing-scope walks.
//! - `navigation.rs` — precise navigation methods + `resolve_scip_symbols`.
//! - `tests.rs` — fixture builders + 12 unit tests (gated behind `cfg(test)`).

mod call_graph;
mod navigation;
mod parse;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use protobuf::Message;
use scip::types::{self as scip_types, Index};

/// A SCIP-backed precise navigation provider.
///
/// Holds the parsed SCIP index in memory and provides O(1) file lookups
/// and O(n_occurrences) symbol searches within a document.
pub struct ScipBackend {
    /// Map from relative file path → parsed Document.
    pub(super) documents: HashMap<String, scip_types::Document>,
    /// Global symbol table: symbol string → SymbolInformation (docs, relationships).
    pub(super) symbol_info: HashMap<String, scip_types::SymbolInformation>,
}

impl ScipBackend {
    /// Load a SCIP index from the given path.
    ///
    /// Typical locations: `index.scip`, `.scip/index.scip`, or a custom path.
    pub fn load(index_path: &Path) -> anyhow::Result<Self> {
        let bytes = fs::read(index_path)?;
        let index = Index::parse_from_bytes(&bytes)?;

        let mut symbol_info = HashMap::new();
        for info in index.external_symbols {
            if !info.symbol.is_empty() {
                symbol_info.insert(info.symbol.clone(), info);
            }
        }

        let mut documents = HashMap::new();
        for doc in index.documents {
            // Collect per-document symbol info into the global table.
            for info in &doc.symbols {
                if !info.symbol.is_empty() && !symbol_info.contains_key(&info.symbol) {
                    symbol_info.insert(info.symbol.clone(), info.clone());
                }
            }
            let path = doc.relative_path.clone();
            if !path.is_empty() {
                documents.insert(path, doc);
            }
        }

        Ok(Self {
            documents,
            symbol_info,
        })
    }

    /// Try to auto-detect a SCIP index file in the project root.
    ///
    /// Checks (in order): `index.scip`, `.scip/index.scip`, `.codelens/index.scip`.
    pub fn detect(project_root: &Path) -> Option<std::path::PathBuf> {
        let candidates = [
            project_root.join("index.scip"),
            project_root.join(".scip").join("index.scip"),
            project_root.join(".codelens").join("index.scip"),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Number of indexed files.
    pub fn file_count(&self) -> usize {
        self.documents.len()
    }

    /// Number of known symbols.
    pub fn symbol_count(&self) -> usize {
        self.symbol_info.len()
    }
}
