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

use crate::call_graph::{is_noise_callee, CalleeEntry, CallerEntry};
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

    /// Find callees of `function_name` defined in `file_path` using SCIP occurrences.
    ///
    /// Strategy: SCIP does not directly model "function body extent", so we
    /// approximate it as the line range between the function's definition
    /// occurrence and the next definition occurrence in the same document.
    /// Reference (non-definition) occurrences whose line falls in that
    /// half-open range are treated as call sites originating from the
    /// queried function.
    ///
    /// For each call site we resolve the callee's definition file by
    /// scanning the index for a matching definition occurrence — providing
    /// the type-aware accuracy that tree-sitter's name-based heuristic
    /// cascade cannot achieve on patterns like Rust dispatch tables.
    ///
    /// Returns an empty Vec when the function cannot be located in the
    /// document (caller should fall back to tree-sitter).
    pub fn find_callees(&self, function_name: &str, file_path: &str) -> Vec<CalleeEntry> {
        let Some(doc) = self.documents.get(file_path) else {
            return Vec::new();
        };

        // Locate the queried function's definition occurrence.
        let mut sorted_defs: Vec<&scip_types::Occurrence> = doc
            .occurrences
            .iter()
            .filter(|occ| Self::is_definition(occ))
            .collect();
        sorted_defs.sort_by_key(|occ| Self::parse_range(occ).0);

        let Some(fn_def_idx) = sorted_defs
            .iter()
            .position(|occ| Self::short_name(&occ.symbol) == function_name)
        else {
            return Vec::new();
        };
        let fn_def = sorted_defs[fn_def_idx];
        let body_start = Self::parse_range(fn_def).0;
        let body_end = sorted_defs
            .get(fn_def_idx + 1)
            .map(|next_def| Self::parse_range(next_def).0)
            .unwrap_or(usize::MAX);

        // Build a global map from SCIP symbol → its definition (file, line)
        // for resolving callee locations on the fly. Keep it lazy and
        // bounded to symbols actually referenced in this body.
        let mut callees: Vec<CalleeEntry> = Vec::new();
        let mut seen: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();
        for occ in &doc.occurrences {
            if Self::is_definition(occ) {
                continue;
            }
            let (line, _, _, _) = Self::parse_range(occ);
            if line < body_start || line >= body_end {
                continue;
            }
            // Self-reference inside its own body (recursion) — skip; keeps
            // parity with tree-sitter behavior for the call_graph harness.
            if Self::short_name(&occ.symbol) == function_name {
                continue;
            }
            let name = Self::short_name(&occ.symbol).to_owned();
            if name.is_empty() || is_noise_callee(&name) {
                continue;
            }
            if !seen.insert((name.clone(), line)) {
                continue;
            }

            // Resolve callee's definition site by scanning the index.
            let resolved_file = self.documents.iter().find_map(|(other_path, other_doc)| {
                other_doc
                    .occurrences
                    .iter()
                    .find(|o| o.symbol == occ.symbol && Self::is_definition(o))
                    .map(|_| other_path.clone())
            });

            callees.push(CalleeEntry {
                name,
                line,
                resolved_file,
                confidence: 0.95,
                resolution: Some("scip"),
            });
        }
        callees
    }

    /// Find callers of `function_name` across the indexed corpus using
    /// reference occurrences plus a next-definition enclosing-scope walk.
    ///
    /// SCIP does not directly model "X calls Y"; it records occurrences
    /// (symbol, role, range) per document. To answer "who calls foo?":
    ///
    ///   1. Resolve foo to one or more SCIP symbols (function-like only,
    ///      filtered by `is_function_like_symbol`).
    ///   2. For every reference (non-definition) occurrence of those
    ///      symbols across all documents, locate the nearest preceding
    ///      function-like definition in the same document — that is the
    ///      enclosing caller. The caller's body extent is approximated by
    ///      "from this def to the next function-like def" (mirrors the
    ///      heuristic used in `find_callees`).
    ///
    /// Returns an empty Vec when no function-like symbol matches the
    /// requested name; callers fall through to tree-sitter cleanly.
    pub fn find_callers(&self, function_name: &str) -> Vec<CallerEntry> {
        // Step 1 — resolve target symbols (must be function-like; we don't
        // want to surface struct/type defs as "callees of themselves").
        let target_symbols: std::collections::HashSet<String> = self
            .symbol_info
            .keys()
            .filter(|s| Self::short_name(s) == function_name && Self::is_function_like_symbol(s))
            .cloned()
            .chain(self.documents.values().flat_map(|doc| {
                doc.occurrences
                    .iter()
                    .filter(|occ| Self::is_definition(occ))
                    .filter(|occ| {
                        Self::short_name(&occ.symbol) == function_name
                            && Self::is_function_like_symbol(&occ.symbol)
                    })
                    .map(|occ| occ.symbol.clone())
            }))
            .collect();
        if target_symbols.is_empty() {
            return Vec::new();
        }

        let mut callers: Vec<CallerEntry> = Vec::new();
        let mut seen: std::collections::HashSet<(String, String, usize)> =
            std::collections::HashSet::new();

        for (path, doc) in &self.documents {
            // Per-document sorted list of function-like definitions for
            // enclosing-scope lookup. Building this once amortizes the
            // O(occ * fn_def) work over each candidate occurrence.
            let mut fn_defs: Vec<(usize, &str)> = doc
                .occurrences
                .iter()
                .filter(|occ| {
                    Self::is_definition(occ) && Self::is_function_like_symbol(&occ.symbol)
                })
                .map(|occ| (Self::parse_range(occ).0, occ.symbol.as_str()))
                .collect();
            fn_defs.sort_by_key(|(line, _)| *line);

            for occ in &doc.occurrences {
                if Self::is_definition(occ) {
                    continue;
                }
                if !target_symbols.contains(&occ.symbol) {
                    continue;
                }
                let (line, _, _, _) = Self::parse_range(occ);

                // Find the nearest fn def at or before `line` whose body
                // extends past `line` (i.e. the next fn def is strictly
                // greater). Skip references that fall outside any
                // function body — they're top-level/static-init refs.
                let Some(enc_idx) = fn_defs.iter().rposition(|(def_line, _)| *def_line <= line)
                else {
                    continue;
                };
                let next_def_line = fn_defs
                    .get(enc_idx + 1)
                    .map(|(l, _)| *l)
                    .unwrap_or(usize::MAX);
                if line >= next_def_line {
                    continue;
                }

                let enc_symbol = fn_defs[enc_idx].1;
                let caller_name = Self::short_name(enc_symbol).to_owned();
                // Self-reference within own body (recursion) — skip to
                // mirror tree-sitter behavior so the call graph harness
                // treats the two backends consistently.
                if caller_name == function_name {
                    continue;
                }
                if !seen.insert((path.clone(), caller_name.clone(), line)) {
                    continue;
                }

                callers.push(CallerEntry {
                    file: path.clone(),
                    function: caller_name,
                    line,
                    confidence: 0.95,
                    resolution: Some("scip"),
                });
            }
        }
        callers
    }

    /// SCIP descriptor heuristic: function/method symbols include `()` in
    /// their descriptor string (e.g. `pkg/mod/foo().` or
    /// `pkg/mod/impl#[`Bar`]baz().`). Types end with `#`, fields with `.`,
    /// neither carries `()`. Cheaper and more portable than reading the
    /// optional SymbolInformation.kind field.
    fn is_function_like_symbol(scip_symbol: &str) -> bool {
        scip_symbol.contains("()")
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

    /// Build a SCIP fixture with two functions in router.rs:
    ///   line 10: `fn handle_request(...)` — definition
    ///   line 12: `dispatch_tool(...)` — reference inside handle_request body
    ///   line 14: `read_resource(...)` — reference inside handle_request body
    ///   line 25: `fn other_fn(...)` — next definition (closes handle_request body at 25)
    ///   line 27: `read_resource(...)` — reference in other_fn (must NOT be a callee)
    /// Plus dispatch_tool's def in dispatch/mod.rs:5 and read_resource's def in
    /// resources.rs:8 so callees can be resolved to their files.
    fn build_callees_fixture() -> Index {
        let mut idx = Index::new();

        let dispatch_tool = "scip-rust cargo codelens-mcp 1.9 dispatch/mod/dispatch_tool().";
        let read_resource = "scip-rust cargo codelens-mcp 1.9 resources/read_resource().";
        let handle_request = "scip-rust cargo codelens-mcp 1.9 server/router/handle_request().";
        let other_fn = "scip-rust cargo codelens-mcp 1.9 server/router/other_fn().";

        let mut router = scip_types::Document::new();
        router.relative_path = "crates/codelens-mcp/src/server/router.rs".to_owned();
        // handle_request def @ line 10
        let mut def = scip_types::Occurrence::new();
        def.range = vec![10, 4, 18];
        def.symbol = handle_request.to_owned();
        def.symbol_roles = 1;
        router.occurrences.push(def);
        // call to dispatch_tool @ line 12
        let mut c1 = scip_types::Occurrence::new();
        c1.range = vec![12, 8, 21];
        c1.symbol = dispatch_tool.to_owned();
        c1.symbol_roles = 0;
        router.occurrences.push(c1);
        // call to read_resource @ line 14
        let mut c2 = scip_types::Occurrence::new();
        c2.range = vec![14, 8, 21];
        c2.symbol = read_resource.to_owned();
        c2.symbol_roles = 0;
        router.occurrences.push(c2);
        // other_fn def @ line 25 (closes handle_request body)
        let mut def2 = scip_types::Occurrence::new();
        def2.range = vec![25, 4, 12];
        def2.symbol = other_fn.to_owned();
        def2.symbol_roles = 1;
        router.occurrences.push(def2);
        // call to read_resource @ line 27 — inside other_fn, NOT handle_request
        let mut c3 = scip_types::Occurrence::new();
        c3.range = vec![27, 8, 21];
        c3.symbol = read_resource.to_owned();
        c3.symbol_roles = 0;
        router.occurrences.push(c3);
        idx.documents.push(router);

        let mut dispatch_doc = scip_types::Document::new();
        dispatch_doc.relative_path = "crates/codelens-mcp/src/dispatch/mod.rs".to_owned();
        let mut d_def = scip_types::Occurrence::new();
        d_def.range = vec![5, 4, 17];
        d_def.symbol = dispatch_tool.to_owned();
        d_def.symbol_roles = 1;
        dispatch_doc.occurrences.push(d_def);
        idx.documents.push(dispatch_doc);

        let mut resources_doc = scip_types::Document::new();
        resources_doc.relative_path = "crates/codelens-mcp/src/resources.rs".to_owned();
        let mut r_def = scip_types::Occurrence::new();
        r_def.range = vec![8, 4, 17];
        r_def.symbol = read_resource.to_owned();
        r_def.symbol_roles = 1;
        resources_doc.occurrences.push(r_def);
        idx.documents.push(resources_doc);

        idx
    }

    #[test]
    fn find_callees_within_function_body_resolves_files() {
        // L1 acceptance — `find_callees(handle_request, router.rs)` must
        // surface the two callees inside the body (dispatch_tool,
        // read_resource) with correct resolved files, and must NOT return
        // the read_resource call that lives in the *next* function.
        let idx = build_callees_fixture();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        let callees =
            backend.find_callees("handle_request", "crates/codelens-mcp/src/server/router.rs");
        let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
        assert!(
            names.contains(&"dispatch_tool"),
            "dispatch_tool missing: {names:?}"
        );
        assert!(
            names.contains(&"read_resource"),
            "read_resource missing: {names:?}"
        );
        // Body extent must exclude the call in the next function.
        let read_lines: Vec<usize> = callees
            .iter()
            .filter(|c| c.name == "read_resource")
            .map(|c| c.line)
            .collect();
        assert_eq!(
            read_lines,
            vec![14],
            "read_resource at line 27 belongs to other_fn, not handle_request"
        );

        let dispatch = callees
            .iter()
            .find(|c| c.name == "dispatch_tool")
            .expect("dispatch_tool entry");
        assert_eq!(
            dispatch.resolved_file.as_deref(),
            Some("crates/codelens-mcp/src/dispatch/mod.rs"),
            "callee def file must be resolved via SCIP"
        );
        assert_eq!(dispatch.resolution, Some("scip"));
        assert!(dispatch.confidence >= 0.9);
    }

    #[test]
    fn find_callees_returns_empty_when_function_absent() {
        // Negative case: when the requested function has no def
        // occurrence in the given file, return empty so the MCP layer
        // cleanly falls through to tree-sitter without claiming a
        // false-positive resolution.
        let idx = build_callees_fixture();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        let callees = backend.find_callees(
            "no_such_function",
            "crates/codelens-mcp/src/server/router.rs",
        );
        assert!(callees.is_empty());

        let callees_wrong_file = backend.find_callees("handle_request", "src/unknown.rs");
        assert!(callees_wrong_file.is_empty());
    }

    #[test]
    fn find_callers_resolves_enclosing_function_via_next_def() {
        // L1 slice 2 acceptance — `find_callers(dispatch_tool)` must
        // attribute the call site at router.rs:12 to handle_request (the
        // function whose body contains line 12) and the call at line 27
        // to other_fn. Top-level references (outside any fn body) are
        // skipped. The fixture covers both happy paths and the
        // "outside-body" rejection case.
        let idx = build_callees_fixture();
        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        let callers = backend.find_callers("read_resource");
        // read_resource is referenced at router.rs:14 (in handle_request)
        // AND at router.rs:27 (in other_fn). Both are valid callers.
        let mut caller_pairs: Vec<(String, usize)> = callers
            .iter()
            .map(|c| (c.function.clone(), c.line))
            .collect();
        caller_pairs.sort();
        assert_eq!(
            caller_pairs,
            vec![
                ("handle_request".to_owned(), 14),
                ("other_fn".to_owned(), 27),
            ],
            "callers should attribute occurrences to their enclosing fn"
        );

        for c in &callers {
            assert_eq!(c.resolution, Some("scip"));
            assert!(c.confidence >= 0.9);
            assert_eq!(c.file, "crates/codelens-mcp/src/server/router.rs");
        }
    }

    #[test]
    fn find_callers_returns_empty_for_unknown_or_non_function() {
        // Negative cases:
        //   1. Unknown name → empty (caller falls through to tree-sitter).
        //   2. A symbol that exists but is not function-like (no `()` in
        //      its descriptor — a struct or field) must not be reported
        //      as having callers; otherwise `get_callers("MyStruct")`
        //      would return everywhere the type is mentioned, which is
        //      not the call-graph contract.
        let mut idx = build_callees_fixture();
        // Append a struct-like symbol "Config#" referenced from inside
        // handle_request body. find_callers("Config") must NOT pick it up.
        let struct_sym = "scip-rust cargo codelens-mcp 1.9 server/router/Config#".to_owned();
        let mut struct_def = scip_types::Occurrence::new();
        struct_def.range = vec![18, 4, 10];
        struct_def.symbol = struct_sym.clone();
        struct_def.symbol_roles = 1; // definition (struct)
        idx.documents[0].occurrences.push(struct_def);
        let mut struct_ref = scip_types::Occurrence::new();
        struct_ref.range = vec![13, 8, 14];
        struct_ref.symbol = struct_sym.clone();
        struct_ref.symbol_roles = 0;
        idx.documents[0].occurrences.push(struct_ref);

        let file = write_index_to_file(&idx);
        let backend = ScipBackend::load(file.path()).unwrap();

        assert!(backend.find_callers("no_such_function").is_empty());
        assert!(
            backend.find_callers("Config").is_empty(),
            "non-function symbols must be filtered by is_function_like_symbol"
        );
    }
}
