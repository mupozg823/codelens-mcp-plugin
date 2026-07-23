use crate::error::CodeLensError;
use codelens_engine::ProjectRoot;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
struct LeaseMetadata {
    generation: u64,
    pid: u32,
    project: String,
    version: String,
    git_sha: String,
    acquired_at_ms: u64,
}

/// Process-owned authority for every writable runtime associated with one
/// project. The open file and its OS advisory lock remain alive for the whole
/// `ProjectContext` lifetime.
#[derive(Debug)]
pub(super) struct ProjectRuntimeLease {
    file: File,
    project: PathBuf,
    lock_path: PathBuf,
    generation: u64,
}

impl ProjectRuntimeLease {
    pub(super) fn try_acquire(project: &ProjectRoot) -> Result<Self, CodeLensError> {
        let runtime_dir = trusted_runtime_dir()?;
        std::fs::create_dir_all(&runtime_dir)?;
        let lock_path = runtime_dir.join(project_lock_name(project));
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;

        if let Err(error) = file.try_lock_exclusive() {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                let holder = read_holder_metadata(&mut file);
                return Err(CodeLensError::ProjectWriterBusy {
                    project: project.as_path().display().to_string(),
                    lock_path: lock_path.display().to_string(),
                    holder,
                });
            }
            return Err(CodeLensError::Io(error));
        }

        let previous_generation = read_generation(&mut file).unwrap_or(0);
        let generation = previous_generation.saturating_add(1);
        let metadata = LeaseMetadata {
            generation,
            pid: std::process::id(),
            project: project.as_path().display().to_string(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            git_sha: crate::build_info::BUILD_GIT_SHA.to_owned(),
            acquired_at_ms: crate::util::now_ms(),
        };
        write_metadata(&mut file, &metadata)?;

        Ok(Self {
            file,
            project: project.as_path().to_path_buf(),
            lock_path,
            generation,
        })
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn project(&self) -> &std::path::Path {
        &self.project
    }

    pub(super) fn lock_path(&self) -> &std::path::Path {
        &self.lock_path
    }

    pub(super) fn health_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "lease_health": "held",
            "generation": self.generation,
            "project": self.project.display().to_string(),
            "lock_path": self.lock_path.display().to_string(),
            "writer_owner": {
                "pid": std::process::id(),
                "version": crate::build_info::BUILD_VERSION,
                "git_sha": crate::build_info::BUILD_GIT_SHA,
            },
        })
    }
}

/// Lock authority must live outside the repository. A project-local path lets
/// an untrusted checkout replace the lock with a symlink and redirect metadata
/// truncation to another user file.
fn trusted_runtime_dir() -> Result<PathBuf, CodeLensError> {
    let home_drive_path = match (std::env::var_os("HOMEDRIVE"), std::env::var_os("HOMEPATH")) {
        (Some(mut drive), Some(path)) => {
            drive.push(path);
            Some(PathBuf::from(drive))
        }
        _ => None,
    };
    let home = select_trusted_home(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("USERPROFILE").map(PathBuf::from),
        home_drive_path,
    )
    .ok_or_else(|| {
        CodeLensError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "an absolute HOME or Windows user profile is required for the trusted CodeLens runtime directory",
        ))
    })?;
    Ok(home
        .join(".codelens")
        .join("runtime")
        .join("project-writers"))
}

fn select_trusted_home(
    home: Option<PathBuf>,
    user_profile: Option<PathBuf>,
    home_drive_path: Option<PathBuf>,
) -> Option<PathBuf> {
    [home, user_profile, home_drive_path]
        .into_iter()
        .flatten()
        .find(|candidate| candidate.is_absolute())
}

fn project_lock_name(project: &ProjectRoot) -> String {
    let identity = std::fs::canonicalize(project.as_path())
        .unwrap_or_else(|_| project.as_path().to_path_buf());
    let digest = Sha256::digest(identity.to_string_lossy().as_bytes());
    format!("{digest:x}.lock")
}

impl Drop for ProjectRuntimeLease {
    fn drop(&mut self) {
        if let Err(error) = fs2::FileExt::unlock(&self.file) {
            tracing::warn!(
                project = %self.project.display(),
                lock_path = %self.lock_path.display(),
                %error,
                "failed to explicitly release project writer lease; file close remains the release boundary"
            );
        }
    }
}

fn read_generation(file: &mut File) -> Option<u64> {
    read_metadata(file).map(|metadata| metadata.generation)
}

fn read_holder_metadata(file: &mut File) -> Option<String> {
    read_metadata(file).and_then(|metadata| serde_json::to_string(&metadata).ok())
}

fn read_metadata(file: &mut File) -> Option<LeaseMetadata> {
    if file.seek(SeekFrom::Start(0)).is_err() {
        return None;
    }
    let mut contents = String::new();
    if file.read_to_string(&mut contents).is_err() || contents.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&contents).ok()
}

