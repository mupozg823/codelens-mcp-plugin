use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

const WAL_CHILD_DB_ENV: &str = "CODELENS_TEST_WAL_CHILD_DB";
const WAL_CHILD_MODE_ENV: &str = "CODELENS_TEST_WAL_CHILD_MODE";
const WAL_CHILD_READY_ENV: &str = "CODELENS_TEST_WAL_CHILD_READY";
const WAL_CHILD_TEST: &str = "db::tests::wal_fault_child_process";
const WAL_CHILD_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum WalChildMode {
    Commit,
    HoldUncommitted,
}

impl WalChildMode {
    const fn env_value(self) -> &'static str {
        match self {
            Self::Commit => "commit",
            Self::HoldUncommitted => "hold-uncommitted",
        }
    }

    fn from_env() -> Option<Self> {
        let value = std::env::var_os(WAL_CHILD_MODE_ENV)?;
        Some(
            match value.to_str().expect("WAL child mode must be UTF-8") {
                "commit" => Self::Commit,
                "hold-uncommitted" => Self::HoldUncommitted,
                value => panic!("unknown WAL child mode: {value}"),
            },
        )
    }
}

fn spawn_wal_child(db_path: &Path, ready_path: &Path, mode: WalChildMode) -> Child {
    let current_exe = std::env::current_exe().expect("current test executable");
    Command::new(current_exe)
        .arg("--exact")
        .arg(WAL_CHILD_TEST)
        .arg("--nocapture")
        .env(WAL_CHILD_DB_ENV, db_path)
        .env(WAL_CHILD_MODE_ENV, mode.env_value())
        .env(WAL_CHILD_READY_ENV, ready_path)
        .spawn()
        .expect("spawn WAL fault child")
}

fn stop_timed_out_child(child: &mut Child, operation: &str) -> ! {
    let kill_result = child.kill();
    let cleanup_deadline = Instant::now() + WAL_CHILD_WAIT;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < cleanup_deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => break None,
            Err(error) => panic!("failed to reap timed-out WAL child: {error}"),
        }
    };
    panic!("WAL child timed out while {operation}: kill={kill_result:?}, exit={exit_status:?}");
}

