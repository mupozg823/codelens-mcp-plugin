//! ADR-0007 Phase 2 — dual-prefix env var compatibility.
//!
//! At v2.0.0 every `CODELENS_*` env var becomes `SYMBIOTE_*`. This
//! module provides the single lookup helper new call sites should use
//! so old `CODELENS_*` consumers keep working, new `SYMBIOTE_*`
//! consumers start working, and the cutover at v2.0.0 is a search &
//! replace rather than a semantic change.
//!
//! The existing fleet of direct `std::env::var("CODELENS_*")` calls is
//! deliberately *not* rewritten in Phase 2 — that would be churn with
//! no user-visible benefit. They get rewritten in Phase 3 (v2.0.0).

use std::env;

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Resolve an env var honoring both `SYMBIOTE_*` and `CODELENS_*`
/// prefixes. If `SYMBIOTE_*` is set, it wins (matches the rebrand
/// direction). Falls back to the canonical `CODELENS_*` name. Returns
/// `None` when neither is set or both are empty.
pub fn env_var_string(canonical_name: &str) -> Option<String> {
    debug_assert!(
        canonical_name.starts_with("CODELENS_"),
        "env_var_string expects a CODELENS_* canonical name, got `{}`",
        canonical_name
    );
    let symbiote_name = canonical_name.replacen("CODELENS_", "SYMBIOTE_", 1);
    match env::var(&symbiote_name) {
        Ok(value) if !value.is_empty() => return Some(value),
        _ => {}
    }
    match env::var(canonical_name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

/// Parse an env var as `u64`. Returns `None` if unset, empty, or unparseable.
pub fn env_var_u64(canonical_name: &str) -> Option<u64> {
    env_var_string(canonical_name).and_then(|s| s.parse().ok())
}

/// Parse an env var as `bool`. Accepts `1`, `true`, `yes` (case-insensitive).
/// Returns `None` if unset or empty; returns `Some(false)` for any other value.
pub fn env_var_bool(canonical_name: &str) -> Option<bool> {
    env_var_string(canonical_name)
        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

// Backwards-compatible alias for existing call sites.
pub use env_var_string as dual_prefix_env;

#[cfg(test)]
mod tests {
    use super::*;

    // Serialize env-var tests — std::env is process-global.
    fn with_env<F: FnOnce()>(symbiote: Option<&str>, codelens: Option<&str>, f: F) {
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev_symbiote = env::var("SYMBIOTE_ENV_COMPAT_TEST").ok();
        let prev_codelens = env::var("CODELENS_ENV_COMPAT_TEST").ok();
        // SAFETY: tests run serially under ENV_LOCK.
        unsafe {
            match symbiote {
                Some(v) => env::set_var("SYMBIOTE_ENV_COMPAT_TEST", v),
                None => env::remove_var("SYMBIOTE_ENV_COMPAT_TEST"),
            }
            match codelens {
                Some(v) => env::set_var("CODELENS_ENV_COMPAT_TEST", v),
                None => env::remove_var("CODELENS_ENV_COMPAT_TEST"),
            }
        }
        f();
        // Restore.
        unsafe {
            match prev_symbiote {
                Some(v) => env::set_var("SYMBIOTE_ENV_COMPAT_TEST", v),
                None => env::remove_var("SYMBIOTE_ENV_COMPAT_TEST"),
            }
            match prev_codelens {
                Some(v) => env::set_var("CODELENS_ENV_COMPAT_TEST", v),
                None => env::remove_var("CODELENS_ENV_COMPAT_TEST"),
            }
        }
    }

    #[test]
    fn symbiote_wins_when_both_set() {
        with_env(Some("sym"), Some("code"), || {
            assert_eq!(
                dual_prefix_env("CODELENS_ENV_COMPAT_TEST").as_deref(),
                Some("sym")
            );
        });
    }

    #[test]
    fn codelens_used_when_only_it_is_set() {
        with_env(None, Some("code"), || {
            assert_eq!(
                dual_prefix_env("CODELENS_ENV_COMPAT_TEST").as_deref(),
                Some("code")
            );
        });
    }

    #[test]
    fn symbiote_used_when_only_it_is_set() {
        with_env(Some("sym"), None, || {
            assert_eq!(
                dual_prefix_env("CODELENS_ENV_COMPAT_TEST").as_deref(),
                Some("sym")
            );
        });
    }

    #[test]
    fn empty_symbiote_falls_back_to_codelens() {
        with_env(Some(""), Some("code"), || {
            assert_eq!(
                dual_prefix_env("CODELENS_ENV_COMPAT_TEST").as_deref(),
                Some("code")
            );
        });
    }

    #[test]
    fn none_when_both_unset() {
        with_env(None, None, || {
            assert_eq!(dual_prefix_env("CODELENS_ENV_COMPAT_TEST"), None);
        });
    }
}
