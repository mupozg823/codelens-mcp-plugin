use super::{find_symbol, get_symbols_overview, SymbolIndex, SymbolKind, SymbolProvenance};
use crate::ProjectRoot;
use std::fs;

#[test]
fn extracts_python_symbols() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let symbols = get_symbols_overview(&project, "src/service.py", 2).expect("symbols");
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0].name, "Service");
    assert_eq!(symbols[0].kind, SymbolKind::Class);
    assert_eq!(symbols[0].children[0].name, "run");
}

#[test]
fn finds_typescript_symbol_with_body() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let matches = find_symbol(&project, "fetchUser", None, true, true, 10).expect("find symbol");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].kind, SymbolKind::Function);
    assert!(matches[0]
        .body
        .as_ref()
        .expect("body")
        .contains("return userId"));
}

#[test]
fn index_refreshes_after_file_change() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let index = SymbolIndex::new_memory(project.clone());

    let initial = index
        .find_symbol("fetchUser", None, false, true, 10)
        .expect("initial symbol lookup");
    assert_eq!(initial.len(), 1);

    fs::write(
        root.join("src/user.ts"),
        "export function loadUser(userId: string) {\n  return userId\n}\n",
    )
    .expect("rewrite ts");

    let refreshed = index
        .find_symbol("loadUser", None, true, true, 10)
        .expect("refreshed symbol lookup");
    assert_eq!(refreshed.len(), 1);
    assert!(refreshed[0]
        .body
        .as_ref()
        .expect("body")
        .contains("loadUser"));
}

#[test]
fn refresh_all_populates_stats() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let index = SymbolIndex::new_memory(project);
    let stats = index.refresh_all().expect("refresh all");
    assert_eq!(stats.supported_files, 2);
    assert_eq!(stats.indexed_files, 2);
    assert_eq!(stats.stale_files, 0);
}

#[test]
fn reloads_index_from_disk() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let index = SymbolIndex::new(project.clone());
    index.refresh_all().expect("refresh all");

    let reloaded = SymbolIndex::new(project);
    let stats = reloaded.stats().expect("stats");
    assert_eq!(stats.indexed_files, 2);
}

#[test]
fn ranked_context_prefers_exact_matches_and_respects_budget() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let index = SymbolIndex::new_memory(project);

    let ranked = index
        .get_ranked_context("fetchUser", None, 40, true, 2)
        .expect("ranked context");

    assert_eq!(ranked.query, "fetchUser");
    assert_eq!(ranked.token_budget, 40);
    assert!(!ranked.symbols.is_empty());
    assert_eq!(ranked.symbols[0].name, "fetchUser");
    assert_eq!(ranked.symbols[0].relevance_score, 100);
    assert!(ranked.symbols[0]
        .body
        .as_ref()
        .expect("body")
        .contains("fetchUser"));
    assert!(ranked.chars_used <= ranked.token_budget * 4);
}

#[test]
fn extracts_go_symbols() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-go-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("main.go"),
        "package main\n\ntype Server struct{}\n\nfunc NewServer() *Server { return &Server{} }\n\nfunc (s *Server) Run() {}\n",
    )
    .expect("write go");
    let project = ProjectRoot::new(&dir).expect("project");
    let symbols = get_symbols_overview(&project, "main.go", 1).expect("symbols");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Server"),
        "expected Server type, got {names:?}"
    );
    assert!(
        names.contains(&"NewServer"),
        "expected NewServer func, got {names:?}"
    );
}

#[test]
fn extracts_java_symbols() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-java-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("Service.java"),
        "public class Service {\n    public Service() {}\n    public void run() {}\n}\n",
    )
    .expect("write java");
    let project = ProjectRoot::new(&dir).expect("project");
    let symbols = get_symbols_overview(&project, "Service.java", 2).expect("symbols");
    assert!(!symbols.is_empty(), "expected symbols in Service.java");
    assert_eq!(symbols[0].name, "Service");
    assert_eq!(symbols[0].kind, SymbolKind::Class);
}

