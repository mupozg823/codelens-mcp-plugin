//! Durable soft state of HTTP sessions, so a session keeps its project
//! binding and client identity when the daemon restarts or the session idles
//! out.
//!
//! Sessions lived only in daemon memory. A client that came back after a
//! restart (every redeploy) or 30 idle minutes was resurrected under the same
//! id (#300) but unbound, so its reads went to the daemon's default project
//! until it called `prepare_harness_session` again, and its `client_name` was
//! gone from telemetry (39 of 233 sessions in two weeks had none on any row).
//!
//! Only soft state is written: the project binding when the caller chose it
//! (initialize parameter or an explicit tool; a request header re-asserts
//! itself on every request), client name, version and host context, and the
//! requested profile. `trusted_client` and every other privilege-bearing
//! field are never stored (guard #2). One small JSON file per session id,
//! owner-only, written atomically; removed on an explicit DELETE and pruned
//! after `ttl`.

use super::project_binding::ProjectBindingSource;
use super::session::SessionClientMetadata;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct JournalEntry {
    pub(crate) client_name: Option<String>,
    pub(crate) client_version: Option<String>,
    pub(crate) host_context: Option<String>,
    pub(crate) requested_profile: Option<String>,
    pub(crate) project_path: Option<String>,
    pub(crate) project_binding_source: Option<String>,
}

