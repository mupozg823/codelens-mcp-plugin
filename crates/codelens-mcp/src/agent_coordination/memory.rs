use super::{
    AgentWorkEntry, CoordinationCounts, CoordinationSnapshot, FileClaimEntry, FileClaimRequest,
};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct ProjectCoordinationState {
    pub(super) agents: HashMap<String, AgentWorkEntry>,
    pub(super) claims: HashMap<String, FileClaimEntry>,
}

pub(super) fn fallback_register(
    entries: &mut HashMap<String, ProjectCoordinationState>,
    scope: &str,
    entry: AgentWorkEntry,
    now_ms: u64,
) -> AgentWorkEntry {
    let project = entries.entry(scope.to_owned()).or_default();
    prune_project(project, now_ms);
    project
        .agents
        .insert(entry.session_id.clone(), entry.clone());
    entry
}

pub(super) fn fallback_claim(
    entries: &mut HashMap<String, ProjectCoordinationState>,
    scope: &str,
    request: &FileClaimRequest<'_>,
    expires_at: u64,
    now_ms: u64,
) -> FileClaimEntry {
    let project = entries.entry(scope.to_owned()).or_default();
    prune_project(project, now_ms);
    let registered_agent = project.agents.get(request.session_id).cloned();
    let claim = project
        .claims
        .entry(request.session_id.to_owned())
        .or_insert_with(|| FileClaimEntry {
            session_id: request.session_id.to_owned(),
            agent_name: registered_agent
                .as_ref()
                .map(|entry| entry.agent_name.clone())
                .unwrap_or_else(|| request.fallback_agent_name.to_owned()),
            branch: registered_agent
                .as_ref()
                .map(|entry| entry.branch.clone())
                .unwrap_or_else(|| request.fallback_branch.to_owned()),
            worktree: registered_agent
                .as_ref()
                .map(|entry| entry.worktree.clone())
                .unwrap_or_else(|| request.fallback_worktree.to_owned()),
            paths: Vec::new(),
            reason: request.reason.to_owned(),
            expires_at,
        });
    if let Some(agent) = registered_agent {
        claim.agent_name = agent.agent_name;
        claim.branch = agent.branch;
        claim.worktree = agent.worktree;
    }
    claim.reason = request.reason.to_owned();
    claim.expires_at = expires_at;
    for path in &request.paths {
        if !claim.paths.iter().any(|existing| existing == path) {
            claim.paths.push(path.clone());
        }
    }
    claim.paths.sort();
    claim.clone()
}

pub(super) fn fallback_release(
    entries: &mut HashMap<String, ProjectCoordinationState>,
    scope: &str,
    session_id: &str,
    paths: &[String],
    now_ms: u64,
) -> (Vec<String>, Option<FileClaimEntry>) {
    let Some(project) = entries.get_mut(scope) else {
        return (Vec::new(), None);
    };
    prune_project(project, now_ms);
    let Some(claim) = project.claims.get_mut(session_id) else {
        return (Vec::new(), None);
    };
    let mut released = Vec::new();
    claim.paths.retain(|path| {
        let should_remove = paths.iter().any(|target| target == path);
        if should_remove {
            released.push(path.clone());
        }
        !should_remove
    });
    released.sort();
    if claim.paths.is_empty() {
        project.claims.remove(session_id);
        return (released, None);
    }
    claim.paths.sort();
    (released, Some(claim.clone()))
}

pub(super) fn fallback_snapshot(
    entries: &mut HashMap<String, ProjectCoordinationState>,
    scope: &str,
    now_ms: u64,
) -> CoordinationSnapshot {
    let Some(project) = entries.get_mut(scope) else {
        return CoordinationSnapshot::default();
    };
    prune_project(project, now_ms);
    let mut agents = project.agents.values().cloned().collect::<Vec<_>>();
    agents.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    let mut claims = project.claims.values().cloned().collect::<Vec<_>>();
    claims.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    CoordinationSnapshot {
        counts: CoordinationCounts {
            active_agents: agents.len(),
            active_claims: claims.len(),
        },
        agents,
        claims,
    }
}

fn prune_project(project: &mut ProjectCoordinationState, now_ms: u64) {
    project.agents.retain(|_, entry| entry.expires_at > now_ms);
    project.claims.retain(|_, entry| entry.expires_at > now_ms);
}
