use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;

use super::{
    ActiveAgentEntry, AgentWorkEntry, AppState, CoordinationCounts, CoordinationLockStats,
    CoordinationSnapshot, FileClaimEntry,
};

const DEFAULT_COORDINATION_TTL_SECS: u64 = 5 * 60;

fn resolve_coordination_session_id(
    session: &SessionRequestContext,
    arguments: &Value,
) -> Result<String, CodeLensError> {
    let explicit = arguments
        .get("session_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (session.is_local(), explicit) {
        (false, Some(explicit)) if explicit != session.session_id => {
            Err(CodeLensError::Validation(format!(
                "coordination session_id `{explicit}` does not match active logical session `{}`",
                session.session_id
            )))
        }
        (_, Some(explicit)) => Ok(explicit.to_owned()),
        (_, None) if !session.session_id.trim().is_empty() => Ok(session.session_id.clone()),
        _ => Err(CodeLensError::MissingParam("session_id".to_owned())),
    }
}

fn coordination_ttl_seconds(arguments: &Value) -> Option<u64> {
    arguments
        .get("ttl_secs")
        .and_then(|value| value.as_u64())
        .or(Some(DEFAULT_COORDINATION_TTL_SECS))
}

fn normalized_claim_paths(
    state: &AppState,
    arguments: &Value,
) -> Result<Vec<String>, CodeLensError> {
    let Some(items) = arguments.get("paths").and_then(|value| value.as_array()) else {
        return Err(CodeLensError::MissingParam("paths".to_owned()));
    };
    let mut paths = Vec::new();
    for item in items.iter().take(64) {
        let Some(path) = item
            .as_str()
            .or_else(|| item.get("path").and_then(|value| value.as_str()))
        else {
            continue;
        };
        let normalized = state.normalize_target_path(path);
        if !paths.iter().any(|existing| existing == &normalized) {
            paths.push(normalized);
        }
    }
    if paths.is_empty() {
        return Err(CodeLensError::MissingParam("paths".to_owned()));
    }
    Ok(paths)
}

fn resolve_git_dir(project_root: &Path) -> Option<PathBuf> {
    let dot_git = project_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    if !dot_git.is_file() {
        return None;
    }
    let contents = fs::read_to_string(dot_git).ok()?;
    let gitdir = contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let gitdir_path = PathBuf::from(gitdir);
    Some(if gitdir_path.is_absolute() {
        gitdir_path
    } else {
        project_root.join(gitdir_path)
    })
}

fn infer_git_branch(project_root: &Path) -> String {
    let Some(git_dir) = resolve_git_dir(project_root) else {
        return String::new();
    };
    let Ok(head) = fs::read_to_string(git_dir.join("HEAD")) else {
        return String::new();
    };
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        return reference
            .trim()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
    }
    if head.is_empty() {
        String::new()
    } else {
        "detached".to_owned()
    }
}

impl AppState {
    pub(crate) fn coordination_snapshot_for_scope(&self, scope: &str) -> CoordinationSnapshot {
        self.coord_store.snapshot(scope)
    }

    pub(crate) fn coordination_counts_for_scope(&self, scope: &str) -> CoordinationCounts {
        self.coordination_snapshot_for_scope(scope).counts
    }

    pub(crate) fn register_agent_work_for_arguments(
        &self,
        arguments: &Value,
    ) -> Result<AgentWorkEntry, CodeLensError> {
        let session = SessionRequestContext::from_json(arguments);
        let session_id = resolve_coordination_session_id(&session, arguments)?;
        let agent_name = crate::tool_runtime::required_string(arguments, "agent_name")?;
        let branch = crate::tool_runtime::required_string(arguments, "branch")?;
        let worktree = crate::tool_runtime::required_string(arguments, "worktree")?;
        let intent = crate::tool_runtime::required_string(arguments, "intent")?;
        let scope = self.project_scope_for_session(&session);
        Ok(self.coord_store.register_agent_work(
            &scope,
            &session_id,
            agent_name,
            branch,
            worktree,
            intent,
            coordination_ttl_seconds(arguments),
        ))
    }

    pub(crate) fn list_active_agents_for_arguments(
        &self,
        arguments: &Value,
    ) -> Vec<ActiveAgentEntry> {
        let session = SessionRequestContext::from_json(arguments);
        self.coord_store
            .active_agents(&self.project_scope_for_session(&session))
    }

    pub(crate) fn claim_files_for_arguments(
        &self,
        arguments: &Value,
    ) -> Result<FileClaimEntry, CodeLensError> {
        let session = SessionRequestContext::from_json(arguments);
        let session_id = resolve_coordination_session_id(&session, arguments)?;
        let reason = crate::tool_runtime::required_string(arguments, "reason")?;
        let paths = normalized_claim_paths(self, arguments)?;
        let scope = self.project_scope_for_session(&session);
        let fallback_agent_name = session
            .client_name
            .clone()
            .unwrap_or_else(|| session_id.clone());
        let fallback_worktree = self.project().as_path().to_string_lossy().to_string();
        let fallback_branch = infer_git_branch(self.project().as_path());
        Ok(self.coord_store.claim_files(
            &scope,
            &session_id,
            &fallback_agent_name,
            &fallback_branch,
            &fallback_worktree,
            paths,
            reason,
            coordination_ttl_seconds(arguments),
        ))
    }

    pub(crate) fn release_files_for_arguments(
        &self,
        arguments: &Value,
    ) -> Result<(String, Vec<String>, Option<FileClaimEntry>), CodeLensError> {
        let session = SessionRequestContext::from_json(arguments);
        let session_id = resolve_coordination_session_id(&session, arguments)?;
        let paths = normalized_claim_paths(self, arguments)?;
        let scope = self.project_scope_for_session(&session);
        let (released_paths, remaining_claim) =
            self.coord_store.release_files(&scope, &session_id, &paths);
        Ok((session_id, released_paths, remaining_claim))
    }

    pub(crate) fn overlapping_claims_for_arguments(
        &self,
        arguments: &Value,
        target_paths: &[String],
    ) -> Vec<FileClaimEntry> {
        let session = SessionRequestContext::from_json(arguments);
        let session_id = resolve_coordination_session_id(&session, arguments)
            .unwrap_or_else(|_| session.session_id.clone());
        self.coord_store.overlapping_claims(
            &self.project_scope_for_session(&session),
            &session_id,
            target_paths,
        )
    }

    pub(crate) fn coordination_snapshot_for_session(
        &self,
        session: &SessionRequestContext,
    ) -> CoordinationSnapshot {
        self.coordination_snapshot_for_scope(&self.project_scope_for_session(session))
    }

    pub(crate) fn active_claim_for_session(
        &self,
        session: &SessionRequestContext,
    ) -> Option<FileClaimEntry> {
        self.coordination_snapshot_for_session(session)
            .claims
            .into_iter()
            .find(|claim| claim.session_id == session.session_id)
    }

    pub(crate) fn coordination_counts_for_session(
        &self,
        session: &SessionRequestContext,
    ) -> CoordinationCounts {
        self.coordination_snapshot_for_session(session).counts
    }

    /// Cumulative `Mutex<HashMap>` contention counters on the coordination
    /// store. Process-global (not per-session) — the counters track the
    /// shared mutex itself, not any one caller. Surfaced through session
    /// metrics so an operator can decide whether the single-mutex design
    /// needs to be sharded before adding any new structure.
    pub(crate) fn coordination_lock_stats(&self) -> CoordinationLockStats {
        self.coord_store.lock_stats()
    }
}
