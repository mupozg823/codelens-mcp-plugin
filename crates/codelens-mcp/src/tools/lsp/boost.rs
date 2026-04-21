use crate::AppState;
use codelens_engine::{LspRequest, find_all_word_matches};

/// P1-4 caller wiring: run a best-effort reference probe for `query`
/// anchored at `path` and turn the hit files into a boost set.
///
/// Strategy mirrors `find_referencing_symbols` with `union=true`: try
/// LSP `textDocument/references` first, then merge tree-sitter text
/// references. The tree-sitter pass is the key fallback — LSP is
/// commonly cold (returning 0 refs for 5-30s on rust-analyzer /
/// pyright), and without a fallback the boost would be silently inert
/// on every cold CLI invocation. The union gives us a populated set
/// whenever either backend resolves anything, so the P1-4 wiring meets
/// the caller regardless of LSP readiness.
pub(crate) fn lsp_boost_probe(
    state: &AppState,
    query: &str,
    path: Option<&str>,
) -> (std::collections::HashMap<String, Vec<usize>>, Option<f64>) {
    let empty = (std::collections::HashMap::new(), None);
    let Some(path) = path else {
        return empty;
    };

    let mut refs_by_file: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();

    if let Some(command) = crate::tools::default_lsp_command_for_path(path) {
        let anchor = match state
            .symbol_index()
            .find_symbol(query, Some(path), false, true, 1)
        {
            Ok(rows) => rows.into_iter().next().map(|s| (s.line, s.column)),
            Err(_) => None,
        };
        if let Some((line, column)) = anchor {
            let request = LspRequest {
                command: command.clone(),
                args: crate::tools::default_lsp_args_for_command(&command),
                file_path: path.to_owned(),
                line,
                column,
                max_results: 64,
            };
            if let Ok(refs) = state.lsp_pool().find_referencing_symbols(&request) {
                for r in refs {
                    refs_by_file.entry(r.file_path).or_default().push(r.line);
                }
            }
        }
    }

    const LSP_BOOST_PER_FILE_CAP: usize = 8;
    const LSP_BOOST_GLOBAL_CAP: usize = 512;

    fn is_test_path(path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        lower.contains("/tests/")
            || lower.contains("/test/")
            || lower.contains("/__tests__/")
            || lower.ends_with(".test.ts")
            || lower.ends_with(".test.tsx")
            || lower.ends_with(".test.js")
            || lower.ends_with(".spec.ts")
            || lower.ends_with(".spec.js")
            || lower.ends_with("_test.go")
            || lower.ends_with("_test.py")
            || lower.ends_with("_test.rs")
    }

    if let Ok(matches) = find_all_word_matches(&state.project(), query) {
        let mut per_file: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut total = 0usize;
        for pass in 0..2 {
            let want_tests = pass == 1;
            for (file, line, _col) in &matches {
                if total >= LSP_BOOST_GLOBAL_CAP {
                    break;
                }
                if is_test_path(file) != want_tests {
                    continue;
                }
                let count = per_file.entry(file.clone()).or_insert(0);
                if *count >= LSP_BOOST_PER_FILE_CAP {
                    continue;
                }
                *count += 1;
                total += 1;
                refs_by_file.entry(file.clone()).or_default().push(*line);
            }
            if total >= LSP_BOOST_GLOBAL_CAP {
                break;
            }
        }
    }

    if refs_by_file.is_empty() {
        return empty;
    }
    let weight = std::env::var("CODELENS_LSP_SIGNAL_WEIGHT")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.25);
    (refs_by_file, Some(weight))
}
