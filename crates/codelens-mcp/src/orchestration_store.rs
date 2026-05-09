use std::collections::HashMap;
use std::sync::Mutex;

use crate::runtime_types::OrchestrationApproval;

pub(crate) struct OrchestrationStore {
    approvals: Mutex<HashMap<String, OrchestrationApproval>>,
}

impl OrchestrationStore {
    pub(crate) fn new() -> Self {
        Self {
            approvals: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn key(project_scope: &str, logical_session: &str, run_id: &str) -> String {
        format!("{project_scope}::{logical_session}::{run_id}")
    }

    pub(crate) fn record_approval(&self, key: String, approval: OrchestrationApproval) {
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, approval);
    }

    pub(crate) fn get_approval(&self, key: &str) -> Option<OrchestrationApproval> {
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned()
    }

    pub(crate) fn remove_for_run(&self, project_scope: &str, run_id: &str) -> usize {
        let scope_prefix = format!("{project_scope}::");
        let run_suffix = format!("::{run_id}");
        let mut approvals = self.approvals.lock().unwrap_or_else(|p| p.into_inner());
        let before = approvals.len();
        approvals.retain(|key, _| !(key.starts_with(&scope_prefix) && key.ends_with(&run_suffix)));
        before.saturating_sub(approvals.len())
    }

    pub(crate) fn clear(&self) {
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}