fn write_metadata(file: &mut File, metadata: &LeaseMetadata) -> Result<(), CodeLensError> {
    let bytes = serde_json::to_vec(metadata)
        .map_err(|error| CodeLensError::Internal(anyhow::Error::new(error)))?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.sync_data()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ProjectRuntimeLease, select_trusted_home};
    use crate::error::CodeLensError;
    use crate::test_helpers::fixtures::temp_project_root;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, Instant};

    const CHILD_PROJECT_ENV: &str = "CODELENS_TEST_LEASE_CHILD_PROJECT";
    const CHILD_READY_ENV: &str = "CODELENS_TEST_LEASE_CHILD_READY";
    const CHILD_RELEASE_ENV: &str = "CODELENS_TEST_LEASE_CHILD_RELEASE";

    #[test]
    fn trusted_runtime_home_falls_back_to_windows_user_profile() {
        let profile = std::env::temp_dir().join("codelens-test-user-profile");
        assert!(profile.is_absolute());
        assert_eq!(
            select_trusted_home(None, Some(profile.clone()), None),
            Some(profile)
        );
    }

    #[test]
    fn trusted_runtime_home_ignores_relative_candidates() {
        let fallback = std::env::temp_dir().join("codelens-test-home-fallback");
        assert_eq!(
            select_trusted_home(
                Some(PathBuf::from("relative-home")),
                Some(fallback.clone()),
                None,
            ),
            Some(fallback)
        );
    }

    #[test]
    fn project_runtime_lease_child_holder() {
        let Some(project_path) = std::env::var_os(CHILD_PROJECT_ENV) else {
            return;
        };
        let ready_path =
            std::path::PathBuf::from(std::env::var_os(CHILD_READY_ENV).expect("child ready path"));
        let release_path = std::path::PathBuf::from(
            std::env::var_os(CHILD_RELEASE_ENV).expect("child release path"),
        );
        let project = codelens_engine::ProjectRoot::new(project_path).expect("child project");
        let _lease = ProjectRuntimeLease::try_acquire(&project).expect("child lease");
        std::fs::write(&ready_path, b"ready").expect("write ready marker");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !release_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(release_path.exists(), "parent did not release child holder");
    }

    #[test]
    fn project_runtime_lease_is_exclusive_across_processes_and_releases_on_exit() {
        let project = temp_project_root("runtime-lease-process");
        let ready_path = project.as_path().join("lease-child.ready");
        let release_path = project.as_path().join("lease-child.release");
        let current_exe = std::env::current_exe().expect("current test executable");
        let mut child = Command::new(current_exe)
            .arg("--exact")
            .arg("state::project_runtime_lease::tests::project_runtime_lease_child_holder")
            .arg("--nocapture")
            .env(CHILD_PROJECT_ENV, project.as_path())
            .env(CHILD_READY_ENV, &ready_path)
            .env(CHILD_RELEASE_ENV, &release_path)
            .spawn()
            .expect("spawn lease holder");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready_path.exists(), "child did not acquire lease in time");

        let busy = ProjectRuntimeLease::try_acquire(&project)
            .expect_err("second process must not acquire the project writer lease");
        assert!(
            matches!(busy, CodeLensError::ProjectWriterBusy { .. }),
            "unexpected busy error: {busy}"
        );

        std::fs::write(&release_path, b"release").expect("release child holder");
        let status = child.wait().expect("wait for lease holder");
        assert!(status.success(), "child lease holder failed: {status}");
        ProjectRuntimeLease::try_acquire(&project)
            .expect("lease must become available after the holder exits");
    }

    #[test]
    fn project_runtime_lease_releases_after_holder_is_killed_and_advances_generation() {
        let project = temp_project_root("runtime-lease-crash-release");
        let baseline_generation = ProjectRuntimeLease::try_acquire(&project)
            .expect("baseline lease")
            .generation();

        let ready_path = project.as_path().join("lease-crash-child.ready");
        let never_release_path = project.as_path().join("lease-crash-child.release");
        let current_exe = std::env::current_exe().expect("current test executable");
        let mut child = Command::new(current_exe)
            .arg("--exact")
            .arg("state::project_runtime_lease::tests::project_runtime_lease_child_holder")
            .arg("--nocapture")
            .env(CHILD_PROJECT_ENV, project.as_path())
            .env(CHILD_READY_ENV, &ready_path)
            .env(CHILD_RELEASE_ENV, &never_release_path)
            .spawn()
            .expect("spawn crash lease holder");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ready_path.exists(),
            "crash child did not acquire lease in time"
        );

        child.kill().expect("kill lease holder");
        let status = child.wait().expect("wait for killed lease holder");
        assert!(
            !status.success(),
            "killed holder unexpectedly exited cleanly"
        );

        let reacquired = ProjectRuntimeLease::try_acquire(&project)
            .expect("OS must release the project lease when the holder is killed");
        assert!(
            reacquired.generation() >= baseline_generation.saturating_add(2),
            "the killed acquisition and reacquisition must both advance durable generation: baseline={baseline_generation}, after={}",
            reacquired.generation()
        );
    }

    #[test]
    fn project_runtime_app_state_child_holder() {
        let Some(project_path) = std::env::var_os(CHILD_PROJECT_ENV) else {
            return;
        };
        let ready_path =
            std::path::PathBuf::from(std::env::var_os(CHILD_READY_ENV).expect("child ready path"));
        let release_path = std::path::PathBuf::from(
            std::env::var_os(CHILD_RELEASE_ENV).expect("child release path"),
        );
        let project = codelens_engine::ProjectRoot::new(project_path).expect("child project");
        let _state =
            crate::AppState::try_new_minimal(project, crate::tool_defs::ToolPreset::Balanced)
                .expect("child app state");
        std::fs::write(&ready_path, b"ready").expect("write ready marker");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !release_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            release_path.exists(),
            "parent did not release child runtime"
        );
    }

    #[test]
    fn full_project_runtime_rejects_second_process_before_wal_contention() {
        let project = temp_project_root("runtime-process-app-state");
        let ready_path = project.as_path().join("runtime-child.ready");
        let release_path = project.as_path().join("runtime-child.release");
        let current_exe = std::env::current_exe().expect("current test executable");
        let mut child = Command::new(current_exe)
            .arg("--exact")
            .arg("state::project_runtime_lease::tests::project_runtime_app_state_child_holder")
            .arg("--nocapture")
            .env(CHILD_PROJECT_ENV, project.as_path())
            .env(CHILD_READY_ENV, &ready_path)
            .env(CHILD_RELEASE_ENV, &release_path)
            .spawn()
            .expect("spawn project runtime holder");

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready_path.exists(), "child runtime did not start in time");

        let error = match crate::AppState::try_new_minimal(
            project.clone(),
            crate::tool_defs::ToolPreset::Balanced,
        ) {
            Ok(_) => panic!("a second process unexpectedly opened the same writable runtime"),
            Err(error) => error,
        };
        let structured = error
            .downcast::<CodeLensError>()
            .expect("busy error must remain typed");
        assert!(matches!(
            structured,
            CodeLensError::ProjectWriterBusy { .. }
        ));

        std::fs::write(&release_path, b"release").expect("release child runtime");
        let status = child.wait().expect("wait for project runtime holder");
        assert!(status.success(), "child project runtime failed: {status}");
        crate::AppState::try_new_minimal(project, crate::tool_defs::ToolPreset::Balanced)
            .expect("writer runtime must become available after child exit");
    }

    #[test]
    fn second_app_state_for_same_project_returns_project_writer_busy() {
        let project = temp_project_root("runtime-lease-app-state");
        let first = crate::AppState::try_new_minimal(
            project.clone(),
            crate::tool_defs::ToolPreset::Balanced,
        )
        .expect("first app state");

        let error = match crate::AppState::try_new_minimal(
            project.clone(),
            crate::tool_defs::ToolPreset::Balanced,
        ) {
            Ok(_) => panic!("second app state unexpectedly acquired the project writer lease"),
            Err(error) => error,
        };
        let structured = error
            .downcast::<CodeLensError>()
            .expect("busy error must remain typed");
        assert!(matches!(
            structured,
            CodeLensError::ProjectWriterBusy { .. }
        ));

        drop(first);
        crate::AppState::try_new_minimal(project, crate::tool_defs::ToolPreset::Balanced)
            .expect("lease must be reusable after the first state drops");
    }

    #[cfg(unix)]
    #[test]
    fn project_runtime_lease_never_follows_repo_controlled_lock_symlink() {
        use std::os::unix::fs::symlink;

        let project = temp_project_root("runtime-lease-symlink");
        let protected = project.as_path().join("protected.txt");
        std::fs::write(&protected, b"must remain unchanged").unwrap();
        let repo_runtime = project.as_path().join(".codelens/runtime");
        std::fs::create_dir_all(&repo_runtime).unwrap();
        let repo_lock = repo_runtime.join("project-writer.lock");
        symlink(&protected, &repo_lock).unwrap();

        let lease = ProjectRuntimeLease::try_acquire(&project).expect("trusted lease path");

        assert_ne!(lease.lock_path(), repo_lock.as_path());
        assert_eq!(
            std::fs::read(&protected).unwrap(),
            b"must remain unchanged",
            "a repository-controlled symlink must never be opened or truncated"
        );
    }
}
