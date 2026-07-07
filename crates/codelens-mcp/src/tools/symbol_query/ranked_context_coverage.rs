use codelens_engine::RankedContextEntry;
use serde_json::{Value, json};

pub(super) fn ranked_context_coverage(
    symbols: &[RankedContextEntry],
    bodies_requested: bool,
    max_tokens: usize,
) -> Value {
    let bodies_returned = symbols
        .iter()
        .filter(|symbol| symbol.body.is_some())
        .count();
    let body_scope = body_scope(bodies_requested, bodies_returned);
    let gaps = coverage_gaps(symbols, bodies_requested, bodies_returned, max_tokens);

    json!({
        "mode": "symbol_cards",
        "flow_complete": false,
        "bodies_requested": bodies_requested,
        "bodies_returned": bodies_returned,
        "symbols_selected": symbols.len(),
        "body_scope": body_scope,
        "gaps": gaps,
        "next_call_hints": next_call_hints(body_scope, symbols.is_empty()),
    })
}

fn body_scope(bodies_requested: bool, bodies_returned: usize) -> &'static str {
    if !bodies_requested {
        "omitted"
    } else if bodies_returned == 0 {
        "none"
    } else {
        "symbol_span"
    }
}

fn coverage_gaps(
    symbols: &[RankedContextEntry],
    bodies_requested: bool,
    bodies_returned: usize,
    max_tokens: usize,
) -> Vec<&'static str> {
    let mut gaps = vec!["not_flow_complete"];
    if symbols.is_empty() {
        gaps.push("no_symbols_returned");
    }
    if !bodies_requested {
        gaps.push("bodies_not_requested");
    } else if bodies_returned == 0 {
        gaps.push("bodies_requested_but_unavailable");
    } else {
        gaps.push("symbol_span_only");
        gaps.push("whole_file_context_not_returned");
    }
    if max_tokens < 8192 {
        gaps.push("budget_compacted");
    }
    gaps
}

fn next_call_hints(body_scope: &str, no_symbols: bool) -> Vec<Value> {
    if no_symbols {
        return vec![json!({
            "tool": "get_ranked_context",
            "reason": "retry_with_narrower_query_or_path",
        })];
    }

    let whole_file_hint = json!({
        "tool": "read_file",
        "reason": "whole_file_or_flow_context",
    });
    if body_scope == "symbol_span" {
        vec![whole_file_hint]
    } else {
        vec![
            json!({
                "tool": "find_symbol",
                "reason": "selected_symbol_body",
            }),
            whole_file_hint,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(body: Option<&str>) -> RankedContextEntry {
        RankedContextEntry {
            name: "search_users".to_owned(),
            kind: "function".to_owned(),
            file: "ranked.py".to_owned(),
            line: 1,
            signature: "def search_users(query):".to_owned(),
            body: body.map(str::to_owned),
            relevance_score: 100,
        }
    }

    #[test]
    fn coverage_reports_omitted_bodies_when_not_requested() {
        let coverage = ranked_context_coverage(&[symbol(None)], false, 12000);

        assert_eq!(coverage["mode"], json!("symbol_cards"));
        assert_eq!(coverage["body_scope"], json!("omitted"));
        assert_eq!(coverage["bodies_requested"], json!(false));
        assert_eq!(coverage["bodies_returned"], json!(0));
        assert_eq!(coverage["flow_complete"], json!(false));
        assert!(
            coverage["gaps"]
                .as_array()
                .expect("gaps")
                .contains(&json!("bodies_not_requested"))
        );
    }

    #[test]
    fn coverage_reports_symbol_span_bodies_without_claiming_flow_completeness() {
        let coverage =
            ranked_context_coverage(&[symbol(Some("def search_users(): pass"))], true, 12000);

        assert_eq!(coverage["body_scope"], json!("symbol_span"));
        assert_eq!(coverage["bodies_requested"], json!(true));
        assert_eq!(coverage["bodies_returned"], json!(1));
        assert_eq!(coverage["flow_complete"], json!(false));
        assert!(
            coverage["gaps"]
                .as_array()
                .expect("gaps")
                .contains(&json!("symbol_span_only"))
        );
    }
}
