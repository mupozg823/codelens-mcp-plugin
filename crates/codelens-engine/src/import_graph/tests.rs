use super::{
    GraphCache, find_dead_code, get_blast_radius, get_importance, get_importers,
    supports_import_graph,
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
    assert!(supports_import_graph("main.swift"));
    assert!(supports_import_graph("main.scala"));
    assert!(supports_import_graph("main.css"));
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
    let content = "pub mod symbols;\npub(crate) mod db;\npub use crate::project::ProjectRoot;\n";
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
    let content = "use App\\Http\\Controllers\\HomeController;\nrequire \"vendor/autoload.php\";\n";
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

    fs::write(
        dir.join("crates")
            .join("codelens-core")
            .join("src/project.rs"),
        "pub struct ProjectRoot;\n",
    )
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
