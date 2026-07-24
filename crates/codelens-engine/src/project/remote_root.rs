//! Reject project roots on remote/network filesystems by default (ADR-0018 P0).
//!
//! The `.codelens/` runtime (SQLite index, WAL, analysis caches) lives inside
//! the project root. Network filesystems break SQLite's locking assumptions,
//! and a stalled network mount can wedge the shared HTTP daemon for every
//! session, so a remote root is refused at construction unless the operator
//! opts in via `CODELENS_ALLOW_REMOTE_PROJECT_ROOT=1` (`SYMBIOTE_` prefix
//! accepted). Detection failure fails open — an unreadable statfs must never
//! block local work. FUSE-backed remotes (sshfs, cloud drives) present as
//! local filesystems and stay out of scope.

use anyhow::{Result, bail};
use std::path::Path;

const OVERRIDE_ENV: &str = "CODELENS_ALLOW_REMOTE_PROJECT_ROOT";
const OVERRIDE_ENV_ALT: &str = "SYMBIOTE_ALLOW_REMOTE_PROJECT_ROOT";

pub(super) fn ensure_local_root(root: &Path) -> Result<()> {
    ensure_local_root_inner(root, remote_fs_kind(root), override_enabled())
}

fn ensure_local_root_inner(root: &Path, detected: Option<String>, allow: bool) -> Result<()> {
    let Some(kind) = detected else {
        return Ok(());
    };
    if allow {
        return Ok(());
    }
    bail!(
        "project root {} is on a remote filesystem ({kind}); the .codelens index \
         (SQLite) is unsafe on network mounts and a stalled mount can wedge the \
         shared daemon. Set {OVERRIDE_ENV}=1 to override.",
        root.display()
    );
}

fn override_enabled() -> bool {
    [OVERRIDE_ENV, OVERRIDE_ENV_ALT].iter().any(|name| {
        std::env::var(name)
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    })
}

/// Well-known network filesystem kinds. Conservative: only kinds that are
/// remote by construction reject; unknown kinds are treated as local.
fn is_remote_fs_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_lowercase().as_str(),
        "smbfs" | "smb" | "smb2" | "cifs" | "nfs" | "nfs4" | "afpfs" | "webdav" | "ftp"
    )
}

#[cfg(target_os = "macos")]
fn remote_fs_kind(path: &Path) -> Option<String> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let name = unsafe { CStr::from_ptr(stats.f_fstypename.as_ptr()) };
    let kind = name.to_string_lossy().into_owned();
    is_remote_fs_kind(&kind).then_some(kind)
}

#[cfg(target_os = "linux")]
fn remote_fs_kind(path: &Path) -> Option<String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stats: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c_path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let kind = match stats.f_type as u32 {
        0x6969 => "nfs",       // NFS_SUPER_MAGIC
        0x517b => "smb",       // SMB_SUPER_MAGIC
        0xff53_4d42 => "cifs", // CIFS_MAGIC_NUMBER
        0xfe53_4d42 => "smb2", // SMB2_MAGIC_NUMBER
        _ => return None,
    };
    Some(kind.to_owned())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remote_fs_kind(_path: &Path) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn remote_kinds_classify_as_remote() {
        for kind in [
            "smbfs", "SMBFS", "smb", "smb2", "cifs", "nfs", "nfs4", "afpfs", "webdav",
        ] {
            assert!(is_remote_fs_kind(kind), "{kind} must classify as remote");
        }
    }

    #[test]
    fn local_kinds_stay_local() {
        for kind in [
            "apfs", "hfs", "ext4", "btrfs", "xfs", "tmpfs", "zfs", "overlay", "",
        ] {
            assert!(!is_remote_fs_kind(kind), "{kind} must classify as local");
        }
    }

    #[test]
    fn detected_remote_root_is_rejected_without_override() {
        let root = PathBuf::from("/tmp/fake-remote");
        let error = ensure_local_root_inner(&root, Some("smbfs".to_owned()), false)
            .expect_err("remote kind without override must reject");
        let message = error.to_string();
        assert!(
            message.contains("smbfs"),
            "message names the kind: {message}"
        );
        assert!(
            message.contains(OVERRIDE_ENV),
            "message names the escape hatch: {message}"
        );
    }

    #[test]
    fn override_admits_detected_remote_root() {
        let root = PathBuf::from("/tmp/fake-remote");
        ensure_local_root_inner(&root, Some("nfs".to_owned()), true)
            .expect("override must admit a remote root");
    }

    #[test]
    fn undetected_root_passes() {
        let root = PathBuf::from("/tmp/local");
        ensure_local_root_inner(&root, None, false).expect("local root must pass");
    }

    #[test]
    fn real_local_tempdir_constructs() {
        let dir = tempfile::tempdir().expect("tempdir");
        ensure_local_root(dir.path()).expect("local tempdir must pass the live detector");
    }
}