#[test]
fn extracts_kotlin_symbols() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-kotlin-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("Main.kt"),
        "class Main {\n    fun greet(name: String): String = \"Hello $name\"\n}\n\nfun main() {}\n",
    )
    .expect("write kotlin");
    let project = ProjectRoot::new(&dir).expect("project");
    let symbols = get_symbols_overview(&project, "Main.kt", 1).expect("symbols");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Main"),
        "expected Main class, got {names:?}"
    );
}

#[test]
fn extracts_rust_symbols() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-rust-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("lib.rs"),
        "pub struct Config { pub name: String }\n\npub trait Handler {\n    fn handle(&self);\n}\n\npub fn run() {}\n",
    )
    .expect("write rust");
    let project = ProjectRoot::new(&dir).expect("project");
    let symbols = get_symbols_overview(&project, "lib.rs", 1).expect("symbols");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"Config"),
        "expected Config struct, got {names:?}"
    );
    assert!(
        names.contains(&"Handler"),
        "expected Handler trait, got {names:?}"
    );
    assert!(names.contains(&"run"), "expected run fn, got {names:?}");
}

#[test]
fn make_symbol_id_format() {
    use super::make_symbol_id;
    assert_eq!(
        make_symbol_id("src/service.py", &SymbolKind::Class, "Service"),
        "src/service.py#class:Service"
    );
    assert_eq!(
        make_symbol_id("src/service.py", &SymbolKind::Method, "Service/run"),
        "src/service.py#method:Service/run"
    );
}

#[test]
fn parse_symbol_id_valid() {
    use super::parse_symbol_id;
    let result = parse_symbol_id("src/service.py#function:Service/run");
    assert_eq!(result, Some(("src/service.py", "function", "Service/run")));
}

#[test]
fn parse_symbol_id_plain_name_returns_none() {
    use super::parse_symbol_id;
    assert_eq!(parse_symbol_id("fetchUser"), None);
    assert_eq!(parse_symbol_id("#class:"), None);
    assert_eq!(parse_symbol_id(""), None);
}

#[test]
fn find_symbol_returns_id_field() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let matches = find_symbol(&project, "fetchUser", None, false, true, 10).expect("find symbol");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "src/user.ts#function:fetchUser");
}

#[test]
fn find_symbol_by_stable_id() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let matches = find_symbol(
        &project,
        "src/user.ts#function:fetchUser",
        None,
        true,
        true,
        10,
    )
    .expect("find by id");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "fetchUser");
    assert_eq!(matches[0].kind, SymbolKind::Function);
    assert!(matches[0].body.is_some());
}

#[test]
fn find_symbol_by_nested_id() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let matches = find_symbol(
        &project,
        "src/service.py#function:Service/run",
        None,
        false,
        true,
        10,
    )
    .expect("find nested by id");
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "run");
    assert_eq!(matches[0].name_path, "Service/run");
}

#[test]
fn get_symbols_overview_includes_id() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let symbols = get_symbols_overview(&project, "src/service.py", 2).expect("symbols");
    assert!(!symbols[0].id.is_empty());
    assert!(symbols[0].id.contains("#class:"));
    let child = &symbols[0].children[0];
    assert!(child.id.contains("#method:Service/run") || child.id.contains("#function:Service/run"));
}

#[test]
fn cached_directory_children_preserve_file_identity_and_provenance() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let index = SymbolIndex::new_memory(project);
    index.refresh_all().expect("refresh all");

    let symbols = index
        .get_symbols_overview_cached("src", 2)
        .expect("cached directory overview");
    let service_file = symbols
        .iter()
        .find(|symbol| symbol.file_path == "src/service.py")
        .expect("service.py file node");
    let run = service_file
        .children
        .iter()
        .find(|child| child.name == "run")
        .expect("run child");

    assert_eq!(run.file_path, "src/service.py");
    assert!(
        run.id.starts_with("src/service.py#"),
        "child id should include file path, got {}",
        run.id
    );
    assert_eq!(run.provenance, SymbolProvenance::EngineCore);
}

