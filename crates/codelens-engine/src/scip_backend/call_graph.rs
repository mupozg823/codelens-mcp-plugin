//! Enclosing-scope call graph queries over a loaded SCIP index.
//!
//! `find_callees` and `find_callers` both approximate function body extent
//! as "from this def to the next def in the same document" — SCIP records
//! identifier occurrences, not body ranges, so we lean on the per-document
//! definition ordering. See the doc-comments on each function for the
//! specific filtering rules each direction needs.

use scip::types::{self as scip_types};

use super::ScipBackend;
use super::parse;
use crate::call_graph::{CalleeEntry, CallerEntry, is_noise_callee};

impl ScipBackend {
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
            .filter(|occ| parse::is_definition(occ))
            .collect();
        sorted_defs.sort_by_key(|occ| parse::parse_range(occ).0);

        let Some(fn_def_idx) = sorted_defs
            .iter()
            .position(|occ| parse::short_name(&occ.symbol) == function_name)
        else {
            return Vec::new();
        };
        let fn_def = sorted_defs[fn_def_idx];
        let body_start = parse::parse_range(fn_def).0;
        let body_end = sorted_defs
            .get(fn_def_idx + 1)
            .map(|next_def| parse::parse_range(next_def).0)
            .unwrap_or(usize::MAX);

        // Build a global map from SCIP symbol → its definition (file, line)
        // for resolving callee locations on the fly. Keep it lazy and
        // bounded to symbols actually referenced in this body.
        let mut callees: Vec<CalleeEntry> = Vec::new();
        let mut seen: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();
        for occ in &doc.occurrences {
            if parse::is_definition(occ) {
                continue;
            }
            let (line, _, _, _) = parse::parse_range(occ);
            if line < body_start || line >= body_end {
                continue;
            }
            // Self-reference inside its own body (recursion) — skip; keeps
            // parity with tree-sitter behavior for the call_graph harness.
            if parse::short_name(&occ.symbol) == function_name {
                continue;
            }
            let name = parse::short_name(&occ.symbol).to_owned();
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
                    .find(|o| o.symbol == occ.symbol && parse::is_definition(o))
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
            .filter(|s| parse::short_name(s) == function_name && parse::is_function_like_symbol(s))
            .cloned()
            .chain(self.documents.values().flat_map(|doc| {
                doc.occurrences
                    .iter()
                    .filter(|occ| parse::is_definition(occ))
                    .filter(|occ| {
                        parse::short_name(&occ.symbol) == function_name
                            && parse::is_function_like_symbol(&occ.symbol)
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
                    parse::is_definition(occ) && parse::is_function_like_symbol(&occ.symbol)
                })
                .map(|occ| (parse::parse_range(occ).0, occ.symbol.as_str()))
                .collect();
            fn_defs.sort_by_key(|(line, _)| *line);

            for occ in &doc.occurrences {
                if parse::is_definition(occ) {
                    continue;
                }
                if !target_symbols.contains(&occ.symbol) {
                    continue;
                }
                let (line, _, _, _) = parse::parse_range(occ);

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
                let caller_name = parse::short_name(enc_symbol).to_owned();
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
}
