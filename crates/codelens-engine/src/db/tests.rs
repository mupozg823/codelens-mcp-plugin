use super::*;
use std::fs;

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
fn indexed_at_bounds_are_none_for_empty_index() {
    let db = IndexDb::open_memory().unwrap();

    assert_eq!(db.max_files_indexed_at().unwrap(), None);
    assert_eq!(db.min_files_indexed_at().unwrap(), None);
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

// A resolver/parser LOGIC fix changes derived analysis for byte-identical
// source, but `commit_analyzed` skips files whose (mtime, hash) are unchanged —
// so the fix never reaches an already-populated index. `ANALYZER_VERSION` closes
// that gap: a stored value below the compiled one must wipe the stale analysis
// on open so `refresh_all` re-derives it with current logic.
#[test]
fn analyzer_version_bump_wipes_stale_analysis_on_reopen() {
    let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-analyzer-version-");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("symbols.db");

    // Simulate an index built by an OLDER binary: real content plus a stored
    // analyzer_version rolled back below the current compiled value.
    {
        let db = IndexDb::open(&db_path).unwrap();
        let fid = db.upsert_file("a.rs", 1, "h", 1, Some("rs")).unwrap();
        db.insert_imports(
            fid,
            &[NewImport {
                target_path: "phantom/mod.rs".into(),
                raw_import: "super::phantom".into(),
            }],
        )
        .unwrap();
        db.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('analyzer_version', '0')",
                [],
            )
            .unwrap();
        assert!(
            db.file_count().unwrap() > 0,
            "precondition: stale analysis present"
        );
    }

    // Re-open with the current binary: ANALYZER_VERSION > stored 0 → wipe.
    let db = IndexDb::open(&db_path).unwrap();
    assert_eq!(
        db.file_count().unwrap(),
        0,
        "stale analysis must be wiped when analyzer_version increases"
    );
    let stored: i64 = db
        .conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'analyzer_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        stored, ANALYZER_VERSION,
        "analyzer_version must be recorded after the wipe"
    );

    // Idempotent: with stored == current, a re-open must NOT wipe fresh content.
    db.upsert_file("b.rs", 2, "h2", 2, Some("rs")).unwrap();
    drop(db);
    let db2 = IndexDb::open(&db_path).unwrap();
    assert_eq!(
        db2.file_count().unwrap(),
        1,
        "content must survive when analyzer_version is unchanged"
    );
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

#[test]
fn open_recreates_corrupt_db_and_wal_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("symbols.db");
    let wal_path = dir.path().join("symbols.db-wal");
    let shm_path = dir.path().join("symbols.db-shm");

    fs::write(&db_path, b"not a sqlite database").unwrap();
    fs::write(&wal_path, b"bad wal").unwrap();
    fs::write(&shm_path, b"bad shm").unwrap();

    let db = IndexDb::open(&db_path).unwrap();
    assert_eq!(db.file_count().unwrap(), 0);

    let file_id = db
        .upsert_file("src/lib.rs", 100, "hash", 12, Some("rs"))
        .unwrap();
    assert!(file_id > 0);
    assert!(db.get_file("src/lib.rs").unwrap().is_some());

    assert!(db_path.is_file());

    let backup_names: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains(".corrupt-"))
        .collect();

    assert!(
        backup_names
            .iter()
            .any(|name| name.starts_with("symbols.db.corrupt-")),
        "expected quarantined primary db file, found {backup_names:?}"
    );
}

#[test]
fn quarantine_corrupt_sqlite_files_moves_sidecars_when_present() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("symbols.db");
    let wal_path = dir.path().join("symbols.db-wal");
    let shm_path = dir.path().join("symbols.db-shm");

    fs::write(&db_path, b"not a sqlite database").unwrap();
    fs::write(&wal_path, b"bad wal").unwrap();
    fs::write(&shm_path, b"bad shm").unwrap();

    let backups = quarantine_corrupt_sqlite_files(&db_path).unwrap();
    let backup_names: Vec<String> = backups
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();

    assert!(
        backup_names
            .iter()
            .any(|name| name.starts_with("symbols.db.corrupt-")),
        "expected quarantined primary db file, found {backup_names:?}"
    );
    assert!(
        backup_names
            .iter()
            .any(|name| name.starts_with("symbols.db-wal.corrupt-")),
        "expected quarantined wal sidecar, found {backup_names:?}"
    );
    assert!(
        backup_names
            .iter()
            .any(|name| name.starts_with("symbols.db-shm.corrupt-")),
        "expected quarantined shm sidecar, found {backup_names:?}"
    );
}

