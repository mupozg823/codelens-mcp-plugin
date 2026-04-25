//! ADR-0009 §1: 3-tier role model + `principals.toml` loader.
//!
//! The role gate enforces `(principal, role) → allowed_tools` at the
//! dispatch entry. This module owns the data model and the file-based
//! configuration; `dispatch::role_gate` owns the enforcement decision.
//!
//! ## Role hierarchy
//!
//! ```text
//! Admin    > Refactor > ReadOnly
//! ```
//!
//! A principal with `Refactor` may call all `ReadOnly` tools; an
//! `Admin` principal may call all `Refactor` tools.
//!
//! ## Required role per tool
//!
//! - **ReadOnly**: every non-mutation tool — `analyze_*`, `find_*`,
//!   `get_*`, `semantic_search`, etc.
//! - **Refactor**: every tool listed in
//!   [`crate::tool_defs::is_content_mutation_tool`] (the 9 raw_fs
//!   primitives + LSP rename + safe_delete + memory writes + refactor
//!   primitives).
//! - **Admin**: reserved for the future `audit_log_query` and job
//!   control tools (P2-F). For now no tool requires it; the variant
//!   exists so the configuration file is forward-compatible.
//!
//! ## Backward compatibility
//!
//! When `principals.toml` is absent, the loader returns a permissive
//! default: every principal id maps to `Refactor`. This preserves the
//! current pre-Phase-2 behaviour. Operators opt in to strict access by
//! placing the file at `<project>/.codelens/principals.toml` or
//! `~/.codelens/principals.toml`.
//!
//! ## File format
//!
//! ```toml
//! [default]
//! role = "ReadOnly"
//!
//! [principal."alice@example.com"]
//! role = "Admin"
//!
//! [principal."ci-bot"]
//! role = "ReadOnly"
//! ```

#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// 3-tier role; ordering encodes the hierarchy
/// (`Admin > Refactor > ReadOnly`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum Role {
    ReadOnly,
    Refactor,
    Admin,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::ReadOnly => "ReadOnly",
            Role::Refactor => "Refactor",
            Role::Admin => "Admin",
        }
    }

    /// True when `self` may call any tool that requires `required`.
    /// Encodes the role hierarchy: `Admin >= Refactor >= ReadOnly`.
    pub fn satisfies(self, required: Role) -> bool {
        self >= required
    }
}

/// Resolved role mapping. Built once at AppState init from a
/// `principals.toml` (if any) and consulted by the role gate on every
/// dispatch.
#[derive(Debug, Clone)]
pub struct Principals {
    default_role: Role,
    by_id: HashMap<String, Role>,
}

impl Principals {
    /// Permissive fallback used when no `principals.toml` is present.
    /// Every principal — including the unknown ones — gets `Refactor`,
    /// preserving the pre-Phase-2 behaviour.
    pub fn permissive_default() -> Self {
        Self {
            default_role: Role::Refactor,
            by_id: HashMap::new(),
        }
    }

    /// Resolve a principal id to its role. Unknown ids fall back to
    /// the default role.
    pub fn resolve(&self, principal_id: Option<&str>) -> Role {
        principal_id
            .and_then(|id| self.by_id.get(id).copied())
            .unwrap_or(self.default_role)
    }

    /// Number of explicit principal entries (excludes default). Useful
    /// for tests and observability.
    pub fn explicit_count(&self) -> usize {
        self.by_id.len()
    }

    pub fn default_role(&self) -> Role {
        self.default_role
    }

