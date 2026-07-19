use codelens_engine::{ProjectRoot, embedding::DuplicatePair};

pub(super) struct DuplicateFilterOutcome {
    pub(super) pairs: Vec<DuplicatePair>,
    pub(super) suppressed_config_code_pairs: usize,
    pub(super) suppressed_same_file_same_symbol_pairs: usize,
    pub(super) suppressed_same_file_cross_symbol_pairs: usize,
    pub(super) suppressed_signature_only_pairs: usize,
}

pub(super) fn normalize_duplicate_scope(
    project: &ProjectRoot,
    scope: Option<&str>,
) -> Option<String> {
    let raw = scope?.trim();
    if raw.is_empty() || raw == "." {
        return None;
    }
    let resolved = project.resolve(raw).ok()?;
    let relative = project.to_relative(resolved);
    if relative.is_empty() || relative == "." {
        None
    } else {
        Some(relative.trim_end_matches('/').to_owned())
    }
}

fn file_in_duplicate_scope(scope: &str, file: &str) -> bool {
    let file = file.trim_start_matches("./");
    file == scope || file.starts_with(&format!("{scope}/"))
}

fn duplicate_pair_in_scope(scope: &str, pair: &DuplicatePair) -> bool {
    file_in_duplicate_scope(scope, &pair.file_a) || file_in_duplicate_scope(scope, &pair.file_b)
}

fn symbol_name_for_duplicate_side<'a>(rendered_symbol: &'a str, file: &str) -> &'a str {
    rendered_symbol
        .strip_prefix(&format!("{file}:"))
        .unwrap_or(rendered_symbol)
}

fn is_config_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    lower.ends_with(".yml")
        || lower.ends_with(".yaml")
        || lower.ends_with(".toml")
        || lower.ends_with(".json")
        || lower.ends_with(".jsonc")
}