fn wait_for_child_marker(child: &mut Child, marker: &Path) {
    let deadline = Instant::now() + WAL_CHILD_WAIT;
    loop {
        if marker.is_file() {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll WAL child") {
            panic!("WAL child exited before ready marker: {status}");
        }
        if Instant::now() >= deadline {
            stop_timed_out_child(child, "waiting for its ready marker");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_child_exit(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + WAL_CHILD_WAIT;
    loop {
        if let Some(status) = child.try_wait().expect("poll WAL child exit") {
            return status;
        }
        if Instant::now() >= deadline {
            stop_timed_out_child(child, "waiting for exit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn wal_fault_child_process() {
    let Some(mode) = WalChildMode::from_env() else {
        return;
    };
    let db_path = PathBuf::from(
        std::env::var_os(WAL_CHILD_DB_ENV).expect("WAL child database path must be set"),
    );
    let ready_path = PathBuf::from(
        std::env::var_os(WAL_CHILD_READY_ENV).expect("WAL child ready path must be set"),
    );

    match mode {
        WalChildMode::Commit => {
            let db = IndexDb::open(&db_path).expect("open WAL child database");
            db.upsert_file("child-committed.rs", 2, "committed", 2, Some("rs"))
                .expect("commit child file");
            fs::write(ready_path, b"committed").expect("write commit child marker");
        }
        WalChildMode::HoldUncommitted => {
            let mut db = IndexDb::open(&db_path).expect("open WAL child database");
            let transaction = db.conn.transaction().expect("begin child transaction");
            ops::upsert_file(
                &transaction,
                "child-uncommitted.rs",
                2,
                "uncommitted",
                2,
                Some("rs"),
            )
            .expect("write uncommitted child file");
            fs::write(ready_path, b"uncommitted").expect("write transaction child marker");

            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            drop(transaction);
            panic!("parent did not kill the uncommitted WAL child");
        }
    }
}

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
fn analyzer_version_two_wipes_pre_owner_qualified_rust_symbols() {
    let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-owner-version-");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("symbols.db");

    // Simulate the v1 analyzer, which indexed an impl method as an unowned
    // function and would let `Type::new` fall back to any basename match.
    {
        let db = IndexDb::open(&db_path).unwrap();
        let fid = db.upsert_file("lib.rs", 1, "h", 1, Some("rs")).unwrap();
        db.insert_symbols(
            fid,
            &[NewSymbol {
                name: "new",
                kind: "function",
                line: 1,
                column_num: 1,
                start_byte: 0,
                end_byte: 1,
                signature: "fn new()",
                name_path: "new",
                parent_id: None,
            }],
        )
        .unwrap();
        db.conn
            .execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('analyzer_version', '1')",
                [],
            )
            .unwrap();
    }

    let db = IndexDb::open(&db_path).unwrap();
    assert_eq!(
        db.file_count().unwrap(),
        0,
        "v1 basename-only Rust symbols must be invalidated for owner-qualified re-indexing"
    );
    let stored: i64 = db
        .conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'analyzer_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        stored, 2,
        "owner-qualified analyzer version must be recorded"
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
fn readonly_wal_snapshot_stays_stable_while_child_commits() {
    // Given: a read-only connection has established an explicit WAL snapshot.
    let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-wal-snapshot-");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("symbols.db");
    let writer = IndexDb::open(&db_path).unwrap();
    writer
        .upsert_file("baseline.rs", 1, "baseline", 1, Some("rs"))
        .unwrap();
    drop(writer);

    let snapshot = IndexDb::open_readonly(&db_path).unwrap().unwrap();
    snapshot.conn.execute_batch("BEGIN DEFERRED").unwrap();
    assert_eq!(snapshot.file_count().unwrap(), 1);

    // When: another process commits a file through the same WAL database.
    let ready_path = dir.join("commit-child.ready");
    let mut child = spawn_wal_child(&db_path, &ready_path, WalChildMode::Commit);
    wait_for_child_marker(&mut child, &ready_path);
    let status = wait_for_child_exit(&mut child);
    assert!(status.success(), "WAL commit child failed: {status}");

    // Then: the established snapshot remains stable while a new one sees it.
    assert!(snapshot.get_file("child-committed.rs").unwrap().is_none());
    let fresh_snapshot = IndexDb::open_readonly(&db_path).unwrap().unwrap();
    assert!(
        fresh_snapshot
            .get_file("child-committed.rs")
            .unwrap()
            .is_some()
    );
    snapshot.conn.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn killed_child_uncommitted_wal_transaction_leaves_no_partial_row() {
    // Given: a child process has written a row inside an open transaction.
    let (_td, dir) = crate::test_helpers::make_unique_temp_dir("codelens-wal-kill-");
    fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("symbols.db");
    let db = IndexDb::open(&db_path).unwrap();
    db.upsert_file("baseline.rs", 1, "baseline", 1, Some("rs"))
        .unwrap();
    drop(db);

    let ready_path = dir.join("transaction-child.ready");
    let mut child = spawn_wal_child(&db_path, &ready_path, WalChildMode::HoldUncommitted);
    wait_for_child_marker(&mut child, &ready_path);

    // When: the process is killed before its transaction can commit.
    child.kill().unwrap();
    let status = wait_for_child_exit(&mut child);
    assert!(!status.success(), "killed WAL child exited successfully");

    // Then: recovery exposes no partial row and SQLite reports a valid DB.
    let recovered = IndexDb::open(&db_path).unwrap();
    assert!(
        recovered
            .get_file("child-uncommitted.rs")
            .unwrap()
            .is_none()
    );
    let integrity: String = recovered
        .conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
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

/// `symbols.parent_id` is a self-referencing foreign key with foreign_keys
/// ON: without an index, every deleted symbol scans the whole table for
/// children, which made re-indexing a changed worktree take minutes.
#[test]
fn symbol_parent_lookup_uses_an_index() {
    fn plan(db: &IndexDb) -> String {
        db.conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT 1 FROM symbols WHERE parent_id = ?1",
                [1],
                |row| row.get::<_, String>(3),
            )
            .expect("query plan")
    }

    let fresh = IndexDb::open_memory().expect("fresh db");
    assert!(
        plan(&fresh).contains("idx_symbols_parent"),
        "{}",
        plan(&fresh)
    );

    // An index created at schema v6 picks the index up on the next open.
    let dir = std::env::temp_dir().join(format!(
        "codelens-db-parent-index-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("symbols.db");
    {
        let db = IndexDb::open(&path).expect("open");
        db.conn
            .execute_batch(
                "DROP INDEX idx_symbols_parent;
                 UPDATE meta SET value = '6' WHERE key = 'schema_version';",
            )
            .expect("downgrade to v6");
    }
    let reopened = IndexDb::open(&path).expect("reopen");
    assert!(
        plan(&reopened).contains("idx_symbols_parent"),
        "{}",
        plan(&reopened)
    );
    let _ = fs::remove_dir_all(&dir);
}

/// Schema 7 kept `symbols_fts` fresh by a lazy rebuild keyed on the symbol
/// count. Opening such an index must re-sync the rows it already holds and
/// leave triggers that keep later writes in step.
#[test]
fn fts_rows_written_before_schema_8_are_searchable_after_upgrade() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-db-fts-triggers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("symbols.db");
    let symbol = |name: &'static str| NewSymbol {
        name,
        kind: "function",
        line: 1,
        column_num: 1,
        start_byte: 0,
        end_byte: 20,
        signature: name,
        name_path: name,
        parent_id: None,
    };
    {
        let db = IndexDb::open(&path).expect("open");
        db.conn
            .execute_batch(
                "DROP TRIGGER symbols_fts_insert;
                 DROP TRIGGER symbols_fts_delete;
                 DROP TRIGGER symbols_fts_update;
                 UPDATE meta SET value = '7' WHERE key = 'schema_version';",
            )
            .expect("downgrade to v7");
        let file_id = db.upsert_file("old.rs", 1, "h", 1, Some("rs")).unwrap();
        db.insert_symbols(file_id, &[symbol("alphawolf")]).unwrap();
    }

    let reopened = IndexDb::open(&path).expect("reopen");
    let names = |query: &str| -> Vec<String> {
        reopened
            .search_symbols_fts(query, 10)
            .unwrap()
            .into_iter()
            .map(|(row, _, _)| row.name)
            .collect()
    };
    assert_eq!(names("alphawolf"), ["alphawolf"]);
    let file_id = reopened
        .upsert_file("new.rs", 1, "h", 1, Some("rs"))
        .unwrap();
    reopened
        .insert_symbols(file_id, &[symbol("omegahawk")])
        .unwrap();
    assert_eq!(names("omegahawk"), ["omegahawk"]);
    reopened
        .upsert_file("old.rs", 2, "h2", 1, Some("rs"))
        .unwrap();
    assert!(names("alphawolf").is_empty());
    drop(reopened);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn open_connections_are_counted_per_path_while_alive() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-db-open-count-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("symbols.db");

    let first = IndexDb::open(&path).expect("open");
    let reader = IndexDb::open_readonly(&path).expect("ro").expect("exists");
    assert_eq!(open_connections_for(&path), 2);
    drop(first);
    assert_eq!(open_connections_for(&path), 1);
    drop(reader);
    assert_eq!(open_connections_for(&path), 0);
    let _ = fs::remove_dir_all(&dir);
}

fn pragma_i64(conn: &Connection, name: &str) -> i64 {
    conn.pragma_query_value(None, name, |row| row.get(0))
        .unwrap()
}

#[test]
fn a_new_index_file_gets_incremental_auto_vacuum() {
    // `auto_vacuum` after `journal_mode = WAL` was silently ignored, so every
    // index was created with it off.
    let dir = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&dir.path().join("symbols.db")).unwrap();
    assert_eq!(pragma_i64(&db.conn, "auto_vacuum"), 2);
}

/// A file laid out like every index created before the pragma order fix:
/// WAL with auto-vacuum off, mostly empty after a large delete.
fn mostly_empty_legacy_db(path: &Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "PRAGMA page_size = 4096; PRAGMA journal_mode = WAL; CREATE TABLE rows (id INTEGER PRIMARY KEY, body BLOB);",
    )
    .unwrap();
    fill_and_delete_most(&conn);
    conn
}

fn fill_and_delete_most(conn: &Connection) {
    let body = vec![7_u8; 1024];
    let tx = conn.unchecked_transaction().unwrap();
    for _ in 0..2_000 {
        tx.execute("INSERT INTO rows (body) VALUES (?1)", [&body])
            .unwrap();
    }
    tx.commit().unwrap();
    conn.execute("DELETE FROM rows WHERE id % 20 != 0", [])
        .unwrap();
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
}

#[test]
fn compaction_gives_back_the_space_deleted_rows_left() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("symbols.db");
    let conn = mostly_empty_legacy_db(&path);
    assert_eq!(pragma_i64(&conn, "auto_vacuum"), 0);
    let file_before = fs::metadata(&path).unwrap().len();

    let report = compact::compact_if_mostly_free(&conn, 0.25, 64 * 1024)
        .unwrap()
        .expect("a mostly empty file is compacted");

    assert!(report.converted, "{report:?}");
    assert!(report.bytes_after * 4 < report.bytes_before, "{report:?}");
    assert_eq!(pragma_i64(&conn, "freelist_count"), 0);
    assert_eq!(pragma_i64(&conn, "auto_vacuum"), 2);
    let file_after = fs::metadata(&path).unwrap().len();
    assert!(
        file_after * 4 < file_before,
        "{file_before} -> {file_after}"
    );
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM rows", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        100
    );

    // Converted once; later deletes are given back in place.
    fill_and_delete_most(&conn);
    let report = compact::compact_if_mostly_free(&conn, 0.25, 64 * 1024)
        .unwrap()
        .expect("compacted again");
    assert!(!report.converted, "{report:?}");
    assert_eq!(pragma_i64(&conn, "freelist_count"), 0);
}

#[test]
fn compaction_leaves_a_mostly_full_file_alone() {
    let dir = tempfile::tempdir().unwrap();
    let conn = Connection::open(dir.path().join("symbols.db")).unwrap();
    conn.execute_batch("PRAGMA journal_mode = WAL; CREATE TABLE rows (body BLOB);")
        .unwrap();
    conn.execute("INSERT INTO rows (body) VALUES (zeroblob(65536))", [])
        .unwrap();
    assert_eq!(
        compact::compact_if_mostly_free(&conn, 0.25, 1).unwrap(),
        None
    );
}

#[test]
fn fts_search_accepts_punctuation_and_operator_words_in_the_query() {
    let db = IndexDb::open_memory().unwrap();
    let file_id = db.upsert_file("app.ts", 100, "h", 10, Some("ts")).unwrap();
    db.insert_symbols(
        file_id,
        &[
            NewSymbol {
                name: "LandingPage",
                kind: "function",
                line: 1,
                column_num: 1,
                start_byte: 0,
                end_byte: 40,
                signature: "export default function LandingPage()",
                name_path: "LandingPage",
                parent_id: None,
            },
            NewSymbol {
                name: "read_config",
                kind: "function",
                line: 5,
                column_num: 1,
                start_byte: 41,
                end_byte: 90,
                signature: "fn read_config(path: &Path) -> Config",
                name_path: "read_config",
                parent_id: None,
            },
        ],
    )
    .unwrap();

    // The index tokenizer splits on every non-alphanumeric character, so a
    // dotted, pathed or operator-laden query must reach the same tokens
    // instead of failing FTS5 parsing and emptying the lexical lane.
    for (query, expected) in [
        ("next.js landing page", "LandingPage"),
        ("std::fs read config", "read_config"),
        ("app/page.tsx landing", "LandingPage"),
        ("read(config)", "read_config"),
        ("config OR NOT", "read_config"),
        ("\"landing\"", "LandingPage"),
    ] {
        let hits = db
            .search_symbols_fts(query, 10)
            .unwrap_or_else(|error| panic!("{query:?} failed: {error:#}"));
        assert!(
            hits.iter().any(|(row, _, _)| row.name == expected),
            "{query:?} did not find {expected}: {:?}",
            hits.iter().map(|(row, _, _)| &row.name).collect::<Vec<_>>()
        );
    }

    for query in ["::", "", "  ", "()"] {
        let hits = db
            .search_symbols_fts(query, 10)
            .unwrap_or_else(|error| panic!("{query:?} failed: {error:#}"));
        assert!(hits.is_empty(), "{query:?} matched {}", hits.len());
    }
}

#[test]
fn fts_search_ignores_stopwords_unless_the_query_has_nothing_else() {
    let db = IndexDb::open_memory().unwrap();
    let file_id = db.upsert_file("ser.rs", 100, "h", 10, Some("rs")).unwrap();
    let symbol = |name: &'static str, line: i64| NewSymbol {
        name,
        kind: "function",
        line,
        column_num: 1,
        start_byte: line * 100,
        end_byte: line * 100 + 50,
        signature: "fn f()",
        name_path: name,
        parent_id: None,
    };
    db.insert_symbols(
        file_id,
        &[
            symbol("IntoIter", 1),
            symbol("serialize_value", 2),
            symbol("set_timeout", 3),
        ],
    )
    .unwrap();
    let names = |query: &str| -> Vec<String> {
        db.search_symbols_fts(query, 10)
            .unwrap()
            .into_iter()
            .map(|(row, _, _)| row.name)
            .collect()
    };

    // `into` is a function word here; matching on it alone floods the lane
    // with `IntoIter`-style names ahead of the real terms.
    assert_eq!(names("serialize into json"), ["serialize_value"]);
    // A query made only of stopwords still searches for them.
    assert_eq!(names("set"), ["set_timeout"]);
}
