use crate::protocol::Tool;
use serde_json::{Value, json};

pub(crate) const TOOL_SCHEMA_REFRESH_ACTION: &str = "reissue_tools_list_or_reconnect";
pub(crate) const TOOL_SCHEMA_REFRESH_HINT: &str = "If cached tool metadata disagrees with this fingerprint, reissue tools/list or reconnect before trusting the old schema.";

pub(crate) fn tool_schema_fingerprint(tools: &[&Tool]) -> String {
    let schemas = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "inputSchema": tool.input_schema,
            })
        })
        .collect::<Vec<_>>();
    crate::util::canonical_sha256_hex(&Value::Array(schemas))
}

pub(crate) fn surface_generation_payload(tools: &[&Tool]) -> Value {
    json!({
        "schema_version": crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION,
        "binary_version": crate::build_info::BUILD_VERSION,
        "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
        "binary_build_time": crate::build_info::BUILD_TIME,
        "tool_schema_fingerprint": tool_schema_fingerprint(tools),
        "refresh_action": TOOL_SCHEMA_REFRESH_ACTION,
        "refresh_hint": TOOL_SCHEMA_REFRESH_HINT,
    })
}
