#[cfg(feature = "semantic")]
use std::collections::HashSet;
#[cfg(feature = "semantic")]
use std::path::Path;

#[cfg(feature = "semantic")]
use super::RetrievalQueryAnalysis;

#[cfg(feature = "semantic")]
pub(crate) fn semantic_query_for_embedding_search(
    analysis: &RetrievalQueryAnalysis,
    project_root: Option<&Path>,
) -> String {
    if analysis.natural_language {
        let project_bridges = project_root.map(load_project_bridges).unwrap_or_default();
        let bridged = bridge_nl_to_code_vocabulary(&analysis.semantic_query, &project_bridges);
        format!("function {}", bridged)
    } else {
        analysis.semantic_query.clone()
    }
}

#[cfg(feature = "semantic")]
/// Load project-specific NL→code bridges from `.codelens/bridges.json`.
/// Format: `[{"nl": "stdin", "code": "run_stdio stdio"}, ...]`
/// Returns empty vec if file missing or malformed.
fn load_project_bridges(project_root: &Path) -> Vec<(String, String)> {
    let path = project_root.join(".codelens/bridges.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("invalid bridges.json: {e}");
            return Vec::new();
        }
    };
    parsed
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|entry| {
            let nl = entry.get("nl")?.as_str()?;
            let code = entry.get("code")?.as_str()?;
            Some((nl.to_owned(), code.to_owned()))
        })
        .collect()
}

#[cfg(feature = "semantic")]
/// Map common NL terms to code-domain equivalents. Two tiers:
/// 1. GENERIC_BRIDGES — language/project independent, always active
/// 2. project_bridges — from `.codelens/bridges.json`, project-specific
fn bridge_nl_to_code_vocabulary(query: &str, project_bridges: &[(String, String)]) -> String {
    let mut result = query.to_owned();
    let mut lowered_result = query.to_ascii_lowercase();
    let mut seen_tokens: HashSet<String> = lowered_result
        .split_whitespace()
        .map(str::to_owned)
        .collect();

    // Generic bridges: language/project independent vocabulary mappings.
    static GENERIC_BRIDGES: &[(&str, &str)] = &[
        ("categorize", "classify"),
        ("category", "classify"),
        ("sort by relevance", "rank score"),
        ("skip comments", "non-code ranges"),
        ("string literals", "non-code ranges"),
        ("functions that call", "callers call_graph"),
        ("who calls", "callers"),
        ("rename a variable", "rename"),
        ("rename a function", "rename"),
        ("search code", "search"),
        ("camelcase", "split identifier camel snake"),
        ("snake_case", "split identifier camel snake"),
        ("parse source", "AST parse"),
        ("into an ast", "AST parse"),
        ("diagnose", "diagnostics"),
    ];

    // Apply a single bridge entry.
    let mut apply = |nl_term: &str, code_term: &str| {
        if !lowered_result.contains(nl_term) {
            return;
        }
        let missing: Vec<&str> = code_term
            .split_whitespace()
            .filter(|t| seen_tokens.insert(t.to_ascii_lowercase()))
            .collect();
        if missing.is_empty() {
            return;
        }
        let joined = missing.join(" ");
        result.push(' ');
        result.push_str(&joined);
        lowered_result.push(' ');
        lowered_result.push_str(&joined.to_ascii_lowercase());
    };

    // Generic bridges are hard-coded and always-on by default. They can be
    // disabled at runtime via CODELENS_GENERIC_BRIDGES_OFF=1 to support
    // bridge-off/generic-on/repo-on ablation runs in the external benchmark
    // matrix (benchmarks/external-3arm.py).
    if !std::env::var("CODELENS_GENERIC_BRIDGES_OFF")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        for (nl, code) in GENERIC_BRIDGES {
            apply(nl, code);
        }
    }
    for (nl, code) in project_bridges {
        apply(nl, code);
    }
    result
}