fn is_code_file(file: &str) -> bool {
    let lower = file.to_ascii_lowercase();
    [
        ".rs", ".py", ".js", ".jsx", ".ts", ".tsx", ".go", ".java", ".kt", ".kts", ".swift", ".rb",
        ".php", ".c", ".h", ".cpp", ".hpp", ".cs", ".scala", ".dart", ".lua", ".ex", ".exs",
        ".erl", ".hrl", ".zig",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

fn is_structural_config_symbol(symbol: &str) -> bool {
    let normalized = symbol
        .trim()
        .trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`')
        .to_ascii_lowercase()
        .replace('-', "_");
    matches!(
        normalized.as_str(),
        "name"
            | "on"
            | "env"
            | "jobs"
            | "job"
            | "steps"
            | "step"
            | "uses"
            | "with"
            | "run"
            | "needs"
            | "permissions"
            | "strategy"
            | "matrix"
            | "workflow_dispatch"
            | "push"
            | "pull_request"
            | "schedule"
            | "branches"
            | "paths"
            | "timeout_minutes"
            | "runs_on"
    )
}

fn is_config_code_duplicate_noise(pair: &DuplicatePair) -> bool {
    let left_config = is_config_file(&pair.file_a);
    let right_config = is_config_file(&pair.file_b);

    // G6.1: config-vs-config pairs (the same CI YAML key across workflows,
    // shared env vars, etc.) are configuration structure, not shared code
    // logic. Their embedding cosine runs very high (0.98+) so the engine's
    // filetype floor cannot catch them — suppress here regardless of symbol.
    if left_config && right_config {
        return true;
    }

    if left_config == right_config {
        return false; // both code files: not config noise
    }

    let left_code = is_code_file(&pair.file_a);
    let right_code = is_code_file(&pair.file_b);
    if !(left_code || right_code) {
        return false;
    }

    if left_config {
        is_structural_config_symbol(symbol_name_for_duplicate_side(&pair.symbol_a, &pair.file_a))
    } else {
        is_structural_config_symbol(symbol_name_for_duplicate_side(&pair.symbol_b, &pair.file_b))
    }
}

fn is_same_file_same_symbol_pair(pair: &DuplicatePair) -> bool {
    pair.file_a == pair.file_b
        && symbol_name_for_duplicate_side(&pair.symbol_a, &pair.file_a)
            == symbol_name_for_duplicate_side(&pair.symbol_b, &pair.file_b)
}

fn is_data_symbol_kind(kind: &str) -> bool {
    matches!(
        kind,
        "variable" | "constant" | "const" | "field" | "property" | "parameter" | "static" | "local"
    )
}

fn is_same_file_cross_symbol_data_noise(pair: &DuplicatePair) -> bool {
    if pair.file_a != pair.file_b {
        return false;
    }
    let name_a = symbol_name_for_duplicate_side(&pair.symbol_a, &pair.file_a);
    let name_b = symbol_name_for_duplicate_side(&pair.symbol_b, &pair.file_b);
    if name_a == name_b {
        // same-file/same-symbol overloads are handled by
        // is_same_file_same_symbol_pair — don't double-count here.
        return false;
    }
    // G7: two *data* symbols (locals/constants/fields) declared in the
    // same file score a high embedding cosine on their short declarations
    // (e.g. `key_files_list` vs `key_files`, 0.9+) but are distinct
    // values, not shared logic. Function/method/class/struct pairs are
    // preserved — a real same-file duplicate *function* is an extract
    // candidate, the one signal worth surfacing here.
    is_data_symbol_kind(&pair.kind_a) && is_data_symbol_kind(&pair.kind_b)
}

pub(crate) fn duplicate_quality_scan_limit(
    include_config_code_pairs: bool,
    include_local_same_symbol_pairs: bool,
    max_pairs: usize,
) -> usize {
    if include_config_code_pairs && include_local_same_symbol_pairs {
        max_pairs
    } else {
        max_pairs.saturating_mul(8).clamp(max_pairs, 2048)
    }
}

/// F17: config-noise filter for the `find_code_duplicates` /
/// `review(mode=dupes)` surface. Reuses the cleanup config-noise
/// suppression ([`is_config_code_duplicate_noise`], G6.1) so CI-YAML
/// structural-key pairs (`env`, `runs_on`, shared `uses`) don't dominate
/// the top results. Unlike the cleanup workflow it does **not** apply the
/// same-file / signature-only filters — those are cleanup-planning
/// heuristics, not raw-audit concerns, so the raw duplicate surface keeps
/// its original behavior apart from config suppression. Passing
/// `include_config_code_pairs = true` restores the fully unfiltered output.
pub(crate) fn filter_find_code_duplicate_pairs(
    project: &ProjectRoot,
    pairs: Vec<DuplicatePair>,
    max_pairs: usize,
    include_config_code_pairs: bool,
) -> Vec<DuplicatePair> {
    filter_duplicate_pairs_for_cleanup(
        project,
        None,
        pairs,
        max_pairs,
        include_config_code_pairs,
        // Preserve the raw-audit surface: same-file and signature-only pairs
        // are cleanup-workflow-only filters, not applied here.
        true,
        true,
    )
    .pairs
}

pub(super) fn filter_duplicate_pairs_for_cleanup(
    project: &ProjectRoot,
    scope: Option<&str>,
    pairs: Vec<DuplicatePair>,
    max_pairs: usize,
    include_config_code_pairs: bool,
    include_local_same_symbol_pairs: bool,
    include_signature_only_matches: bool,
) -> DuplicateFilterOutcome {
    let normalized_scope = normalize_duplicate_scope(project, scope);
    let mut suppressed_config_code_pairs = 0usize;
    let mut suppressed_same_file_same_symbol_pairs = 0usize;
    let mut suppressed_same_file_cross_symbol_pairs = 0usize;
    let mut suppressed_signature_only_pairs = 0usize;
    let pairs = pairs
        .into_iter()
        .filter(|pair| {
            normalized_scope
                .as_deref()
                .is_none_or(|scope| duplicate_pair_in_scope(scope, pair))
        })
        .filter(|pair| {
            if include_config_code_pairs || !is_config_code_duplicate_noise(pair) {
                return true;
            }
            suppressed_config_code_pairs += 1;
            false
        })
        .filter(|pair| {
            if include_local_same_symbol_pairs || !is_same_file_same_symbol_pair(pair) {
                return true;
            }
            suppressed_same_file_same_symbol_pairs += 1;
            false
        })
        .filter(|pair| {
            // G7: same-file cross-symbol *data* pairs (adjacent locals /
            // constants) are embedding noise, not shared logic. Gated by
            // the same include_local flag as same-symbol helper noise.
            if include_local_same_symbol_pairs || !is_same_file_cross_symbol_data_noise(pair) {
                return true;
            }
            suppressed_same_file_cross_symbol_pairs += 1;
            false
        })
        .filter(|pair| {
            // #299: namespaced wrappers around a shared helper match by
            // signature + identifier shape but differ in body
            // predicates. Suppress those false positives by default;
            // callers can opt-in for debugging.
            if include_signature_only_matches || !pair.signature_only_match {
                return true;
            }
            suppressed_signature_only_pairs += 1;
            false
        })
        .take(max_pairs)
        .collect();

    DuplicateFilterOutcome {
        pairs,
        suppressed_config_code_pairs,
        suppressed_same_file_same_symbol_pairs,
        suppressed_same_file_cross_symbol_pairs,
        suppressed_signature_only_pairs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duplicate_pair_with_symbols(
        file_a: &str,
        symbol_a: &str,
        file_b: &str,
        symbol_b: &str,
    ) -> DuplicatePair {
        DuplicatePair {
            symbol_a: format!("{file_a}:{symbol_a}"),
            symbol_b: format!("{file_b}:{symbol_b}"),
            file_a: file_a.to_owned(),
            file_b: file_b.to_owned(),
            line_a: 1,
            line_b: 1,
            similarity: 0.99,
            body_token_jaccard: None,
            signature_only_match: false,
            kind_a: "function".to_owned(),
            kind_b: "function".to_owned(),
        }
    }

    fn duplicate_pair(file_a: &str, file_b: &str) -> DuplicatePair {
        duplicate_pair_with_symbols(file_a, "a", file_b, "b")
    }

    fn duplicate_pair_with_kinds(
        file_a: &str,
        symbol_a: &str,
        kind_a: &str,
        file_b: &str,
        symbol_b: &str,
        kind_b: &str,
    ) -> DuplicatePair {
        let mut pair = duplicate_pair_with_symbols(file_a, symbol_a, file_b, symbol_b);
        pair.kind_a = kind_a.to_owned();
        pair.kind_b = kind_b.to_owned();
        pair
    }

    fn temp_project() -> ProjectRoot {
        let dir = std::env::temp_dir().join(format!(
            "codelens-workflow-scope-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("crates")).unwrap();
        ProjectRoot::new_exact(&dir).unwrap()
    }

    #[test]
    fn config_vs_config_pairs_suppressed_by_default() {
        // G6.1 dogfood: .yml<->.yml structural-key pairs (CI workflow keys,
        // env vars) are configuration structure, not shared code logic.
        let project = temp_project();
        let pairs = vec![
            duplicate_pair_with_symbols(
                ".github/workflows/build.yml",
                "uses",
                ".github/workflows/release.yml",
                "uses",
            ),
            duplicate_pair_with_symbols(
                ".github/workflows/build.yml",
                "FORCE_JAVASCRIPT_ACTIONS_TO_NODE24",
                ".github/workflows/release.yml",
                "FORCE_JAVASCRIPT_ACTIONS_TO_NODE24",
            ),
            duplicate_pair_with_symbols("crates/a.rs", "foo", "crates/b.rs", "bar"),
        ];
        let filtered =
            filter_duplicate_pairs_for_cleanup(&project, None, pairs, 20, false, false, false);
        assert_eq!(
            filtered.pairs.len(),
            1,
            "only the real code pair should survive"
        );
        assert_eq!(filtered.pairs[0].file_a, "crates/a.rs");
        assert_eq!(filtered.suppressed_config_code_pairs, 2);
    }

    #[test]
    fn config_vs_config_pairs_restored_when_included() {
        let project = temp_project();
        let pairs = vec![duplicate_pair_with_symbols(
            ".github/workflows/build.yml",
            "uses",
            ".github/workflows/release.yml",
            "uses",
        )];
        let filtered =
            filter_duplicate_pairs_for_cleanup(&project, None, pairs, 20, true, false, false);
        assert_eq!(
            filtered.pairs.len(),
            1,
            "config-config restored when included"
        );
    }

    #[test]
    fn same_file_cross_symbol_variable_pairs_suppressed_by_default() {
        // G7 dogfood: adjacent local variables in one function
        // (key_files_list vs key_files) score 0.9+ cosine on their short
        // declarations but are distinct values — not shared logic. A real
        // same-file *function* duplicate (an extract candidate) must survive.
        let project = temp_project();
        let pairs = vec![
            duplicate_pair_with_kinds(
                "benchmarks/x.py",
                "key_files_list",
                "variable",
                "benchmarks/x.py",
                "key_files",
                "variable",
            ),
            duplicate_pair_with_kinds(
                "benchmarks/x.py",
                "helper_a",
                "function",
                "benchmarks/x.py",
                "helper_b",
                "function",
            ),
        ];
        let filtered =
            filter_duplicate_pairs_for_cleanup(&project, None, pairs, 20, false, false, false);
        assert_eq!(
            filtered.pairs.len(),
            1,
            "variable noise suppressed, function duplicate preserved"
        );
        assert_eq!(filtered.pairs[0].symbol_a, "benchmarks/x.py:helper_a");
        assert_eq!(filtered.suppressed_same_file_cross_symbol_pairs, 1);
    }

    #[test]
    fn same_file_cross_symbol_variable_pairs_restored_when_included() {
        let project = temp_project();
        let pairs = vec![duplicate_pair_with_kinds(
            "benchmarks/x.py",
            "key_files_list",
            "variable",
            "benchmarks/x.py",
            "key_files",
            "variable",
        )];
        let filtered =
            filter_duplicate_pairs_for_cleanup(&project, None, pairs, 20, false, true, false);
        assert_eq!(
            filtered.pairs.len(),
            1,
            "cross-symbol variable noise restored when local pairs included"
        );
        assert_eq!(filtered.suppressed_same_file_cross_symbol_pairs, 0);
    }

    #[test]
    fn find_code_duplicate_pairs_suppress_config_noise_by_default() {
        // F17: review(mode=dupes) / find_code_duplicates must apply the same
        // G6.1 config-noise suppression that cleanup uses, so CI-YAML
        // structural-key pairs (env / runs_on) don't dominate the top
        // results. It must NOT inherit the same-file / signature-only cleanup
        // filters — those are cleanup-planning concerns, not raw duplicate audit.
        let project = temp_project();
        let pairs = vec![
            duplicate_pair_with_symbols(
                ".github/workflows/build.yml",
                "runs_on",
                ".github/workflows/release.yml",
                "runs_on",
            ),
            duplicate_pair_with_symbols("crates/a.rs", "foo", "crates/b.rs", "bar"),
        ];
        let filtered = filter_find_code_duplicate_pairs(&project, pairs, 20, false);
        assert_eq!(
            filtered.len(),
            1,
            "config-config noise suppressed by default"
        );
        assert_eq!(filtered[0].file_a, "crates/a.rs");
    }

    #[test]
    fn find_code_duplicate_pairs_include_config_restores_original() {
        let project = temp_project();
        let pairs = vec![duplicate_pair_with_symbols(
            ".github/workflows/build.yml",
            "runs_on",
            ".github/workflows/release.yml",
            "runs_on",
        )];
        let filtered = filter_find_code_duplicate_pairs(&project, pairs, 20, true);
        assert_eq!(
            filtered.len(),
            1,
            "config-config restored when include_config_code_pairs=true"
        );
    }

    #[test]
    fn find_code_duplicate_pairs_preserve_same_file_and_signature_only() {
        // The dupes surface only gains config suppression; it must keep the
        // same-file and signature-only pairs the cleanup workflow filters out.
        let project = temp_project();
        let same_file = duplicate_pair_with_symbols(
            "crates/x.rs",
            "guidance_payload",
            "crates/x.rs",
            "guidance_payload",
        );
        let mut sig_only =
            duplicate_pair_with_symbols("crates/a.rs", "wrap_a", "crates/b.rs", "wrap_b");
        sig_only.body_token_jaccard = Some(0.2);
        sig_only.signature_only_match = true;
        let filtered =
            filter_find_code_duplicate_pairs(&project, vec![same_file, sig_only], 20, false);
        assert_eq!(
            filtered.len(),
            2,
            "same-file and signature-only pairs preserved on the dupes surface"
        );
    }

    #[test]
    fn duplicate_scope_filter_drops_pairs_fully_outside_scope() {
        let project = temp_project();
        let pairs = vec![
            duplicate_pair(
                ".github/workflows/benchmark.yml",
                ".github/workflows/build.yml",
            ),
            duplicate_pair(
                "crates/codelens-mcp/src/tools/workflows.rs",
                ".github/workflows/build.yml",
            ),
        ];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            false,
            false,
            false,
        );

        assert_eq!(filtered.pairs.len(), 1);
        assert_eq!(
            filtered.pairs[0].file_a,
            "crates/codelens-mcp/src/tools/workflows.rs"
        );
    }

    #[test]
    fn duplicate_quality_filter_suppresses_workflow_key_code_pairs_by_default() {
        let project = temp_project();
        let pairs = vec![
            duplicate_pair_with_symbols(
                ".github/workflows/pages.yml",
                "workflow_dispatch",
                "crates/codelens-mcp/src/integration_tests/workflow/mod.rs",
                "dispatch",
            ),
            duplicate_pair_with_symbols(
                "crates/codelens-mcp/src/tools/workflows.rs",
                "cleanup_duplicate_logic",
                "crates/codelens-mcp/src/tools/mod.rs",
                "cleanup_duplicate_logic",
            ),
        ];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            false,
            false,
            false,
        );

        assert_eq!(filtered.suppressed_config_code_pairs, 1);
        assert_eq!(filtered.pairs.len(), 1);
        assert_eq!(
            filtered.pairs[0].file_a,
            "crates/codelens-mcp/src/tools/workflows.rs"
        );
    }

    #[test]
    fn duplicate_quality_filter_can_include_config_code_pairs() {
        let project = temp_project();
        let pairs = vec![duplicate_pair_with_symbols(
            ".github/workflows/pages.yml",
            "workflow_dispatch",
            "crates/codelens-mcp/src/integration_tests/workflow/mod.rs",
            "dispatch",
        )];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            true,
            false,
            false,
        );

        assert_eq!(filtered.suppressed_config_code_pairs, 0);
        assert_eq!(filtered.pairs.len(), 1);
    }

    #[test]
    fn duplicate_quality_filter_suppresses_same_file_same_symbol_pairs_by_default() {
        let project = temp_project();
        let pairs = vec![
            duplicate_pair_with_symbols(
                "crates/codelens-mcp/src/tools/session/capabilities.rs",
                "guidance_payload",
                "crates/codelens-mcp/src/tools/session/capabilities.rs",
                "guidance_payload",
            ),
            duplicate_pair_with_symbols(
                "crates/codelens-mcp/src/state/coordination.rs",
                "list_active_agents",
                "crates/codelens-mcp/src/tools/session/coordination.rs",
                "list_active_agents",
            ),
        ];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            false,
            false,
            false,
        );

        assert_eq!(filtered.suppressed_same_file_same_symbol_pairs, 1);
        assert_eq!(filtered.pairs.len(), 1);
        assert_eq!(
            filtered.pairs[0].file_b,
            "crates/codelens-mcp/src/tools/session/coordination.rs"
        );
    }

    #[test]
    fn duplicate_quality_filter_suppresses_signature_only_pairs_by_default() {
        // #299: a pair flagged signature_only_match must be hidden by
        // default. The fixture pairs two namespaced wrappers whose
        // body bodies diverge — the embedding-side cosine put them
        // high but body_token_jaccard contradicted.
        let project = temp_project();
        let real = duplicate_pair_with_symbols(
            "crates/codelens-engine/src/symbols/parse.rs",
            "parse_program",
            "crates/codelens-engine/src/symbols/eval.rs",
            "eval_program",
        );
        let mut signature_only = duplicate_pair_with_symbols(
            "crates/codelens-engine/src/call_graph/resolve.rs",
            "collect_call_graph_candidates",
            "crates/codelens-engine/src/import_graph/mod.rs",
            "collect_import_graph_candidates",
        );
        signature_only.body_token_jaccard = Some(0.21);
        signature_only.signature_only_match = true;

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            vec![real.clone(), signature_only],
            20,
            false,
            false,
            false,
        );

        assert_eq!(filtered.suppressed_signature_only_pairs, 1);
        assert_eq!(filtered.pairs.len(), 1);
        assert_eq!(filtered.pairs[0].file_a, real.file_a);
    }

    #[test]
    fn duplicate_quality_filter_can_include_signature_only_pairs() {
        // #299 opt-out: callers can flip the include flag to surface
        // signature-only matches for debugging.
        let project = temp_project();
        let mut pair = duplicate_pair_with_symbols(
            "crates/codelens-engine/src/call_graph/resolve.rs",
            "collect_call_graph_candidates",
            "crates/codelens-engine/src/import_graph/mod.rs",
            "collect_import_graph_candidates",
        );
        pair.body_token_jaccard = Some(0.21);
        pair.signature_only_match = true;

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            vec![pair],
            20,
            false,
            false,
            true,
        );

        assert_eq!(filtered.suppressed_signature_only_pairs, 0);
        assert_eq!(filtered.pairs.len(), 1);
        assert!(filtered.pairs[0].signature_only_match);
    }

    #[test]
    fn duplicate_quality_filter_can_include_same_file_same_symbol_pairs() {
        let project = temp_project();
        let pairs = vec![duplicate_pair_with_symbols(
            "crates/codelens-mcp/src/tools/session/capabilities.rs",
            "guidance_payload",
            "crates/codelens-mcp/src/tools/session/capabilities.rs",
            "guidance_payload",
        )];

        let filtered = filter_duplicate_pairs_for_cleanup(
            &project,
            Some(project.as_path().join("crates").to_str().unwrap()),
            pairs,
            20,
            false,
            true,
            false,
        );

        assert_eq!(filtered.suppressed_same_file_same_symbol_pairs, 0);
        assert_eq!(filtered.pairs.len(), 1);
    }
}
