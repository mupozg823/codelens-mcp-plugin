//! I3.6 (runtime-convergence plan): memory-pressure admission gate for heavy
//! indexing jobs — the vm_stat load-shedding back-import from the CodeGraph
//! study. A heavy index job defers while the OS reports memory pressure at
//! warning level or above, so the daemon never piles indexing onto an already
//! pressured machine (broadcast-coexistence requirement). The probe reads
//! `kern.memorystatus_vm_pressure_level` — the sysctl behind macOS
//! `memory_pressure`/`vm_stat` tooling. Non-macOS targets and probe failures
//! read as `Normal` (fail-open): admission gating must never strand a job.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryPressure {
    Normal,
    Warning,
    Critical,
}

impl MemoryPressure {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            MemoryPressure::Normal => "normal",
            MemoryPressure::Warning => "warning",
            MemoryPressure::Critical => "critical",
        }
    }

    fn is_pressured(&self) -> bool {
        !matches!(self, MemoryPressure::Normal)
    }
}

/// Outcome of the admission wait for one heavy index job.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum IndexAdmission {
    /// The job may start. `under_pressure` is true when the defer budget ran
    /// out while pressure persisted — the job proceeds anyway so it can never
    /// starve, but callers may want to surface that in the job record.
    Admitted {
        waited_ms: u64,
        under_pressure: bool,
    },
    /// The `on_defer` callback asked to stop waiting (job cancelled).
    Aborted,
}

/// Job kinds heavy enough to gate on memory pressure.
pub(crate) const HEAVY_INDEX_KINDS: &[&str] = &["refresh_symbol_index", "index_embeddings"];

pub(crate) fn index_pressure_max_defer_ms() -> u64 {
    // Under cargo test the defer budget defaults to 0 (gate admits
    // immediately): a genuinely pressured dev machine otherwise stalls
    // unrelated background-job tests past their poll budgets — measured on
    // refresh_symbol_index_background_queues_and_completes_job with ~67MB
    // free. Tests that want the deferral behavior opt in via the env var;
    // the pure wait_for_index_admission seam stays fully testable either way.
    let default_secs = if cfg!(test) { 0 } else { 120 };
    crate::env_compat::env_var_u64("CODELENS_INDEX_PRESSURE_MAX_DEFER_SECS")
        .unwrap_or(default_secs)
        .saturating_mul(1000)
}

/// Wait until the probe reports `Normal`, the defer budget is exhausted, or
/// `on_defer` (called once per pressured poll with the accumulated wait and
/// the observed level) returns `false` to abort. Pure seam — the runner
/// injects the real probe/sleep, tests inject deterministic ones.
pub(crate) fn wait_for_index_admission(
    max_defer_ms: u64,
    poll_ms: u64,
    mut probe: impl FnMut() -> MemoryPressure,
    mut sleep: impl FnMut(u64),
    mut on_defer: impl FnMut(u64, MemoryPressure) -> bool,
) -> IndexAdmission {
    let mut waited_ms = 0u64;
    loop {
        let level = probe();
        if !level.is_pressured() {
            return IndexAdmission::Admitted {
                waited_ms,
                under_pressure: false,
            };
        }
        if waited_ms >= max_defer_ms {
            return IndexAdmission::Admitted {
                waited_ms,
                under_pressure: true,
            };
        }
        if !on_defer(waited_ms, level) {
            return IndexAdmission::Aborted;
        }
        sleep(poll_ms);
        waited_ms = waited_ms.saturating_add(poll_ms);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn current_memory_pressure() -> MemoryPressure {
    let Ok(name) = std::ffi::CString::new("kern.memorystatus_vm_pressure_level") else {
        return MemoryPressure::Normal;
    };
    let mut level: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut level as *mut libc::c_int as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return MemoryPressure::Normal;
    }
    // Kernel levels: 1 = normal, 2 = warning, 4 = critical.
    match level {
        l if l >= 4 => MemoryPressure::Critical,
        2..=3 => MemoryPressure::Warning,
        _ => MemoryPressure::Normal,
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn current_memory_pressure() -> MemoryPressure {
    MemoryPressure::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_immediately_when_normal() {
        let admission = wait_for_index_admission(
            10_000,
            1_000,
            || MemoryPressure::Normal,
            |_| panic!("must not sleep when normal"),
            |_, _| panic!("must not defer when normal"),
        );
        assert_eq!(
            admission,
            IndexAdmission::Admitted {
                waited_ms: 0,
                under_pressure: false
            }
        );
    }

    #[test]
    fn defers_until_pressure_clears() {
        let mut levels = [
            MemoryPressure::Warning,
            MemoryPressure::Critical,
            MemoryPressure::Normal,
        ]
        .into_iter();
        let mut defers = Vec::new();
        let mut slept_ms = 0u64;
        let admission = wait_for_index_admission(
            60_000,
            2_000,
            move || levels.next().expect("probe called after admission"),
            |ms| slept_ms += ms,
            |waited, level| {
                defers.push((waited, level));
                true
            },
        );
        assert_eq!(
            admission,
            IndexAdmission::Admitted {
                waited_ms: 4_000,
                under_pressure: false
            }
        );
        assert_eq!(
            defers,
            vec![
                (0, MemoryPressure::Warning),
                (2_000, MemoryPressure::Critical)
            ]
        );
        assert_eq!(slept_ms, 4_000, "two pressured polls sleep twice");
    }

    #[test]
    fn defer_budget_exhaustion_admits_under_pressure() {
        let admission = wait_for_index_admission(
            4_000,
            2_000,
            || MemoryPressure::Warning,
            |_| {},
            |_, _| true,
        );
        assert_eq!(
            admission,
            IndexAdmission::Admitted {
                waited_ms: 4_000,
                under_pressure: true
            }
        );
    }

    #[test]
    fn on_defer_false_aborts() {
        let admission = wait_for_index_admission(
            60_000,
            2_000,
            || MemoryPressure::Warning,
            |_| {},
            |_, _| false,
        );
        assert_eq!(admission, IndexAdmission::Aborted);
    }

    #[test]
    fn live_probe_returns_a_level_without_crashing() {
        let level = current_memory_pressure();
        assert!(matches!(
            level,
            MemoryPressure::Normal | MemoryPressure::Warning | MemoryPressure::Critical
        ));
    }

    #[test]
    fn heavy_kind_list_covers_the_two_index_jobs() {
        assert!(HEAVY_INDEX_KINDS.contains(&"refresh_symbol_index"));
        assert!(HEAVY_INDEX_KINDS.contains(&"index_embeddings"));
        assert_eq!(HEAVY_INDEX_KINDS.len(), 2);
    }
}
