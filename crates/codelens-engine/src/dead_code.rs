use crate::call_graph::{extract_calls, is_noise_callee};
use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::import_graph::cfg_test::cfg_test_regions;
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

/// Directory names whose files a test runner or `cargo bench` collects itself.
const TEST_DIR_SEGMENTS: &[&str] = &["tests", "test", "__tests__", "spec", "benches"];

/// Next.js app-router files the framework loads by name.
const NEXT_APP_FILE_STEMS: &[&str] = &[
    "page",
    "layout",
    "route",
    "loading",
    "error",
    "not-found",
    "template",
    "default",
    "global-error",
    "opengraph-image",
    "twitter-image",
    "icon",
    "apple-icon",
    "sitemap",
    "robots",
    "manifest",
];

/// Next.js files the framework loads from the project (or `src/`) root.
const NEXT_ROOT_FILE_STEMS: &[&str] = &["middleware", "proxy", "instrumentation"];

fn is_js_family(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts")
    )
}

fn is_test_file(path: &Path, name: &str) -> bool {
    let in_test_dir = path.parent().is_some_and(|parent| {
        parent.components().any(|c| {
            c.as_os_str()
                .to_str()
                .is_some_and(|s| TEST_DIR_SEGMENTS.contains(&s))
        })
    });
    in_test_dir
        || (is_js_family(path) && (name.contains(".test.") || name.contains(".spec.")))
        || name.ends_with("_test.go")
        || name.ends_with("_spec.rb")
        || name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.ends_with("_test.rs")
        || (name.ends_with(".py")
            && (name.starts_with("test_") || name.ends_with("_test.py") || name == "conftest.py"))
}

fn is_framework_convention_file(path: &Path, name: &str) -> bool {
    if !is_js_family(path) {
        return false;
    }
    let stem = name.split('.').next().unwrap_or(name);
    let parent = path.parent().unwrap_or(Path::new(""));
    let under_app_dir = parent.components().any(|c| c.as_os_str() == "app");
    let at_root = parent.as_os_str().is_empty() || parent == Path::new("src");
    (under_app_dir && NEXT_APP_FILE_STEMS.contains(&stem))
        || (at_root && NEXT_ROOT_FILE_STEMS.contains(&stem))
}

/// `vitest.config.ts`, `next.config.mjs`, `.eslintrc.cjs`, … — read by the
/// tool they configure.
fn is_tool_config_file(path: &Path, name: &str) -> bool {
    is_js_family(path) && (name.contains(".config.") || name.starts_with('.'))
}

/// Files something other than an import loads: entry/init files, tests the
/// runner collects, framework route files and tool configs. "Nothing imports
/// it" is their expected shape, not evidence of death.
pub(super) fn is_entry_point_file(file: &str) -> bool {
    let path = Path::new(file);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(file);
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
    ) || is_test_file(path, name)
        || is_framework_convention_file(path, name)
        || is_tool_config_file(path, name)
}

/// True when every `fn name` definition in a Rust file sits inside a
/// `#[cfg(test)]` item: a test or a test helper, run by the harness. Test
/// functions inside `mod tests { … }` are indented, so the top-level scan
/// gives them no line and the attribute check never ran for them.
fn defined_only_in_cfg_test(source: &str, regions: &[std::ops::Range<usize>], name: &str) -> bool {
    let needle = format!("fn {name}");
    let mut definitions = source
        .match_indices(&needle)
        .map(|(at, _)| at)
        .filter(|&at| source[at + needle.len()..].starts_with(['(', '<']))
        .peekable();
    definitions.peek().is_some() && definitions.all(|at| regions.iter().any(|r| r.contains(&at)))
}

/// A script run directly: a shebang line, or a Python `__main__` guard.
fn is_executable_script(content: &str) -> bool {
    content.starts_with("#!")
        || content.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("if __name__") && line.contains("__main__")
        })
}

/// Files that launch other files by path: `node scripts/x.mjs` in
/// `package.json`, `run:` steps and path globs in CI workflows, `setupFiles`
/// in `vitest.config.ts`, `CMD` in a Dockerfile.
fn is_launch_manifest(name: &str) -> bool {
    matches!(
        name,
        "package.json" | "Makefile" | "justfile" | "Procfile" | "Dockerfile"
    ) || is_tool_config_file(Path::new(name), name)
}