/// #349: identifier matching is NFC-canonical end to end — an NFD-form
/// Hangul name (decomposed jamo, e.g. pasted from a macOS filename)
/// inserts as NFC and is found by the NFC query an agent types, and an
/// NFD query still hits the NFC row.
#[test]
fn nfd_hangul_symbol_round_trips_via_nfc() {
    let nfd_name = "\u{1112}\u{116e}\u{110b}\u{116f}\u{11ab}\u{110c}\u{1161}_\u{1107}\u{1167}\u{11ab}\u{1112}\u{1167}\u{11bc}"; // "후원자_변형" decomposed
    let nfc_name = "후원자_변형";
    assert_ne!(nfd_name.as_bytes(), nfc_name.as_bytes());

    let db = IndexDb::open_memory().unwrap();
    let fid = db
        .upsert_file("src/lib.rs", 100, "h1", 10, Some("rs"))
        .unwrap();
    db.insert_symbols(
        fid,
        &[NewSymbol {
            name: nfd_name,
            kind: "function",
            line: 2,
            column_num: 8,
            start_byte: 0,
            end_byte: 50,
            signature: "pub fn 후원자_변형()",
            name_path: nfd_name,
            parent_id: None,
        }],
    )
    .unwrap();

    // NFC query (what an agent types) finds the row.
    let hits = db.find_symbols_by_name(nfc_name, None, true, 10).unwrap();
    assert_eq!(hits.len(), 1, "NFC query must hit the NFD-source symbol");
    assert_eq!(hits[0].name, nfc_name, "stored name is canonical NFC");

    // NFD query (byte-faithful copy from source) also still hits.
    let hits = db.find_symbols_by_name(nfd_name, None, true, 10).unwrap();
    assert_eq!(hits.len(), 1, "NFD query must normalize and hit too");

    // The JOINed variant follows the same contract.
    let hits = db.find_symbols_with_path(nfc_name, true, 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].1, "src/lib.rs");
}

/// #353: call-graph edges follow the same NFC contract as symbols —
/// NFD-source caller/callee names insert as NFC and are found by NFC
/// (and NFD) queries on both directions.
#[test]
fn nfd_hangul_call_edges_round_trip_via_nfc() {
    let nfd_caller = "\u{1112}\u{116e}\u{110b}\u{116f}\u{11ab}\u{1111}\u{1161}\u{1109}\u{1165}"; // "후원파서"
    let nfd_callee =
        "\u{1100}\u{1173}\u{11b7}\u{110b}\u{1162}\u{11a8}_\u{110e}\u{116e}\u{110e}\u{116e}\u{11af}"; // "금액_추출"
    let nfc_caller = "후원파서";
    let nfc_callee = "금액_추출";
    assert_ne!(nfd_caller.as_bytes(), nfc_caller.as_bytes());

    let db = IndexDb::open_memory().unwrap();
    let fid = db
        .upsert_file("src/p.py", 100, "h1", 10, Some("py"))
        .unwrap();
    db.insert_calls(
        fid,
        &[NewCall {
            caller_name: nfd_caller.to_owned(),
            callee_name: nfd_callee.to_owned(),
            line: 15,
        }],
    )
    .unwrap();

    let callers = db.get_callers_cached(nfc_callee, 10).unwrap();
    assert_eq!(
        callers.len(),
        1,
        "NFC callee query must hit the NFD-source edge"
    );
    assert_eq!(callers[0].1, nfc_caller, "stored caller is canonical NFC");

    let callers_nfd = db.get_callers_cached(nfd_callee, 10).unwrap();
    assert_eq!(
        callers_nfd.len(),
        1,
        "NFD callee query normalizes and hits too"
    );

    let callees = db.get_callees_cached(nfc_caller, None, 10).unwrap();
    assert_eq!(callees.len(), 1);
    assert_eq!(callees[0].0, nfc_callee);
}
