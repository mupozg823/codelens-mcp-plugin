use super::{find_symbol, get_symbols_overview, SymbolIndex, SymbolKind};
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