/// Path tokens named by the launch manifests of one directory, relative to
/// that directory.
#[derive(Default)]
struct LaunchRefs {
    paths: HashSet<String>,
    globs: Vec<regex::Regex>,
}

impl LaunchRefs {
    fn add_text(&mut self, text: &str) {
        let is_separator = |c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '=' | '|' | ':'
                )
        };
        for token in text.split(is_separator) {
            let token = token.trim_start_matches("./");
            if token.is_empty() || !token.contains(['/', '.']) {
                continue;
            }
            if token.contains('*') {
                if let Some(glob) = glob_to_regex(token) {
                    self.globs.push(glob);
                }
            } else {
                self.paths.insert(token.to_owned());
            }
        }
    }

    fn names(&self, relative: &str) -> bool {
        self.paths.contains(relative) || self.globs.iter().any(|g| g.is_match(relative))
    }
}

/// `scripts/test-*-db.mjs` → `^scripts/test-[^/]*-db\.mjs$`; `**` spans
/// directories.
///
/// Only globs whose file-name part starts with a literal name a family of
/// launched scripts. `lib/**` in a workflow `paths:` trigger, `"**/*.ts"` in a
/// lint script or a coverage `include` match every file and would hide all
/// dead code, so they are dropped.
fn glob_to_regex(glob: &str) -> Option<regex::Regex> {
    let file_part = glob.rsplit('/').next().unwrap_or(glob);
    if !file_part
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    let mut pattern = String::from("^");
    let mut rest = glob;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("**/") {
            pattern.push_str("(?:.*/)?");
            rest = after;
        } else if let Some(after) = rest.strip_prefix("**") {
            pattern.push_str(".*");
            rest = after;
        } else {
            match c {
                '*' => pattern.push_str("[^/]*"),
                '?' => pattern.push_str("[^/]"),
                _ => pattern.push_str(&regex::escape(&c.to_string())),
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    pattern.push('$');
    regex::Regex::new(&pattern).ok()
}

/// Launch references per directory, loaded on first use. A file counts as
/// launched when a manifest in one of its ancestor directories names it
/// relative to that directory, or a root CI workflow names it relative to the
/// root.
struct LaunchIndex<'a> {
    root: &'a Path,
    by_dir: HashMap<String, LaunchRefs>,
}

impl<'a> LaunchIndex<'a> {
    fn new(root: &'a Path) -> Self {
        let mut index = Self {
            root,
            by_dir: HashMap::new(),
        };
        let mut root_refs = index.load_dir("");
        if let Ok(entries) = std::fs::read_dir(root.join(".github/workflows")) {
            for entry in entries.flatten() {
                let path = entry.path();
                if matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("yml" | "yaml")
                ) && let Ok(text) = std::fs::read_to_string(&path)
                {
                    root_refs.add_text(&text);
                }
            }
        }
        index.by_dir.insert(String::new(), root_refs);
        index
    }

    fn load_dir(&self, dir: &str) -> LaunchRefs {
        let mut refs = LaunchRefs::default();
        let Ok(entries) = std::fs::read_dir(self.root.join(dir)) else {
            return refs;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if is_launch_manifest(name)
                && let Ok(text) = std::fs::read_to_string(entry.path())
            {
                refs.add_text(&text);
            }
        }
        refs
    }

    fn launches(&mut self, file: &str) -> bool {
        let mut dir = file;
        while let Some(cut) = dir.rfind('/') {
            dir = &dir[..cut];
            if !self.by_dir.contains_key(dir) {
                let refs = self.load_dir(dir);
                self.by_dir.insert(dir.to_owned(), refs);
            }
            if self.by_dir[dir].names(&file[cut + 1..]) {
                return true;
            }
        }
        self.by_dir[""].names(file)
    }
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

/// #F18: non-source config/data files (`.toml`/`.yml`/`.yaml`/`.json`) are
/// indexed as import-graph nodes but are never "imported" the way source
/// modules are, so a DB-backed graph (`build_import_graph` over every
/// indexed path) always reports them as orphans. They are configuration,
/// not dead code. The extension set mirrors the structured/config filetypes
/// the duplicate detector already treats specially
/// (`embedding::duplicates::structured_filetype_floor`).
pub(super) fn is_non_source_config_file(file: &str) -> bool {
    matches!(
        Path::new(file)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("toml" | "yml" | "yaml" | "json")
    )
}

/// Pass-1 verdict for a file: it is a dead-code candidate when nothing
/// imports it and it is neither a recognised entry point nor a non-source
/// config/data file (see [`is_non_source_config_file`]).
fn is_pass1_dead_file(file: &str, has_no_importers: bool) -> bool {
    has_no_importers && !is_entry_point_file(file) && !is_non_source_config_file(file)
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

/// `None` for the whole project; otherwise the scope as a relative path
/// without `./` or a trailing `/`.
fn normalize_scope(scope: Option<&str>) -> Option<&str> {
    let scope = scope?.trim();
    let scope = scope
        .strip_prefix("./")
        .unwrap_or(scope)
        .trim_end_matches('/');
    (!scope.is_empty() && scope != ".").then_some(scope)
}

/// Path-segment containment: `lib/donation` covers `lib/donation/x.ts` but not
/// `lib/donation-old/x.ts`.
fn in_scope(file: &str, scope: Option<&str>) -> bool {
    scope.is_none_or(|scope| {
        file.strip_prefix(scope)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// True when any directory on the path is hidden (`.github/`, `.harness/`):
/// tooling areas, ranked after project code so they do not fill a capped list.
fn is_under_hidden_dir(file: &str) -> bool {
    file.rsplit_once('/')
        .is_some_and(|(dir, _)| dir.split('/').any(|segment| segment.starts_with('.')))
}

/// Dead-code candidates under `scope` (`None` or `"."` = whole project).
///
/// The scope is applied before `max_results`: filtering a whole-project list
/// that was already cut to `max_results` dropped every scoped candidate that
/// sorted after the cut.
pub fn find_dead_code_v2(
    project: &ProjectRoot,
    scope: Option<&str>,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntryV2>> {
    let scope = normalize_scope(scope);
    let mut results: Vec<DeadCodeEntryV2> = Vec::new();

    // ── Pass 1: unreferenced files ────────────────────────────────────────────
    let graph = cache.get_or_build(project)?;
    let mut launches = LaunchIndex::new(project.as_path());
    let mut pass1_files: HashSet<String> = HashSet::new();
    for (file, node) in graph.iter() {
        if !in_scope(file, scope) || !is_pass1_dead_file(file, node.imported_by.is_empty()) {
            continue;
        }
        if launches.launches(file)
            || std::fs::read_to_string(project.as_path().join(file))
                .is_ok_and(|content| is_executable_script(&content))
        {
            continue;
        }
        pass1_files.insert(file.clone());
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

        if !in_scope(&relative, scope)
            || pass1_files.contains(&relative)
            || is_entry_point_file(&relative)
        {
            continue;
        }

        let source = std::fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = source.lines().collect();
        let test_regions = if relative.ends_with(".rs") {
            cfg_test_regions(&source)
        } else {
            Vec::new()
        };

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
            // The call graph drops calls to these names as builtin noise, so
            // `all_callees` can never contain them: a user `hash`/`id`/`read`
            // looked dead however often it was called (winnation 2026-10-27).
            if is_noise_callee(&func_name) {
                continue;
            }
            if func_line > 0 && has_decorator(&lines, func_line) {
                continue;
            }
            if !test_regions.is_empty()
                && defined_only_in_cfg_test(&source, &test_regions, &func_name)
            {
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
            .then(is_under_hidden_dir(&a.file).cmp(&is_under_hidden_dir(&b.file)))
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
    fn pass1_excludes_non_source_config_files() {
        // F18: config/data files (.toml/.yml/.yaml/.json) are indexed as
        // import-graph nodes but are never "imported" the way source modules
        // are, so a DB-backed graph reports them as orphans on every run.
        // They are configuration, not dead code — Pass 1 must not flag them.
        for config in [
            ".cargo/config.toml",
            ".cargo/audit.toml",
            ".codex/agents/reviewer.toml",
            "config/app.yml",
            "config/app.yaml",
            "tsconfig.json",
        ] {
            assert!(
                !is_pass1_dead_file(config, true),
                "{config:?} is config, not dead code"
            );
        }
        // A genuinely unreferenced *source* file is still reported.
        assert!(is_pass1_dead_file("src/orphan.rs", true));
        assert!(is_pass1_dead_file("app/unused.py", true));
        // Entry points and imported files remain excluded.
        assert!(!is_pass1_dead_file("src/lib.rs", true));
        assert!(!is_pass1_dead_file("src/used.rs", false));
    }

    #[test]
    fn scope_is_a_path_segment_prefix() {
        for whole in [None, Some("."), Some("./"), Some(""), Some("  ")] {
            assert_eq!(normalize_scope(whole), None, "{whole:?}");
        }
        assert_eq!(
            normalize_scope(Some("./lib/donation/")),
            Some("lib/donation")
        );
        assert!(in_scope("lib/donation/queue.ts", Some("lib/donation")));
        assert!(in_scope("lib/donation", Some("lib/donation")));
        assert!(!in_scope("lib/donation-old/queue.ts", Some("lib/donation")));
        assert!(!in_scope("app/lib/donation/queue.ts", Some("lib/donation")));
        assert!(in_scope("anything.ts", None));
    }

    // winnation 2026-10-27: 1,482 of 1,740 "no importers" files were tests and
    // 118 were Next.js route files — loaded by the runner or framework.
    #[test]
    fn runner_and_framework_files_are_entry_points() {
        for file in [
            "lib/donation/queue.test.ts",
            "components/Panel.spec.tsx",
            "tests/e2e/public-layout.ts",
            "src/__tests__/helpers.js",
            "crates/x/tests/integration.rs",
            "crates/x/benches/indexing.rs",
            "pkg/server_test.go",
            "app/test_models.py",
            "app/models_test.py",
            "conftest.py",
            "app/(site)/page.tsx",
            "app/api/cron/expire-charges/route.ts",
            "src/app/layout.tsx",
            "apps/web/app/opengraph-image.tsx",
            "middleware.ts",
            "src/proxy.ts",
            "instrumentation.ts",
            "vitest.config.ts",
            "next.config.mjs",
            ".eslintrc.cjs",
        ] {
            assert!(
                is_entry_point_file(file),
                "{file:?} should be an entry point"
            );
        }
        for file in [
            "lib/donation/queue.ts",
            "lib/page.tsx",
            "lib/middleware.ts",
            "app/unused.py",
            "app/(site)/page-header.tsx",
            "lib/testing.ts",
            "lib/contest.py",
        ] {
            assert!(!is_entry_point_file(file), "{file:?} is ordinary source");
        }
    }

    #[test]
    fn executable_scripts_are_entry_points() {
        assert!(is_executable_script("#!/usr/bin/env node\nmain();\n"));
        assert!(is_executable_script(
            "def main():\n    pass\n\nif __name__ == \"__main__\":\n    main()\n"
        ));
        assert!(!is_executable_script("export const x = 1;\n"));
        assert!(!is_executable_script("# if __name__ in docs\nx = 1\n"));
    }

    #[test]
    fn launch_index_reads_scripts_workflows_and_tool_configs() {
        let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-dead-launch-");
        let write = |rel: &str, text: &str| {
            let path = dir.join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        };
        write(
            "package.json",
            r#"{"scripts": {"test:bridge:db": "node scripts/test-unified-bridge-db.mjs", "lint": "eslint \"**/*.ts\""}}"#,
        );
        write(
            ".github/workflows/db.yml",
            "on:\n  push:\n    paths:\n      - \"lib/**\"\njobs:\n  t:\n    steps:\n      - run: for f in scripts/test-playback-*-db.mjs; do node \"$f\"; done\n",
        );
        write(
            "vitest.config.ts",
            "export default { test: { setupFiles: [\"./vitest.setup.network.ts\"] } };\n",
        );
        write(
            "workers/dispatcher/package.json",
            r#"{"scripts": {"start": "node src/main.ts"}}"#,
        );

        let mut index = LaunchIndex::new(&dir);
        for launched in [
            "scripts/test-unified-bridge-db.mjs",
            "scripts/test-playback-handoff-db.mjs",
            "vitest.setup.network.ts",
            "workers/dispatcher/src/main.ts",
        ] {
            assert!(index.launches(launched), "{launched:?} should be launched");
        }
        // `lib/**` (a trigger filter) and `**/*.ts` (a lint target) match every
        // file; honouring them would hide all dead code.
        for not_launched in [
            "lib/orphan.ts",
            "scripts/test-panda-db.mjs",
            "src/main.ts",
            "scripts/test-playback-handoff-db.mjs.bak",
        ] {
            assert!(
                !index.launches(not_launched),
                "{not_launched:?} should not be launched"
            );
        }
    }

    // winnation 2026-10-27: `dead_code_report {"path": "lib/donation"}` filtered
    // a whole-project list already cut to 20 entries, all under `.github/` and
    // `.harness/`, so no scoped candidate could appear.
    #[test]
    fn scope_applies_before_the_result_cap() {
        let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-dead-scope-");
        std::fs::create_dir_all(dir.join("aaa")).expect("mkdir aaa");
        std::fs::create_dir_all(dir.join("zzz")).expect("mkdir zzz");
        std::fs::create_dir_all(dir.join(".harness")).expect("mkdir .harness");
        for i in 0..30 {
            std::fs::write(
                dir.join(format!("aaa/orphan{i:02}.ts")),
                "export const x = 1;\n",
            )
            .expect("write aaa orphan");
        }
        std::fs::write(dir.join(".harness/probe.ts"), "export const p = 1;\n")
            .expect("write hidden orphan");
        std::fs::write(dir.join("zzz/orphan.ts"), "export const z = 1;\n")
            .expect("write zzz orphan");
        std::fs::write(dir.join("zzz/orphan.test.ts"), "import './other';\n")
            .expect("write test file");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let scoped = find_dead_code_v2(&project, Some("zzz/"), 5, &cache).expect("scoped");
        let files: Vec<&str> = scoped.iter().map(|e| e.file.as_str()).collect();
        assert!(files.contains(&"zzz/orphan.ts"), "{files:?}");
        assert!(files.iter().all(|f| f.starts_with("zzz/")), "{files:?}");
        assert!(!files.contains(&"zzz/orphan.test.ts"), "{files:?}");

        let whole = find_dead_code_v2(&project, None, 0, &cache).expect("whole");
        let pass1: Vec<&str> = whole
            .iter()
            .filter(|e| e.pass == 1)
            .map(|e| e.file.as_str())
            .collect();
        assert_eq!(pass1.last(), Some(&".harness/probe.ts"), "{pass1:?}");
    }

    #[test]
    fn helpers_named_like_builtins_are_not_judged() {
        let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-dead-noise-");
        std::fs::create_dir_all(dir.join("lib")).expect("mkdir lib");
        std::fs::write(
            dir.join("lib/util.js"),
            "export function hash(value) {\n  return String(value).length;\n}\n\nexport function orphanHelper() {\n  return hash(1);\n}\n",
        )
        .expect("write util");
        std::fs::write(
            dir.join("lib/user.js"),
            "import { hash } from './util';\nexport const h = hash(2);\n",
        )
        .expect("write user");

        let project = ProjectRoot::new(&dir).expect("project");
        let found = find_dead_code_v2(&project, Some("lib/util.js"), 0, &GraphCache::new(0))
            .expect("dead code");
        let symbols: Vec<&str> = found.iter().filter_map(|e| e.symbol.as_deref()).collect();
        assert!(symbols.contains(&"orphanHelper"), "{found:?}");
        assert!(!symbols.contains(&"hash"), "{found:?}");
    }

    #[test]
    fn rust_tests_are_not_dead_code() {
        for file in [
            "src/db/tests.rs",
            "src/cli/doctor_tests.rs",
            "src/lsp/pool_test.rs",
        ] {
            assert!(is_entry_point_file(file), "{file:?} is a test module file");
        }
        assert!(!is_entry_point_file("src/contests.rs"));

        let source = "pub fn helper() {}\n\n#[cfg(test)]\nmod tests {\n    fn make() {}\n\n    #[test]\n    fn helper_runs() { make(); }\n}\n";
        let regions = cfg_test_regions(source);
        assert!(defined_only_in_cfg_test(source, &regions, "helper_runs"));
        assert!(defined_only_in_cfg_test(source, &regions, "make"));
        assert!(!defined_only_in_cfg_test(source, &regions, "helper"));
        assert!(!defined_only_in_cfg_test(source, &regions, "absent"));
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
