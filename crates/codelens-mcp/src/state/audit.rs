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
    /// Discovery failures (a `principals.toml` that is present but
    /// unreadable / unparseable) resolve by whether the runtime can
    /// mutate at all:
    /// - **any mutation-capable mode** (`mutation_allowed_in_runtime()`
    ///   is true — i.e. `Standard`, which is the stdio and
    ///   unspecified-`--daemon-mode` default, and `MutationEnabled`):
    ///   fail closed to `strict_default` (every id maps to `ReadOnly`,
    ///   so code-mutation tools are denied) and log at `error`. A
    ///   runtime that can apply mutations must not silently open up when
    ///   its RBAC file is broken.
    /// - **read-only daemon** (`RuntimeDaemonMode::ReadOnly`): fall back
    ///   to `permissive_default` (every id maps to `Refactor`) and log
    ///   at `warn`. It cannot mutate anyway, so a malformed file need
    ///   not block its read/analysis path.
    ///
    /// A *missing* file is not an error — `discover` returns the
    /// env-selected default and never reaches this branch — so an
    /// install that never deployed a `principals.toml` is unaffected in
    /// every mode. Fail-closed only bites once an RBAC file exists and
    /// is broken.
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
        // Any runtime that can apply mutations (Standard — the stdio /
        // unspecified default — and MutationEnabled) must fail closed on
        // a broken RBAC file; only a read-only daemon keeps the legacy
        // permissive fallback.
        let mutation_allowed = self.mutation_allowed_in_runtime();
        let mut failed_closed = false;
        let resolved = match crate::principals::Principals::discover(&dir) {
            Ok(p) => p,
            Err(error) if mutation_allowed => {
                failed_closed = true;
                tracing::error!(
                    error = %error,
                    audit_dir = %dir.display(),
                    daemon_mode = self.daemon_mode().as_str(),
                    "failed to load principals.toml in a mutation-capable runtime — \
                     refusing permissive fallback and denying all code-mutation tools \
                     (every principal mapped to ReadOnly); fix principals.toml"
                );
                crate::principals::Principals::strict_default()
            }
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
        // Only emit the "strict by env with no principals.toml" notice
        // for the genuine env-selected-strict path. The fail-closed
        // branch above also yields a ReadOnly/0-explicit result, but its
        // cause (a present-but-broken file) is already logged at `error`,
        // and this notice's wording ("no principals.toml") would be false.
        if !failed_closed
            && resolved.default_role() == crate::principals::Role::ReadOnly
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
        tracing::debug!("CODELENS_AUDIT_RETENTION_DAYS={days} — audit retention disabled");
        return;
    }
    let now_ms = crate::util::now_ms() as i64;
    let cutoff_ms = now_ms.saturating_sub(days.saturating_mul(86_400_000));
    match sink.prune_older_than(cutoff_ms) {
        Ok(0) => {
            tracing::debug!(
                retention_days = days,
                "audit retention sweep — no rows pruned"
            );
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

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::principals::Role;
    use crate::state::RuntimeDaemonMode;
    use codelens_engine::ProjectRoot;

    /// Build a temp project whose `.codelens/principals.toml` is present
    /// but unparseable (unknown role string → deserialize error), so
    /// `Principals::discover` returns `Err` rather than a missing-file
    /// default. Returns the project plus a keep-alive dir handle.
    fn project_with_malformed_principals(label: &str) -> (ProjectRoot, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-audit-principals-{label}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(dir.join(".codelens")).unwrap();
        std::fs::write(dir.join("lib.rs"), "fn sample() {}\n").unwrap();
        std::fs::write(
            dir.join(".codelens").join("principals.toml"),
            "[default]\nrole = \"Superuser\"\n",
        )
        .unwrap();
        let project = ProjectRoot::new_exact(&dir).unwrap();
        (project, dir)
    }

    #[test]
    fn mutation_daemon_fails_closed_on_malformed_principals() {
        let (project, _dir) = project_with_malformed_principals("mutation");
        let state = AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full);
        state.configure_daemon_mode(RuntimeDaemonMode::MutationEnabled);
        let principals = state.principals();
        assert_eq!(
            principals.default_role(),
            Role::ReadOnly,
            "mutation-enabled daemon must fail closed (ReadOnly) on a malformed principals.toml"
        );
    }

    #[test]
    fn standard_mode_also_fails_closed_on_malformed_principals() {
        let (project, _dir) = project_with_malformed_principals("standard");
        let state = AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full);
        // Standard is the stdio / unspecified --daemon-mode default and is
        // mutation-capable (mutation_allowed_in_runtime() == true), so it
        // must fail closed just like MutationEnabled.
        state.configure_daemon_mode(RuntimeDaemonMode::Standard);
        let principals = state.principals();
        assert_eq!(
            principals.default_role(),
            Role::ReadOnly,
            "Standard (mutation-capable) mode must fail closed on a malformed principals.toml"
        );
    }

    #[test]
    fn read_path_keeps_permissive_fallback_on_malformed_principals() {
        let (project, _dir) = project_with_malformed_principals("readpath");
        let state = AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full);
        // Read-only daemon is a non-mutation mode: legacy permissive
        // fallback (every principal → Refactor) is preserved.
        state.configure_daemon_mode(RuntimeDaemonMode::ReadOnly);
        let principals = state.principals();
        assert_eq!(
            principals.default_role(),
            Role::Refactor,
            "non-mutation modes must retain the permissive Refactor fallback"
        );
    }
}
