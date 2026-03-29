mod dead_code;
mod parsers;
mod resolvers;

use crate::db::{index_db_path, IndexDb};
use crate::project::{collect_files, ProjectRoot};
use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ── Re-exports ───────────────────────────────────────────────────────────────

pub use dead_code::{find_dead_code, find_dead_code_v2, DeadCodeEntryV2};
pub use parsers::extract_imports_for_file;
pub use resolvers::resolve_module_for_file;

/// Use lang_registry as the single source of truth for supported extensions.
pub fn is_import_supported(ext: &str) -> bool {
    crate::lang_registry::supports_imports(ext)
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlastRadiusEntry {
    pub file: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImporterEntry {
    pub file: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportanceEntry {
    pub file: String,
    pub score: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntry {
    pub file: String,
    pub symbol: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub(crate) imports: HashSet<String>,
    pub(crate) imported_by: HashSet<String>,
}

// ── GraphCache ───────────────────────────────────────────────────────────────

pub struct GraphCache {
    inner: Mutex<GraphCacheInner>,
    /// Monotonically increasing counter -- bumped on every invalidation.
    generation: AtomicU64,
}

struct GraphCacheInner {
    graph: Option<Arc<HashMap<String, FileNode>>>,
    /// Generation at which this cache entry was built.
    built_generation: u64,
}

impl GraphCache {
    /// Create a new cache.  The `_ttl_secs` parameter is kept for API
    /// compatibility but no longer used -- invalidation is generation-based.
    pub fn new(_ttl_secs: u64) -> Self {
        Self {
            inner: Mutex::new(GraphCacheInner {
                graph: None,
                built_generation: 0,
            }),
            generation: AtomicU64::new(1), // start at 1 so default 0 is always stale
        }
    }

    pub fn get_or_build(&self, project: &ProjectRoot) -> Result<Arc<HashMap<String, FileNode>>> {
        let current_gen = self.generation.load(Ordering::Acquire);
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("graph cache lock poisoned"))?;
        if let Some(graph) = &inner.graph {
            if inner.built_generation == current_gen {
                return Ok(Arc::clone(graph));
            }
        }
        let graph = Arc::new(build_graph(project)?);
        inner.graph = Some(Arc::clone(&graph));
        inner.built_generation = current_gen;
        Ok(graph)
    }

    /// Return per-file PageRank scores from the cached graph.
    pub fn file_pagerank_scores(&self, project: &ProjectRoot) -> HashMap<String, f64> {
        let graph = match self.get_or_build(project) {
            Ok(g) => g,
            Err(_) => return HashMap::new(),
        };
        compute_pagerank(&graph)
    }

    /// Bump the generation counter, causing the next `get_or_build` to rebuild.
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::Release);
    }

    /// Current generation (for diagnostics / testing).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }
}

// ── Public API functions ─────────────────────────────────────────────────────

pub fn supports_import_graph(file_path: &str) -> bool {
    crate::lang_registry::supports_imports_for_path(Path::new(file_path))
}

pub fn get_blast_radius(
    project: &ProjectRoot,
    file_path: &str,
    max_depth: usize,
    cache: &GraphCache,
) -> Result<Vec<BlastRadiusEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = cache.get_or_build(project)?;
    let target = normalize_key(file_path);
    let mut result = HashMap::new();
    let mut queue = VecDeque::from([(target.clone(), 0usize)]);

    while let Some((current, depth)) = queue.pop_front() {
        if depth > max_depth || result.contains_key(&current) {
            continue;
        }
        if current != target {
            result.insert(current.clone(), depth);
        }

        let Some(node) = graph.get(&current) else {
            continue;
        };
        for importer in &node.imported_by {
            if !result.contains_key(importer) {
                queue.push_back((importer.clone(), depth + 1));
            }
        }
    }

    let mut entries: Vec<_> = result
        .into_iter()
        .map(|(file, depth)| BlastRadiusEntry { file, depth })
        .collect();
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.file.cmp(&b.file)));
    Ok(entries)
}

pub fn get_importers(
    project: &ProjectRoot,
    file_path: &str,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<ImporterEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = cache.get_or_build(project)?;
    let target = normalize_key(file_path);
    let importers = graph
        .get(&target)
        .map(|node| {
            let mut entries = node
                .imported_by
                .iter()
                .cloned()
                .map(|file| ImporterEntry { file })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.file.cmp(&b.file));
            if max_results > 0 && entries.len() > max_results {
                entries.truncate(max_results);
            }
            entries
        })
        .unwrap_or_default();
    Ok(importers)
}

