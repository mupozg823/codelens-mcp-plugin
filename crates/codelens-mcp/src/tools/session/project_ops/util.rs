use serde_json::Value;

/// Issue #186: heuristic for spotting Claude/Codex internal workspace
/// directories whose basename is itself an anonymized agent hash
/// (e.g. `agent-a110134bd9c6e7440`). When `prepare_harness_session`
/// resolves the active project to one of these instead of the
/// daemon's CLI startup root, downstream tools see a near-empty
/// index and emit false-positive `no_supported_files` warnings.
///
/// We treat any name that begins with `agent-` and whose suffix is
/// strictly hex (≥ 12 chars of `[0-9a-fA-F]`) as anonymized — the
/// shape that the harness itself produces. Limiting to hex avoids
/// false-positives on real project directories like `agent-server/`
/// or `agent-orchestrator/` where alphabetic chars like `r`/`s`/`t`
/// rule out a hash interpretation.
pub(super) fn is_anonymized_agent_project_name(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("agent-") else {
        return false;
    };
    suffix.len() >= 12 && suffix.chars().all(|c| c.is_ascii_hexdigit())
}

pub(super) fn client_tool_schema_fingerprint(arguments: &Value) -> Option<&str> {
    [
        "known_tool_schema_fingerprint",
        "client_tool_schema_fingerprint",
        "_session_tool_schema_fingerprint",
        "_tool_schema_fingerprint",
    ]
    .iter()
    .find_map(|key| arguments.get(*key).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty())
}
