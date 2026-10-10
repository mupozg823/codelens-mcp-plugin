//! Replacing a file's contents with no window in which it is empty or half
//! written.
//!
//! `fs::write` truncates the file before writing, so a process killed in
//! between, or a reader that looks at the wrong moment, sees a truncated
//! source file or memory. Edits and memories go to a temporary file beside
//! the target instead, which is then renamed over it.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `contents` to `path` atomically, as a drop-in for `fs::write`:
/// - an existing file keeps its permissions, and a read-only one is refused
///   as `fs::write` would refuse it (a rename alone would replace it);
/// - a symlink keeps pointing at its target, whose contents are replaced;
/// - a new file gets the same default permissions `fs::write` gives it;
/// - on any error the original file is untouched and no temporary is left.
///
/// A hard link to the file stops sharing it, since the path now names a new
/// inode.
pub fn write_atomic(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let target = resolve_target(path.as_ref());
    let existing = fs::metadata(&target).ok();
    if let Some(metadata) = &existing
        && metadata.permissions().readonly()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{} is read-only", target.display()),
        ));
    }
    let temp = temp_path_beside(&target)?;
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
    {
        Ok(file) => file,
        // A directory that takes no new entries but holds a writable file:
        // `fs::write` succeeded there, so keep doing that, non-atomically.
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied && existing.is_some() => {
            return fs::write(&target, contents);
        }
        Err(error) => return Err(error),
    };
    let result = (|| {
        file.write_all(contents.as_ref())?;
        file.sync_all()?;
        if let Some(metadata) = &existing {
            fs::set_permissions(&temp, metadata.permissions())?;
        }
        fs::rename(&temp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// An existing path resolves through symlinks so the link survives; a new
/// file is created where it was named.
fn resolve_target(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn temp_path_beside(target: &Path) -> io::Result<PathBuf> {
    let file_name = target.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} has no file name", target.display()),
        )
    })?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.join(format!(
        ".{}.{}.{}.codelens-tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    )))
}

#[cfg(test)]
mod tests {
    use super::write_atomic;
    use std::fs;

    fn leftovers(dir: &std::path::Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".codelens-tmp"))
            .collect()
    }

    #[test]
    fn a_concurrent_reader_never_sees_a_partial_file() {
        // `fs::write` truncates first, so a reader polling the file between
        // the truncate and the last byte sees a short read.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.rs");
        let a = "a".repeat(2 * 1024 * 1024);
        let b = "b".repeat(2 * 1024 * 1024);
        fs::write(&path, &a).unwrap();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let reader = {
            let path = path.clone();
            let done = std::sync::Arc::clone(&done);
            std::thread::spawn(move || {
                let mut torn = 0usize;
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok(bytes) = fs::read(&path)
                        && bytes.len() != 2 * 1024 * 1024
                    {
                        torn += 1;
                    }
                }
                torn
            })
        };
        for round in 0..60 {
            write_atomic(&path, if round % 2 == 0 { &b } else { &a }).unwrap();
        }
        done.store(true, std::sync::atomic::Ordering::Relaxed);

        assert_eq!(reader.join().unwrap(), 0, "a reader saw a partial file");
        assert!(leftovers(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn an_existing_file_keeps_its_permissions_and_a_symlink_stays_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("run.sh");
        fs::write(&script, "echo old\n").unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o750)).unwrap();
        let link = dir.path().join("link.sh");
        std::os::unix::fs::symlink(&script, &link).unwrap();

        write_atomic(&link, "echo new\n").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&script).unwrap(), "echo new\n");
        assert_eq!(
            fs::metadata(&script).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert!(leftovers(dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_new_file_gets_the_permissions_fs_write_would_give_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("reference.txt"), "x").unwrap();

        write_atomic(dir.path().join("created.txt"), "x").unwrap();

        let mode = |name: &str| {
            fs::metadata(dir.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("created.txt"), mode("reference.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn a_writable_file_in_a_closed_directory_is_still_written() {
        // fs::write managed this, so the atomic path must not start failing.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let closed = dir.path().join("closed");
        fs::create_dir(&closed).unwrap();
        let path = closed.join("notes.md");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o555)).unwrap();

        let result = write_atomic(&path, "new");
        fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();

        result.unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn a_read_only_file_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("locked.txt");
        fs::write(&path, "original").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();

        let error = write_atomic(&path, "replacement").unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        assert!(leftovers(dir.path()).is_empty());
    }
}
