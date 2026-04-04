use std::fs;
use std::path::Path;

use crate::error::CodeLensError;

/// Append a mutation audit event to `mutation-audit.jsonl` in the given audit directory.
pub(crate) fn record_mutation_audit(
    audit_dir: &Path,
    now_ms: u64,
    project_scope: &str,
    daemon_mode: &str,
    surface: &str,
    tool: &str,
    arguments: &serde_json::Value,
) -> Result<(), CodeLensError> {
    fs::create_dir_all(audit_dir)?;
    let path = audit_dir.join("mutation-audit.jsonl");

    let session_id = arguments
        .get("_session_id")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let session_trusted_client = arguments
        .get("_session_trusted_client")
        .and_then(|value| value.as_bool());
    let session_requested_profile = arguments
        .get("_session_requested_profile")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let session_client_name = arguments
        .get("_session_client_name")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let session_client_version = arguments
        .get("_session_client_version")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);

    let scrubbed_arguments = match arguments {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .filter(|(key, _)| !key.starts_with("_session_"))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
        other => other.clone(),
    };

    let event = serde_json::json!({
        "timestamp_ms": now_ms,
        "project_scope": project_scope,
        "surface": surface,
        "daemon_mode": daemon_mode,
        "tool": tool,
        "arguments": scrubbed_arguments,
        "session": {
            "id": session_id,
            "trusted_client": session_trusted_client,
            "requested_profile": session_requested_profile,
            "client_name": session_client_name,
            "client_version": session_client_version,
        },
    });

    let mut line =
        serde_json::to_string(&event).map_err(|error| CodeLensError::Internal(error.into()))?;
    line.push('\n');

    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}
