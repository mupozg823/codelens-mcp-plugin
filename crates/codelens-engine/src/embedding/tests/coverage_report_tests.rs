use super::*;

#[test]
fn coverage_report_includes_readiness_percent_and_stale_reason() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();
    engine
        .store
        .set_meta_value("last_index_sha", "0123456789abcdef0123456789abcdef01234567")
        .unwrap();

    let clean = engine.coverage_report(&project).unwrap();
    assert_eq!(clean.indexed_symbols, 2);
    assert_eq!(clean.indexed_files, 1);
    assert_eq!(clean.checked_files, 1);
    assert_eq!(clean.ready_files, 1);
    assert_eq!(clean.readiness_percent, 100);
    assert_eq!(clean.unchanged_files, 1);
    assert_eq!(clean.stale_files, 0);
    assert_eq!(clean.missing_files, 0);
    assert!(clean.stale_file_reasons.is_empty());
    assert_eq!(clean.stale_file_reasons_omitted, 0);
    assert_eq!(
        clean.last_index_sha.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );

    let changed_source = "def hello(name):\n    print(name)\n\ndef world():\n    return 42\n";
    write_python_file_with_symbols(
        dir.path(),
        "main.py",
        changed_source,
        "hash2",
        &[
            ("hello", "def hello(name):", "hello"),
            ("world", "def world():", "world"),
        ],
    );

    let stale = engine.coverage_report(&project).unwrap();
    assert_eq!(stale.checked_files, 1);
    assert_eq!(stale.ready_files, 0);
    assert_eq!(stale.readiness_percent, 0);
    assert_eq!(stale.unchanged_files, 0);
    assert_eq!(stale.stale_files, 1);
    assert_eq!(
        stale.stale_file_reasons,
        vec![EmbeddingStaleFileReason {
            file_path: "main.py".to_owned(),
            reason: EmbeddingStaleReason::EmbeddingKeysChanged,
        }]
    );
    assert_eq!(
        engine.store.count().unwrap(),
        2,
        "report must stay read-only"
    );
}

#[test]
fn coverage_report_reports_missing_embedding_reason() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (_dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();
    engine.store.delete_by_file(&["main.py"]).unwrap();

    let report = engine.coverage_report(&project).unwrap();
    assert_eq!(report.checked_files, 1);
    assert_eq!(report.ready_files, 0);
    assert_eq!(report.readiness_percent, 0);
    assert_eq!(report.missing_files, 1);
    assert_eq!(report.skipped_new_files, 1);
    assert_eq!(
        report.stale_file_reasons,
        vec![EmbeddingStaleFileReason {
            file_path: "main.py".to_owned(),
            reason: EmbeddingStaleReason::MissingEmbeddings,
        }]
    );
}

#[test]
fn inspect_existing_coverage_includes_readiness_without_loading_model() {
    let _lock = MODEL_LOCK.lock().unwrap();
    skip_without_embedding_model!();
    let (dir, project) = make_project_with_source();
    let engine = EmbeddingEngine::new(&project).unwrap();
    engine.index_from_project(&project).unwrap();
    drop(engine);

    let clean = EmbeddingEngine::inspect_existing_coverage(&project)
        .unwrap()
        .expect("coverage should be available from existing index");
    assert_eq!(clean.indexed_symbols, 2);
    assert_eq!(clean.indexed_files, 1);
    assert_eq!(clean.checked_files, 1);
    assert_eq!(clean.ready_files, 1);
    assert_eq!(clean.readiness_percent, 100);
    assert_eq!(clean.unchanged_files, 1);
    assert_eq!(clean.stale_files, 0);

    let changed_source = "def hello(name):\n    print(name)\n\ndef world():\n    return 42\n";
    write_python_file_with_symbols(
        dir.path(),
        "main.py",
        changed_source,
        "hash2",
        &[
            ("hello", "def hello(name):", "hello"),
            ("world", "def world():", "world"),
        ],
    );

    let stale = EmbeddingEngine::inspect_existing_coverage(&project)
        .unwrap()
        .expect("coverage should remain available from existing index");
    assert_eq!(stale.checked_files, 1);
    assert_eq!(stale.ready_files, 0);
    assert_eq!(stale.readiness_percent, 0);
    assert_eq!(stale.unchanged_files, 0);
    assert_eq!(stale.stale_files, 1);
    assert_eq!(
        stale.stale_file_reasons,
        vec![EmbeddingStaleFileReason {
            file_path: "main.py".to_owned(),
            reason: EmbeddingStaleReason::EmbeddingKeysChanged,
        }]
    );
}