#[test]
fn extracts_csharp_symbols() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-csharp-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("Service.cs"),
        "namespace MyApp {\n    public class Service {\n        public Service() {}\n        public void Run() {}\n    }\n    public interface IService {}\n    public enum Status { Active, Inactive }\n}\n",
    )
    .expect("write cs");
    let project = ProjectRoot::new(&dir).expect("project");
    let symbols = get_symbols_overview(&project, "Service.cs", 2).expect("symbols");
    let names: Vec<&str> = symbols
        .iter()
        .flat_map(|s| {
            let mut v = vec![s.name.as_str()];
            v.extend(s.children.iter().map(|c| c.name.as_str()));
            v
        })
        .collect();
    assert!(
        names.contains(&"MyApp"),
        "expected namespace MyApp, got {names:?}"
    );
    assert!(
        names.contains(&"Service"),
        "expected class Service, got {names:?}"
    );
    assert!(
        names.contains(&"IService"),
        "expected interface IService, got {names:?}"
    );
    assert!(
        names.contains(&"Status"),
        "expected enum Status, got {names:?}"
    );
}

#[test]
fn extracts_dart_symbols() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-dart-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("main.dart"),
        "class UserService {\n  void fetchUser() {}\n}\n\nenum Role { admin, user }\n\nvoid main() {}\n",
    )
    .expect("write dart");
    let project = ProjectRoot::new(&dir).expect("project");
    let symbols = get_symbols_overview(&project, "main.dart", 2).expect("symbols");
    let names: Vec<&str> = symbols
        .iter()
        .flat_map(|s| {
            let mut v = vec![s.name.as_str()];
            v.extend(s.children.iter().map(|c| c.name.as_str()));
            v
        })
        .collect();
    assert!(
        names.contains(&"UserService"),
        "expected class UserService, got {names:?}"
    );
    assert!(names.contains(&"Role"), "expected enum Role, got {names:?}");
    assert!(
        names.contains(&"main"),
        "expected function main, got {names:?}"
    );
}

#[test]
fn prune_to_budget_respects_char_limit() {
    use super::ranking::prune_to_budget;
    use super::types::{SymbolInfo, SymbolProvenance};

    let symbols: Vec<(SymbolInfo, i32)> = (0..20)
        .map(|i| {
            (
                SymbolInfo {
                    name: format!("sym_{i}"),
                    kind: SymbolKind::Function,
                    file_path: "test.rs".into(),
                    line: i,
                    column: 0,
                    signature: format!("fn sym_{i}()"),
                    name_path: format!("sym_{i}"),
                    id: format!("test.rs#function:sym_{i}"),
                    body: None,
                    children: Vec::new(),
                    start_byte: 0,
                    end_byte: 0,
                    provenance: SymbolProvenance::default(),
                },
                100 - i as i32,
            )
        })
        .collect();

    // Very small budget: should not fit all 20 symbols
    let (selected, chars_used, _pruned_count, _last_kept_score) =
        prune_to_budget(symbols, 50, false, std::path::Path::new("/nonexistent"));
    assert!(!selected.is_empty());
    assert!(selected.len() < 20, "budget should limit entries");
    assert!(chars_used <= 50 * 4);
}

#[test]
fn prune_to_budget_includes_first_even_if_oversized() {
    use super::ranking::prune_to_budget;
    use super::types::{SymbolInfo, SymbolProvenance};

    let symbols = vec![(
        SymbolInfo {
            name: "big_symbol".into(),
            kind: SymbolKind::Function,
            file_path: "test.rs".into(),
            line: 1,
            column: 0,
            signature: "fn big_symbol()".into(),
            name_path: "big_symbol".into(),
            id: "test.rs#function:big_symbol".into(),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        },
        100,
    )];

    // Budget of 1 token = 4 chars, way too small for even the JSON entry
    let (selected, chars_used, _pruned_count, _last_kept_score) =
        prune_to_budget(symbols, 1, false, std::path::Path::new("/nonexistent"));
    assert_eq!(selected.len(), 1, "first entry must always be included");
    // chars_used is capped at char_budget (max_tokens * 4 = 4), even though
    // the serialized entry is larger. The key invariant: first entry always included.
    assert!(chars_used > 0);
}

