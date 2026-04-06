use super::*;

#[test]
fn creates_schema_and_upserts_file() {
    let db = IndexDb::open_memory().unwrap();
    let id = db
        .upsert_file("src/main.py", 1000, "abc123", 256, Some("py"))
        .unwrap();
    assert!(id > 0);

    let file = db.get_file("src/main.py").unwrap().unwrap();
    assert_eq!(file.content_hash, "abc123");
    assert_eq!(file.size_bytes, 256);

    // Upsert same path with new hash
    let id2 = db
        .upsert_file("src/main.py", 2000, "def456", 512, Some("py"))
        .unwrap();
    assert_eq!(id, id2);
    let file = db.get_file("src/main.py").unwrap().unwrap();
    assert_eq!(file.content_hash, "def456");
}

#[test]
fn fresh_file_check() {
    let db = IndexDb::open_memory().unwrap();
    db.upsert_file("a.py", 100, "hash1", 10, Some("py"))
        .unwrap();

    assert!(db.get_fresh_file("a.py", 100, "hash1").unwrap().is_some());
    assert!(db.get_fresh_file("a.py", 200, "hash1").unwrap().is_none());
    assert!(db.get_fresh_file("a.py", 100, "hash2").unwrap().is_none());
}

#[test]
fn inserts_and_queries_symbols() {
    let db = IndexDb::open_memory().unwrap();
    let file_id = db.upsert_file("main.py", 100, "h", 10, Some("py")).unwrap();

    let syms = vec![
        NewSymbol {
            name: "Service",
            kind: "class",
            line: 1,
            column_num: 1,
            start_byte: 0,
            end_byte: 50,
            signature: "class Service:",
            name_path: "Service",
            parent_id: None,
        },
        NewSymbol {
            name: "run",
            kind: "method",
            line: 2,
            column_num: 5,
            start_byte: 20,
            end_byte: 48,
            signature: "def run(self):",
            name_path: "Service/run",
            parent_id: None,
        },
    ];
    let ids = db.insert_symbols(file_id, &syms).unwrap();
    assert_eq!(ids.len(), 2);

    let found = db.find_symbols_by_name("Service", None, true, 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, "class");

    let found = db
        .find_symbols_by_name("run", Some("main.py"), true, 10)
        .unwrap();
    assert_eq!(found.len(), 1);

    let found = db.find_symbols_by_name("erv", None, false, 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "Service");
}

#[test]
fn upsert_file_clears_old_symbols() {
    let db = IndexDb::open_memory().unwrap();
    let file_id = db.upsert_file("a.py", 100, "h1", 10, Some("py")).unwrap();
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "Old",
            kind: "class",
            line: 1,
            column_num: 1,
            start_byte: 0,
            end_byte: 10,
            signature: "class Old:",
            name_path: "Old",
            parent_id: None,
        }],
    )
    .unwrap();

    // Re-upsert should clear old symbols
    let file_id2 = db.upsert_file("a.py", 200, "h2", 20, Some("py")).unwrap();
    assert_eq!(file_id, file_id2);
    let found = db.find_symbols_by_name("Old", None, true, 10).unwrap();
    assert!(found.is_empty());
}

#[test]
fn streams_symbols_grouped_by_file_in_path_order() {
    let db = IndexDb::open_memory().unwrap();
    let b_file_id = db.upsert_file("b.py", 100, "hb", 10, Some("py")).unwrap();
    let a_file_id = db.upsert_file("a.py", 100, "ha", 10, Some("py")).unwrap();

    db.insert_symbols(
        b_file_id,
        &[
            NewSymbol {
                name: "b_second",
                kind: "function",
                line: 2,
                column_num: 0,
                start_byte: 20,
                end_byte: 30,
                signature: "def b_second():",
                name_path: "b_second",
                parent_id: None,
            },
            NewSymbol {
                name: "b_first",
                kind: "function",
                line: 1,
                column_num: 0,
                start_byte: 0,
                end_byte: 10,
                signature: "def b_first():",
                name_path: "b_first",
                parent_id: None,
            },
        ],
    )
    .unwrap();
    db.insert_symbols(
        a_file_id,
        &[NewSymbol {
            name: "a_only",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 10,
            signature: "def a_only():",
            name_path: "a_only",
            parent_id: None,
        }],
    )
    .unwrap();

    let mut groups = Vec::new();
    let count = db
        .for_each_file_symbols_with_bytes(|file_path, symbols| {
            groups.push((
                file_path,
                symbols
                    .into_iter()
                    .map(|symbol| symbol.name)
                    .collect::<Vec<_>>(),
            ));
            Ok(())
        })
        .unwrap();

    assert_eq!(count, 3);
    assert_eq!(
        groups,
        vec![
            ("a.py".to_string(), vec!["a_only".to_string()]),
            (
                "b.py".to_string(),
                vec!["b_first".to_string(), "b_second".to_string()]
            ),
        ]
    );
}

#[test]
fn import_graph_operations() {
    let db = IndexDb::open_memory().unwrap();
    let main_id = db
        .upsert_file("main.py", 100, "h1", 10, Some("py"))
        .unwrap();
    let utils_id = db
        .upsert_file("utils.py", 100, "h2", 10, Some("py"))
        .unwrap();
    let _models_id = db
        .upsert_file("models.py", 100, "h3", 10, Some("py"))
        .unwrap();

    db.insert_imports(
        main_id,
        &[NewImport {
            target_path: "utils.py".into(),
            raw_import: "utils".into(),
        }],
    )
    .unwrap();
    db.insert_imports(
        utils_id,
        &[NewImport {
            target_path: "models.py".into(),
            raw_import: "models".into(),
        }],
    )
    .unwrap();

    let importers = db.get_importers("utils.py").unwrap();
    assert_eq!(importers, vec!["main.py"]);

    let imports_of = db.get_imports_of("main.py").unwrap();
    assert_eq!(imports_of, vec!["utils.py"]);

    let graph = db.build_import_graph().unwrap();
    assert_eq!(graph.len(), 3);
    assert_eq!(graph["utils.py"].1, vec!["main.py"]); // imported_by
}

#[test]
fn content_hash_is_deterministic() {
    let h1 = content_hash(b"hello world");
    let h2 = content_hash(b"hello world");
    let h3 = content_hash(b"hello world!");
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);
}

#[test]
fn with_transaction_auto_rollback_on_error() {
    let mut db = IndexDb::open_memory().unwrap();
    let result: anyhow::Result<()> = db.with_transaction(|conn| {
        ops::upsert_file(conn, "rollback_test.py", 100, "h1", 10, Some("py"))?;
        anyhow::bail!("simulated error");
    });
    assert!(result.is_err());
    // File should not exist — transaction was rolled back
    assert!(db.get_file("rollback_test.py").unwrap().is_none());
}
