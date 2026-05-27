use super::policy::{POLICY_FILE_BASENAME, glob_match};
use super::*;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

fn unique_tmp(name: &str) -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "codelens_mem_test_{}_{}_{name}",
        std::process::id(),
        id
    ))
}

fn setup_for(test_name: &str) -> PathBuf {
    let root = unique_tmp(test_name);
    let _ = fs::remove_dir_all(&root);
    let dir = root.join(".codelens").join("memories");
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn cleanup_for(test_name: &str) {
    let root = unique_tmp(test_name);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn write_and_read() {
    let dir = setup_for("write_and_read");
    write_memory(&dir, "test_note", "hello").unwrap();
    let content = read_memory(&dir, "test_note").unwrap();
    assert_eq!(content, "hello");
    cleanup_for("write_and_read");
}

#[test]
fn read_only_policy_blocks_write() {
    let dir = setup_for("read_only_policy");
    // Write a note first
    write_memory(&dir, "adr/001", "decision").unwrap();
    // Install policy making adr/* read-only
    fs::write(dir.join(POLICY_FILE_BASENAME), "read_only = [\"adr/*\"]").unwrap();
    // Writing should fail
    let result = write_memory(&dir, "adr/001", "override");
    assert!(result.is_err(), "write to read-only should fail");
    cleanup_for("read_only_policy");
}

#[test]
fn ignored_policy_hides_from_list() {
    let dir = setup_for("ignored_policy");
    write_memory(&dir, "scratch/notes", "tmp").unwrap();
    write_memory(&dir, "real/doc", "important").unwrap();
    fs::write(dir.join(POLICY_FILE_BASENAME), "ignored = [\"scratch/*\"]").unwrap();
    let names = list_memory_names(&dir, None);
    assert!(!names.iter().any(|n| n.contains("scratch")));
    assert!(names.iter().any(|n| n.contains("real")));
    cleanup_for("ignored_policy");
}

#[test]
fn policy_parser_uses_toml_semantics() {
    let policy = MemoryPolicy::parse(
        r#"
            read_only = ["adr/*", "release,notes"]
            ignored = ['scratch/*']
            max_age_days = 30
            "#,
    );

    assert!(policy.is_read_only("adr/001"));
    assert!(policy.is_read_only("release,notes"));
    assert!(policy.is_ignored("scratch/tmp"));
    assert_eq!(policy.max_age_days, Some(30));
}

#[test]
fn archive_and_restore() {
    let dir = setup_for("archive_and_restore");
    write_memory(&dir, "old_doc", "stale").unwrap();
    archive_memory(&dir, "old_doc").unwrap();
    // Should not appear in normal listing
    let names = list_memory_names(&dir, None);
    assert!(!names.iter().any(|n| n == "old_doc"));
    // Should appear in archived listing
    let archived = list_archived(&dir).unwrap();
    assert!(archived.iter().any(|n| n.contains("old_doc")));
    // Restore
    restore_archived(&dir, "archived/old_doc").unwrap();
    let names = list_memory_names(&dir, None);
    assert!(names.iter().any(|n| n == "old_doc"));
    cleanup_for("archive_and_restore");
}

#[test]
fn glob_match_basics() {
    assert!(glob_match("adr/*", "adr/001"));
    assert!(glob_match("adr/*", "adr/some/nested"));
    assert!(glob_match("bench-*", "bench-v1"));
    assert!(!glob_match("adr/*", "real/doc"));
    assert!(glob_match("*", "anything"));
    assert!(glob_match("test?", "test1"));
    assert!(!glob_match("test?", "test12"));
}

#[test]
fn resolve_path_rejects_traversal() {
    let dir = setup_for("resolve_traversal");
    let result = resolve_memory_path(&dir, "../etc/passwd");
    assert!(result.is_err());
    cleanup_for("resolve_traversal");
}

#[test]
fn tier_resolution_project_first() {
    let root = unique_tmp("tier_project");
    let _ = fs::remove_dir_all(&root);
    let project_dir = root.clone();
    let project_memories = project_dir.join(".codelens").join("memories");
    fs::create_dir_all(&project_memories).unwrap();
    write_memory(&project_memories, "project_doc", "from project").unwrap();

    let loc = resolve_memory_tier("project_doc", &project_dir, None);
    assert_eq!(loc.tier, MemoryTier::Project);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn tier_resolution_global_prefix() {
    let root = unique_tmp("tier_global");
    let _ = fs::remove_dir_all(&root);
    let project_dir = root.clone();
    let global_dir = unique_tmp("tier_global_g");
    let _ = fs::remove_dir_all(&global_dir);
    fs::create_dir_all(&global_dir).unwrap();
    write_memory(&global_dir, "shared_doc", "from global").unwrap();

    let loc = resolve_memory_tier("global/shared_doc", &project_dir, Some(&global_dir));
    assert_eq!(loc.tier, MemoryTier::Global);
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&global_dir);
}
