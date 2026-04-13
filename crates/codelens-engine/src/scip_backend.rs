//! SCIP (Source Code Intelligence Protocol) backend implementing [`PreciseBackend`].
//!
//! Loads a SCIP index file (`index.scip`) and provides type-aware definitions,
//! references, hover docs, and diagnostics. This is the "precise path" that
//! complements tree-sitter's "fast path".
//!
//! Gated behind the `scip-backend` feature.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use protobuf::Message;
use scip::types::{self as scip_types, Index};

use crate::ir::{
    CodeDiagnostic, DiagnosticSeverity, IntelligenceSource, PreciseBackend, SearchCandidate,
};

/// A SCIP-backed precise navigation provider.
///
/// Holds the parsed SCIP index in memory and provides O(1) file lookups
/// and O(n_occurrences) symbol searches within a document.
pub struct ScipBackend {
    /// Map from relative file path → parsed Document.
    documents: HashMap<String, scip_types::Document>,
    /// Global symbol table: symbol string → SymbolInformation (docs, relationships).
    symbol_info: HashMap<String, scip_types::SymbolInformation>,
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

    /// Extract the short symbol name from a SCIP symbol string.
    ///
    /// SCIP symbols look like `scip-rust cargo codelens-engine 1.9.21 src/ir.rs/PreciseBackend#`.
    /// We extract the last descriptor component (e.g., `PreciseBackend`).
    fn short_name(scip_symbol: &str) -> &str {
        // The symbol string ends with a descriptor suffix like `SymbolName#` or `SymbolName.`
        // Strip trailing punctuation and get the last path component.
        let trimmed = scip_symbol.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
        trimmed
            .rsplit(|c: char| c == '/' || c == '.' || c == '#')
            .next()
            .unwrap_or(trimmed)
    }

    /// Check if an occurrence's symbol_roles includes the Definition role.
    fn is_definition(occ: &scip_types::Occurrence) -> bool {
        // SymbolRole::Definition has value 1 in the SCIP spec.
        occ.symbol_roles & 1 != 0
    }

    /// Parse occurrence range into (start_line, start_col, end_line, end_col).
    fn parse_range(occ: &scip_types::Occurrence) -> (usize, usize, usize, usize) {
        let r = &occ.range;
        match r.len() {
            3 => (r[0] as usize, r[1] as usize, r[0] as usize, r[2] as usize),
            4 => (r[0] as usize, r[1] as usize, r[2] as usize, r[3] as usize),
            _ => (0, 0, 0, 0),
        }
    }
}