impl JournalEntry {
    fn from_metadata(metadata: &SessionClientMetadata) -> Self {
        let chosen = matches!(
            metadata.project_binding_source,
            ProjectBindingSource::ExplicitTool | ProjectBindingSource::InitializeParam
        );
        Self {
            client_name: metadata.client_name.clone(),
            client_version: metadata.client_version.clone(),
            host_context: metadata.host_context.clone(),
            requested_profile: metadata.requested_profile.clone(),
            project_path: metadata.project_path.clone().filter(|_| chosen),
            project_binding_source: chosen
                .then(|| metadata.project_binding_source.as_str().to_owned()),
        }
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub(crate) fn binding_source(&self) -> Option<ProjectBindingSource> {
        match self.project_binding_source.as_deref()? {
            "explicit_tool" => Some(ProjectBindingSource::ExplicitTool),
            "initialize_param" => Some(ProjectBindingSource::InitializeParam),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SessionJournal {
    dir: PathBuf,
    ttl: Duration,
    cap: usize,
}

impl SessionJournal {
    pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
    pub(crate) const DEFAULT_CAP: usize = 2048;

    pub(crate) fn at(dir: PathBuf, ttl: Duration, cap: usize) -> Self {
        Self { dir, ttl, cap }
    }

    /// The journal under the trusted runtime directory, or `None` when it is
    /// disabled (`CODELENS_SESSION_JOURNAL=0`) or the directory cannot be
    /// created.
    pub(crate) fn default_location() -> Option<Self> {
        if matches!(
            std::env::var("CODELENS_SESSION_JOURNAL")
                .ok()
                .as_deref()
                .map(str::trim),
            Some("0" | "false" | "off" | "no")
        ) {
            return None;
        }
        let dir = crate::state::runtime_dir().ok()?.join("sessions");
        std::fs::create_dir_all(&dir).ok()?;
        let journal = Self::at(dir, Self::DEFAULT_TTL, Self::DEFAULT_CAP);
        journal.prune();
        Some(journal)
    }

    fn path_for(&self, id: &str) -> Option<PathBuf> {
        // Ids are UUID-shaped (validated by the store); refuse anything that
        // could leave the directory.
        let safe = !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        safe.then(|| self.dir.join(format!("{id}.json")))
    }

    pub(crate) fn record(&self, id: &str, metadata: &SessionClientMetadata) {
        let Some(path) = self.path_for(id) else {
            return;
        };
        let entry = JournalEntry::from_metadata(metadata);
        if entry.is_empty() {
            return;
        }
        if let Err(error) = write_owner_only(&path, &entry) {
            tracing::debug!(%error, path = %path.display(), "session journal write skipped");
        }
    }

    pub(crate) fn load(&self, id: &str) -> Option<JournalEntry> {
        let path = self.path_for(id)?;
        let age = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())?;
        if age > self.ttl {
            let _ = std::fs::remove_file(&path);
            return None;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub(crate) fn forget(&self, id: &str) {
        if let Some(path) = self.path_for(id) {
            let _ = std::fs::remove_file(path);
        }
    }

    /// Drop entries older than the TTL, then the oldest beyond the cap.
    pub(crate) fn prune(&self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        let mut kept = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
            match modified.and_then(|time| now.duration_since(time).ok()) {
                Some(age) if age <= self.ttl => kept.push((modified, path)),
                _ => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        if kept.len() > self.cap {
            kept.sort_by_key(|(modified, _)| *modified);
            let excess = kept.len() - self.cap;
            for (_, path) in kept.into_iter().take(excess) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn write_owner_only(path: &Path, entry: &JournalEntry) -> std::io::Result<()> {
    let staging = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let body = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staging)?;
        std::io::Write::write_all(&mut file, &body)?;
    }
    std::fs::rename(&staging, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(label: &str) -> (tempfile::TempDir, SessionJournal) {
        let dir = tempfile::Builder::new()
            .prefix(&format!("codelens-journal-{label}-"))
            .tempdir()
            .unwrap();
        let journal = SessionJournal::at(dir.path().to_path_buf(), Duration::from_secs(60), 3);
        (dir, journal)
    }

    fn metadata(source: ProjectBindingSource) -> SessionClientMetadata {
        SessionClientMetadata {
            client_name: Some("claude-code".to_owned()),
            client_version: Some("2.1.296".to_owned()),
            host_context: Some("claude-code".to_owned()),
            trusted_client: Some(true),
            project_path: Some("/repo".to_owned()),
            project_binding_source: source,
            ..SessionClientMetadata::default()
        }
    }

    #[test]
    fn records_identity_and_a_chosen_binding_but_never_trust() {
        let (_dir, journal) = journal("record");
        let id = "0b8d4c1e-1111-4222-8333-944455556666";
        journal.record(id, &metadata(ProjectBindingSource::ExplicitTool));

        let entry = journal.load(id).expect("entry");
        assert_eq!(entry.client_name.as_deref(), Some("claude-code"));
        assert_eq!(entry.project_path.as_deref(), Some("/repo"));
        assert_eq!(
            entry.binding_source(),
            Some(ProjectBindingSource::ExplicitTool)
        );
        let raw = std::fs::read_to_string(journal.path_for(id).unwrap()).unwrap();
        assert!(!raw.contains("trusted"), "{raw}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(journal.path_for(id).unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_header_or_default_binding_is_not_recorded() {
        let (_dir, journal) = journal("header");
        let id = "0b8d4c1e-1111-4222-8333-944455556667";
        journal.record(id, &metadata(ProjectBindingSource::RequestHeader));
        let entry = journal.load(id).expect("identity is still recorded");
        assert_eq!(entry.project_path, None);
        assert_eq!(entry.binding_source(), None);
    }

    #[test]
    fn forget_and_unsafe_ids() {
        let (_dir, journal) = journal("forget");
        let id = "0b8d4c1e-1111-4222-8333-944455556668";
        journal.record(id, &metadata(ProjectBindingSource::ExplicitTool));
        journal.forget(id);
        assert!(journal.load(id).is_none());
        journal.record("../escape", &metadata(ProjectBindingSource::ExplicitTool));
        assert!(journal.load("../escape").is_none());
    }

    #[test]
    fn prune_keeps_the_newest_within_the_cap() {
        let (_dir, journal) = journal("prune");
        let ids = [
            "0b8d4c1e-1111-4222-8333-000000000001",
            "0b8d4c1e-1111-4222-8333-000000000002",
            "0b8d4c1e-1111-4222-8333-000000000003",
            "0b8d4c1e-1111-4222-8333-000000000004",
        ];
        for (index, id) in ids.iter().enumerate() {
            journal.record(id, &metadata(ProjectBindingSource::ExplicitTool));
            let when = SystemTime::now() - Duration::from_secs(40 - index as u64 * 10);
            std::fs::File::options()
                .write(true)
                .open(journal.path_for(id).unwrap())
                .unwrap()
                .set_modified(when)
                .unwrap();
        }
        journal.prune();
        assert!(journal.load(ids[0]).is_none(), "oldest beyond the cap goes");
        assert!(journal.load(ids[3]).is_some());
    }
}
