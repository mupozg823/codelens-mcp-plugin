use crate::runtime_types::OrchestrationApproval;
use serde_json::{Map, Value, json};

use super::AppState;

impl AppState {
    fn orchestration_key_for_scope(
        &self,
        scope: &str,
        logical_session: &str,
        run_id: &str,
    ) -> String {
        crate::orchestration_store::OrchestrationStore::key(scope, logical_session, run_id)
    }

    fn orchestration_key_for_session(
        &self,
        session: &crate::session_context::SessionRequestContext,
        logical_session: &str,
        run_id: &str,
    ) -> String {
        self.orchestration_key_for_scope(
            &self.project_scope_for_session(session),
            logical_session,
            run_id,
        )
    }

    pub(crate) fn clear_orchestration_approvals(&self) {
        self.orchestration_store.clear();
    }

    pub(crate) fn record_orchestration_approval(
        &self,
        logical_session: &str,
        run_id: String,
        actor: String,
        target_paths: Vec<String>,
        approved_actions: Vec<String>,
        analysis_id: Option<String>,
    ) {
        let approval = OrchestrationApproval {
            run_id: run_id.clone(),
            actor,
            timestamp_ms: crate::util::now_ms(),
            target_paths,
            approved_actions,
            analysis_id,
        };
        let key = self.orchestration_key_for_scope(
            &self.current_project_scope(),
            logical_session,
            &run_id,
        );
        self.orchestration_store.record_approval(key, approval);
    }

    pub(crate) fn orchestration_approval_for_session(
        &self,
        session: &crate::session_context::SessionRequestContext,
        logical_session: &str,
        run_id: &str,
    ) -> Option<OrchestrationApproval> {
        self.orchestration_store
            .get_approval(&self.orchestration_key_for_session(session, logical_session, run_id))
    }

    pub(crate) fn orchestration_approval_for_current_scope(
        &self,
        logical_session: &str,
        run_id: &str,
    ) -> Option<OrchestrationApproval> {
        self.orchestration_store
            .get_approval(&self.orchestration_key_for_scope(
                &self.current_project_scope(),
                logical_session,
                run_id,
            ))
    }

    pub(crate) fn append_orchestration_event_for_current_scope(
        &self,
        logical_session: &str,
        run_id: &str,
        event_name: &str,
        from: Option<&str>,
        to: &str,
        mut extra: Map<String, Value>,
    ) -> Option<Value> {
        let approval = self.orchestration_approval_for_current_scope(logical_session, run_id)?;
        let analysis_id = approval.analysis_id.clone()?;
        let scope = self.current_project_scope();
        let mut event = Map::new();
        event.insert("run_id".to_owned(), json!(run_id));
        event.insert("event".to_owned(), json!(event_name));
        event.insert(
            "from".to_owned(),
            from.map_or(Value::Null, |value| json!(value)),
        );
        event.insert("to".to_owned(), json!(to));
        event.insert("timestamp_ms".to_owned(), json!(crate::util::now_ms()));
        event.insert("audit_required".to_owned(), json!(true));
        event.insert("approval_actor".to_owned(), json!(approval.actor));
        for (key, value) in std::mem::take(&mut extra) {
            event.insert(key, value);
        }
        let event = Value::Object(event);

        let mut audit_events = self
            .get_analysis_section(&analysis_id, "audit_events")
            .unwrap_or_else(|_| json!({"events": []}));
        if let Some(events) = audit_events
            .get_mut("events")
            .and_then(|value| value.as_array_mut())
        {
            events.push(event.clone());
        } else {
            audit_events = json!({"events": [event.clone()]});
        }
        let _ = self.upsert_analysis_section_for_scope(
            &scope,
            &analysis_id,
            "audit_events",
            &audit_events,
        );

        if let Ok(mut run) = self.get_analysis_section(&analysis_id, "orchestration_run")
            && let Some(obj) = run.as_object_mut()
        {
            obj.insert("state".to_owned(), json!(to));
            obj.insert("last_event".to_owned(), json!(event_name));
            obj.insert(
                "last_event_timestamp_ms".to_owned(),
                event
                    .get("timestamp_ms")
                    .cloned()
                    .unwrap_or_else(|| json!(crate::util::now_ms())),
            );
            let _ = self.upsert_analysis_section_for_scope(
                &scope,
                &analysis_id,
                "orchestration_run",
                &run,
            );
        }

        Some(event)
    }
}