impl PreciseBackend for ScipBackend {
    fn find_definitions(
        &self,
        symbol: &str,
        _file_path: &str,
        _line: usize,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let mut results = Vec::new();

        // First try to find the symbol at the given location to get the SCIP symbol string.
        // Then search all documents for definition occurrences of that symbol.
        let target_symbols = self.resolve_scip_symbols(symbol, _file_path, _line);

        for target in &target_symbols {
            for (path, doc) in &self.documents {
                for occ in &doc.occurrences {
                    if &occ.symbol == target && Self::is_definition(occ) {
                        let (line, _, _, _) = Self::parse_range(occ);
                        let short = Self::short_name(&occ.symbol);
                        let sig = self
                            .symbol_info
                            .get(&occ.symbol)
                            .and_then(|info| info.documentation.first())
                            .cloned()
                            .unwrap_or_default();

                        results.push(SearchCandidate {
                            name: short.to_owned(),
                            kind: "symbol".to_owned(),
                            file_path: path.clone(),
                            line,
                            signature: sig,
                            name_path: Some(occ.symbol.clone()),
                            body: None,
                            score: 1.0,
                            source: IntelligenceSource::Scip,
                        });
                    }
                }
            }
        }

        // Fallback: if no SCIP symbol resolved, do name-based search across all documents.
        if results.is_empty() {
            for (path, doc) in &self.documents {
                for occ in &doc.occurrences {
                    if Self::is_definition(occ) && Self::short_name(&occ.symbol) == symbol {
                        let (line, _, _, _) = Self::parse_range(occ);
                        results.push(SearchCandidate {
                            name: symbol.to_owned(),
                            kind: "symbol".to_owned(),
                            file_path: path.clone(),
                            line,
                            signature: String::new(),
                            name_path: Some(occ.symbol.clone()),
                            body: None,
                            score: 0.9,
                            source: IntelligenceSource::Scip,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    fn find_references(
        &self,
        symbol: &str,
        _file_path: &str,
        _line: usize,
    ) -> anyhow::Result<Vec<SearchCandidate>> {
        let mut results = Vec::new();
        let target_symbols = self.resolve_scip_symbols(symbol, _file_path, _line);

        for target in &target_symbols {
            for (path, doc) in &self.documents {
                for occ in &doc.occurrences {
                    if &occ.symbol == target {
                        let (line, _, _, _) = Self::parse_range(occ);
                        let is_def = Self::is_definition(occ);
                        results.push(SearchCandidate {
                            name: Self::short_name(&occ.symbol).to_owned(),
                            kind: if is_def {
                                "definition".to_owned()
                            } else {
                                "reference".to_owned()
                            },
                            file_path: path.clone(),
                            line,
                            signature: String::new(),
                            name_path: Some(occ.symbol.clone()),
                            body: None,
                            score: if is_def { 1.0 } else { 0.8 },
                            source: IntelligenceSource::Scip,
                        });
                    }
                }
            }
        }

        // Fallback: name-based search
        if results.is_empty() {
            for (path, doc) in &self.documents {
                for occ in &doc.occurrences {
                    if Self::short_name(&occ.symbol) == symbol {
                        let (line, _, _, _) = Self::parse_range(occ);
                        results.push(SearchCandidate {
                            name: symbol.to_owned(),
                            kind: "reference".to_owned(),
                            file_path: path.clone(),
                            line,
                            signature: String::new(),
                            name_path: Some(occ.symbol.clone()),
                            body: None,
                            score: 0.7,
                            source: IntelligenceSource::Scip,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    fn hover(&self, file_path: &str, line: usize, column: usize) -> anyhow::Result<Option<String>> {
        let Some(doc) = self.documents.get(file_path) else {
            return Ok(None);
        };

        for occ in &doc.occurrences {
            let (start_line, start_col, end_line, end_col) = Self::parse_range(occ);
            if line >= start_line && line <= end_line && column >= start_col && column < end_col {
                // Check override documentation first.
                if !occ.override_documentation.is_empty() {
                    return Ok(Some(occ.override_documentation.join("\n")));
                }
                // Then check global symbol info.
                if let Some(info) = self.symbol_info.get(&occ.symbol) {
                    if !info.documentation.is_empty() {
                        return Ok(Some(info.documentation.join("\n")));
                    }
                }
                // Return symbol name as fallback.
                return Ok(Some(occ.symbol.clone()));
            }
        }

        Ok(None)
    }

    fn diagnostics(&self, file_path: &str) -> anyhow::Result<Vec<CodeDiagnostic>> {
        let Some(doc) = self.documents.get(file_path) else {
            return Ok(Vec::new());
        };

        let mut diags = Vec::new();
        for occ in &doc.occurrences {
            for d in &occ.diagnostics {
                let severity = match d.severity.enum_value() {
                    Ok(scip_types::Severity::Error) => DiagnosticSeverity::Error,
                    Ok(scip_types::Severity::Warning) => DiagnosticSeverity::Warning,
                    Ok(scip_types::Severity::Information) => DiagnosticSeverity::Info,
                    Ok(scip_types::Severity::Hint) => DiagnosticSeverity::Hint,
                    _ => DiagnosticSeverity::Warning,
                };
                let (line, col, _, _) = Self::parse_range(occ);
                diags.push(CodeDiagnostic {
                    file_path: file_path.to_owned(),
                    line,
                    column: col,
                    severity,
                    message: d.message.clone(),
                    source: IntelligenceSource::Scip,
                    code: if d.code.is_empty() {
                        None
                    } else {
                        Some(d.code.clone())
                    },
                });
            }
        }

        Ok(diags)
    }

    fn source(&self) -> IntelligenceSource {
        IntelligenceSource::Scip
    }

    fn has_index_for(&self, file_path: &str) -> bool {
        self.documents.contains_key(file_path)
    }
}

impl ScipBackend {
    /// Resolve a user-facing symbol name + location to SCIP symbol strings.
    ///
    /// Strategy: if the file has an occurrence at the given line whose short name
    /// matches, use its full SCIP symbol. Otherwise, collect all SCIP symbols
    /// whose short name matches anywhere in the index.
    fn resolve_scip_symbols(&self, name: &str, file_path: &str, line: usize) -> Vec<String> {
        // 1. Try exact location match first.
        if let Some(doc) = self.documents.get(file_path) {
            for occ in &doc.occurrences {
                let (occ_line, _, _, _) = Self::parse_range(occ);
                if occ_line == line && Self::short_name(&occ.symbol) == name {
                    return vec![occ.symbol.clone()];
                }
            }
            // 2. Same file, any line — if the name is rare enough.
            let mut candidates: Vec<String> = doc
                .occurrences
                .iter()
                .filter(|occ| Self::short_name(&occ.symbol) == name)
                .map(|occ| occ.symbol.clone())
                .collect();
            candidates.dedup();
            if candidates.len() == 1 {
                return candidates;
            }
        }

        // 3. Global: collect all unique SCIP symbols with matching short name.
        let mut global: Vec<String> = self
            .symbol_info
            .keys()
            .filter(|s| Self::short_name(s) == name)
            .cloned()
            .collect();
        global.dedup();
        global
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Build a minimal SCIP index in memory for testing.
    fn build_test_index() -> Index {
        let mut idx = Index::new();

        let mut doc = scip_types::Document::new();
        doc.relative_path = "src/main.rs".to_owned();

        // Definition occurrence
        let mut def_occ = scip_types::Occurrence::new();
        def_occ.range = vec![10, 4, 18]; // line 10, col 4..18
        def_occ.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
        def_occ.symbol_roles = 1; // Definition
        doc.occurrences.push(def_occ);

        // Reference occurrence
        let mut ref_occ = scip_types::Occurrence::new();
        ref_occ.range = vec![20, 8, 22]; // line 20, col 8..22
        ref_occ.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
        ref_occ.symbol_roles = 0; // Reference (not definition)
        doc.occurrences.push(ref_occ);

        // Symbol info
        let mut info = scip_types::SymbolInformation::new();
        info.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
        info.documentation = vec!["A test struct for unit testing.".to_owned()];
        doc.symbols.push(info);

        // Second file with a reference
        let mut doc2 = scip_types::Document::new();
        doc2.relative_path = "src/lib.rs".to_owned();

        let mut ref_occ2 = scip_types::Occurrence::new();
        ref_occ2.range = vec![5, 0, 8];
        ref_occ2.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
        ref_occ2.symbol_roles = 0;
        doc2.occurrences.push(ref_occ2);

        idx.documents.push(doc);
        idx.documents.push(doc2);
        idx
    }

    fn write_index_to_file(idx: &Index) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        let bytes = idx.write_to_bytes().unwrap();
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_load_and_file_count() {
        let idx = build_test_index();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();
        assert_eq!(backend.file_count(), 2);
        assert!(backend.symbol_count() >= 1);
    }

    #[test]
    fn test_has_index_for() {
        let idx = build_test_index();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();
        assert!(backend.has_index_for("src/main.rs"));
        assert!(backend.has_index_for("src/lib.rs"));
        assert!(!backend.has_index_for("src/unknown.rs"));
    }

    #[test]
    fn test_find_definitions() {
        let idx = build_test_index();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        let defs = backend
            .find_definitions("MyStruct", "src/main.rs", 10)
            .unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "MyStruct");
        assert_eq!(defs[0].file_path, "src/main.rs");
        assert_eq!(defs[0].line, 10);
        assert!(matches!(defs[0].source, IntelligenceSource::Scip));
    }

    #[test]
    fn test_find_references_cross_file() {
        let idx = build_test_index();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        let refs = backend
            .find_references("MyStruct", "src/main.rs", 10)
            .unwrap();
        // Should find: 1 def in main.rs + 1 ref in main.rs + 1 ref in lib.rs = 3
        assert_eq!(refs.len(), 3);
        let files: Vec<&str> = refs.iter().map(|r| r.file_path.as_str()).collect();
        assert!(files.contains(&"src/main.rs"));
        assert!(files.contains(&"src/lib.rs"));
    }

    #[test]
    fn test_hover() {
        let idx = build_test_index();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        let hover = backend.hover("src/main.rs", 10, 5).unwrap();
        assert!(hover.is_some());
        assert!(hover.unwrap().contains("test struct"));
    }

    #[test]
    fn test_short_name() {
        assert_eq!(
            ScipBackend::short_name("scip-rust cargo pkg 0.1.0 src/main.rs/MyStruct#"),
            "MyStruct"
        );
        assert_eq!(
            ScipBackend::short_name("scip-go gomod example.com/pkg src/handler.go/HandleRequest."),
            "HandleRequest"
        );
        assert_eq!(ScipBackend::short_name("simple_name"), "simple_name");
    }

    #[test]
    fn test_source() {
        let idx = build_test_index();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();
        assert!(matches!(backend.source(), IntelligenceSource::Scip));
    }
}
