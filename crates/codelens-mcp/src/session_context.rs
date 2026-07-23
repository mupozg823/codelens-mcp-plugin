/// Session metadata extracted from `_session_*` JSON keys in tool call arguments
/// or resource request params. Parsed once, then passed to access control, audit,
/// and resource handlers — eliminates duplicate extraction from raw JSON.
///
/// Session context parsed from `_session_*` keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SessionContextSource {
    #[default]
    StdioArguments,
    HttpTransport,
}

thread_local! {
    static REQUEST_CONTEXT_SOURCE: std::cell::Cell<SessionContextSource> =
        const { std::cell::Cell::new(SessionContextSource::StdioArguments) };
}

struct RequestContextSourceGuard(SessionContextSource);

impl Drop for RequestContextSourceGuard {
    fn drop(&mut self) {
        REQUEST_CONTEXT_SOURCE.set(self.0);
    }
}

/// Execute one request inside a transport-owned provenance scope. The source
/// never comes from JSON, so `_session_*` arguments cannot promote themselves
/// into an authenticated HTTP request.
pub(crate) fn with_http_transport_context<T>(f: impl FnOnce() -> T) -> T {
    let previous = REQUEST_CONTEXT_SOURCE.replace(SessionContextSource::HttpTransport);
    let _guard = RequestContextSourceGuard(previous);
    f()
}

fn current_context_source() -> SessionContextSource {
    REQUEST_CONTEXT_SOURCE.get()
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SessionRequestContext {
    pub source: SessionContextSource,
    pub session_id: String,
    pub deferred_loading: bool,
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    pub project_path: Option<String>,
    pub loaded_namespaces: Vec<String>,
    pub loaded_tiers: Vec<String>,
    pub full_tool_exposure: bool,
    pub trusted_client: bool,
    pub requested_profile: Option<String>,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    /// L1 (ADR-0009 §1): principal id derived from the request channel
    /// (HTTP JWT `sub` claim or `X-Codelens-Principal` header). When
    /// present, the role gate uses this in preference to
    /// `CODELENS_PRINCIPAL` env. None for stdio + dev mode where no
    /// channel binding is available — env fallback applies.
    pub principal_id: Option<String>,
}

impl SessionRequestContext {
    /// Extract session context from a JSON value containing `_session_*` keys.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let source = current_context_source();
        let transport_authenticated = matches!(source, SessionContextSource::HttpTransport);
        Self {
            source,
            session_id: str_field(value, "_session_id").unwrap_or_else(|| "local".to_owned()),
            deferred_loading: bool_field(value, "_session_deferred_tool_loading"),
            project_path: str_field(value, "_session_project_path"),
            loaded_namespaces: string_array_field(value, "_session_loaded_namespaces"),
            loaded_tiers: string_array_field(value, "_session_loaded_tiers"),
            full_tool_exposure: bool_field(value, "_session_full_tool_exposure"),
            trusted_client: transport_authenticated && bool_field(value, "_session_trusted_client"),
            requested_profile: str_field(value, "_session_requested_profile"),
            client_name: str_field(value, "_session_client_name"),
            client_version: str_field(value, "_session_client_version"),
            principal_id: transport_authenticated
                .then(|| str_field(value, "_session_principal_id"))
                .flatten(),
        }
    }

    pub fn is_local(&self) -> bool {
        self.session_id == "local"
    }

    pub fn is_transport_authenticated(&self) -> bool {
        matches!(self.source, SessionContextSource::HttpTransport)
    }
}

fn bool_field(value: &serde_json::Value, key: &str) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn str_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

fn string_array_field(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|v| v.as_str())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_json_ignores_unprovenanced_principal_id() {
        let ctx = SessionRequestContext::from_json(&json!({
            "_session_principal_id": "alice@example.com",
        }));
        assert!(ctx.principal_id.is_none());
        assert!(!ctx.is_transport_authenticated());
    }

    #[test]
    fn from_json_returns_none_principal_when_absent() {
        let ctx = SessionRequestContext::from_json(&json!({"_session_id": "sess-1"}));
        assert!(ctx.principal_id.is_none());
    }

    #[test]
    fn caller_json_cannot_claim_http_transport_provenance() {
        let ctx = SessionRequestContext::from_json(&json!({
            "_session_transport_authenticated": true,
            "_session_id": "http-session",
            "_session_trusted_client": true,
        }));
        assert!(!ctx.is_transport_authenticated());
        assert!(!ctx.trusted_client);
        assert_eq!(ctx.source, SessionContextSource::StdioArguments);
    }

    #[test]
    fn transport_scope_authenticates_server_injected_identity() {
        let ctx = with_http_transport_context(|| {
            SessionRequestContext::from_json(&json!({
                "_session_principal_id": "alice@example.com",
                "_session_trusted_client": true,
            }))
        });
        assert!(ctx.is_transport_authenticated());
        assert!(ctx.trusted_client);
        assert_eq!(ctx.principal_id.as_deref(), Some("alice@example.com"));
    }
}
