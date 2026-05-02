use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::CodeLensError;
use crate::runtime_types::{
    AnalysisArtifact, AnalysisReadiness, AnalysisSummary, AnalysisVerifierCheck,
};

pub(crate) const MAX_ANALYSIS_ARTIFACTS: usize = 50;
const TTL_MS: u64 = 6 * 60 * 60 * 1000; // 6 hours

pub(crate) struct AnalysisArtifactStore {
    analysis_dir: Mutex<PathBuf>,
    seq: std::sync::atomic::AtomicU64,
    order: Mutex<VecDeque<String>>,
    artifacts: Mutex<HashMap<String, AnalysisArtifact>>,
}

impl AnalysisArtifactStore {
    pub fn new(analysis_dir: PathBuf) -> Self {
        Self {
            analysis_dir: Mutex::new(analysis_dir),
            seq: std::sync::atomic::AtomicU64::new(0),
            order: Mutex::new(VecDeque::with_capacity(MAX_ANALYSIS_ARTIFACTS)),
            artifacts: Mutex::new(HashMap::new()),
        }
    }

    pub fn analysis_dir(&self) -> PathBuf {
        self.analysis_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_analysis_dir(&self, dir: PathBuf) {
        *self.analysis_dir.lock().unwrap_or_else(|p| p.into_inner()) = dir;
    }

    fn artifact_dir(&self, analysis_id: &str) -> PathBuf {
        self.analysis_dir().join(analysis_id)
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

    fn expired(created_at_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(created_at_ms) > TTL_MS
    }

    // ── Disk I/O ────────────────────────────────────────────────────────

    fn write_to_disk(
        &self,
        artifact: &AnalysisArtifact,
        sections: &std::collections::BTreeMap<String, serde_json::Value>,
    ) -> Result<(), CodeLensError> {
        let dir = self.artifact_dir(&artifact.id);
        fs::create_dir_all(&dir)?;
        let summary_bytes =
            serde_json::to_vec_pretty(artifact).map_err(|e| CodeLensError::Internal(e.into()))?;
        fs::write(dir.join("summary.json"), summary_bytes)?;
        for (section, value) in sections {
            let path = dir.join(format!("{}.json", Self::sanitize_section_name(section)));
            let bytes =
                serde_json::to_vec_pretty(value).map_err(|e| CodeLensError::Internal(e.into()))?;
            fs::write(path, bytes)?;
        }
        Ok(())
    }

    fn read_from_disk(
        &self,
        analysis_id: &str,
        project_scope: Option<&str>,
    ) -> Option<AnalysisArtifact> {
        let path = self.artifact_dir(analysis_id).join("summary.json");
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AnalysisArtifact>(&bytes).ok())
            .filter(|a| matches_scope(a.project_scope.as_deref(), project_scope))
    }

    fn remove_from_disk(&self, analysis_id: &str) {
        let _ = fs::remove_dir_all(self.artifact_dir(analysis_id));
    }

    fn list_ids_on_disk(&self) -> Vec<String> {
        let entries = match fs::read_dir(self.analysis_dir()) {
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
            .filter(|name| !name.is_empty() && name != "jobs")
            .collect()
    }

    // ── Cleanup / Prune ─────────────────────────────────────────────────

    pub fn cleanup_stale_dirs(&self, now_ms: u64) {
        let entries = match fs::read_dir(self.analysis_dir()) {
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

    fn prune(&self, now_ms: u64) {
        let expired = {
            let arts = self.artifacts.lock().unwrap_or_else(|p| p.into_inner());
            arts.iter()
                .filter(|(_, a)| Self::expired(a.created_at_ms, now_ms))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };

        let mut evicted = expired;
        {
            let mut order = self.order.lock().unwrap_or_else(|p| p.into_inner());
            if !evicted.is_empty() {
                order.retain(|id| !evicted.contains(id));
            }
            while order.len() > MAX_ANALYSIS_ARTIFACTS {
                if let Some(oldest) = order.pop_front() {
                    evicted.push(oldest);
                }
            }
        }

        if evicted.is_empty() {
            return;
        }
        evicted.sort();
        evicted.dedup();
        let mut arts = self.artifacts.lock().unwrap_or_else(|p| p.into_inner());
        for id in &evicted {
            arts.remove(id);
        }
        drop(arts);
        for id in evicted {
            self.remove_from_disk(&id);
        }
    }

    // ── Public API ──────────────────────────────────────────────────────

    pub fn clear(&self) {
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.order.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    #[allow(clippy::too_many_arguments)]
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
        let available_sections = sections.keys().cloned().collect::<Vec<_>>();
        let created_at_ms = crate::util::now_ms();
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = format!("analysis-{created_at_ms}-{seq}");
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
        self.write_to_disk(&artifact, &sections)?;
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.clone(), artifact.clone());
        self.order
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push_back(id);
        self.prune(created_at_ms);
        Ok(artifact)
    }

    pub fn get(&self, analysis_id: &str, project_scope: Option<&str>) -> Option<AnalysisArtifact> {
        self.prune(crate::util::now_ms());
        if let Some(artifact) = self
            .artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(analysis_id)
            .cloned()
            .filter(|a| matches_scope(a.project_scope.as_deref(), project_scope))
        {
            return Some(artifact);
        }
        let artifact = self.read_from_disk(analysis_id, project_scope)?;
        self.artifacts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(analysis_id.to_owned(), artifact.clone());
        let mut order = self.order.lock().unwrap_or_else(|p| p.into_inner());
        if !order.iter().any(|existing| existing == analysis_id) {
            order.push_back(analysis_id.to_owned());
        }
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
    /// - L3 (Cold):   same scope, any tool, most recent within TTL
    pub fn find_reusable_tiered(
        &self,
        tool_name: &str,
        cache_key: &str,
        surface_label: &str,
        project_scope: Option<&str>,
    ) -> Option<(AnalysisArtifact, crate::runtime_types::CacheHitTier)> {
        self.prune(crate::util::now_ms());
        for id in self.list_ids_on_disk() {
            let _ = self.get(&id, project_scope);
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

        // L3: cold match — same scope, stored artifact has no cache_key (generic), most recent
        if let Some(artifact) = order.iter().find_map(|id| {
            let a = arts.get(id)?;
            (matches_scope(a.project_scope.as_deref(), project_scope) && a.cache_key.is_none())
                .then(|| a.clone())
        }) {
            return Some((artifact, crate::runtime_types::CacheHitTier::Cold));
        }

        None
    }

    pub fn list_summaries(&self, project_scope: Option<&str>) -> Vec<AnalysisSummary> {
        self.prune(crate::util::now_ms());
        for id in self.list_ids_on_disk() {
            let _ = self.get(&id, project_scope);
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
            .map(|a| AnalysisSummary {
                id: a.id.clone(),
                tool_name: a.tool_name.clone(),
                summary: a.summary.clone(),
                surface: a.surface.clone(),
                created_at_ms: a.created_at_ms,
            })
            .collect()
    }

    pub fn get_section(
        &self,
        analysis_id: &str,
        section: &str,
    ) -> Result<serde_json::Value, CodeLensError> {
        self.prune(crate::util::now_ms());
        let path = self
            .artifact_dir(analysis_id)
            .join(format!("{}.json", Self::sanitize_section_name(section)));
        let bytes = fs::read(&path)?;
        serde_json::from_slice(&bytes).map_err(|e| CodeLensError::Internal(e.into()))
    }

    #[cfg(test)]
    pub fn set_created_at_for_test(
        &self,
        analysis_id: &str,
        created_at_ms: u64,
    ) -> std::io::Result<()> {
        let summary_path = self.artifact_dir(analysis_id).join("summary.json");
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

fn matches_scope(artifact_scope: Option<&str>, current_scope: Option<&str>) -> bool {
    match (artifact_scope, current_scope) {
        (Some(a), Some(c)) => a == c,
        (None, _) => true,
        (_, None) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_types::{AnalysisArtifact, AnalysisReadiness, CacheHitTier};

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
                std::collections::BTreeMap::from([("summary".to_owned(), serde_json::json!({}))]),
            )
            .unwrap();
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

    #[test]
    fn tiered_cold_hit_different_tool_same_scope_generic() {
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
            .find_reusable_tiered("change_request", "other-key", "full", Some("/proj"))
            .unwrap();
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
        assert!(store
            .find_reusable_tiered("impact_report", "key1", "full", Some("/proj"))
            .is_none());
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
}
