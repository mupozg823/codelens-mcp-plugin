use crate::call_graph::extract_calls;
use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::import_graph::parsers::collect_top_level_funcs;
use crate::import_graph::{DeadCodeEntry, GraphCache, collect_candidate_files};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntryV2 {
    pub file: String,
    pub symbol: Option<String>,
    pub kind: Option<String>,
    pub line: Option<usize>,
    pub reason: String,
    pub pass: u8,
    /// #268: confidence tier for this dead-code finding. `"high"` for
    /// clean cases the import graph can fully account for.
    /// `"needs_structural_evidence"` for TypeScript request/schema/type
    /// files whose exported interfaces are frequently consumed through
    /// structural patterns (`z.infer<...>`, `as Request`, object-literal
    /// flow) that the import graph cannot trace — reviewers should
    /// cross-check before deleting.
    pub confidence: String,
}

/// #268: file is a TypeScript module whose dead-code verdict warrants a
/// confidence downgrade because exported types are commonly consumed via
/// structural typing (Zod-inferred shapes, `as RequestType` casts,
/// object-literal flow) that `import_graph` does not trace. The
/// signal is intentionally name-shaped — file path contains one of the
/// recognised request/schema/contract tokens. False positives here only
/// soften the verdict; they do not flip a real orphan to "alive".
pub(super) fn is_ts_structural_likely(file: &str) -> bool {
    let path = Path::new(file);
    let ext_ok = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("ts" | "tsx")
    );
    if !ext_ok {
        return false;
    }
    const STRUCTURAL_NAME_TOKENS: &[&str] = &[
        "request",
        "schema",
        "types",
        "contract",
        "interface",
        "model",
        "dto",
    ];
    let name_lc = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let parent_lc = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    STRUCTURAL_NAME_TOKENS
        .iter()
        .any(|kw| name_lc.contains(kw) || parent_lc.contains(kw))
}

/// #268: tier label for entries the import graph can fully account for.
pub(super) const CONFIDENCE_HIGH: &str = "high";

/// #268: tier label for TS request/schema/type entries whose orphan
/// verdict needs cross-check (Zod-inferred shapes, structural casts,
/// route-handler body usage are invisible to `import_graph`).
pub(super) const CONFIDENCE_STRUCTURAL: &str = "needs_structural_evidence";

pub(super) fn confidence_tier_for_file(file: &str) -> &'static str {
    if is_ts_structural_likely(file) {
        CONFIDENCE_STRUCTURAL
    } else {
        CONFIDENCE_HIGH
    }
}

/// Exception file names that should not be flagged as dead (entry points / init files).
pub(super) fn is_entry_point_file(file: &str) -> bool {
    let name = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file);
    matches!(
        name,
        "__init__.py"
            | "mod.rs"
            | "lib.rs"
            | "main.rs"
            | "index.ts"
            | "index.js"
            | "index.tsx"
            | "index.jsx"
    )
}

/// Exception symbol names that should not be flagged as dead.
pub(super) fn is_entry_point_symbol(name: &str) -> bool {
    name == "main"
        || name == "__init__"
        || name == "setUp"
        || name == "tearDown"
        || name.starts_with("test_")
        || name.starts_with("Test")
}

/// Check whether any line preceding a symbol definition starts with `@`
/// (decorator pattern). Scans upward through stacked decorators.
/// `lines` is the 0-indexed source lines; `symbol_line` is 1-indexed.
pub(super) fn has_decorator(lines: &[&str], symbol_line: usize) -> bool {
    if symbol_line < 2 {
        return false;
    }
    // Scan upward from the line before the definition
    let mut idx = symbol_line - 2; // convert to 0-indexed, then go one line back
    loop {
        match lines.get(idx) {
            Some(line) if line.trim_start().starts_with('@') => return true,
            Some(line) if line.trim().is_empty() => {} // skip blank lines between decorators
            _ => return false,
        }
        if idx == 0 {
            return false;
        }
        idx -= 1;
    }
}

