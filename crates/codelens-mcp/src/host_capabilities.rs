use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HostCapabilities {
    pub(crate) native_tool_search: bool,
    pub(crate) native_subagents: bool,
    pub(crate) nested_subagents: bool,
    pub(crate) native_worktrees: bool,
    pub(crate) native_edit: bool,
    pub(crate) mcp_tasks: bool,
    pub(crate) dynamic_tool_list: bool,
    pub(crate) workspace_binding: bool,
    pub(crate) approval_or_elicitation: bool,
}

impl HostCapabilities {
    pub(crate) fn from_arguments(arguments: &Value) -> Option<Self> {
        Self::from_named_fields(arguments, "host_capabilities", "_session_host_capabilities")
    }

    #[cfg(feature = "http")]
    pub(crate) fn from_initialize_params(params: &Value) -> Option<Self> {
        Self::from_named_fields(params, "hostCapabilities", "host_capabilities")
    }

    fn from_named_fields(value: &Value, primary: &str, fallback: &str) -> Option<Self> {
        [primary, fallback].into_iter().find_map(|field| {
            value
                .get(field)
                .filter(|candidate| candidate.is_object())
                .and_then(|candidate| serde_json::from_value(candidate.clone()).ok())
        })
    }

    pub(crate) fn negotiated_payload(capabilities: Option<Self>) -> Value {
        let declared = capabilities.is_some();
        let capabilities = capabilities.unwrap_or_default();
        let mut payload = json!(capabilities);
        payload["declared"] = json!(declared);
        payload
    }

    pub(crate) fn for_request(
        state: &crate::AppState,
        arguments: &Value,
        logical_session_id: &str,
    ) -> Option<Self> {
        let _ = logical_session_id;
        Self::from_arguments(arguments).or_else(|| state.local_host_capabilities())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_declared_boolean_capabilities() {
        let parsed = HostCapabilities::from_arguments(&json!({
            "host_capabilities": {
                "native_tool_search": true,
                "native_edit": true,
            }
        }))
        .expect("declared capability object");

        assert!(parsed.native_tool_search);
        assert!(parsed.native_edit);
        assert!(!parsed.native_subagents);
    }

    #[test]
    fn negotiated_payload_distinguishes_absent_declaration() {
        assert_eq!(
            HostCapabilities::negotiated_payload(None)["declared"],
            json!(false)
        );
    }

    #[cfg(feature = "http")]
    #[test]
    fn parses_initialize_capabilities_without_host_identity_inference() {
        let parsed = HostCapabilities::from_initialize_params(&json!({
            "clientInfo": {"name": "generic-mcp-host"},
            "hostCapabilities": {
                "native_tool_search": true,
                "dynamic_tool_list": true,
            }
        }))
        .expect("initialize capability object");

        assert!(parsed.native_tool_search);
        assert!(parsed.dynamic_tool_list);
        assert!(!parsed.native_subagents);
    }
}
