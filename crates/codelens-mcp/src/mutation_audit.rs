use std::fs;
use std::path::Path;

use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;

/// Append a mutation audit event to `mutation-audit.jsonl` in the given audit directory.
pub(crate) fn record_mutation_audit(
    audit_dir: &Path,
    now_ms: u64,
    project_scope: &str,
    daemon_mode: &str,
    surface: &str,
    tool: &str,
    arguments: &serde_json::Value,
    session: &SessionRequestContext,
) -> Result<(), CodeLensError> {
    fs::create_dir_all(audit_dir)?;
    let path = audit_dir.join("mutation-audit.jsonl");

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
            "id": session.session_id,
            "trusted_client": session.trusted_client,
            "requested_profile": session.requested_profile,
            "client_name": session.client_name,
            "client_version": session.client_version,
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
