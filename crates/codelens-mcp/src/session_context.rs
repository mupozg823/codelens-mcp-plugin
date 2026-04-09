/// Session metadata extracted from `_session_*` JSON keys in tool call arguments
/// or resource request params. Parsed once, then passed to access control, audit,
/// and resource handlers — eliminates duplicate extraction from raw JSON.
///
/// Session context parsed from `_session_*` keys.
#[derive(Clone, Debug, Default)]
pub(crate) struct SessionRequestContext {
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
}

impl SessionRequestContext {
    /// Extract session context from a JSON value containing `_session_*` keys.
    pub fn from_json(value: &serde_json::Value) -> Self {
        Self {
            session_id: str_field(value, "_session_id").unwrap_or_else(|| "local".to_owned()),
            deferred_loading: bool_field(value, "_session_deferred_tool_loading"),
            project_path: str_field(value, "_session_project_path"),
            loaded_namespaces: string_array_field(value, "_session_loaded_namespaces"),
            loaded_tiers: string_array_field(value, "_session_loaded_tiers"),
            full_tool_exposure: bool_field(value, "_session_full_tool_exposure"),
            trusted_client: bool_field(value, "_session_trusted_client"),
            requested_profile: str_field(value, "_session_requested_profile"),
            client_name: str_field(value, "_session_client_name"),
            client_version: str_field(value, "_session_client_version"),
        }
    }

    pub fn is_local(&self) -> bool {
        self.session_id == "local"
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
