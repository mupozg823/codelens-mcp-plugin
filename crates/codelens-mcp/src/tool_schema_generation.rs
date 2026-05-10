use crate::protocol::Tool;
use serde_json::{json, Value};

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
        "tool_schema_fingerprint": tool_schema_fingerprint(tools),
        "refresh_action": TOOL_SCHEMA_REFRESH_ACTION,
        "refresh_hint": TOOL_SCHEMA_REFRESH_HINT,
        "runtime": {
            "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
            "binary_build_time": crate::build_info::BUILD_TIME,
        },
    })
}

#[cfg(test)]
mod surface_generation_split_tests {
    use super::surface_generation_payload;
    use crate::protocol::Tool;

    #[test]
    fn payload_top_level_keeps_only_stable_fields() {
        let tools: Vec<&Tool> = Vec::new();
        let payload = surface_generation_payload(&tools);
        let obj = payload.as_object().expect("object");

        assert!(obj.contains_key("schema_version"));
        assert!(obj.contains_key("binary_version"));
        assert!(obj.contains_key("tool_schema_fingerprint"));
        assert!(obj.contains_key("refresh_action"));
        assert!(obj.contains_key("refresh_hint"));

        assert!(
            !obj.contains_key("binary_git_sha"),
            "binary_git_sha must move under runtime to keep prompt-cache prefix stable"
        );
        assert!(
            !obj.contains_key("binary_build_time"),
            "binary_build_time must move under runtime"
        );

        let runtime = obj
            .get("runtime")
            .and_then(|v| v.as_object())
            .expect("runtime nested object present");
        assert!(runtime.contains_key("binary_git_sha"));
        assert!(runtime.contains_key("binary_build_time"));
    }
}
