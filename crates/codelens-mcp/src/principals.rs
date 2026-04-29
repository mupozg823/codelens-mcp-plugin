
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

    /// Strict fallback used when `CODELENS_AUTH_MODE=strict` is set
    /// and no `principals.toml` is present. Every principal —
    /// including the unknown ones — gets `ReadOnly`, so any
    /// mutation tool is denied until the operator places an explicit
    /// principals.toml. This makes "secure by default" opt-in
    /// rather than the global default (which would break existing
    /// stdio installations).
    pub fn strict_default() -> Self {
        Self {
            default_role: Role::ReadOnly,
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
    /// When no file is found, the fallback is chosen by
    /// `CODELENS_AUTH_MODE`:
    /// - unset / `permissive` → [`Self::permissive_default`]
    /// - `strict` → [`Self::strict_default`] (every unknown id is
    ///   `ReadOnly`, so mutation tools are denied)
    ///
    /// Parse errors propagate so misconfiguration is surfaced loudly
    /// at startup instead of silently falling back.
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
        Ok(Self::default_for_env())
    }

    /// Choose between [`Self::permissive_default`] and
    /// [`Self::strict_default`] based on `CODELENS_AUTH_MODE`.
    fn default_for_env() -> Self {
        match std::env::var("CODELENS_AUTH_MODE")
            .ok()
            .as_deref()
            .map(str::trim)
        {
            Some("strict") => Self::strict_default(),
            _ => Self::permissive_default(),
        }
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

/// Required role to call `tool`.
///
/// Tier mapping:
/// - `Admin` — `audit_log_query` and other administrative queries
///   that touch the durable audit log or principals registry.
/// - `Refactor` — code-mutation tools (every entry in
///   [`crate::tool_defs::is_content_mutation_tool`] except the
///   memory carve-out).
/// - `ReadOnly` — everything else, including memory tools.
///
/// Memory tools (`write_memory`/`delete_memory`/`rename_memory`)
/// are a deliberate exception: they mutate agent-side context, not
/// the project's source tree, so the role gate treats them as
/// `ReadOnly`. They remain in `is_content_mutation_tool` so the
/// audit sink still records each memory change.
pub fn required_role_for(tool: &str) -> Role {
    match tool {
        "audit_log_query" => Role::Admin,
        "write_memory" | "delete_memory" | "rename_memory" => Role::ReadOnly,
        other if crate::tool_defs::is_content_mutation_tool(other) => Role::Refactor,
        _ => Role::ReadOnly,
    }
}

/// Resolve the principal id for the current request from the
/// environment. Stdio-only fallback: the dispatch path prefers the
/// session-bound id when available — see [`resolve_principal_id`].
pub fn current_principal_id() -> Option<String> {
    std::env::var("CODELENS_PRINCIPAL").ok()
}

/// L1 (ADR-0009 §1): resolve the principal id for one dispatch call.
///
/// Priority order:
/// 1. `session.principal_id` — populated from the HTTP JWT `sub`
///    claim (or `X-Codelens-Principal` header in dev mode) by the
///    HTTP transport before the request is dispatched.
/// 2. `CODELENS_PRINCIPAL` env — stdio fallback.
/// 3. `None` — falls through to the `default` role in
///    `principals.toml`.
pub fn resolve_principal_id(
    session: &crate::session_context::SessionRequestContext,
) -> Option<String> {
    if let Some(id) = session.principal_id.as_deref().filter(|s| !s.is_empty()) {
        return Some(id.to_owned());
    }
    current_principal_id()
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
    fn strict_default_denies_mutation_for_every_unknown_id() {
        // CODELENS_AUTH_MODE=strict fallback: nobody is privileged
        // until principals.toml lists them.
        let p = Principals::strict_default();
        assert_eq!(p.default_role(), Role::ReadOnly);
        assert_eq!(p.resolve(None), Role::ReadOnly);
        assert_eq!(p.resolve(Some("anyone")), Role::ReadOnly);
        assert!(
            !p.resolve(Some("anyone"))
                .satisfies(required_role_for("create_text_file")),
            "strict default must deny code-mutation tools"
        );
        assert!(
            p.resolve(Some("anyone"))
                .satisfies(required_role_for("write_memory")),
            "strict default must still allow memory-tier tools (M6)"
        );
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
    }

    #[test]
    fn memory_tools_are_readonly_despite_being_mutation_tools() {
        // Memory writes are agent-context, not codebase mutation.
        // The role gate must let read-only principals call them, but
        // the audit sink still tracks them via is_content_mutation_tool.
        for tool in ["write_memory", "delete_memory", "rename_memory"] {
            assert_eq!(
                required_role_for(tool),
                Role::ReadOnly,
                "{tool} should be ReadOnly for the role gate"
            );
            assert!(
                crate::tool_defs::is_content_mutation_tool(tool),
                "{tool} should still appear in is_content_mutation_tool for audit"
            );
        }
    }

    #[test]
    fn required_role_for_query_tool_is_readonly() {
        assert_eq!(required_role_for("find_symbol"), Role::ReadOnly);
        assert_eq!(required_role_for("get_callers"), Role::ReadOnly);
        assert_eq!(required_role_for("analyze_change_request"), Role::ReadOnly);
        assert_eq!(required_role_for("semantic_search"), Role::ReadOnly);
    }

    #[test]
    fn resolve_principal_id_prefers_session_over_env() {
        // Build a session whose principal_id is set (e.g. JWT sub claim
        // injected by the HTTP transport).
        let session =
            crate::session_context::SessionRequestContext::from_json(&serde_json::json!({
                "_session_principal_id": "alice@example.com",
            }));
        let resolved = resolve_principal_id(&session);
        assert_eq!(resolved.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn resolve_principal_id_treats_empty_session_value_as_absent() {
        let session =
            crate::session_context::SessionRequestContext::from_json(&serde_json::json!({
                "_session_principal_id": "",
            }));
        // Empty string must NOT shadow the env fallback. We assert the
        // function does not return Some("") — env may still produce
        // None depending on test runner state, which is allowed.
        match resolve_principal_id(&session) {
            Some(s) => assert!(!s.is_empty(), "empty session id must not surface"),
            None => {}
        }
    }

    #[test]
    fn audit_log_query_requires_admin() {
        // P2-F: durable audit log is admin-tier; ReadOnly + Refactor
        // principals must be denied.
        assert_eq!(required_role_for("audit_log_query"), Role::Admin);
        assert!(
            !Role::ReadOnly.satisfies(required_role_for("audit_log_query")),
            "ReadOnly must NOT call audit_log_query"
        );
        assert!(
            !Role::Refactor.satisfies(required_role_for("audit_log_query")),
            "Refactor must NOT call audit_log_query"
        );
        assert!(
            Role::Admin.satisfies(required_role_for("audit_log_query")),
            "Admin must be able to call audit_log_query"
        );
    }

    #[test]
    fn required_role_for_unknown_tool_defaults_to_readonly() {
        // Conservative default: any tool we have not catalogued is
        // assumed read-only and gated minimally. (Mutation tools
        // must be explicitly declared in is_content_mutation_tool.)
        assert_eq!(required_role_for("nonexistent_tool"), Role::ReadOnly);
    }
}
