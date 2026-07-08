//! Drift gates for the tool-recommendation engine.
//!
//! The `suggest_next` table and the hard-coded `suggested_next_tools` JSON
//! literals used to reference tools that had been renamed, tombstoned, or never
//! registered at all — so an agent following a suggestion could be steered to a
//! tool that does not exist in `tools/list`. These tests make that class of
//! drift a compile-of-the-test-suite failure instead of a silent runtime dead
//! end: every key/value/literal/grant is cross-checked against the canonical
//! `tools.toml` registry.
//!
//! Paths are resolved from `CARGO_MANIFEST_DIR` (the `codelens-mcp` crate) so
//! the tests are location-independent.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Feature-gated tools (compiled only with `--features semantic`). They are
/// present in `tools.toml` but excluded from grant gating so a no-semantic
/// build does not fail on a skill/agent that lists them.
const FEATURE_GATED: &[&str] = &[
    "semantic_search",
    "index_embeddings",
    "embedding_coverage_report",
    "find_similar_code",
    "find_code_duplicates",
    "classify_symbol",
    "find_misplaced_code",
];

/// Deprecated aliases intentionally retained as `suggest_next` *keys* even
/// though they are no longer registered tools (legacy-name → canonical-workflow
/// routing). Guarded behaviourally by `suggest_next_prefers_canonical_workflows`.
const INTENTIONAL_ALIAS_KEYS: &[&str] = &["analyze_change_impact"];

/// Dispatch-only tools (ADR-0009/D3, #346): callable via `tools/call` behind the
/// `:7838` mutation gate but intentionally absent from `tools.toml`, so they
/// never surface in `tools/list`. They are legitimate `suggest_next` *values* /
/// literals — an agent can execute them — so the drift gate treats them as known
/// exactly like `FEATURE_GATED`. Sourced from the crate's own pending-D3
/// allowlists (minus any `TOMBSTONED_TOOLS`) so this set can never drift from the
/// live dispatch table.
fn is_dispatch_only(name: &str) -> bool {
    let in_d3 = crate::tools::PENDING_D3_SYMBOLIC_EDIT_CORE.contains(&name)
        || crate::tools::PENDING_D3_REFACTOR_SUBSTRATE.contains(&name);
    let tombstoned = crate::tools::TOMBSTONED_TOOLS
        .iter()
        .any(|(tool, _)| *tool == name);
    in_d3 && !tombstoned
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Repository root — two levels up from `crates/codelens-mcp`.
fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(Path::parent)
        .expect("crate is nested two levels under the repo root")
        .to_path_buf()
}

