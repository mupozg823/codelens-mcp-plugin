use super::SemanticCoverage;
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(feature = "http")]
use super::HTTP_UNREACHABLE_HINT;
#[cfg(feature = "http")]
use serde_json::{Value, json};

#[cfg(feature = "http")]
pub(super) fn probe_http_coverage(
    url: &str,
    headers: &BTreeMap<String, String>,
    cwd: &Path,
) -> SemanticCoverage {
    match call_embedding_coverage(url, headers, cwd) {
        Ok(report) => SemanticCoverage::from_report(report),
        Err(detail) => SemanticCoverage::failed("unreachable", detail, HTTP_UNREACHABLE_HINT),
    }
}

#[cfg(not(feature = "http"))]
pub(super) fn probe_http_coverage(
    _url: &str,
    _headers: &BTreeMap<String, String>,
    _cwd: &Path,
) -> SemanticCoverage {
    SemanticCoverage::skipped(
        "http_client_unavailable",
        "binary was built without the http feature; strict semantic coverage probe is unavailable",
    )
}

#[cfg(feature = "http")]
fn call_embedding_coverage(
    url: &str,
    headers: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Value, String> {
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 9001,
        "method": "initialize",
        "params": {
            "clientInfo": {"name": "codelens-doctor", "version": "0"},
            "project": cwd,
            "profile": "review",
            "deferredToolLoading": true,
        },
    });
    let (_, session_id) = post_json_rpc(url, headers, None, &initialize)?;
    let list_tools = json!({
        "jsonrpc": "2.0",
        "id": 9002,
        "method": "tools/list",
        "params": {
            "namespace": "symbols",
            "includeOutputSchema": false,
            "includeAnnotations": false,
        },
    });
    let (list_payload, _) = post_json_rpc(url, headers, session_id.as_deref(), &list_tools)?;
    if let Some(error) = list_payload.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("JSON-RPC error");
        return Err(format!("tools/list namespace=symbols failed: {message}"));
    }
    let call = json!({
        "jsonrpc": "2.0",
        "id": 9003,
        "method": "tools/call",
        "params": {
            "name": "embedding_coverage_report",
            "arguments": {},
        },
    });
    let (payload, _) = post_json_rpc(url, headers, session_id.as_deref(), &call)?;
    extract_tool_payload(&payload)
}

#[cfg(feature = "http")]
fn post_json_rpc(
    url: &str,
    headers: &BTreeMap<String, String>,
    session_id: Option<&str>,
    body: &Value,
) -> Result<(Value, Option<String>), String> {
    let mut request = ureq::post(url).set("content-type", "application/json");
    for (key, value) in headers {
        request = request.set(key, value);
    }
    if let Some(session_id) = session_id {
        request = request.set("mcp-session-id", session_id);
    }
    let response = request
        .send_string(&body.to_string())
        .map_err(|err| format!("HTTP JSON-RPC request failed: {err}"))?;
    let session_id = response.header("mcp-session-id").map(str::to_owned);
    let text = response
        .into_string()
        .map_err(|err| format!("failed to read HTTP response: {err}"))?;
    let payload = serde_json::from_str::<Value>(&text)
        .map_err(|err| format!("HTTP response was not JSON: {err}"))?;
    Ok((payload, session_id))
}

#[cfg(feature = "http")]
fn extract_tool_payload(payload: &Value) -> Result<Value, String> {
    if let Some(error) = payload.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("JSON-RPC error");
        return Err(message.to_owned());
    }
    let result = payload
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing JSON-RPC result".to_owned())?;
    if result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|entry| entry.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("tool returned isError=true");
        return Err(text.to_owned());
    }
    if let Some(data) = result.get("structuredContent").and_then(Value::as_object) {
        return Ok(Value::Object(data.clone()));
    }
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|content| content.first())
        .and_then(|entry| entry.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| "tool content text missing".to_owned())?;
    let decoded = serde_json::from_str::<Value>(text)
        .map_err(|err| format!("tool content was not JSON: {err}"))?;
    Ok(decoded.get("data").cloned().unwrap_or(decoded))
}
