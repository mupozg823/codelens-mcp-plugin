//! Seed a brand-new symbol index from a sibling checkout of the same
//! repository.
//!
//! Agents bind CodeLens to freshly created git worktrees all the time
//! (`.claude/worktrees/*`, `.codex/worktrees/*`). Each one is a new project
//! root with no index, so the first bind parsed the whole tree inside the MCP
//! request — 10–120 s on a ~3k-file repo, and the dominant source of
//! `prepare_harness_session` timeouts. A sibling checkout (the main worktree or
//! another linked worktree) usually holds an index of nearly the same content.
//! Copying it gives `refresh_all` a baseline whose content hashes match, so
//! only files that really differ are parsed and the rest are re-stamped.
//!
//! Only the on-disk git layout is read (`.git` file, `commondir`,
//! `worktrees/*/gitdir`); no `git` process is spawned. Every failure is
//! non-fatal: the caller falls back to a normal full index build.

use crate::db::index_db_path;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Default ceiling on the copy. Seeding only saves parse time; past this it
/// costs more than it saves, so it is abandoned for a normal build. Live
/// binds on 2026-09-28 spent 1-2 h here (seed source under concurrent write).
const DEFAULT_SEED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn seed_timeout() -> std::time::Duration {
    std::env::var("CODELENS_SEED_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(DEFAULT_SEED_TIMEOUT, std::time::Duration::from_secs)
}

/// Copy a sibling-checkout index into `project_root`'s index path when that
/// path has no index yet. Returns the source database on success, `None`
/// when there is nothing to seed from or an index already exists.
pub fn seed_index_from_sibling_checkout(project_root: &Path) -> Result<Option<PathBuf>> {
    seed_index_with_timeout(project_root, seed_timeout())
}

fn seed_index_with_timeout(
    project_root: &Path,
    timeout: std::time::Duration,
) -> Result<Option<PathBuf>> {
    let destination = index_db_path(project_root);
    if destination.exists() {
        return Ok(None);
    }
    let Some(source) = preferred_sibling_index(project_root) else {
        return Ok(None);
    };
    let parent = destination
        .parent()
        .context("index path has no parent directory")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create index directory {}", parent.display()))?;

    // VACUUM INTO reads one consistent snapshot even while the sibling's
    // daemon is writing, and compacts free pages. Write beside the target and
    // rename so a concurrent opener never sees a half-written file.
    let staging = parent.join(format!("symbols.db.seed-{}", std::process::id()));
    let _ = fs::remove_file(&staging);
    let copy = || -> Result<()> {
        let conn = rusqlite::Connection::open_with_flags(
            &source,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("open seed source {}", source.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        // Watchdog: interrupt the copy at the deadline instead of letting
        // the bind wait on it.
        let interrupt = conn.get_interrupt_handle();
        let (done, finished) = std::sync::mpsc::channel::<()>();
        let watchdog = std::thread::Builder::new()
            .name("codelens-seed-watchdog".to_owned())
            .spawn(move || {
                use std::sync::mpsc::RecvTimeoutError;
                if finished.recv_timeout(timeout) != Err(RecvTimeoutError::Timeout) {
                    return;
                }
                // An interrupt issued before the statement starts is a no-op,
                // so keep interrupting until the copy reports back.
                loop {
                    interrupt.interrupt();
                    match finished.recv_timeout(std::time::Duration::from_millis(10)) {
                        Err(RecvTimeoutError::Timeout) => continue,
                        _ => return,
                    }
                }
            })
            .context("spawn seed watchdog")?;
        let copied = conn.execute("VACUUM INTO ?1", [staging.to_string_lossy().as_ref()]);
        let _ = done.send(());
        let _ = watchdog.join();
        copied.with_context(|| {
            format!(
                "copy seed index from {} (limit {}s)",
                source.display(),
                timeout.as_secs()
            )
        })?;
        Ok(())
    };
    if let Err(error) = copy() {
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    if destination.exists() {
        // Another opener created the index meanwhile; keep theirs.
        let _ = fs::remove_file(&staging);
        return Ok(None);
    }
    fs::rename(&staging, &destination)
        .with_context(|| format!("install seeded index at {}", destination.display()))?;
    Ok(Some(source))
}

/// The index to seed from, at the same path inside another checkout of the
/// repository (a project may be a subdirectory of the worktree): the main
/// checkout's when it has one — it is the long-lived, rarely rebuilt copy —
/// otherwise the linked worktree whose database file was written last. The
/// WAL is deliberately ignored: a fresh WAL marks an index being rebuilt
/// right now, the worst copy source.
fn preferred_sibling_index(project_root: &Path) -> Option<PathBuf> {
    let (toplevel, dot_git) = find_git_toplevel(project_root)?;
    let subpath = project_root.strip_prefix(&toplevel).ok()?.to_path_buf();
    let common_dir = git_common_dir(&toplevel, &dot_git)?;
    let own = canonical(&toplevel);
    let (main, linked) = sibling_checkouts(&common_dir);
    let candidate_for = |checkout: &PathBuf| -> Option<PathBuf> {
        if canonical(checkout) == own {
            return None;
        }
        let candidate = index_db_path(&checkout.join(&subpath));
        candidate.is_file().then_some(candidate)
    };
    if let Some(main_index) = main.as_ref().and_then(candidate_for) {
        return Some(main_index);
    }
    linked
        .iter()
        .filter_map(candidate_for)
        .filter_map(|candidate| {
            let written = fs::metadata(&candidate).ok()?.modified().ok()?;
            Some((written, candidate))
        })
        .max_by_key(|(written, _)| *written)
        .map(|(_, candidate)| candidate)
}

/// Nearest ancestor (inclusive) holding a `.git` entry.
fn find_git_toplevel(start: &Path) -> Option<(PathBuf, PathBuf)> {
    start.ancestors().find_map(|dir| {
        let dot_git = dir.join(".git");
        dot_git.exists().then(|| (dir.to_path_buf(), dot_git))
    })
}

/// Resolve the shared git directory for a main checkout (`.git` is a
/// directory) or a linked worktree (`.git` is a `gitdir:` file whose target
/// names the shared directory in its `commondir` file).
fn git_common_dir(toplevel: &Path, dot_git: &Path) -> Option<PathBuf> {
    if dot_git.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    let pointer = fs::read_to_string(dot_git).ok()?;
    let gitdir = pointer.strip_prefix("gitdir:")?.trim();
    let gitdir = resolve_relative(toplevel, gitdir);
    let common = fs::read_to_string(gitdir.join("commondir")).ok()?;
    // `commondir` is usually `../..`; normalize so the `.git` name test holds.
    Some(canonical(&resolve_relative(&gitdir, common.trim())))
}

/// The main checkout (for a non-bare repository) and every linked worktree
/// registered under `<common>/worktrees/*/gitdir`.
fn sibling_checkouts(common_dir: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
    let main = common_dir
        .file_name()
        .is_some_and(|name| name == ".git")
        .then(|| common_dir.parent().map(Path::to_path_buf))
        .flatten();
    let mut checkouts = Vec::new();
    let Ok(entries) = fs::read_dir(common_dir.join("worktrees")) else {
        return (main, checkouts);
    };
    for entry in entries.flatten() {
        let Ok(gitdir) = fs::read_to_string(entry.path().join("gitdir")) else {
            continue;
        };
        // `gitdir` names the worktree's `.git` file; its parent is the checkout.
        let dot_git = resolve_relative(&entry.path(), gitdir.trim());
        if let Some(checkout) = dot_git.parent() {
            checkouts.push(checkout.to_path_buf());
        }
    }
    (main, checkouts)
}

fn resolve_relative(base: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectRoot, SymbolIndex};

    fn scratch(label: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "codelens-seed-{label}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// Lay out `<root>/main` (a `.git` directory) and a linked worktree
    /// `<root>/wt` exactly as `git worktree add` does, each holding `sub/`.
    fn repository_with_worktree(label: &str) -> (PathBuf, PathBuf) {
        let root = scratch(label);
        let main = root.join("main");
        let worktree = root.join("wt");
        let admin = main.join(".git/worktrees/wt");
        fs::create_dir_all(&admin).expect("worktree admin dir");
        fs::create_dir_all(main.join("sub")).expect("main sub");
        fs::create_dir_all(worktree.join("sub")).expect("worktree sub");
        fs::write(admin.join("commondir"), "../..\n").expect("commondir");
        fs::write(
            admin.join("gitdir"),
            format!("{}\n", worktree.join(".git").display()),
        )
        .expect("gitdir");
        fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", admin.display()),
        )
        .expect("dot git file");
        for checkout in [&main, &worktree] {
            fs::write(
                checkout.join("sub/lib.py"),
                "def seeded_symbol():\n    pass\n",
            )
            .expect("source");
        }
        (main, worktree)
    }

    #[test]
    fn worktree_subproject_is_seeded_from_the_main_checkout() {
        let (main, worktree) = repository_with_worktree("subproject");
        let main_index =
            SymbolIndex::new(ProjectRoot::new_exact(main.join("sub")).expect("main root"))
                .expect("main index");
        main_index.refresh_all().expect("index main");
        drop(main_index);

        let seeded = seed_index_from_sibling_checkout(&worktree.join("sub")).expect("seed");

        assert_eq!(
            seeded.map(|path| canonical(&path)),
            Some(canonical(&index_db_path(&main.join("sub"))))
        );
        let index =
            SymbolIndex::new(ProjectRoot::new_exact(worktree.join("sub")).expect("worktree root"))
                .expect("seeded index");
        let stats = index.refresh_all().expect("refresh seeded index");
        assert_eq!(stats.indexed_files, 1);
        assert_eq!(stats.stale_files, 0);
        assert_eq!(
            index
                .find_symbol("seeded_symbol", None, false, true, 10)
                .expect("find")
                .len(),
            1
        );
    }

    #[test]
    fn main_checkout_is_preferred_over_a_newer_linked_worktree() {
        let (main, worktree) = repository_with_worktree("prefer-main");
        // A second linked worktree whose index is written after main's.
        let other = main.parent().expect("root").join("wt2");
        let admin = main.join(".git/worktrees/wt2");
        fs::create_dir_all(&admin).expect("admin");
        fs::create_dir_all(other.join("sub")).expect("sub");
        fs::write(admin.join("commondir"), "../..\n").expect("commondir");
        fs::write(
            admin.join("gitdir"),
            format!("{}\n", other.join(".git").display()),
        )
        .expect("gitdir");
        fs::write(other.join(".git"), format!("gitdir: {}\n", admin.display())).expect("dot git");
        fs::write(other.join("sub/lib.py"), "def seeded_symbol():\n    pass\n").expect("source");
        for checkout in [&main, &other] {
            SymbolIndex::new(ProjectRoot::new_exact(checkout.join("sub")).expect("root"))
                .expect("index")
                .refresh_all()
                .expect("refresh");
        }

        let seeded = seed_index_from_sibling_checkout(&worktree.join("sub")).expect("seed");

        assert_eq!(
            seeded.map(|path| canonical(&path)),
            Some(canonical(&index_db_path(&main.join("sub"))))
        );
    }

    #[test]
    fn a_copy_past_the_deadline_is_abandoned_without_installing_anything() {
        let (main, worktree) = repository_with_worktree("deadline");
        SymbolIndex::new(ProjectRoot::new_exact(main.join("sub")).expect("main root"))
            .expect("main index")
            .refresh_all()
            .expect("index main");

        let result = seed_index_with_timeout(&worktree.join("sub"), std::time::Duration::ZERO);

        assert!(result.is_err(), "a zero deadline must interrupt the copy");
        let index_dir = index_db_path(&worktree.join("sub"));
        assert!(!index_dir.exists(), "no partial index may be installed");
        let leftovers: Vec<_> = fs::read_dir(index_dir.parent().expect("dir"))
            .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(
            leftovers.is_empty(),
            "staging file must be removed: {leftovers:?}"
        );
    }

    #[test]
    fn existing_index_is_never_overwritten() {
        let (main, worktree) = repository_with_worktree("existing");
        SymbolIndex::new(ProjectRoot::new_exact(main.join("sub")).expect("main root"))
            .expect("main index")
            .refresh_all()
            .expect("index main");
        SymbolIndex::new(ProjectRoot::new_exact(worktree.join("sub")).expect("worktree root"))
            .expect("own index");

        let seeded = seed_index_from_sibling_checkout(&worktree.join("sub")).expect("seed");

        assert_eq!(seeded, None);
    }

    #[test]
    fn project_outside_git_has_nothing_to_seed() {
        let dir = scratch("nogit");
        assert_eq!(seed_index_from_sibling_checkout(&dir).expect("seed"), None);
    }
}
