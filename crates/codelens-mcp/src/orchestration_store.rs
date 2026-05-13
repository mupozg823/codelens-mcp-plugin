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

    pub(crate) fn clear(&self) {
        self.approvals
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}