#[test]
fn prune_to_budget_reports_dropped_count_and_last_kept_score() {
    use super::ranking::prune_to_budget;
    use super::types::{SymbolInfo, SymbolProvenance};

    let symbols: Vec<(SymbolInfo, i32)> = (0..5)
        .map(|i| {
            (
                SymbolInfo {
                    name: format!("sym_{i}"),
                    kind: SymbolKind::Function,
                    file_path: "a.rs".into(),
                    line: i,
                    column: 0,
                    signature: format!("fn sym_{i}()"),
                    name_path: format!("sym_{i}"),
                    id: format!("a.rs#function:sym_{i}"),
                    body: None,
                    children: Vec::new(),
                    start_byte: 0,
                    end_byte: 0,
                    provenance: SymbolProvenance::default(),
                },
                100 - (i as i32) * 10,
            )
        })
        .collect();

    // Budget too tight to fit all five — expect a drop.
    let (kept, _chars_used, pruned_count, last_kept_score) =
        prune_to_budget(symbols, 50, false, std::path::Path::new("/tmp"));
    assert!(
        pruned_count + kept.len() == 5,
        "{pruned_count} + {} != 5",
        kept.len()
    );
    assert!(pruned_count > 0, "budget 50 should not fit all 5");
    let last_expected = kept.last().map(|e| e.relevance_score as f64).unwrap_or(0.0);
    assert_eq!(last_kept_score, last_expected);
}

#[test]
fn ranked_context_with_lsp_boost_promotes_boost_files() {
    // Caller-level wiring: when an LSP `textDocument/references` probe
    // produces a set of file paths, feeding those paths through the
    // `get_ranked_context_cached_with_lsp_boost` method must re-rank a
    // symbol that lives in one of those files above an otherwise
    // identical candidate in an unrelated file. A large weight pins
    // down the direction of the boost regardless of other ranking
    // signals the real pipeline may contribute.
    use std::collections::{HashMap, HashSet};

    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-boost-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(dir.join("src/a.rs"), "pub fn handler() -> u32 { 1 }\n").expect("write a.rs");
    fs::write(dir.join("src/b.rs"), "pub fn handler() -> u32 { 2 }\n").expect("write b.rs");

    let project = ProjectRoot::new(&dir).expect("project");
    let index = SymbolIndex::new_memory(project);
    index.refresh_all().expect("refresh index");

    // Baseline: without boost, either file may win — we only need an
    // honest "before" to prove the boost flips direction, so capture it.
    let baseline = index
        .get_ranked_context_cached("handler", None, 400, false, 2, None, HashMap::new())
        .expect("baseline ranked");
    assert!(
        baseline.symbols.len() >= 2,
        "fixture should produce two candidates, got {}",
        baseline.symbols.len()
    );
    let baseline_first = baseline.symbols[0].file.clone();
    let other_file = if baseline_first == "src/a.rs" {
        "src/b.rs"
    } else {
        "src/a.rs"
    };

    // Boost the OTHER file via LSP boost — high weight forces the
    // boost signal to dominate, so the loser of the baseline must
    // become the winner.
    let mut boost: HashSet<String> = HashSet::new();
    boost.insert(other_file.to_string());

    let ranked = index
        .get_ranked_context_cached_with_lsp_boost(
            "handler",
            None,
            400,
            false,
            2,
            None,
            HashMap::new(),
            None,
            boost,
            Some(10.0),
        )
        .expect("ranked with lsp boost");

    assert!(!ranked.symbols.is_empty(), "expected at least one symbol");
    assert_eq!(
        ranked.symbols[0].file, other_file,
        "LSP-boosted file must rank first when weight is positive"
    );
}