/// PageRank over the import graph (damping=0.85, 20 iterations).
fn compute_pagerank(graph: &HashMap<String, FileNode>) -> HashMap<String, f64> {
    if graph.is_empty() {
        return HashMap::new();
    }
    let damping = 0.85;
    let n = graph.len() as f64;
    let mut scores: HashMap<String, f64> = graph.keys().cloned().map(|k| (k, 1.0 / n)).collect();
    let out_degree: HashMap<&str, usize> = graph
        .iter()
        .map(|(k, node)| (k.as_str(), node.imports.len()))
        .collect();
    for _ in 0..20 {
        let mut next: HashMap<String, f64> = HashMap::new();
        for (key, node) in graph.iter() {
            let mut incoming = 0.0;
            for importer in &node.imported_by {
                let importer_score = scores.get(importer).copied().unwrap_or(0.0);
                let degree = out_degree
                    .get(importer.as_str())
                    .copied()
                    .unwrap_or(1)
                    .max(1) as f64;
                incoming += importer_score / degree;
            }
            next.insert(key.clone(), (1.0 - damping) / n + damping * incoming);
        }
        scores = next;
    }
    scores
}

pub fn get_importance(
    project: &ProjectRoot,
    top_n: usize,
    cache: &GraphCache,
) -> Result<Vec<ImportanceEntry>> {
    let graph = cache.get_or_build(project)?;
    let scores = compute_pagerank(&graph);

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut entries: Vec<_> = ranked
        .into_iter()
        .map(|(file, score)| ImportanceEntry {
            file,
            score: format!("{score:.4}"),
        })
        .collect();
    if top_n > 0 && entries.len() > top_n {
        entries.truncate(top_n);
    }
    Ok(entries)
}

/// Public accessor for the import graph, used by sibling modules (e.g. circular).
pub(crate) fn build_graph_pub(
    project: &ProjectRoot,
    cache: &GraphCache,
) -> Result<Arc<HashMap<String, FileNode>>> {
    cache.get_or_build(project)
}

// ── Graph building (internal) ────────────────────────────────────────────────

fn build_graph(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    // Try to load from SQLite first
    let db_path = index_db_path(project.as_path());
    if db_path.is_file() {
        if let Ok(db) = IndexDb::open(&db_path) {
            if db.file_count()? > 0 {
                return build_graph_from_db(&db);
            }
        }
    }

    // Fallback: scan files directly
    build_graph_from_files(project)
}

fn build_graph_from_db(db: &IndexDb) -> Result<HashMap<String, FileNode>> {
    let db_graph = db.build_import_graph()?;
    let mut graph = HashMap::new();
    for (path, (imports, imported_by)) in db_graph {
        graph.insert(
            path,
            FileNode {
                imports: imports.into_iter().collect(),
                imported_by: imported_by.into_iter().collect(),
            },
        );
    }
    Ok(graph)
}

fn build_graph_from_files(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    let files = collect_candidate_files(project.as_path())?;
    let mut graph = HashMap::new();

    for file in &files {
        let rel = project.to_relative(file);
        let imports = parsers::extract_imports(file)
            .into_iter()
            .filter_map(|module| resolvers::resolve_module(project, file, &module))
            .collect::<HashSet<_>>();
        graph.insert(
            rel.clone(),
            FileNode {
                imports,
                imported_by: HashSet::new(),
            },
        );
    }

    let edges: Vec<(String, String)> = graph
        .iter()
        .flat_map(|(from_file, node)| {
            node.imports
                .iter()
                .cloned()
                .map(|to_file| (from_file.clone(), to_file))
                .collect::<Vec<_>>()
        })
        .collect();

    for (from_file, to_file) in edges {
        if let Some(node) = graph.get_mut(&to_file) {
            node.imported_by.insert(from_file);
        }
    }

    Ok(graph)
}

fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| {
        crate::lang_registry::supports_imports_for_path(path)
    })
}