    /// Discover a `principals.toml` and parse it.
    ///
    /// Search order (first existing wins):
    /// 1. `<project>/.codelens/principals.toml`
    /// 2. `$HOME/.codelens/principals.toml`
    ///
    /// Returns the permissive default when no file is found. Parse
    /// errors propagate so misconfiguration is surfaced loudly at
    /// startup instead of silently falling back to `Refactor`.
    pub fn discover(project_audit_dir: &Path) -> Result<Self> {
        // project audit dir is `<project>/.codelens/audit/`; principals.toml
        // lives one directory up at `<project>/.codelens/principals.toml`.
        if let Some(codelens_dir) = project_audit_dir.parent() {
            let project_path = codelens_dir.join("principals.toml");
            if project_path.exists() {
                return Self::load_from(&project_path);
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            let user_path = Path::new(&home).join(".codelens").join("principals.toml");
            if user_path.exists() {
                return Self::load_from(&user_path);
            }
        }
        Ok(Self::permissive_default())
    }

    /// Parse a specific TOML file. Public for testability.
    pub fn load_from(path: &Path) -> Result<Self> {
        let bytes = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Self::parse(&bytes)
            .with_context(|| format!("failed to parse principals file {}", path.display()))
    }

    /// Parse from in-memory TOML text. Public for unit tests.
    pub fn parse(text: &str) -> Result<Self> {
        #[derive(Deserialize)]
        struct DefaultEntry {
            role: Role,
        }
        #[derive(Deserialize)]
        struct PrincipalEntry {
            role: Role,
        }
        #[derive(Deserialize)]
        struct File {
            default: Option<DefaultEntry>,
            #[serde(default)]
            principal: HashMap<String, PrincipalEntry>,
        }
        let parsed: File = toml::from_str(text).context("toml parse error")?;
        let default_role = parsed.default.map(|d| d.role).unwrap_or(Role::Refactor);
        let by_id = parsed
            .principal
            .into_iter()
            .map(|(id, entry)| (id, entry.role))
            .collect();
        Ok(Self {
            default_role,
            by_id,
        })
    }
}

/// Required role to call `tool`. Maps every tool name to one of the
/// three tiers; the rule is simple by design (mutation tools require
/// Refactor; everything else is ReadOnly). New tools added in the
/// future are ReadOnly by default unless they appear in
/// [`crate::tool_defs::is_content_mutation_tool`].
pub fn required_role_for(tool: &str) -> Role {
    if crate::tool_defs::is_content_mutation_tool(tool) {
        Role::Refactor
    } else {
        Role::ReadOnly
    }
}

/// Resolve the principal id for the current request from the
/// environment.
///
/// Priority order:
/// 1. `CODELENS_PRINCIPAL` env var (stdio + dev mode)
/// 2. None (caller-provided HTTP / JWT bindings come in P2-C-follow-up)
///
/// Phase 2-C limits the binding to env-only so the role gate can land
/// without coupling to the HTTP feature flag. Header / JWT extraction
/// is mechanical to add in a follow-up once the Authorization plumbing
/// is in place.
pub fn current_principal_id() -> Option<String> {
    std::env::var("CODELENS_PRINCIPAL").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_ordering_encodes_hierarchy() {
        assert!(Role::Admin > Role::Refactor);
        assert!(Role::Refactor > Role::ReadOnly);
        assert!(Role::Admin.satisfies(Role::ReadOnly));
        assert!(Role::Refactor.satisfies(Role::ReadOnly));
        assert!(!Role::ReadOnly.satisfies(Role::Refactor));
        assert!(!Role::Refactor.satisfies(Role::Admin));
    }

    #[test]
    fn permissive_default_resolves_every_id_to_refactor() {
        let p = Principals::permissive_default();
        assert_eq!(p.resolve(None), Role::Refactor);
        assert_eq!(p.resolve(Some("alice")), Role::Refactor);
        assert_eq!(p.explicit_count(), 0);
    }

    #[test]
    fn parse_minimal_default_only() {
        let toml = r#"
            [default]
            role = "ReadOnly"
        "#;
        let p = Principals::parse(toml).expect("parse ok");
        assert_eq!(p.default_role(), Role::ReadOnly);
        assert_eq!(p.resolve(None), Role::ReadOnly);
        assert_eq!(p.explicit_count(), 0);
    }

    #[test]
    fn parse_full_principal_table() {
        let toml = r#"
            [default]
            role = "ReadOnly"

            [principal."alice@example.com"]
            role = "Admin"

            [principal."ci-bot"]
            role = "Refactor"
        "#;
        let p = Principals::parse(toml).expect("parse ok");
        assert_eq!(p.default_role(), Role::ReadOnly);
        assert_eq!(p.resolve(Some("alice@example.com")), Role::Admin);
        assert_eq!(p.resolve(Some("ci-bot")), Role::Refactor);
        assert_eq!(
            p.resolve(Some("unknown")),
            Role::ReadOnly,
            "unknown id falls back to default_role"
        );
        assert_eq!(p.explicit_count(), 2);
    }

    #[test]
    fn parse_missing_default_uses_refactor() {
        let toml = r#"
            [principal."alice"]
            role = "Admin"
        "#;
        let p = Principals::parse(toml).expect("parse ok");
        assert_eq!(p.default_role(), Role::Refactor);
        assert_eq!(p.resolve(None), Role::Refactor);
        assert_eq!(p.resolve(Some("alice")), Role::Admin);
    }

    #[test]
    fn parse_invalid_role_string_errors() {
        let toml = r#"
            [default]
            role = "Superuser"
        "#;
        let result = Principals::parse(toml);
        assert!(result.is_err(), "invalid role must surface as Err");
    }

    #[test]
    fn discover_returns_permissive_when_nothing_present() {
        // Empty tempdir whose parent is also empty — neither
        // project nor user file exists.
        let dir = std::env::temp_dir().join(format!(
            "codelens-principals-empty-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Force HOME to an empty dir for this test so the
        // user-level fallback also misses.
        let original_home = std::env::var_os("HOME");
        let fake_home = dir.join("fake_home");
        std::fs::create_dir_all(&fake_home).unwrap();
        unsafe {
            std::env::set_var("HOME", &fake_home);
        }
        let p = Principals::discover(&dir).expect("discover ok");
        // Restore HOME before assertions so a panic does not leak.
        unsafe {
            match original_home {
                Some(h) => std::env::set_var("HOME", h),
                None => std::env::remove_var("HOME"),
            }
        }
        assert_eq!(p.default_role(), Role::Refactor);
        assert_eq!(p.explicit_count(), 0);
    }

    #[test]
    fn discover_loads_project_local_file() {
        let codelens_dir = std::env::temp_dir().join(format!(
            "codelens-principals-loaded-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let audit_dir = codelens_dir.join("audit");
        std::fs::create_dir_all(&audit_dir).unwrap();
        let principals_path = codelens_dir.join("principals.toml");
        std::fs::write(
            &principals_path,
            r#"
[default]
role = "ReadOnly"

[principal."alice"]
role = "Admin"
"#,
        )
        .unwrap();
        let p = Principals::discover(&audit_dir).expect("discover ok");
        assert_eq!(p.default_role(), Role::ReadOnly);
        assert_eq!(p.resolve(Some("alice")), Role::Admin);
    }

    #[test]
    fn required_role_for_mutation_tool_is_refactor() {
        assert_eq!(required_role_for("create_text_file"), Role::Refactor);
        assert_eq!(required_role_for("delete_lines"), Role::Refactor);
        assert_eq!(required_role_for("rename_symbol"), Role::Refactor);
        assert_eq!(required_role_for("write_memory"), Role::Refactor);
    }

    #[test]
    fn required_role_for_query_tool_is_readonly() {
        assert_eq!(required_role_for("find_symbol"), Role::ReadOnly);
        assert_eq!(required_role_for("get_callers"), Role::ReadOnly);
        assert_eq!(required_role_for("analyze_change_request"), Role::ReadOnly);
        assert_eq!(required_role_for("semantic_search"), Role::ReadOnly);
    }

    #[test]
    fn required_role_for_unknown_tool_defaults_to_readonly() {
        // Conservative default: any tool we have not catalogued is
        // assumed read-only and gated minimally. (Mutation tools
        // must be explicitly declared in is_content_mutation_tool.)
        assert_eq!(required_role_for("nonexistent_tool"), Role::ReadOnly);
    }
}
