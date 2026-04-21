use codelens_engine::SymbolInfo;

pub fn flatten_symbols(symbols: &[SymbolInfo]) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    let mut stack = symbols.to_vec();
    while let Some(mut symbol) = stack.pop() {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        stack.extend(children);
    }
    flat
}

/// Follow-up tool hints for a BM25 symbol card.
///
/// Mirrors the `bm25-sparse-lane-spec` matrix. Frontier-model harnesses
/// select their next tool off this list, so the output is part of the
/// response contract. Keep it short (1-3 entries) — the goal is
/// guidance, not an exhaustive menu.
pub(super) fn suggested_follow_up(kind: &str, exported: bool) -> Vec<&'static str> {
    let base: Vec<&'static str> = match kind {
        "function" | "method" => vec!["find_symbol", "get_file_diagnostics"],
        "class" | "interface" | "enum" | "type_alias" => {
            vec!["find_symbol", "find_referencing_symbols"]
        }
        "module" | "file" => vec!["get_symbols_overview", "find_referencing_symbols"],
        "variable" | "property" => vec!["find_symbol", "find_referencing_symbols"],
        _ => vec!["find_symbol"],
    };
    if exported
        && matches!(kind, "function" | "method" | "class" | "interface")
        && !base.contains(&"find_referencing_symbols")
    {
        let mut with_refs = base.clone();
        with_refs.push("find_referencing_symbols");
        return with_refs;
    }
    base
}

/// Cross-field confidence tier for a BM25 symbol card.
///
/// Without a separate dense arm, we cannot yet compute a true
/// BM25-vs-dense agreement signal. This heuristic is the *cross-field*
/// proxy: a result that matches query terms on the high-weight
/// identifier fields (`name`, `name_path`) **and** covers most of the
/// unique query terms is a high-confidence hit; a result that matches
/// only on low-weight fields (body lexical chunk, doc comment) is low.
///
/// - `high`   — ≥80% query-term coverage AND a hit on name or name_path
/// - `medium` — 2+ matched terms OR a name/name_path hit
/// - `low`    — single term hit, or matches only on body/doc fields
///
/// Frontier-model callers use this to decide whether to trust the card
/// for direct consumption or to cross-check via `find_symbol` +
/// `find_referencing_symbols` before acting.
pub(super) fn confidence_tier(
    matched_terms: &[String],
    unique_query_terms: usize,
    name: &str,
    name_path: &str,
) -> &'static str {
    if matched_terms.is_empty() || unique_query_terms == 0 {
        return "low";
    }
    let coverage = matched_terms.len() as f64 / unique_query_terms as f64;
    let name_lower = name.to_ascii_lowercase();
    let name_path_lower = name_path.to_ascii_lowercase();
    let identifier_hit = matched_terms.iter().any(|term| {
        let term_lower = term.to_ascii_lowercase();
        name_lower.contains(&term_lower) || name_path_lower.contains(&term_lower)
    });

    if coverage >= 0.8 && identifier_hit {
        "high"
    } else if identifier_hit || matched_terms.len() >= 2 {
        "medium"
    } else {
        "low"
    }
}

#[cfg(test)]
mod flatten_symbols_tests {
    use super::flatten_symbols;
    use codelens_engine::{SymbolInfo, SymbolKind, SymbolProvenance};

    fn symbol(name: &str, children: Vec<SymbolInfo>) -> SymbolInfo {
        SymbolInfo {
            name: name.to_owned(),
            kind: SymbolKind::Function,
            file_path: "src/lib.rs".to_owned(),
            line: 1,
            column: 0,
            signature: format!("fn {name}()"),
            name_path: name.to_owned(),
            id: name.to_owned(),
            provenance: SymbolProvenance::default(),
            body: None,
            children,
            start_byte: 0,
            end_byte: 0,
            end_line: 1,
        }
    }

    #[test]
    fn flatten_symbols_keeps_nested_children() {
        let symbols = vec![symbol(
            "root",
            vec![symbol("child", vec![symbol("grandchild", Vec::new())])],
        )];
        let flat = flatten_symbols(&symbols);
        let names = flat
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"root"));
        assert!(names.contains(&"child"));
        assert!(names.contains(&"grandchild"));
    }
}

#[cfg(test)]
mod suggested_follow_up_tests {
    use super::suggested_follow_up;

    #[test]
    fn function_gets_body_then_diagnostics() {
        let hints = suggested_follow_up("function", false);
        assert_eq!(hints.first().copied(), Some("find_symbol"));
        assert!(hints.contains(&"get_file_diagnostics"));
    }

    #[test]
    fn class_gets_body_and_references() {
        let hints = suggested_follow_up("class", false);
        assert_eq!(hints, vec!["find_symbol", "find_referencing_symbols"]);
    }

    #[test]
    fn module_gets_overview_first() {
        let hints = suggested_follow_up("module", false);
        assert_eq!(hints.first().copied(), Some("get_symbols_overview"));
    }

    #[test]
    fn exported_function_also_offers_references() {
        let hints = suggested_follow_up("function", true);
        assert!(hints.contains(&"find_referencing_symbols"));
        assert!(hints.contains(&"find_symbol"));
    }

    #[test]
    fn unknown_kind_falls_back_to_find_symbol() {
        let hints = suggested_follow_up("unknown", false);
        assert_eq!(hints, vec!["find_symbol"]);
    }
}

#[cfg(test)]
mod confidence_tier_tests {
    use super::confidence_tier;

    #[test]
    fn full_coverage_on_name_path_is_high() {
        let matched = vec!["dispatch".to_owned(), "tool".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 2, "dispatch_tool", "dispatch::dispatch_tool"),
            "high"
        );
    }

    #[test]
    fn partial_coverage_with_name_hit_is_medium() {
        let matched = vec!["dispatch".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 3, "dispatch_tool", "dispatch::dispatch_tool"),
            "medium"
        );
    }

    #[test]
    fn body_only_match_is_low() {
        let matched = vec!["invoke".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 2, "dispatch_tool", "dispatch::dispatch_tool"),
            "low"
        );
    }

    #[test]
    fn multiple_matches_without_name_hit_is_medium() {
        let matched = vec!["invoke".to_owned(), "handler".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 3, "dispatch_tool", "dispatch::dispatch_tool"),
            "medium"
        );
    }

    #[test]
    fn empty_matched_is_low() {
        assert_eq!(confidence_tier(&[], 2, "x", "a::x"), "low");
    }

    #[test]
    fn zero_query_terms_is_low() {
        let matched = vec!["dispatch".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 0, "dispatch_tool", "dispatch::dispatch_tool"),
            "low"
        );
    }
}
