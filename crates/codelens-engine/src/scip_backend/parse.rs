//! SCIP occurrence/symbol parsing helpers.
//!
//! Pure functions over `scip_types::Occurrence` and SCIP symbol strings —
//! no `ScipBackend` state. Lifted out of `mod.rs` so the navigation and
//! call-graph sub-modules can call them without the whole inherent-impl
//! soup.

use scip::types::{self as scip_types};

/// Extract the short symbol name from a SCIP symbol string.
///
/// SCIP symbols look like `scip-rust cargo codelens-engine 1.9.21 src/ir.rs/PreciseBackend#`.
/// We extract the last descriptor component (e.g., `PreciseBackend`).
pub(super) fn short_name(scip_symbol: &str) -> &str {
    // The symbol string ends with a descriptor suffix like `SymbolName#` or `SymbolName.`
    // Strip trailing punctuation and get the last path component.
    let trimmed = scip_symbol.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_');
    trimmed.rsplit(['/', '.', '#']).next().unwrap_or(trimmed)
}

/// Check if an occurrence's symbol_roles includes the Definition role.
pub(super) fn is_definition(occ: &scip_types::Occurrence) -> bool {
    // SymbolRole::Definition has value 1 in the SCIP spec.
    occ.symbol_roles & 1 != 0
}

/// Parse occurrence range into (start_line, start_col, end_line, end_col).
pub(super) fn parse_range(occ: &scip_types::Occurrence) -> (usize, usize, usize, usize) {
    let r = &occ.range;
    match r.len() {
        3 => (r[0] as usize, r[1] as usize, r[0] as usize, r[2] as usize),
        4 => (r[0] as usize, r[1] as usize, r[2] as usize, r[3] as usize),
        _ => (0, 0, 0, 0),
    }
}

/// SCIP descriptor heuristic: function/method symbols include `()` in
/// their descriptor string (e.g. `pkg/mod/foo().` or
/// `pkg/mod/impl#[`Bar`]baz().`). Types end with `#`, fields with `.`,
/// neither carries `()`. Cheaper and more portable than reading the
/// optional SymbolInformation.kind field.
pub(super) fn is_function_like_symbol(scip_symbol: &str) -> bool {
    scip_symbol.contains("()")
}

/// Estimate the inclusive last line of a definition's body.
///
/// SCIP records occurrence ranges that cover the symbol identifier
/// (e.g. the function name), not the body. Most indexers emit a
/// single-line range there, so we use the same enclosing-scope
/// heuristic as `find_callees`: the body extends until just before
/// the next definition occurrence in the same document. If no
/// following definition exists, returns `None` — the MCP layer falls
/// back to its 50-line slice. If the occurrence itself already spans
/// multiple lines (rare; rust-analyzer's macro-expanded items can do
/// this), we honor that span when it exceeds the next-def estimate.
pub(super) fn body_end_line(
    doc: &scip_types::Document,
    start_line: usize,
    occ_end_line: usize,
) -> Option<usize> {
    let mut sibling_starts: Vec<usize> = doc
        .occurrences
        .iter()
        .filter(|occ| is_definition(occ))
        .map(|occ| parse_range(occ).0)
        .filter(|line| *line > start_line)
        .collect();
    sibling_starts.sort_unstable();
    let next_def = sibling_starts.first().copied();
    let sibling_estimate = next_def.map(|n| n.saturating_sub(1));
    match (sibling_estimate, occ_end_line) {
        (Some(est), occ) if occ > est => Some(occ),
        (Some(est), _) if est > start_line => Some(est),
        (None, occ) if occ > start_line => Some(occ),
        _ => None,
    }
}