/// Parse the canonical tool registry (`tools.toml`) for the set of real tool
/// names. Tool entries are `name = "<snake>"` at column 0; parameter tables use
/// `name = { ... }`, so the trailing quote in the pattern excludes them.
fn live_tools() -> HashSet<String> {
    let path = manifest_dir().join("tools.toml");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let re = regex::Regex::new(r#"(?m)^name = "([a-z0-9_]+)""#).unwrap();
    let set: HashSet<String> = re.captures_iter(&src).map(|c| c[1].to_string()).collect();
    assert!(
        set.len() > 50,
        "tools.toml parse produced only {} names — pattern out of date?",
        set.len()
    );
    set
}

fn tool_is_known(name: &str, live: &HashSet<String>) -> bool {
    FEATURE_GATED.contains(&name) || is_dispatch_only(name) || live.contains(name)
}

/// (3a) Every key and value in the `suggest_next` table resolves to a real
/// `tools.toml` tool (keys may additionally be a documented deprecated alias).
#[test]
fn suggest_next_table_only_references_live_tools() {
    let live = live_tools();
    let table = crate::tools::SUGGEST_NEXT_TABLE;

    let mut bad = Vec::new();
    let mut seen_keys = HashSet::new();
    for (key, values) in table {
        if !seen_keys.insert(*key) {
            bad.push(format!("duplicate key `{key}` (linear lookup shadows it)"));
        }
        if !live.contains(*key) && !INTENTIONAL_ALIAS_KEYS.contains(key) {
            bad.push(format!("key `{key}` is not a live tools.toml tool"));
        }
        for value in *values {
            if !tool_is_known(value, &live) {
                bad.push(format!(
                    "value `{value}` under key `{key}` is not a live tools.toml tool"
                ));
            }
        }
    }

    assert!(
        bad.is_empty(),
        "suggest_next drift:\n  {}",
        bad.join("\n  ")
    );
    // Guard against the table silently emptying via a bad refactor.
    assert!(
        table.len() > 40,
        "suggest_next table shrank to {} entries — unexpected",
        table.len()
    );
}

/// (3b) Hard-coded `suggested_next_tools` JSON literals in the owned suggestion
/// sources reference only real tools. Covers both the direct array form
/// (`"suggested_next_tools": [ ... ]`) and the conditional `json!([ ... ])` form
/// used by `propagate_deletions`.
#[test]
fn source_suggested_next_tools_literals_reference_live_tools() {
    let live = live_tools();
    let dir = manifest_dir();
    let sources = [
        dir.join("src/tools/composite.rs"),
        dir.join("src/tools/suggestions.rs"),
        // The LSP arm of `propagate_deletions` also emits a conditional
        // `suggested_next_tools` json literal — it was previously outside the
        // census and shipped a tombstoned `delete_lines` suggestion (#346).
        dir.join("src/tools/semantic_edit/safe_delete.rs"),
    ];

    let direct_re = regex::Regex::new(r#""suggested_next_tools"\s*:\s*\[([^\]]*)\]"#).unwrap();
    let key_re = regex::Regex::new(r#""suggested_next_tools"\s*:\s*"#).unwrap();
    let json_arr_re = regex::Regex::new(r#"json!\(\[([^\]]*)\]\)"#).unwrap();
    let name_re = regex::Regex::new(r#""([a-z][a-z0-9_]+)""#).unwrap();

    let mut bad = Vec::new();
    let mut checked = 0usize;
    let check = |array_body: &str, file: &Path, bad: &mut Vec<String>, checked: &mut usize| {
        for cap in name_re.captures_iter(array_body) {
            let name = &cap[1];
            *checked += 1;
            if !tool_is_known(name, &live) {
                bad.push(format!(
                    "{}: suggested_next_tools references non-existent tool `{name}`",
                    file.display()
                ));
            }
        }
    };

    for file in &sources {
        let src = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));

        // Direct array form.
        for cap in direct_re.captures_iter(&src) {
            check(&cap[1], file, &mut bad, &mut checked);
        }

        // Conditional `if ... { json!([..]) } else { json!([..]) }` form.
        for m in key_re.find_iter(&src) {
            let tail = &src[m.end()..];
            if !tail.starts_with("if") {
                continue; // direct form already handled above
            }
            let window = &tail[..tail.len().min(400)];
            for cap in json_arr_re.captures_iter(window).take(2) {
                check(&cap[1], file, &mut bad, &mut checked);
            }
        }
    }

    assert!(
        bad.is_empty(),
        "suggested_next_tools literal drift:\n  {}",
        bad.join("\n  ")
    );
    assert!(
        checked > 0,
        "extracted zero tool names — the literal shape changed, regex is stale"
    );
}

// NOTE: a `plugin_json_version == workspace Cargo.toml version` equality gate
// was intentionally NOT added here. `release-plz` bumps only `Cargo.toml` (+
// CHANGELOG) and no automation syncs `.claude-plugin/plugin.json`, so such a gate
// would turn every automated release PR red until the manifest was hand-bumped.
// `scripts/validate-plugin-manifest.py` validates the manifest's shape instead.

/// (3d) Every tool granted in a skill/agent frontmatter `tools:` array is a real
/// tool. Only the frontmatter allow-list is gated — `disallowedTools:` (a deny
/// list that legitimately names removed tools) and prose backtick references are
/// intentionally excluded.
#[test]
fn skill_and_agent_tool_grants_reference_live_tools() {
    let live = live_tools();
    let root = repo_root();

    let mut md_files = Vec::new();
    for dir in [
        root.join("skills"),
        root.join("agents"),
        root.join(".claude/agents"),
    ] {
        collect_markdown(&dir, &mut md_files);
    }
    assert!(
        !md_files.is_empty(),
        "no skill/agent markdown files discovered under skills/, agents/, .claude/agents/"
    );

    // `^tools:` anchors to the frontmatter allow-list line, never the substring
    // inside `disallowedTools:`. Non-greedy up to the first `]` keeps it to that
    // one array. The optional `mcp__codelens__` prefix covers agent grants;
    // skills list bare names.
    let tools_block_re = regex::Regex::new(r"(?ms)^tools:\s*\[(.*?)\]").unwrap();
    let name_re = regex::Regex::new(r"(?:mcp__codelens__)?([a-z][a-z0-9_]+)").unwrap();

    let mut bad = Vec::new();
    let mut checked = 0usize;
    for file in &md_files {
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let Some(front) = frontmatter(&text) else {
            continue;
        };
        let Some(cap) = tools_block_re.captures(front) else {
            continue;
        };
        for name in name_re.captures_iter(&cap[1]) {
            let tool = &name[1];
            checked += 1;
            if !tool_is_known(tool, &live) {
                bad.push(format!(
                    "{}: tools grant references non-existent tool `{tool}`",
                    file.display()
                ));
            }
        }
    }

    assert!(
        bad.is_empty(),
        "skill/agent tool-grant drift:\n  {}",
        bad.join("\n  ")
    );
    assert!(
        checked > 0,
        "no tool grants extracted — frontmatter `tools:` shape changed?"
    );
}

/// Return the YAML frontmatter body delimited by the first two `---` fences.
fn frontmatter(text: &str) -> Option<&str> {
    let mut parts = text.splitn(3, "---");
    let _before = parts.next()?; // text before the opening fence (empty)
    let body = parts.next()?; // frontmatter body
    parts.next()?; // require a closing fence to exist
    Some(body)
}

/// Recursively collect `*.md` files under `dir` (missing dirs are ignored).
fn collect_markdown(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}