pub fn find_dead_code(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntry>> {
    let graph = cache.get_or_build(project)?;
    let mut dead: Vec<_> = graph
        .iter()
        .filter(|(_, node)| node.imported_by.is_empty())
        .map(|(file, _)| DeadCodeEntry {
            file: file.clone(),
            symbol: None,
            reason: "no importers".to_owned(),
        })
        .collect();
    dead.sort_by(|a, b| a.file.cmp(&b.file));
    if max_results > 0 && dead.len() > max_results {
        dead.truncate(max_results);
    }
    Ok(dead)
}

pub fn find_dead_code_v2(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntryV2>> {
    let mut results: Vec<DeadCodeEntryV2> = Vec::new();

    // ── Pass 1: unreferenced files ────────────────────────────────────────────
    let graph = cache.get_or_build(project)?;
    for (file, node) in graph.iter() {
        if node.imported_by.is_empty() && !is_entry_point_file(file) {
            results.push(DeadCodeEntryV2 {
                file: file.clone(),
                symbol: None,
                kind: None,
                line: None,
                reason: "no importers".to_owned(),
                pass: 1,
                confidence: confidence_tier_for_file(file).to_owned(),
            });
        }
    }

    // ── Pass 2: unreferenced symbols ─────────────────────────────────────────
    let candidate_files = collect_candidate_files(project.as_path())?;
    let mut all_callees: HashSet<String> = HashSet::new();
    for path in &candidate_files {
        for edge in extract_calls(path) {
            all_callees.insert(edge.callee_name);
        }
    }

    for path in &candidate_files {
        let relative = project.to_relative(path);

        if results.iter().any(|e| e.file == relative && e.pass == 1) {
            continue;
        }
        if is_entry_point_file(&relative) {
            continue;
        }

        let source = std::fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = source.lines().collect();

        let edges = extract_calls(path);
        let mut defined_funcs: HashMap<String, usize> = HashMap::new();
        for edge in &edges {
            defined_funcs.entry(edge.caller_name.clone()).or_insert(0);
        }
        collect_top_level_funcs(path, &source, &mut defined_funcs);

        for (func_name, func_line) in defined_funcs {
            if func_name == "<module>" {
                continue;
            }
            if is_entry_point_symbol(&func_name) {
                continue;
            }
            if func_line > 0 && has_decorator(&lines, func_line) {
                continue;
            }
            if !all_callees.contains(&func_name) {
                results.push(DeadCodeEntryV2 {
                    file: relative.clone(),
                    symbol: Some(func_name),
                    kind: Some("function".to_owned()),
                    line: if func_line > 0 { Some(func_line) } else { None },
                    reason: "unreferenced symbol".to_owned(),
                    pass: 2,
                    confidence: confidence_tier_for_file(&relative).to_owned(),
                });
            }
        }
    }

    results.sort_by(|a, b| {
        a.pass
            .cmp(&b.pass)
            .then(a.file.cmp(&b.file))
            .then(a.symbol.cmp(&b.symbol))
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_request_files_downgrade_confidence() {
        // #268: request/schema/types/contract files commonly export
        // interfaces consumed via structural typing — must downgrade.
        for path in [
            "src/api/request.ts",
            "src/api/RequestTypes.ts",
            "src/server/schema.ts",
            "src/contracts/UserContract.ts",
            "src/types/index.ts",
            "src/models/User.ts",
            "src/dtos/CreateUser.ts",
            "components/MyForm/types.tsx",
            "lib/Interface.ts",
        ] {
            assert!(
                is_ts_structural_likely(path),
                "{path:?} should be downgraded"
            );
            assert_eq!(confidence_tier_for_file(path), CONFIDENCE_STRUCTURAL);
        }
    }

    #[test]
    fn non_ts_or_unrelated_files_keep_high_confidence() {
        for path in [
            "src/main.rs",
            "src/utils.py",
            "scripts/build.sh",
            "src/app/page.tsx", // not a request/schema/type-shaped name
            "src/hooks/useGifStudio.ts",
            "Cargo.toml",
            "package.json",
        ] {
            assert!(
                !is_ts_structural_likely(path),
                "{path:?} should keep high confidence"
            );
            assert_eq!(confidence_tier_for_file(path), CONFIDENCE_HIGH);
        }
    }

    #[test]
    fn javascript_request_file_is_not_downgraded() {
        // The downgrade is TypeScript-specific: TS structural typing +
        // Zod schemas are the false-positive class. Plain JS does not
        // get the same benefit.
        assert!(!is_ts_structural_likely("src/api/request.js"));
        assert_eq!(
            confidence_tier_for_file("src/api/request.js"),
            CONFIDENCE_HIGH
        );
    }
}
