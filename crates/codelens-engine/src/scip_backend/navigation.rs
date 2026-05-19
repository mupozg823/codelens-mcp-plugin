//! `PreciseBackend` trait impl: definitions, references, hover, diagnostics.
//!
//! Plus the private `resolve_scip_symbols` resolver shared by find_definitions
//! and find_references — it maps a user-facing short name + location to one or
//! more concrete SCIP symbol strings, with file-local matches preferred over
//! global ones.

use scip::types::{self as scip_types};

use super::ScipBackend;
use super::parse;
use crate::ir::{
    CodeDiagnostic, DiagnosticSeverity, IntelligenceSource, PreciseBackend, SearchCandidate,
};

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
                    if &occ.symbol == target && parse::is_definition(occ) {
                        let (line, _, occ_end_line, _) = parse::parse_range(occ);
                        let short = parse::short_name(&occ.symbol);
                        // Issue #245: previously `signature` was filled
                        // from `SymbolInformation.documentation.first()`,
                        // which is the rustdoc comment text — not a
                        // declaration line. The MCP layer then labelled
                        // doc-comment prose as
                        // `signature_source: scip_signature` and bypassed
                        // the source-line-read fallback added in #235-B.
                        // Documentation is surfaced separately via the
                        // MCP `documentation` field built from `hover()`.
                        results.push(SearchCandidate {
                            name: short.to_owned(),
                            kind: "symbol".to_owned(),
                            file_path: path.clone(),
                            line,
                            end_line: parse::body_end_line(doc, line, occ_end_line),
                            signature: String::new(),
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
                    if parse::is_definition(occ) && parse::short_name(&occ.symbol) == symbol {
                        let (line, _, occ_end_line, _) = parse::parse_range(occ);
                        results.push(SearchCandidate {
                            name: symbol.to_owned(),
                            kind: "symbol".to_owned(),
                            file_path: path.clone(),
                            line,
                            end_line: parse::body_end_line(doc, line, occ_end_line),
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
                        let (line, _, _, _) = parse::parse_range(occ);
                        let is_def = parse::is_definition(occ);
                        results.push(SearchCandidate {
                            name: parse::short_name(&occ.symbol).to_owned(),
                            kind: if is_def {
                                "definition".to_owned()
                            } else {
                                "reference".to_owned()
                            },
                            file_path: path.clone(),
                            line,
                            end_line: None,
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
                    if parse::short_name(&occ.symbol) == symbol {
                        let (line, _, _, _) = parse::parse_range(occ);
                        results.push(SearchCandidate {
                            name: symbol.to_owned(),
                            kind: "reference".to_owned(),
                            file_path: path.clone(),
                            line,
                            end_line: None,
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
            let (start_line, start_col, end_line, end_col) = parse::parse_range(occ);
            if line >= start_line && line <= end_line && column >= start_col && column < end_col {
                // Check override documentation first.
                if !occ.override_documentation.is_empty() {
                    return Ok(Some(occ.override_documentation.join("\n")));
                }
                // Then check global symbol info.
                if let Some(info) = self
                    .symbol_info
                    .get(&occ.symbol)
                    .filter(|info| !info.documentation.is_empty())
                {
                    return Ok(Some(info.documentation.join("\n")));
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
                let (line, col, _, _) = parse::parse_range(occ);
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
                let (occ_line, _, _, _) = parse::parse_range(occ);
                if occ_line == line && parse::short_name(&occ.symbol) == name {
                    return vec![occ.symbol.clone()];
                }
            }
            // 2. Same file, any line — if the name is rare enough.
            let mut candidates: Vec<String> = doc
                .occurrences
                .iter()
                .filter(|occ| parse::short_name(&occ.symbol) == name)
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
            .filter(|s| parse::short_name(s) == name)
            .cloned()
            .collect();
        global.dedup();
        global
    }
}
