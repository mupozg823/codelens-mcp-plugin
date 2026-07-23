use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::CodeLensError;
use crate::runtime_types::{
    AnalysisArtifact, AnalysisReadiness, AnalysisSummary, AnalysisVerifierCheck,
};
use crate::util::matches_scope;

pub(crate) const MAX_ANALYSIS_ARTIFACTS: usize = 50;
const TTL_MS: u64 = 6 * 60 * 60 * 1000; // 6 hours
const STAGING_CLEANUP_GRACE_MS: u64 = 5 * 60 * 1000; // 5 minutes

/// Runtime cap override. `CODELENS_MAX_ANALYSIS_ARTIFACTS` accepts any non-zero
/// usize; falls back to [`MAX_ANALYSIS_ARTIFACTS`] when unset or invalid.
fn configured_max_analysis_artifacts() -> usize {
    std::env::var("CODELENS_MAX_ANALYSIS_ARTIFACTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(MAX_ANALYSIS_ARTIFACTS)
}

/// Runtime TTL override (hours). `CODELENS_ANALYSIS_TTL_HOURS` accepts any
/// non-zero u64; falls back to [`TTL_MS`] (6 h) when unset or invalid.
fn configured_analysis_ttl_ms() -> u64 {
    std::env::var("CODELENS_ANALYSIS_TTL_HOURS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map(|h| h.saturating_mul(60 * 60 * 1000))
        .unwrap_or(TTL_MS)
}

pub(crate) struct AnalysisArtifactStore {
    analysis_dir: Mutex<PathBuf>,
    disk_io: Mutex<()>,
    seq: std::sync::atomic::AtomicU64,
    order: Mutex<VecDeque<String>>,
    artifacts: Mutex<HashMap<String, AnalysisArtifact>>,
    artifact_dirs: Mutex<HashMap<String, PathBuf>>,
    #[cfg(test)]
    summary_reads: std::sync::atomic::AtomicU64,
}

impl AnalysisArtifactStore {
    pub fn new(analysis_dir: PathBuf) -> Self {
        Self {
            analysis_dir: Mutex::new(analysis_dir),
            disk_io: Mutex::new(()),
            seq: std::sync::atomic::AtomicU64::new(0),
            order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_ARTIFACTS)),
            artifacts: Mutex::new(HashMap::new()),
            artifact_dirs: Mutex::new(HashMap::new()),
            #[cfg(test)]
            summary_reads: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn analysis_dir(&self) -> PathBuf {
        self.analysis_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_analysis_dir(&self, dir: PathBuf) {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        *self.analysis_dir.lock().unwrap_or_else(|p| p.into_inner()) = dir;
    }

    fn artifact_dir_in(&self, analysis_dir: &Path, analysis_id: &str) -> PathBuf {
        analysis_dir.join(analysis_id)
    }

    fn staging_dir_in(&self, analysis_dir: &Path, analysis_id: &str) -> PathBuf {
        analysis_dir.join(format!(".{analysis_id}.tmp"))
    }

    fn sanitize_section_name(section: &str) -> String {
        section
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn section_file_name(section: &str) -> Result<String, CodeLensError> {
        let sanitized = Self::sanitize_section_name(section);
        if section.is_empty()
            || section.len() > 128
            || sanitized != section
            || sanitized == "summary"
        {
            return Err(CodeLensError::Validation(format!(
                "invalid analysis section name `{section}`"
            )));
        }
        Ok(format!("{sanitized}.json"))
    }

    fn matches_project_scope(stored_scope: Option<&str>, requested_scope: &str) -> bool {
        stored_scope.is_some() && matches_scope(stored_scope, Some(requested_scope))
    }

    fn write_file_atomically(path: &std::path::Path, bytes: &[u8]) -> Result<(), CodeLensError> {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| CodeLensError::Validation("invalid artifact file path".to_owned()))?;
        let temporary = path.with_file_name(format!(".{file_name}.tmp"));
        let result = (|| {
            fs::write(&temporary, bytes)?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn expired(created_at_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(created_at_ms) > configured_analysis_ttl_ms()
    }

    fn remember_order(&self, analysis_id: &str) {
        let mut order = self.order.lock().unwrap_or_else(|p| p.into_inner());
        if !order.iter().any(|existing| existing == analysis_id) {
            order.push_back(analysis_id.to_owned());
        }
    }

    fn staging_created_at_ms(path: &Path, name: &str) -> Option<u64> {
        name.strip_prefix(".analysis-")
            .and_then(|name| name.strip_suffix(".tmp"))
            .and_then(|analysis_suffix| analysis_suffix.split('-').next())
            .and_then(|timestamp| timestamp.parse::<u64>().ok())
            .or_else(|| {
                fs::metadata(path)
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            })
    }

    fn staging_is_stale(path: &Path, name: &str, now_ms: u64) -> bool {
        Self::staging_created_at_ms(path, name).is_some_and(|created_at_ms| {
            now_ms.saturating_sub(created_at_ms) > STAGING_CLEANUP_GRACE_MS
        })
    }

    // ── Disk I/O ────────────────────────────────────────────────────────

    fn write_to_disk_in_dir(
        &self,
        analysis_dir: &Path,
        artifact: &AnalysisArtifact,
        sections: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<(), CodeLensError> {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.artifact_dir_in(analysis_dir, &artifact.id);
        let staging = self.staging_dir_in(analysis_dir, &artifact.id);
        if dir.exists() {
            return Err(CodeLensError::Validation(format!(
                "analysis artifact `{}` already exists",
                artifact.id
            )));
        }
        let summary_bytes =
            serde_json::to_vec_pretty(artifact).map_err(|e| CodeLensError::Internal(e.into()))?;
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging)?;
        let publish_result = (|| {
            for (section, value) in sections {
                let file_name = Self::section_file_name(section)?;
                let bytes = serde_json::to_vec_pretty(value)
                    .map_err(|e| CodeLensError::Internal(e.into()))?;
                fs::write(staging.join(file_name), bytes)?;
            }
            fs::write(staging.join("summary.json"), summary_bytes)?;
            fs::rename(&staging, &dir)?;
            Ok(())
        })();
        if publish_result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        publish_result
    }

    fn read_from_disk_in_dir(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        project_scope: &str,
    ) -> Option<AnalysisArtifact> {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        self.read_from_disk_locked(analysis_dir, analysis_id, project_scope)
    }

    fn read_from_disk_locked(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        project_scope: &str,
    ) -> Option<AnalysisArtifact> {
        let artifact = self.read_summary_from_disk_locked(analysis_dir, analysis_id)?;
        if Self::expired(artifact.created_at_ms, crate::util::now_ms()) {
            self.remove_from_disk_locked(analysis_dir, analysis_id);
            return None;
        }
        Self::matches_project_scope(artifact.project_scope.as_deref(), project_scope)
            .then_some(artifact)
    }

    fn read_summary_from_disk_locked(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
    ) -> Option<AnalysisArtifact> {
        #[cfg(test)]
        self.summary_reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = self
            .artifact_dir_in(analysis_dir, analysis_id)
            .join("summary.json");
        let artifact = fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AnalysisArtifact>(&bytes).ok())?;
        (artifact.id == analysis_id).then_some(artifact)
    }

    fn list_artifacts_on_disk_locked(
        &self,
        analysis_dir: &Path,
    ) -> Vec<(String, AnalysisArtifact)> {
        self.list_ids_on_disk_locked(analysis_dir)
            .into_iter()
            .filter_map(|id| {
                self.read_summary_from_disk_locked(analysis_dir, &id)
                    .map(|artifact| (id, artifact))
            })
            .collect()
    }

    fn remove_from_disk_locked(&self, analysis_dir: &Path, analysis_id: &str) {
        let _ = fs::remove_dir_all(self.artifact_dir_in(analysis_dir, analysis_id));
    }

    fn list_ids_on_disk_in_dir(&self, analysis_dir: &Path) -> Vec<String> {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        self.list_ids_on_disk_locked(analysis_dir)
    }

    fn list_ids_on_disk_locked(&self, analysis_dir: &Path) -> Vec<String> {
        let entries = match fs::read_dir(analysis_dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                path.is_dir().then(|| {
                    path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                })
            })
            .filter(|name| !name.is_empty() && !name.starts_with('.') && name != "jobs")
            .collect()
    }

    // ── Cleanup / Prune ─────────────────────────────────────────────────

    pub fn cleanup_stale_dirs(&self, now_ms: u64) {
        let analysis_dir = self.analysis_dir();
        self.cleanup_stale_dirs_in_dir(&analysis_dir, now_ms);
    }

    pub fn cleanup_stale_dirs_in_dir(&self, analysis_dir: &Path, now_ms: u64) {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        self.cleanup_stale_dirs_locked(analysis_dir, now_ms);
    }

    fn cleanup_stale_dirs_locked(&self, analysis_dir: &Path, now_ms: u64) {
        let entries = match fs::read_dir(analysis_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("jobs") {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|name| name.to_str())
                && name.starts_with(".analysis-")
                && name.ends_with(".tmp")
            {
                if Self::staging_is_stale(&path, name, now_ms) {
                    let _ = fs::remove_dir_all(&path);
                }
                continue;
            }
            let created = fs::read(path.join("summary.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<AnalysisArtifact>(&b).ok())
                .map(|a| a.created_at_ms);
            match created {
                Some(ts) if Self::expired(ts, now_ms) => {
                    let _ = fs::remove_dir_all(&path);
                }
                None => {
                    let _ = fs::remove_dir_all(&path);
                }
                _ => {}
            }
        }
    }

    fn prune_in_dir(&self, analysis_dir: &Path, now_ms: u64) {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        self.prune_locked(analysis_dir, now_ms);
    }

    fn prune_locked(&self, analysis_dir: &Path, now_ms: u64) {
        let mut disk_artifacts = self.list_artifacts_on_disk_locked(analysis_dir);
        disk_artifacts.sort_by(|(left_id, left), (right_id, right)| {
            left.created_at_ms
                .cmp(&right.created_at_ms)
                .then_with(|| left_id.cmp(right_id))
        });

        let mut evicted_disk_ids = disk_artifacts
            .iter()
            .filter(|(_, artifact)| Self::expired(artifact.created_at_ms, now_ms))
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        let live_ids = disk_artifacts
            .iter()
            .filter(|(id, _)| !evicted_disk_ids.contains(id))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let excess = live_ids
            .len()
            .saturating_sub(configured_max_analysis_artifacts());
        evicted_disk_ids.extend(live_ids.into_iter().take(excess));

        let valid_disk_ids = disk_artifacts
            .into_iter()
            .map(|(id, _)| id)
            .collect::<HashSet<_>>();
        let resident_target_ids = self
            .artifact_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|(_, artifact_dir)| artifact_dir.as_path() == analysis_dir)
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        let forgotten_resident_ids = resident_target_ids
            .iter()
            .filter(|id| !valid_disk_ids.contains(*id) || evicted_disk_ids.contains(*id))
            .cloned()
            .collect::<HashSet<_>>();

        if !forgotten_resident_ids.is_empty() {
            self.order
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .retain(|id| !forgotten_resident_ids.contains(id));
        }
        let mut arts = self.artifacts.lock().unwrap_or_else(|p| p.into_inner());
        for id in &forgotten_resident_ids {
            arts.remove(id);
        }
        drop(arts);
        let mut artifact_dirs = self.artifact_dirs.lock().unwrap_or_else(|p| p.into_inner());
        for id in &forgotten_resident_ids {
            if artifact_dirs
                .get(id)
                .is_some_and(|artifact_dir| artifact_dir.as_path() == analysis_dir)
            {
                artifact_dirs.remove(id);
            }
        }
        drop(artifact_dirs);
        for id in evicted_disk_ids {
            self.remove_from_disk_locked(analysis_dir, &id);
        }
    }

    // ── Public API ──────────────────────────────────────────────────────

    pub fn clear(&self) {
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.artifact_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.order.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)]
    pub fn store(
        &self,
        tool_name: &str,
        surface_label: &str,
        project_scope: String,
        cache_key: Option<String>,
        summary: String,
        top_findings: Vec<String>,
        risk_level: String,
        confidence: f64,
        next_actions: Vec<String>,
        blockers: Vec<String>,
        readiness: AnalysisReadiness,
        verifier_checks: Vec<AnalysisVerifierCheck>,
        sections: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<AnalysisArtifact, CodeLensError> {
        let analysis_dir = self.analysis_dir();
        self.store_in_dir(
            &analysis_dir,
            tool_name,
            surface_label,
            project_scope,
            cache_key,
            summary,
            top_findings,
            risk_level,
            confidence,
            next_actions,
            blockers,
            readiness,
            verifier_checks,
            sections,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn store_in_dir(
        &self,
        analysis_dir: &Path,
        tool_name: &str,
        surface_label: &str,
        project_scope: String,
        cache_key: Option<String>,
        summary: String,
        top_findings: Vec<String>,
        risk_level: String,
        confidence: f64,
        next_actions: Vec<String>,
        blockers: Vec<String>,
        readiness: AnalysisReadiness,
        verifier_checks: Vec<AnalysisVerifierCheck>,
        sections: std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<AnalysisArtifact, CodeLensError> {
        let available_sections = sections.keys().cloned().collect::<Vec<_>>();
        let created_at_ms = crate::util::now_ms();
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("analysis-{created_at_ms}-{}-{seq}", std::process::id());
        let artifact = AnalysisArtifact {
            id: id.clone(),
            tool_name: tool_name.to_owned(),
            surface: surface_label.to_owned(),
            project_scope: Some(project_scope),
            cache_key,
            summary,
            top_findings,
            risk_level,
            confidence,
            next_actions,
            blockers,
            readiness,
            verifier_checks,
            available_sections,
            created_at_ms,
        };
        self.write_to_disk_in_dir(analysis_dir, &artifact, &sections)?;
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), artifact.clone());
        self.artifact_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), analysis_dir.to_path_buf());
        self.remember_order(&id);
        self.prune_in_dir(analysis_dir, created_at_ms);
        Ok(artifact)
    }

    #[allow(dead_code)]
    pub fn get(&self, analysis_id: &str, project_scope: &str) -> Option<AnalysisArtifact> {
        let analysis_dir = self.analysis_dir();
        self.get_in_dir(&analysis_dir, analysis_id, project_scope)
    }

    pub fn get_in_dir(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        project_scope: &str,
    ) -> Option<AnalysisArtifact> {
        self.prune_in_dir(analysis_dir, crate::util::now_ms());
        self.get_in_dir_without_prune(analysis_dir, analysis_id, project_scope)
    }

    fn get_in_dir_without_prune(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        project_scope: &str,
    ) -> Option<AnalysisArtifact> {
        let is_resident_in_dir = self
            .artifact_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(analysis_id)
            .is_some_and(|artifact_dir| artifact_dir.as_path() == analysis_dir);
        if let Some(artifact) = self
            .artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(analysis_id)
            .cloned()
            .filter(|artifact| {
                is_resident_in_dir
                    && Self::matches_project_scope(artifact.project_scope.as_deref(), project_scope)
            })
        {
            return Some(artifact);
        }
        let artifact = self.read_from_disk_in_dir(analysis_dir, analysis_id, project_scope)?;
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(analysis_id.to_owned(), artifact.clone());
        self.artifact_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(analysis_id.to_owned(), analysis_dir.to_path_buf());
        self.remember_order(analysis_id);
        Some(artifact)
    }

    #[allow(dead_code)] // backward-compatibility wrapper for external callers
    pub fn find_reusable(
        &self,
        tool_name: &str,
        cache_key: &str,
        surface_label: &str,
        project_scope: Option<&str>,
    ) -> Option<AnalysisArtifact> {
        self.find_reusable_tiered(tool_name, cache_key, surface_label, project_scope)
            .map(|(artifact, _)| artifact)
    }

    /// Tiered cache lookup:
    /// - L1 (Exact):  tool_name + cache_key + surface + scope match
    /// - L2 (Warm):   tool_name + surface + scope match (any cache_key)
    /// - L3 (Cold):   tool_name + scope match (generic cache_key, any surface)
    ///
    /// All tiers require `tool_name` to match. Earlier revisions allowed L3
    /// to fall back across different tools when the stored artifact had no
    /// `cache_key`, but that produced cross-tool payload poisoning: e.g. a
    /// generic `dead_code_report` (args `{}`) would be returned verbatim
    /// for a later `module_boundary_report` call whose summary, findings,
    /// and section layout are unrelated. See issue G2 (2026-05-18 self-
    /// dogfood).
    pub fn find_reusable_tiered(
        &self,
        tool_name: &str,
        cache_key: &str,
        surface_label: &str,
        project_scope: Option<&str>,
    ) -> Option<(AnalysisArtifact, crate::runtime_types::CacheHitTier)> {
        let analysis_dir = self.analysis_dir();
        self.find_reusable_tiered_in_dir(
            &analysis_dir,
            tool_name,
            cache_key,
            surface_label,
            project_scope,
        )
    }

    pub fn find_reusable_tiered_in_dir(
        &self,
        analysis_dir: &Path,
        tool_name: &str,
        cache_key: &str,
        surface_label: &str,
        project_scope: Option<&str>,
    ) -> Option<(AnalysisArtifact, crate::runtime_types::CacheHitTier)> {
        self.prune_in_dir(analysis_dir, crate::util::now_ms());
        if let Some(project_scope) = project_scope {
            for id in self.list_ids_on_disk_in_dir(analysis_dir) {
                let _ = self.get_in_dir_without_prune(analysis_dir, &id, project_scope);
            }
        }
        let order = self
            .order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        let arts = self.artifacts.lock().unwrap_or_else(|p| p.into_inner());

        // L1: exact match
        if let Some(artifact) = order.iter().find_map(|id| {
            let a = arts.get(id)?;
            (a.tool_name == tool_name
                && a.surface == surface_label
                && matches_scope(a.project_scope.as_deref(), project_scope)
                && a.cache_key.as_deref() == Some(cache_key))
            .then(|| a.clone())
        }) {
            return Some((artifact, crate::runtime_types::CacheHitTier::Exact));
        }

        // L2: warm match — same tool + surface + scope, stored artifact has no cache_key (generic)
        if let Some(artifact) = order.iter().find_map(|id| {
            let a = arts.get(id)?;
            (a.tool_name == tool_name
                && a.surface == surface_label
                && matches_scope(a.project_scope.as_deref(), project_scope)
                && a.cache_key.is_none())
            .then(|| a.clone())
        }) {
            return Some((artifact, crate::runtime_types::CacheHitTier::Warm));
        }

        // L3: cold match — same tool + scope, stored artifact has no cache_key (generic).
        // Surface is intentionally not constrained so e.g. a `reviewer-graph`
        // call can reuse a `refactor-full` artifact from the same scope; but
        // tool_name MUST match so payload shapes stay compatible.
        if let Some(artifact) = order.iter().find_map(|id| {
            let a = arts.get(id)?;
            (a.tool_name == tool_name
                && matches_scope(a.project_scope.as_deref(), project_scope)
                && a.cache_key.is_none())
            .then(|| a.clone())
        }) {
            return Some((artifact, crate::runtime_types::CacheHitTier::Cold));
        }

        None
    }

    #[allow(dead_code)]
    pub fn list_summaries(&self, project_scope: Option<&str>) -> Vec<AnalysisSummary> {
        let analysis_dir = self.analysis_dir();
        self.list_summaries_in_dir(&analysis_dir, project_scope)
    }

    pub fn list_summaries_in_dir(
        &self,
        analysis_dir: &Path,
        project_scope: Option<&str>,
    ) -> Vec<AnalysisSummary> {
        self.prune_in_dir(analysis_dir, crate::util::now_ms());
        if let Some(project_scope) = project_scope {
            for id in self.list_ids_on_disk_in_dir(analysis_dir) {
                let _ = self.get_in_dir_without_prune(analysis_dir, &id, project_scope);
            }
        }
        let order = self
            .order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let arts = self.artifacts.lock().unwrap_or_else(|p| p.into_inner());
        order
            .iter()
            .rev()
            .filter_map(|id| arts.get(id))
            .filter(|a| matches_scope(a.project_scope.as_deref(), project_scope))
            .map(|a| AnalysisSummary {
                id: a.id.clone(),
                tool_name: a.tool_name.clone(),
                summary: a.summary.clone(),
                surface: a.surface.clone(),
                created_at_ms: a.created_at_ms,
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_section(
        &self,
        analysis_id: &str,
        section: &str,
        project_scope: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        let analysis_dir = self.analysis_dir();
        self.get_section_in_dir(&analysis_dir, analysis_id, section, project_scope)
    }

    pub fn get_section_in_dir(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        section: &str,
        project_scope: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.get_in_dir(analysis_dir, analysis_id, project_scope)
            .ok_or_else(|| {
                CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`"))
            })?;
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        let path = self
            .artifact_dir_in(analysis_dir, analysis_id)
            .join(Self::section_file_name(section)?);
        let bytes = fs::read(&path)?;
        serde_json::from_slice(&bytes).map_err(|e| CodeLensError::Internal(e.into()))
    }

    #[allow(dead_code)]
    pub fn upsert_section(
        &self,
        analysis_id: &str,
        section: &str,
        value: &serde_json::Value,
        project_scope: &str,
    ) -> Result<(), CodeLensError> {
        let analysis_dir = self.analysis_dir();
        self.upsert_section_in_dir(&analysis_dir, analysis_id, section, value, project_scope)
    }

    pub fn upsert_section_in_dir(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        section: &str,
        value: &serde_json::Value,
        project_scope: &str,
    ) -> Result<(), CodeLensError> {
        self.get_in_dir(analysis_dir, analysis_id, project_scope)
            .ok_or_else(|| {
                CodeLensError::NotFound(format!("unknown analysis_id `{analysis_id}`"))
            })?;
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        let dir = self.artifact_dir_in(analysis_dir, analysis_id);
        let summary_path = dir.join("summary.json");
        let bytes = fs::read(&summary_path)?;
        let mut artifact: AnalysisArtifact =
            serde_json::from_slice(&bytes).map_err(|e| CodeLensError::Internal(e.into()))?;
        if !artifact
            .available_sections
            .iter()
            .any(|existing| existing == section)
        {
            artifact.available_sections.push(section.to_owned());
            artifact.available_sections.sort();
        }
        let section_path = dir.join(Self::section_file_name(section)?);
        let section_bytes =
            serde_json::to_vec_pretty(value).map_err(|e| CodeLensError::Internal(e.into()))?;
        Self::write_file_atomically(&section_path, &section_bytes)?;
        let summary_bytes =
            serde_json::to_vec_pretty(&artifact).map_err(|e| CodeLensError::Internal(e.into()))?;
        Self::write_file_atomically(&summary_path, &summary_bytes)?;
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(analysis_id.to_owned(), artifact);
        Ok(())
    }

    #[cfg(test)]
    pub fn set_created_at_for_test_in_dir(
        &self,
        analysis_dir: &Path,
        analysis_id: &str,
        created_at_ms: u64,
    ) -> std::io::Result<()> {
        let _disk_guard = self.disk_io.lock().unwrap_or_else(|p| p.into_inner());
        let summary_path = self
            .artifact_dir_in(analysis_dir, analysis_id)
            .join("summary.json");
        let bytes = fs::read(&summary_path)?;
        let mut artifact: AnalysisArtifact = serde_json::from_slice(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        artifact.created_at_ms = created_at_ms;
        let updated = serde_json::to_vec_pretty(&artifact)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&summary_path, updated)?;
        let mut arts = self.artifacts.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(a) = arts.get_mut(analysis_id) {
            a.created_at_ms = created_at_ms;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_types::{AnalysisArtifact, AnalysisReadiness, CacheHitTier};

    static ANALYSIS_CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn make_test_artifact(
        id: &str,
        tool_name: &str,
        surface: &str,
        project_scope: Option<&str>,
        cache_key: Option<&str>,
        created_at_ms: u64,
    ) -> AnalysisArtifact {
        AnalysisArtifact {
            id: id.to_owned(),
            tool_name: tool_name.to_owned(),
            surface: surface.to_owned(),
            project_scope: project_scope.map(|s| s.to_owned()),
            cache_key: cache_key.map(|s| s.to_owned()),
            summary: "test".to_owned(),
            top_findings: vec!["finding".to_owned()],
            risk_level: "medium".to_owned(),
            confidence: 0.5,
            next_actions: vec!["act".to_owned()],
            blockers: vec![],
            readiness: AnalysisReadiness::default(),
            verifier_checks: vec![],
            available_sections: vec!["summary".to_owned()],
            created_at_ms,
        }
    }

    /// Spawn an isolated artifact store under a fresh `TempDir`. Returning
    /// the `TempDir` keeps the directory alive for the duration of the test
    /// (it is removed on drop). Avoids the previous per-pid directory race
    /// where parallel tests sharing the same `codelens-test-<pid>` path
    /// trampled each other's I/O — see EINVAL flake on macOS APFS.
    fn make_store() -> (AnalysisArtifactStore, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("tempdir for artifact store test");
        let dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&dir).expect("create artifact dir");
        (AnalysisArtifactStore::new(dir), tmp)
    }

    fn store_artifact(store: &AnalysisArtifactStore, artifact: AnalysisArtifact) {
        store
            .store(
                &artifact.tool_name,
                &artifact.surface,
                artifact.project_scope.clone().unwrap_or_default(),
                artifact.cache_key.clone(),
                artifact.summary.clone(),
                artifact.top_findings.clone(),
                artifact.risk_level.clone(),
                artifact.confidence,
                artifact.next_actions.clone(),
                artifact.blockers.clone(),
                artifact.readiness.clone(),
                artifact.verifier_checks.clone(),
                std::collections::BTreeMap::from([("details".to_owned(), serde_json::json!({}))]),
            )
            .unwrap();
    }

    fn store_scoped_artifact(
        store: &AnalysisArtifactStore,
        analysis_dir: &Path,
        project_scope: &str,
        label: &str,
    ) -> AnalysisArtifact {
        store
            .store_in_dir(
                analysis_dir,
                label,
                "full",
                project_scope.to_owned(),
                None,
                format!("{label} summary"),
                vec![],
                "low".to_owned(),
                0.9,
                vec![],
                vec![],
                AnalysisReadiness::default(),
                vec![],
                std::collections::BTreeMap::from([(
                    "details".to_owned(),
                    serde_json::json!({"label": label}),
                )]),
            )
            .expect("store scoped artifact")
    }

    #[test]
    fn failed_store_does_not_publish_partial_artifact_directory() {
        let (store, _tmp) = make_store();
        let result = store.store(
            "module_boundary_report",
            "full",
            "/proj".to_owned(),
            None,
            "summary".to_owned(),
            vec![],
            "low".to_owned(),
            0.9,
            vec![],
            vec![],
            AnalysisReadiness::default(),
            vec![],
            std::collections::BTreeMap::from([
                (
                    "details".to_owned(),
                    serde_json::json!({"written_before_failure": true}),
                ),
                ("invalid/name".to_owned(), serde_json::json!({})),
            ]),
        );

        assert!(result.is_err(), "invalid section path must fail the store");
        let residue = std::fs::read_dir(store.analysis_dir())
            .expect("read analysis directory")
            .flatten()
            .filter(|entry| entry.path().is_dir() || entry.path().is_file())
            .collect::<Vec<_>>();
        assert!(
            residue.is_empty(),
            "failed store leaked a discoverable partial artifact or staging residue: {residue:?}"
        );
    }

    #[test]
    fn list_summaries_filters_resident_artifacts_by_project_scope() {
        let (store, _tmp) = make_store();
        let foreign = store
            .store(
                "foreign_tool",
                "full",
                "/foreign/project".to_owned(),
                None,
                "foreign summary".to_owned(),
                vec![],
                "low".to_owned(),
                0.9,
                vec![],
                vec![],
                AnalysisReadiness::default(),
                vec![],
                std::collections::BTreeMap::from([("details".to_owned(), serde_json::json!({}))]),
            )
            .expect("store foreign artifact");
        let same_scope = store
            .store(
                "same_scope_tool",
                "full",
                "/active/project".to_owned(),
                None,
                "same-scope summary".to_owned(),
                vec![],
                "low".to_owned(),
                0.9,
                vec![],
                vec![],
                AnalysisReadiness::default(),
                vec![],
                std::collections::BTreeMap::from([("details".to_owned(), serde_json::json!({}))]),
            )
            .expect("store same-scope artifact");

        let summaries = store.list_summaries(Some("/active/project"));

        assert_eq!(
            summaries.len(),
            1,
            "only the active project should be listed"
        );
        assert_eq!(summaries[0].id, same_scope.id);
        assert_eq!(summaries[0].tool_name, "same_scope_tool");
        assert_eq!(summaries[0].summary, "same-scope summary");
        assert!(
            summaries.iter().all(|summary| summary.id != foreign.id),
            "foreign artifact id must not leak into scoped summaries"
        );
    }

    #[test]
    fn concurrent_scoped_stores_publish_only_to_their_explicit_directories() {
        let tmp = tempfile::tempdir().expect("tempdir for concurrent artifact store test");
        let default_dir = tmp.path().join("default");
        let project_a_dir = tmp.path().join("project-a");
        let project_b_dir = tmp.path().join("project-b");
        for dir in [&default_dir, &project_a_dir, &project_b_dir] {
            std::fs::create_dir_all(dir).expect("create artifact directory");
        }
        let store = std::sync::Arc::new(AnalysisArtifactStore::new(default_dir.clone()));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));

        let workers = [
            (project_a_dir.clone(), "/project-a", "project_a_tool"),
            (project_b_dir.clone(), "/project-b", "project_b_tool"),
        ]
        .into_iter()
        .map(|(analysis_dir, scope, tool_name)| {
            let store = std::sync::Arc::clone(&store);
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .store_in_dir(
                        &analysis_dir,
                        tool_name,
                        "full",
                        scope.to_owned(),
                        None,
                        format!("{tool_name} summary"),
                        vec![],
                        "low".to_owned(),
                        0.9,
                        vec![],
                        vec![],
                        AnalysisReadiness::default(),
                        vec![],
                        std::collections::BTreeMap::from([(
                            "details".to_owned(),
                            serde_json::json!({"tool": tool_name}),
                        )]),
                    )
                    .expect("scoped store succeeds")
            })
        })
        .collect::<Vec<_>>();

        barrier.wait();
        for worker in workers {
            worker.join().expect("scoped store worker joins");
        }

        assert_eq!(store.list_ids_on_disk_in_dir(&project_a_dir).len(), 1);
        assert_eq!(store.list_ids_on_disk_in_dir(&project_b_dir).len(), 1);
        assert!(store.list_ids_on_disk_in_dir(&default_dir).is_empty());
    }

    #[test]
    fn generated_artifact_id_includes_process_id() {
        let (store, _tmp) = make_store();
        let analysis_dir = store.analysis_dir();
        let artifact = store_scoped_artifact(&store, &analysis_dir, "/project", "pid_tool");
        let id_parts = artifact.id.split('-').collect::<Vec<_>>();

        assert_eq!(
            id_parts.len(),
            4,
            "artifact id keeps its opaque four-part shape"
        );
        assert_eq!(
            id_parts[2],
            std::process::id().to_string(),
            "process identity prevents same-millisecond cross-daemon collisions",
        );
    }

    #[test]
    fn cap_prune_in_one_directory_preserves_other_directory_artifact() {
        let _env_guard = ANALYSIS_CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir for scoped cap pruning test");
        let default_dir = tmp.path().join("default");
        let project_a_dir = tmp.path().join("project-a");
        let project_b_dir = tmp.path().join("project-b");
        for dir in [&default_dir, &project_a_dir, &project_b_dir] {
            std::fs::create_dir_all(dir).expect("create artifact directory");
        }
        let store = AnalysisArtifactStore::new(default_dir);
        let project_b =
            store_scoped_artifact(&store, &project_b_dir, "/project-b", "project_b_live");

        for index in 0..=MAX_ANALYSIS_ARTIFACTS {
            store_scoped_artifact(
                &store,
                &project_a_dir,
                "/project-a",
                &format!("project_a_{index}"),
            );
        }

        assert_eq!(
            store.list_ids_on_disk_in_dir(&project_a_dir).len(),
            MAX_ANALYSIS_ARTIFACTS,
            "project A should enforce its own cap"
        );
        assert!(
            project_b_dir.join(&project_b.id).exists(),
            "project A cap pruning must not delete project B disk payload"
        );
        assert!(
            store
                .get_in_dir(&project_b_dir, &project_b.id, "/project-b")
                .is_some(),
            "project B handle must remain resident/readable"
        );
    }

    #[test]
    fn ttl_prune_in_one_directory_preserves_expired_other_directory_until_its_turn() {
        let tmp = tempfile::tempdir().expect("tempdir for scoped ttl pruning test");
        let default_dir = tmp.path().join("default");
        let project_a_dir = tmp.path().join("project-a");
        let project_b_dir = tmp.path().join("project-b");
        for dir in [&default_dir, &project_a_dir, &project_b_dir] {
            std::fs::create_dir_all(dir).expect("create artifact directory");
        }
        let store = AnalysisArtifactStore::new(default_dir);
        let project_a =
            store_scoped_artifact(&store, &project_a_dir, "/project-a", "project_a_old");
        let project_b =
            store_scoped_artifact(&store, &project_b_dir, "/project-b", "project_b_old");
        store
            .set_created_at_for_test_in_dir(&project_a_dir, &project_a.id, 0)
            .expect("age project A artifact");
        store
            .set_created_at_for_test_in_dir(&project_b_dir, &project_b.id, 0)
            .expect("age project B artifact");

        let _ = store.list_summaries_in_dir(&project_a_dir, Some("/project-a"));

        assert!(!project_a_dir.join(&project_a.id).exists());
        assert!(
            project_b_dir.join(&project_b.id).exists(),
            "project A TTL pruning must not delete project B payload"
        );
        assert!(
            store
                .artifacts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&project_b.id),
            "project B handle must remain resident until project B is pruned"
        );
    }

    #[test]
    fn cold_non_default_reuse_does_not_revive_expired_disk_artifact() {
        let tmp = tempfile::tempdir().expect("tempdir for cold TTL regression");
        let default_dir = tmp.path().join("default");
        let scoped_dir = tmp.path().join("project-artifacts");
        for dir in [&default_dir, &scoped_dir] {
            std::fs::create_dir_all(dir).expect("create artifact directory");
        }

        let writer = AnalysisArtifactStore::new(default_dir.clone());
        let stale = store_scoped_artifact(&writer, &scoped_dir, "/project", "reusable_tool");
        writer
            .set_created_at_for_test_in_dir(&scoped_dir, &stale.id, 0)
            .expect("age disk artifact");
        drop(writer);

        let cold_reader = AnalysisArtifactStore::new(default_dir);
        assert!(
            cold_reader
                .artifact_dirs
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_empty(),
            "regression requires a cold store with no resident directory index",
        );

        let reusable = cold_reader.find_reusable_tiered_in_dir(
            &scoped_dir,
            "reusable_tool",
            "new-cache-key",
            "full",
            Some("/project"),
        );

        assert!(
            reusable.is_none(),
            "expired disk artifact must not be revived"
        );
        assert!(
            !scoped_dir.join(&stale.id).exists(),
            "cold read must remove the expired artifact directory",
        );
        assert!(
            cold_reader
                .list_summaries_in_dir(&scoped_dir, Some("/project"))
                .is_empty(),
            "expired artifact must stay absent from a later cold list",
        );
    }

    #[test]
    fn cleanup_preserves_fresh_staging_and_removes_stale_staging_from_other_store() {
        let tmp = tempfile::tempdir().expect("tempdir for staging cleanup regression");
        let analysis_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&analysis_dir).expect("create artifact directory");
        let publisher = AnalysisArtifactStore::new(analysis_dir.clone());
        let cleaner = AnalysisArtifactStore::new(analysis_dir.clone());
        let now_ms = crate::util::now_ms();
        let fresh =
            publisher.staging_dir_in(&analysis_dir, &format!("analysis-{now_ms}-publisher"));
        let stale = publisher.staging_dir_in(&analysis_dir, "analysis-0-abandoned");
        std::fs::create_dir_all(&fresh).expect("create live publisher staging directory");
        std::fs::write(fresh.join("details.json"), b"{}").expect("write live staging marker");
        std::fs::create_dir_all(&stale).expect("create abandoned staging directory");
        std::fs::write(stale.join("details.json"), b"{}").expect("write stale staging marker");

        cleaner.cleanup_stale_dirs_in_dir(&analysis_dir, now_ms);

        assert!(
            fresh.exists(),
            "one store must not remove another store's fresh publish staging directory",
        );
        assert!(
            !stale.exists(),
            "staging directory older than the cleanup grace must be removed",
        );
    }

    #[test]
    fn delayed_store_commit_after_disk_list_records_order_once() {
        let tmp = tempfile::tempdir().expect("tempdir for order race regression");
        let analysis_dir = tmp.path().join("artifacts");
        std::fs::create_dir_all(&analysis_dir).expect("create artifact directory");
        let store = AnalysisArtifactStore::new(analysis_dir.clone());
        let now_ms = crate::util::now_ms();
        let artifact = make_test_artifact(
            &format!("analysis-{now_ms}-publisher"),
            "race_tool",
            "full",
            Some("/project"),
            None,
            now_ms,
        );
        store
            .write_to_disk_in_dir(
                &analysis_dir,
                &artifact,
                &std::collections::BTreeMap::from([("details".to_owned(), serde_json::json!({}))]),
            )
            .expect("publish artifact before memory commit");

        let listed = store.list_summaries_in_dir(&analysis_dir, Some("/project"));
        assert_eq!(listed.len(), 1, "concurrent list hydrates the published id");

        // Model the original publisher resuming after the list hydrated the
        // same disk artifact but before its own memory/order commit.
        store
            .artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(artifact.id.clone(), artifact.clone());
        store
            .artifact_dirs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(artifact.id.clone(), analysis_dir);
        store.remember_order(&artifact.id);

        let occurrences = store
            .order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|id| id.as_str() == artifact.id)
            .count();
        assert_eq!(occurrences, 1, "order insertion must be idempotent");
    }

    #[test]
    fn cold_list_and_store_prune_seeded_disk_artifacts_to_exact_cap() {
        let _env_guard = ANALYSIS_CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir for cold cap regression");
        let default_dir = tmp.path().join("default");
        let analysis_dir = tmp.path().join("project-artifacts");
        for dir in [&default_dir, &analysis_dir] {
            std::fs::create_dir_all(dir).expect("create artifact directory");
        }
        let seeder = AnalysisArtifactStore::new(default_dir.clone());
        let base_ms = crate::util::now_ms().saturating_sub(MAX_ANALYSIS_ARTIFACTS as u64 + 10);
        let mut seeded_ids = Vec::new();
        for index in 0..=MAX_ANALYSIS_ARTIFACTS {
            let created_at_ms = base_ms + index as u64;
            let artifact = make_test_artifact(
                &format!("analysis-{created_at_ms}-seed-{index}"),
                "seed_tool",
                "full",
                Some("/project"),
                None,
                created_at_ms,
            );
            seeded_ids.push(artifact.id.clone());
            seeder
                .write_to_disk_in_dir(
                    &analysis_dir,
                    &artifact,
                    &std::collections::BTreeMap::from([(
                        "details".to_owned(),
                        serde_json::json!({"index": index}),
                    )]),
                )
                .expect("seed disk artifact without resident bookkeeping");
        }
        drop(seeder);

        let cold_store = AnalysisArtifactStore::new(default_dir);
        let summaries = cold_store.list_summaries_in_dir(&analysis_dir, Some("/project"));
        assert_eq!(
            summaries.len(),
            MAX_ANALYSIS_ARTIFACTS,
            "cold list must expose exactly the configured disk cap",
        );
        assert_eq!(
            cold_store.list_ids_on_disk_in_dir(&analysis_dir).len(),
            MAX_ANALYSIS_ARTIFACTS,
            "cold list must prune disk before hydrating",
        );
        assert!(
            !analysis_dir.join(&seeded_ids[0]).exists(),
            "cold list must evict the oldest seeded disk artifact",
        );

        store_scoped_artifact(&cold_store, &analysis_dir, "/project", "post_restart_tool");
        assert_eq!(
            cold_store.list_ids_on_disk_in_dir(&analysis_dir).len(),
            MAX_ANALYSIS_ARTIFACTS,
            "post-restart store must keep the disk cap exact",
        );
        assert!(
            !analysis_dir.join(&seeded_ids[1]).exists(),
            "post-restart store must evict the next-oldest disk artifact",
        );
    }

    #[test]
    fn cold_list_reads_each_summary_twice_instead_of_quadratically() {
        const ARTIFACT_COUNT: usize = 4;
        let tmp = tempfile::tempdir().expect("tempdir for cold hydration read-count regression");
        let default_dir = tmp.path().join("default");
        let analysis_dir = tmp.path().join("project-artifacts");
        for dir in [&default_dir, &analysis_dir] {
            std::fs::create_dir_all(dir).expect("create artifact directory");
        }
        let seeder = AnalysisArtifactStore::new(default_dir.clone());
        let base_ms = crate::util::now_ms().saturating_sub(ARTIFACT_COUNT as u64);
        for index in 0..ARTIFACT_COUNT {
            let created_at_ms = base_ms + index as u64;
            let artifact = make_test_artifact(
                &format!("analysis-{created_at_ms}-seed-{index}"),
                "seed_tool",
                "full",
                Some("/project"),
                None,
                created_at_ms,
            );
            seeder
                .write_to_disk_in_dir(
                    &analysis_dir,
                    &artifact,
                    &std::collections::BTreeMap::from([(
                        "details".to_owned(),
                        serde_json::json!({"index": index}),
                    )]),
                )
                .expect("seed disk artifact");
        }
        drop(seeder);

        let cold_store = AnalysisArtifactStore::new(default_dir);
        let summaries = cold_store.list_summaries_in_dir(&analysis_dir, Some("/project"));
        let summary_reads = cold_store
            .summary_reads
            .load(std::sync::atomic::Ordering::Relaxed);

        assert_eq!(summaries.len(), ARTIFACT_COUNT);
        assert_eq!(
            summary_reads,
            (ARTIFACT_COUNT * 2) as u64,
            "one prune scan plus one cold hydration read per artifact is linear",
        );
    }

    #[test]
    fn tiered_exact_hit() {
        let (store, _tmp) = make_store();
        let artifact = make_test_artifact(
            "a1",
            "impact_report",
            "full",
            Some("/proj"),
            Some("key1"),
            crate::util::now_ms(),
        );
        store_artifact(&store, artifact);
        let (found, tier) = store
            .find_reusable_tiered("impact_report", "key1", "full", Some("/proj"))
            .unwrap();
        assert_eq!(found.tool_name, "impact_report");
        assert_eq!(tier, CacheHitTier::Exact);
    }

    #[test]
    fn tiered_warm_hit_same_tool_no_cache_key() {
        let (store, _tmp) = make_store();
        let artifact = make_test_artifact(
            "a1",
            "impact_report",
            "full",
            Some("/proj"),
            None, // generic analysis
            crate::util::now_ms(),
        );
        store_artifact(&store, artifact);
        let (found, tier) = store
            .find_reusable_tiered("impact_report", "new-key", "full", Some("/proj"))
            .unwrap();
        assert_eq!(found.tool_name, "impact_report");
        assert_eq!(tier, CacheHitTier::Warm);
    }

    /// Regression for G2 (2026-05-18): an earlier `find_reusable_tiered`
    /// allowed L3 cold-tier to fall back across different tools when the
    /// stored artifact had no `cache_key`. That meant a generic
    /// `dead_code_report` (args `{}` → `cache_key = None`) was returned
    /// verbatim for a later `module_boundary_report` call, even though the
    /// two tools produce structurally different payloads. The hit now
    /// requires `tool_name` to match in every tier.
    #[test]
    fn tiered_miss_when_different_tool_even_if_scope_generic() {
        let (store, _tmp) = make_store();
        let artifact = make_test_artifact(
            "a1",
            "impact_report",
            "full",
            Some("/proj"),
            None, // generic analysis
            crate::util::now_ms(),
        );
        store_artifact(&store, artifact);
        assert!(
            store
                .find_reusable_tiered("change_request", "other-key", "full", Some("/proj"))
                .is_none(),
            "cross-tool cold-tier reuse must not poison a different tool's payload"
        );
    }

    /// L3 still hits when the same tool revisits the same scope with a
    /// different surface (e.g. planner-readonly → reviewer-graph) and the
    /// original artifact was stored without a cache_key.
    #[test]
    fn tiered_cold_hit_same_tool_different_surface() {
        let (store, _tmp) = make_store();
        let artifact = make_test_artifact(
            "a1",
            "impact_report",
            "refactor-full",
            Some("/proj"),
            None, // generic analysis
            crate::util::now_ms(),
        );
        store_artifact(&store, artifact);
        let (found, tier) = store
            .find_reusable_tiered("impact_report", "any-key", "reviewer-graph", Some("/proj"))
            .expect("same-tool generic artifact should still hit via L3");
        assert_eq!(found.tool_name, "impact_report");
        assert_eq!(tier, CacheHitTier::Cold);
    }

    #[test]
    fn tiered_miss_different_scope() {
        let (store, _tmp) = make_store();
        let artifact = make_test_artifact(
            "a1",
            "impact_report",
            "full",
            Some("/other"),
            Some("key1"),
            crate::util::now_ms(),
        );
        store_artifact(&store, artifact);
        assert!(
            store
                .find_reusable_tiered("impact_report", "key1", "full", Some("/proj"))
                .is_none()
        );
    }

    #[test]
    fn tiered_exact_preferred_over_warm() {
        let (store, _tmp) = make_store();
        let warm = make_test_artifact(
            "warm",
            "impact_report",
            "full",
            Some("/proj"),
            None, // generic → eligible for warm
            crate::util::now_ms() - 1000,
        );
        let exact = make_test_artifact(
            "exact",
            "impact_report",
            "full",
            Some("/proj"),
            Some("key1"),
            crate::util::now_ms(),
        );
        store_artifact(&store, warm);
        store_artifact(&store, exact);
        let (found, tier) = store
            .find_reusable_tiered("impact_report", "key1", "full", Some("/proj"))
            .unwrap();
        assert_eq!(found.cache_key.as_deref(), Some("key1"));
        assert_eq!(tier, CacheHitTier::Exact);
    }

    #[test]
    fn scoped_lookup_rejects_artifact_from_another_scope() {
        let (store, _tmp) = make_store();
        let stored = store
            .store(
                "review_architecture",
                "full",
                "/explicit/path/passed/by/caller".to_owned(),
                Some("key1".to_owned()),
                "test".to_owned(),
                vec!["finding".to_owned()],
                "medium".to_owned(),
                0.5,
                vec!["act".to_owned()],
                vec![],
                AnalysisReadiness::default(),
                vec![],
                std::collections::BTreeMap::from([("details".to_owned(), serde_json::json!({}))]),
            )
            .expect("store succeeds");
        let id = stored.id.as_str();

        assert!(
            store.get(id, "/some/other/active/scope").is_none(),
            "lookup with a mismatched scope must miss",
        );
        assert!(
            store.get(id, "/explicit/path/passed/by/caller").is_some(),
            "lookup with the stored scope must remain available",
        );
    }

    #[test]
    fn configured_caps_respect_env_overrides_and_reject_invalid() {
        let _env_guard = ANALYSIS_CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let saved_max = std::env::var("CODELENS_MAX_ANALYSIS_ARTIFACTS").ok();
        let saved_ttl = std::env::var("CODELENS_ANALYSIS_TTL_HOURS").ok();

        // SAFETY: env-var mutation is safe here — these vars are dedicated to
        // this test and no other thread/test reads them concurrently. Restored
        // at the end via the saved snapshots.
        unsafe {
            std::env::set_var("CODELENS_MAX_ANALYSIS_ARTIFACTS", "200");
            std::env::set_var("CODELENS_ANALYSIS_TTL_HOURS", "12");
        }
        assert_eq!(configured_max_analysis_artifacts(), 200);
        assert_eq!(configured_analysis_ttl_ms(), 12 * 60 * 60 * 1000);

        unsafe {
            std::env::set_var("CODELENS_MAX_ANALYSIS_ARTIFACTS", "0");
            std::env::set_var("CODELENS_ANALYSIS_TTL_HOURS", "0");
        }
        assert_eq!(
            configured_max_analysis_artifacts(),
            MAX_ANALYSIS_ARTIFACTS,
            "zero rejected, fallback to default cap",
        );
        assert_eq!(
            configured_analysis_ttl_ms(),
            TTL_MS,
            "zero rejected, fallback to default TTL",
        );

        unsafe {
            std::env::set_var("CODELENS_MAX_ANALYSIS_ARTIFACTS", "garbage");
            std::env::set_var("CODELENS_ANALYSIS_TTL_HOURS", "garbage");
        }
        assert_eq!(configured_max_analysis_artifacts(), MAX_ANALYSIS_ARTIFACTS);
        assert_eq!(configured_analysis_ttl_ms(), TTL_MS);

        unsafe {
            match saved_max {
                Some(v) => std::env::set_var("CODELENS_MAX_ANALYSIS_ARTIFACTS", v),
                None => std::env::remove_var("CODELENS_MAX_ANALYSIS_ARTIFACTS"),
            }
            match saved_ttl {
                Some(v) => std::env::set_var("CODELENS_ANALYSIS_TTL_HOURS", v),
                None => std::env::remove_var("CODELENS_ANALYSIS_TTL_HOURS"),
            }
        }
    }
}