#[test]
fn ranked_context_without_lsp_boost_is_neutral() {
    // Passing an empty boost set (or None weight) must leave the result
    // indistinguishable from the legacy `get_ranked_context_cached`
    // entrypoint, preserving the "opt-in, default no-op" contract.
    use std::collections::{HashMap, HashSet};

    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let index = SymbolIndex::new_memory(project);
    index.refresh_all().expect("refresh index");

    let baseline = index
        .get_ranked_context_cached("fetchUser", None, 200, false, 2, None, HashMap::new())
        .expect("baseline");
    let via_boost = index
        .get_ranked_context_cached_with_lsp_boost(
            "fetchUser",
            None,
            200,
            false,
            2,
            None,
            HashMap::new(),
            None,
            HashSet::new(),
            None,
        )
        .expect("via lsp boost path");

    let baseline_keys: Vec<_> = baseline
        .symbols
        .iter()
        .map(|e| (e.file.clone(), e.name.clone(), e.relevance_score))
        .collect();
    let via_keys: Vec<_> = via_boost
        .symbols
        .iter()
        .map(|e| (e.file.clone(), e.name.clone(), e.relevance_score))
        .collect();
    assert_eq!(baseline_keys, via_keys, "empty boost must be a no-op");
}

#[test]
fn rank_symbols_returns_full_scored_list() {
    use super::ranking::{rank_symbols, RankingContext};
    use super::types::{SymbolInfo, SymbolProvenance};

    let symbols: Vec<SymbolInfo> = ["alpha", "beta_alpha", "gamma"]
        .iter()
        .map(|name| SymbolInfo {
            name: name.to_string(),
            kind: SymbolKind::Function,
            file_path: "test.rs".into(),
            line: 1,
            column: 0,
            signature: format!("fn {name}()"),
            name_path: name.to_string(),
            id: format!("test.rs#function:{name}"),
            body: None,
            children: Vec::new(),
            start_byte: 0,
            end_byte: 0,
            provenance: SymbolProvenance::default(),
        })
        .collect();

    let ctx = RankingContext::text_only();
    let scored = rank_symbols("alpha", symbols, &ctx);
    // "alpha" and "beta_alpha" match; "gamma" does not
    assert_eq!(scored.len(), 2);
    // Exact match should score higher
    assert_eq!(scored[0].0.name, "alpha");
    assert!(scored[0].1 >= scored[1].1);
}

#[test]
fn score_and_rank_empty_query() {
    use super::ranking::{rank_symbols, RankingContext};
    use super::types::{SymbolInfo, SymbolProvenance};

    let symbols = vec![SymbolInfo {
        name: "anything".into(),
        kind: SymbolKind::Function,
        file_path: "test.rs".into(),
        line: 1,
        column: 0,
        signature: "fn anything()".into(),
        name_path: "anything".into(),
        id: "test.rs#function:anything".into(),
        body: None,
        children: Vec::new(),
        start_byte: 0,
        end_byte: 0,
        provenance: SymbolProvenance::default(),
    }];

    let ctx = RankingContext::text_only();
    let scored = rank_symbols("", symbols, &ctx);
    // Empty string is a substring of any name, so score_symbol returns Some(60).
    // rank_symbols passes all symbols that score_symbol accepts.
    assert_eq!(scored.len(), 1);
    assert!(scored[0].1 > 0);
}

fn fixture_root() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-symbols-fixture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("src")).expect("create src");
    fs::write(
        dir.join("src/service.py"),
        "class Service:\n    def run(self):\n        return True\n\nvalue = 1\n",
    )
    .expect("write python");
    fs::write(
        dir.join("src/user.ts"),
        "export interface User { id: string }\nexport function fetchUser(userId: string) {\n  return userId\n}\n",
    )
    .expect("write ts");
    dir
}
