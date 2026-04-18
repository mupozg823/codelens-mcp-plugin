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
///
/// `canonical_name` must be the `CODELENS_*` form (e.g.
/// `"CODELENS_RATE_LIMIT"`). Passing a `SYMBIOTE_*` form is a
/// programmer error and panics in debug, silently degrades in release.
///
/// Wired into the startup env-var reads in `main.rs` so both prefixes
/// work at runtime; ADR-0007 Phase 3 (v2.0.0) will rename the canonical
/// form from `CODELENS_*` to `SYMBIOTE_*` and this helper becomes a
/// single-prefix lookup. Until then, every call site that used to do
/// `std::env::var("CODELENS_*")` should route through here.
pub fn dual_prefix_env(canonical_name: &str) -> Option<String> {
    debug_assert!(
        canonical_name.starts_with("CODELENS_"),
        "dual_prefix_env expects a CODELENS_* canonical name, got `{}`",
        canonical_name
    );
    let symbiote_name = canonical_name.replacen("CODELENS_", "SYMBIOTE_", 1);
    if let Ok(value) = env::var(&symbiote_name)
        && !value.is_empty()
    {
        return Some(value);
    }
    match env::var(canonical_name) {
        Ok(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

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
