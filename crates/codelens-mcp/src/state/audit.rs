use super::AppState;
use std::sync::Arc;

impl AppState {
    /// ADR-0009 §1: lazy accessor for the resolved principal-to-role
    /// mapping for the *currently active* project. L6 — multi-project:
    /// each `audit_dir()` (which traces the active project) maps to
    /// its own cached `Principals`. Switching projects mid-session
    /// pulls the right `principals.toml` from the cache or discovers
    /// it on first access.
    ///
    /// Discovery failures fall back to the permissive default (every
    /// id maps to `Refactor`) so a malformed file does not block the
    /// dispatch path — the parse error is logged at warn.
    pub(crate) fn principals(&self) -> Arc<crate::principals::Principals> {
        let dir = self.audit_dir();
        {
            let cache = self
                .principals_by_audit_dir
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(existing) = cache.get(&dir) {
                return Arc::clone(existing);
            }
        }
        let resolved = match crate::principals::Principals::discover(&dir) {
            Ok(p) => p,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    audit_dir = %dir.display(),
                    "failed to load principals.toml — falling back to permissive default \
                     (every principal mapped to Refactor)"
                );
                crate::principals::Principals::permissive_default()
            }
        };
        if resolved.default_role() == crate::principals::Role::ReadOnly
            && resolved.explicit_count() == 0
        {
            tracing::warn!(
                audit_dir = %dir.display(),
                "CODELENS_AUTH_MODE=*** in effect with no principals.toml — \
                 every principal is ReadOnly and code-mutation tools will be denied"
            );
        }
        let arc = Arc::new(resolved);
        let mut cache = self
            .principals_by_audit_dir
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        cache.entry(dir).or_insert_with(|| Arc::clone(&arc));
        arc
    }

    /// ADR-0009 §2: lazy accessor for the durable audit sink for the
    /// *currently active* project. L6 — multi-project: each
    /// `audit_dir()` is bound to its own SQLite log; activating
    /// another project switches to that project's sink while keeping
    /// the original alive in cache for when we come back.
    /// `prune_older_than` runs once per (state, project) pair —
    /// re-activating a project does not re-prune.
    /// Failure to open returns `None` so dispatch never fails on
    /// audit alone — the failure is logged and the call proceeds.
    pub(crate) fn audit_sink(&self) -> Option<Arc<crate::audit_sink::AuditSink>> {
        let dir = self.audit_dir();
        {
            let cache = self.audit_sinks.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(existing) = cache.get(&dir) {
                return Some(Arc::clone(existing));
            }
        }
        match crate::audit_sink::AuditSink::open(&dir) {
            Ok(sink) => {
                run_audit_retention_sweep(&sink);
                let arc = Arc::new(sink);
                let mut cache = self.audit_sinks.lock().unwrap_or_else(|p| p.into_inner());
                let entry = cache.entry(dir).or_insert_with(|| Arc::clone(&arc));
                Some(Arc::clone(entry))
            }
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    audit_dir = %dir.display(),
                    "failed to open audit_log.sqlite — audit_sink disabled for this state"
                );
                None
            }
        }
    }
}

/// ADR-0009 §2: prune audit rows older than the retention window.
/// Runs once per `AuditSink::open` (i.e. on the first call to
/// `audit_sink()` per AppState lifetime). The window is controlled
/// by `CODELENS_AUDIT_RETENTION_DAYS`:
/// - unset → 90 days (ADR default)
/// - `0` or negative → retention disabled, no rows pruned
/// - any positive integer → rows older than that many days deleted
///
/// Failures (parse / SQL) are logged at warn and never propagated;
/// the audit sink stays usable even if the prune step misfires.
fn run_audit_retention_sweep(sink: &crate::audit_sink::AuditSink) {
    let days = crate::env_compat::env_var_u64("CODELENS_AUDIT_RETENTION_DAYS")
        .map(|d| d as i64)
        .unwrap_or(90);
    if days <= 0 {
        tracing::debug!(
            "CODELENS_AUDIT_RETENTION_DAYS={days} — audit retention disabled"
        );
        return;
    }
    let now_ms = crate::util::now_ms() as i64;
    let cutoff_ms = now_ms.saturating_sub(days.saturating_mul(86_400_000));
    match sink.prune_older_than(cutoff_ms) {
        Ok(0) => {
            tracing::debug!(retention_days = days, "audit retention sweep — no rows pruned");
        }
        Ok(removed) => {
            tracing::info!(
                retention_days = days,
                pruned_rows = removed,
                "audit retention sweep removed {removed} rows"
            );
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                retention_days = days,
                "audit retention sweep failed — sink remains usable"
            );
        }
    }
}