fn normalize_key(file_path: &str) -> String {
    file_path.replace('\\', "/")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{
        find_dead_code, get_blast_radius, get_importance, get_importers, supports_import_graph,
        GraphCache,
    };
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn calculates_python_blast_radius() {
        let dir = temp_project_dir("python");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let radius = get_blast_radius(&project, "models.py", 3, &cache).expect("blast radius");
        assert_eq!(
            radius,
            vec![
                super::BlastRadiusEntry {
                    file: "utils.py".to_owned(),
                    depth: 1,
                },
                super::BlastRadiusEntry {
                    file: "main.py".to_owned(),
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn calculates_typescript_blast_radius() {
        let dir = temp_project_dir("typescript");
        fs::create_dir_all(dir.join("lib")).expect("mkdir");
        fs::write(
            dir.join("app.ts"),
            "import { greet } from './lib/greet'\nconsole.log(greet())\n",
        )
        .expect("write app");
        fs::write(
            dir.join("lib/greet.ts"),
            "import { User } from './user'\nexport const greet = () => new User()\n",
        )
        .expect("write greet");
        fs::write(dir.join("lib/user.ts"), "export class User {}\n").expect("write user");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let radius = get_blast_radius(&project, "lib/user.ts", 3, &cache).expect("blast radius");
        assert_eq!(
            radius,
            vec![
                super::BlastRadiusEntry {
                    file: "lib/greet.ts".to_owned(),
                    depth: 1,
                },
                super::BlastRadiusEntry {
                    file: "app.ts".to_owned(),
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn reports_supported_extensions() {
        assert!(supports_import_graph("main.py"));
        assert!(supports_import_graph("main.ts"));
        assert!(supports_import_graph("Main.java"));
        assert!(supports_import_graph("main.go"));
        assert!(supports_import_graph("main.kt"));
        assert!(supports_import_graph("main.rs"));
        assert!(supports_import_graph("main.rb"));
        assert!(supports_import_graph("main.c"));
        assert!(supports_import_graph("main.cpp"));
        assert!(supports_import_graph("main.h"));
        assert!(supports_import_graph("main.php"));
        assert!(!supports_import_graph("main.swift"));
    }

    #[test]
    fn extracts_go_imports() {
        let content = r#"
package main

import "fmt"
import (
    "os"
    "path/filepath"
)
"#;
        let imports = super::parsers::extract_go_imports(content);
        assert!(imports.contains(&"fmt".to_owned()), "single import");
        assert!(imports.contains(&"os".to_owned()), "block import os");
        assert!(
            imports.contains(&"path/filepath".to_owned()),
            "block import path"
        );
    }

    #[test]
    fn extracts_java_imports() {
        let content = "import com.example.Foo;\nimport static com.example.Utils.helper;\n";
        let imports = super::parsers::extract_java_imports(content);
        assert!(imports.contains(&"com.example.Foo".to_owned()));
        assert!(imports.contains(&"com.example.Utils.helper".to_owned()));
    }

    #[test]
    fn extracts_kotlin_imports() {
        let content = "import com.example.Foo\nimport com.example.Bar as B\n";
        let imports = super::parsers::extract_kotlin_imports(content);
        assert!(imports.contains(&"com.example.Foo".to_owned()));
        assert!(imports.contains(&"com.example.Bar".to_owned()));
    }

    #[test]
    fn extracts_rust_imports() {
        let content = "use crate::utils;\nuse super::models;\nmod config;\n";
        let imports = super::parsers::extract_rust_imports(content);
        assert!(imports.contains(&"crate::utils".to_owned()));
        assert!(imports.contains(&"super::models".to_owned()));
        assert!(imports.contains(&"config".to_owned()));
    }

    #[test]
    fn extracts_rust_pub_mod_and_pub_use() {
        let content =
            "pub mod symbols;\npub(crate) mod db;\npub use crate::project::ProjectRoot;\n";
        let imports = super::parsers::extract_rust_imports(content);
        assert!(
            imports.contains(&"symbols".to_owned()),
            "pub mod should be captured"
        );
        assert!(
            imports.contains(&"db".to_owned()),
            "pub(crate) mod should be captured"
        );
        assert!(
            imports.contains(&"crate::project::ProjectRoot".to_owned()),
            "pub use should be captured"
        );
    }

    #[test]
    fn extracts_rust_brace_group_imports() {
        let content = "use crate::{symbols, db};\nuse crate::foo::{Bar, Baz};\n";
        let imports = super::parsers::extract_rust_imports(content);
        assert!(
            imports.contains(&"crate::symbols".to_owned()),
            "brace group item 1"
        );
        assert!(
            imports.contains(&"crate::db".to_owned()),
            "brace group item 2"
        );
        assert!(
            imports.contains(&"crate::foo::Bar".to_owned()),
            "nested brace 1"
        );
        assert!(
            imports.contains(&"crate::foo::Baz".to_owned()),
            "nested brace 2"
        );
    }

    #[test]
    fn extracts_ruby_imports() {
        let content = "require \"json\"\nrequire_relative \"../lib/helper\"\nload \"tasks.rb\"\n";
        let imports = super::parsers::extract_ruby_imports(content);
        assert!(imports.contains(&"json".to_owned()));
        assert!(imports.contains(&"../lib/helper".to_owned()));
        assert!(imports.contains(&"tasks.rb".to_owned()));
    }

    #[test]
    fn extracts_c_imports() {
        let content = "#include \"mylib.h\"\n#include <stdio.h>\n";
        let imports = super::parsers::extract_c_imports(content);
        assert!(imports.contains(&"mylib.h".to_owned()));
        assert!(imports.contains(&"stdio.h".to_owned()));
    }

    #[test]
    fn extracts_php_imports() {
        let content =
            "use App\\Http\\Controllers\\HomeController;\nrequire \"vendor/autoload.php\";\n";
        let imports = super::parsers::extract_php_imports(content);
        assert!(imports.contains(&"App\\Http\\Controllers\\HomeController".to_owned()));
        assert!(imports.contains(&"vendor/autoload.php".to_owned()));
    }

    #[test]
    fn returns_importers() {
        let dir = temp_project_dir("importers");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        fs::write(dir.join("utils.py"), "def greet():\n    return 1\n").expect("write utils");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let importers = get_importers(&project, "utils.py", 10, &cache).expect("importers");
        assert_eq!(
            importers,
            vec![
                super::ImporterEntry {
                    file: "main.py".to_owned(),
                },
                super::ImporterEntry {
                    file: "worker.py".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn returns_importance_ranking() {
        let dir = temp_project_dir("importance");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        fs::write(
            dir.join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let ranking = get_importance(&project, 10, &cache).expect("importance");
        assert!(!ranking.is_empty());
        assert_eq!(
            ranking.first().map(|it| it.file.as_str()),
            Some("models.py")
        );
        assert!(ranking.iter().all(|it| !it.score.is_empty()));
    }

    #[test]
    fn returns_dead_code_candidates() {
        let dir = temp_project_dir("dead-code");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(dir.join("utils.py"), "def greet():\n    return 1\n").expect("write utils");
        fs::write(dir.join("unused.py"), "def helper():\n    return 2\n").expect("write unused");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let dead = find_dead_code(&project, 10, &cache).expect("dead code");
        assert_eq!(
            dead,
            vec![
                super::DeadCodeEntry {
                    file: "main.py".to_owned(),
                    symbol: None,
                    reason: "no importers".to_owned(),
                },
                super::DeadCodeEntry {
                    file: "unused.py".to_owned(),
                    symbol: None,
                    reason: "no importers".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn resolves_cross_crate_workspace_imports() {
        let dir = temp_project_dir("cross-crate");
        let core_src = dir.join("crates").join("codelens-core").join("src");
        let mcp_src = dir.join("crates").join("codelens-mcp").join("src");
        fs::create_dir_all(&core_src).expect("mkdir core/src");
        fs::create_dir_all(&mcp_src).expect("mkdir mcp/src");

        fs::write(
            dir.join("crates").join("codelens-core").join("Cargo.toml"),
            "[package]\nname = \"codelens-core\"\n",
        )
        .expect("write core Cargo.toml");
        fs::write(
            dir.join("crates").join("codelens-mcp").join("Cargo.toml"),
            "[package]\nname = \"codelens-mcp\"\n",
        )
        .expect("write mcp Cargo.toml");

        fs::write(core_src.join("project.rs"), "pub struct ProjectRoot;\n")
            .expect("write project.rs");

        let main_rs = mcp_src.join("main.rs");
        fs::write(
            &main_rs,
            "use codelens_core::project::ProjectRoot;\nfn main() {}\n",
        )
        .expect("write main.rs");

        let project = ProjectRoot::new(&dir).expect("project");

        let resolved = super::resolvers::resolve_module_for_file(
            &project,
            &main_rs,
            "codelens_core::project::ProjectRoot",
        );
        assert_eq!(
            resolved,
            Some("crates/codelens-core/src/project.rs".to_owned()),
            "cross-crate import should resolve to crates/codelens-core/src/project.rs"
        );
    }

    fn temp_project_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-import-graph-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        dir
    }
}
