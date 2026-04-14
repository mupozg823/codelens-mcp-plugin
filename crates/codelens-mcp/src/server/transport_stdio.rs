use super::router::{handle_request, protocol_error_response};
use crate::AppState;
use crate::protocol::JsonRpcRequest;
use anyhow::Result;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::io::{self, BufRead, Write};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StdioWireFormat {
    ContentLength,
    LineDelimitedJson,
}

#[derive(Serialize)]
#[serde(untagged)]
enum StdioResponsePayload {
    Single(crate::protocol::JsonRpcResponse),
    Batch(Vec<crate::protocol::JsonRpcResponse>),
}

fn trim_line_endings(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn parse_content_length_header(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    if !name.trim().eq_ignore_ascii_case("Content-Length") {
        return None;
    }
    value.trim().parse::<usize>().ok()
}

fn read_next_stdio_message<R: BufRead>(
    reader: &mut R,
) -> io::Result<Option<(String, StdioWireFormat)>> {
    loop {
        let mut first_line = String::new();
        let bytes = reader.read_line(&mut first_line)?;
        if bytes == 0 {
            return Ok(None);
        }

        let trimmed = trim_line_endings(&first_line).trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return Ok(Some((
                trimmed.to_owned(),
                StdioWireFormat::LineDelimitedJson,
            )));
        }

        let mut content_length = parse_content_length_header(trimmed);
        loop {
            let mut header_line = String::new();
            let bytes = reader.read_line(&mut header_line)?;
            if bytes == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "unexpected EOF while reading stdio frame headers",
                ));
            }
            let header = trim_line_endings(&header_line).trim();
            if header.is_empty() {
                break;
            }
            if content_length.is_none() {
                content_length = parse_content_length_header(header);
            }
        }

        let length = content_length.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing Content-Length header in stdio frame: {trimmed}"),
            )
        })?;
        let mut payload = vec![0_u8; length];
        reader.read_exact(&mut payload)?;
        let payload = String::from_utf8(payload).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("stdio frame body is not valid UTF-8: {error}"),
            )
        })?;
        return Ok(Some((payload, StdioWireFormat::ContentLength)));
    }
}

fn write_stdio_payload<W: Write, T: Serialize>(
    writer: &mut W,
    payload: &T,
    format: StdioWireFormat,
) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    match format {
        StdioWireFormat::ContentLength => {
            write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
            writer.write_all(&body)?;
        }
        StdioWireFormat::LineDelimitedJson => {
            writer.write_all(&body)?;
            writer.write_all(b"\n")?;
        }
    }
    writer.flush()?;
    Ok(())
}

fn request_session_id(request: &JsonRpcRequest) -> Option<String> {
    let params = request.params.as_ref()?;
    params
        .get("_session_id")
        .and_then(|value| value.as_str())
        .or_else(|| {
            params
                .get("arguments")
                .and_then(|value| value.get("_session_id"))
                .and_then(|value| value.as_str())
        })
        .map(ToOwned::to_owned)
}

fn explicit_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(ToOwned::to_owned)
}

fn explicit_bool_field(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|field| field.as_bool())
}

fn explicit_string_array_field(value: &Value, key: &str) -> Option<Vec<String>> {
    value.get(key).and_then(|field| {
        field.as_array().map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(ToOwned::to_owned)
                .collect()
        })
    })
}

fn is_known_tier_label(value: &str) -> bool {
    matches!(value, "primitive" | "analysis" | "workflow")
}

fn explicit_stdio_metadata(request: &JsonRpcRequest) -> crate::state::LogicalSessionMetadataUpdate {
    let mut metadata = crate::state::LogicalSessionMetadataUpdate::default();
    let Some(params) = request.params.as_ref() else {
        return metadata;
    };
    let source = if request.method == "tools/call" {
        params.get("arguments").unwrap_or(params)
    } else {
        params
    };
    let initialize_client_name = if request.method == "initialize" {
        params
            .get("clientInfo")
            .and_then(|value| value.get("name"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    } else {
        None
    };
    let initialize_client_version = if request.method == "initialize" {
        params
            .get("clientInfo")
            .and_then(|value| value.get("version"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    } else {
        None
    };
    let initialize_requested_profile = if request.method == "initialize" {
        params
            .get("profile")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    } else {
        None
    };
    metadata.client_name =
        explicit_string_field(source, "_session_client_name").or_else(|| initialize_client_name.clone());
    metadata.client_version =
        explicit_string_field(source, "_session_client_version").or_else(|| initialize_client_version);
    metadata.requested_profile = explicit_string_field(source, "_session_requested_profile")
        .or_else(|| initialize_requested_profile);
    metadata.deferred_tool_loading = explicit_bool_field(source, "_session_deferred_tool_loading")
        .or_else(|| {
            if request.method == "initialize" {
                params
                    .get("deferredToolLoading")
                    .and_then(|value| value.as_bool())
                    .or_else(|| {
                        initialize_client_name
                            .as_deref()
                            .map(|name| crate::client_profile::ClientProfile::detect(Some(name)))
                            .and_then(|profile| profile.default_deferred_tool_loading())
                    })
            } else {
                None
            }
        });
    metadata.project_path = explicit_string_field(source, "_session_project_path");
    metadata.loaded_namespaces = explicit_string_array_field(source, "_session_loaded_namespaces");
    metadata.loaded_tiers = explicit_string_array_field(source, "_session_loaded_tiers");
    metadata.full_tool_exposure = explicit_bool_field(source, "_session_full_tool_exposure");
    metadata
}

fn initialize_stdio_session_defaults(state: &Arc<AppState>, session_id: &str) {
    let Some(runtime) = state.logical_session_snapshot(session_id) else {
        return;
    };
    if runtime.surface.is_some() || runtime.token_budget.is_some() {
        return;
    }

    let client = runtime
        .client_name
        .as_deref()
        .map(|name| crate::client_profile::ClientProfile::detect(Some(name)))
        .unwrap_or_else(|| state.client_profile());
    let (surface, budget) = if let Some(profile) = runtime
        .requested_profile
        .as_deref()
        .and_then(crate::tool_defs::ToolProfile::from_str)
    {
        (
            crate::tool_defs::ToolSurface::Profile(profile),
            crate::tool_defs::default_budget_for_profile(profile).max(client.default_budget()),
        )
    } else if matches!(client, crate::client_profile::ClientProfile::Codex) {
        let indexed_files = state
            .symbol_index()
            .stats()
            .map(|stats| stats.indexed_files)
            .unwrap_or(0);
        let (surface, budget, _label) = client.recommended_surface_and_budget(indexed_files);
        (surface, budget)
    } else {
        (*state.surface(), state.token_budget())
    };

    let session = crate::session_context::SessionRequestContext {
        session_id: session_id.to_owned(),
        ..Default::default()
    };
    state.set_execution_surface_and_budget(&session, surface, budget);

    if runtime.project_path.is_none() {
        state.upsert_logical_session_metadata(
            session_id,
            crate::state::LogicalSessionMetadataUpdate {
                project_path: Some(state.current_project_scope()),
                ..Default::default()
            },
        );
    }
}

fn stdio_session_fields(
    session_id: &str,
    session: &crate::state::LogicalSessionRuntime,
) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert("_session_id".to_owned(), json!(session_id));
    fields.insert(
        "_session_requested_profile".to_owned(),
        json!(session.requested_profile),
    );
    fields.insert(
        "_session_client_name".to_owned(),
        json!(session.client_name),
    );
    fields.insert(
        "_session_client_version".to_owned(),
        json!(session.client_version),
    );
    fields.insert(
        "_session_deferred_tool_loading".to_owned(),
        json!(session.deferred_tool_loading),
    );
    fields.insert(
        "_session_project_path".to_owned(),
        json!(session.project_path),
    );
    fields.insert(
        "_session_loaded_namespaces".to_owned(),
        json!(session.loaded_namespaces),
    );
    fields.insert(
        "_session_loaded_tiers".to_owned(),
        json!(session.loaded_tiers),
    );
    fields.insert(
        "_session_full_tool_exposure".to_owned(),
        json!(session.full_tool_exposure),
    );
    fields
}

fn inject_missing_stdio_session_fields(
    request: &mut JsonRpcRequest,
    session_id: &str,
    session: &crate::state::LogicalSessionRuntime,
) {
    let fields = stdio_session_fields(session_id, session);
    let Some(params) = request
        .params
        .get_or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        return;
    };
    if request.method == "tools/call" {
        let Some(arguments) = params
            .entry("arguments".to_owned())
            .or_insert_with(|| json!({}))
            .as_object_mut()
        else {
            return;
        };
        for (key, value) in fields {
            arguments.entry(key).or_insert(value);
        }
        return;
    }
    for (key, value) in fields {
        params.entry(key).or_insert(value);
    }
}

fn update_stdio_deferred_state(state: &Arc<AppState>, request: &JsonRpcRequest, session_id: &str) {
    let Some(params) = request.params.as_ref() else {
        return;
    };
    let session = crate::session_context::SessionRequestContext {
        session_id: session_id.to_owned(),
        ..Default::default()
    };
    let surface = state.execution_surface(&session);
    match request.method.as_str() {
        "tools/list" => {
            let full_listing = params.get("full").and_then(|value| value.as_bool()) == Some(true);
            if full_listing {
                state.enable_full_tool_exposure_for_session(&session);
            } else {
                if let Some(namespace) = params.get("namespace").and_then(|value| value.as_str())
                    && crate::tool_defs::visible_namespaces(surface).contains(&namespace)
                {
                    state.record_deferred_axes_for_session(&session, Some(namespace), None);
                }
                if let Some(tier) = params.get("tier").and_then(|value| value.as_str())
                    && is_known_tier_label(tier)
                {
                    state.record_deferred_axes_for_session(&session, None, Some(tier));
                }
            }
        }
        "resources/read" => {
            let uri = params
                .get("uri")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !matches!(uri, "codelens://tools/list" | "codelens://tools/list/full") {
                return;
            }
            let full_listing = uri == "codelens://tools/list/full"
                || params.get("full").and_then(|value| value.as_bool()) == Some(true);
            if full_listing {
                state.enable_full_tool_exposure_for_session(&session);
            } else {
                if let Some(namespace) = params.get("namespace").and_then(|value| value.as_str())
                    && crate::tool_defs::visible_namespaces(surface).contains(&namespace)
                {
                    state.record_deferred_axes_for_session(&session, Some(namespace), None);
                }
                if let Some(tier) = params.get("tier").and_then(|value| value.as_str())
                    && is_known_tier_label(tier)
                {
                    state.record_deferred_axes_for_session(&session, None, Some(tier));
                }
            }
        }
        _ => {}
    }
}

fn prepare_stdio_request(state: &Arc<AppState>, request: &mut JsonRpcRequest) {
    let Some(session_id) = request_session_id(request) else {
        return;
    };
    let metadata = explicit_stdio_metadata(request);
    state.upsert_logical_session_metadata(&session_id, metadata);
    if request.method == "initialize" {
        initialize_stdio_session_defaults(state, &session_id);
    }
    update_stdio_deferred_state(state, request, &session_id);
    if let Some(snapshot) = state.logical_session_snapshot(&session_id) {
        inject_missing_stdio_session_fields(request, &session_id, &snapshot);
    }
}

fn process_stdio_payload(state: &Arc<AppState>, payload: &str) -> Option<StdioResponsePayload> {
    let trimmed = payload.trim_start();
    if trimmed.starts_with('[') {
        return match serde_json::from_str::<Vec<JsonRpcRequest>>(trimmed) {
            Ok(requests) => {
                let mut responses = Vec::new();
                for mut request in requests {
                    prepare_stdio_request(state, &mut request);
                    if let Some(response) = handle_request(state, request) {
                        responses.push(response);
                    }
                }
                if responses.is_empty() {
                    None
                } else {
                    Some(StdioResponsePayload::Batch(responses))
                }
            }
            Err(error) => Some(StdioResponsePayload::Single(protocol_error_response(
                state,
                None,
                -32700,
                format!("Batch parse error: {error}"),
                None,
                None,
                "transport_stdio",
                "parse",
            ))),
        };
    }

    match serde_json::from_str::<JsonRpcRequest>(trimmed) {
        Ok(mut request) => {
            prepare_stdio_request(state, &mut request);
            handle_request(state, request).map(StdioResponsePayload::Single)
        }
        Err(error) => Some(StdioResponsePayload::Single(protocol_error_response(
            state,
            None,
            -32700,
            format!("Parse error: {error}"),
            None,
            None,
            "transport_stdio",
            "parse",
        ))),
    }
}

pub(crate) fn run_stdio(state: Arc<AppState>) -> Result<()> {
    state.metrics().record_transport_session("stdio");
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = io::stdout().lock();

    while let Some((payload, format)) = read_next_stdio_message(&mut stdin)? {
        if let Some(response) = process_stdio_payload(&state, &payload) {
            write_stdio_payload(&mut stdout, &response, format)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codelens_engine::ProjectRoot;
    use serde_json::json;
    use std::io::Cursor;

    fn test_state() -> Arc<AppState> {
        let dir = std::env::temp_dir().join(format!(
            "codelens-stdio-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.txt"), "world\n").unwrap();
        let project = ProjectRoot::new(dir.to_str().unwrap()).unwrap();
        Arc::new(AppState::new(
            project,
            crate::tool_defs::ToolPreset::Balanced,
        ))
    }

    fn round_trip_stdio(state: &Arc<AppState>, input: &[u8]) -> String {
        let mut cursor = Cursor::new(input);
        let (payload, format) = read_next_stdio_message(&mut cursor)
            .unwrap()
            .expect("message");
        let response = process_stdio_payload(state, &payload).expect("response");
        let mut out = Vec::new();
        write_stdio_payload(&mut out, &response, format).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn content_length_body(text: &str) -> &str {
        text.split("\r\n\r\n")
            .nth(1)
            .expect("content-length framed body")
    }

    fn response_value(text: &str) -> serde_json::Value {
        let body = if text.starts_with("Content-Length: ") {
            content_length_body(text)
        } else {
            text.trim_end()
        };
        serde_json::from_str(body).unwrap()
    }

    fn first_resource_text(text: &str) -> String {
        response_value(text)
            .get("result")
            .and_then(|result| result.get("contents"))
            .and_then(|contents| contents.as_array())
            .and_then(|contents| contents.first())
            .and_then(|content| content.get("text"))
            .and_then(|text| text.as_str())
            .map(ToOwned::to_owned)
            .unwrap_or_default()
    }

    fn first_tool_payload(text: &str) -> serde_json::Value {
        let value = response_value(text);
        let mut payload = value
            .get("result")
            .and_then(|result| result.get("content"))
            .and_then(|contents| contents.as_array())
            .and_then(|contents| contents.first())
            .and_then(|content| content.get("text"))
            .and_then(|text| text.as_str())
            .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
            .unwrap_or_default();

        if let Some(structured_content) = value
            .get("result")
            .and_then(|result| result.get("structuredContent"))
            .cloned()
        {
            if !payload.is_object() {
                payload = json!({});
            }
            payload
                .as_object_mut()
                .expect("payload object")
                .insert("data".to_owned(), structured_content);
        }

        payload
    }

    fn content_length_request(body: &str) -> Vec<u8> {
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
    }

    fn write_mock_lsp(project_root: &std::path::Path) -> std::path::PathBuf {
        let mock_lsp = concat!(
            "#!/usr/bin/env python3\n",
            "import sys, json\n",
            "def read_msg():\n",
            "    h = ''\n",
            "    while True:\n",
            "        c = sys.stdin.buffer.read(1)\n",
            "        if not c: return None\n",
            "        h += c.decode('ascii')\n",
            "        if h.endswith('\\r\\n\\r\\n'): break\n",
            "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
            "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
            "def send(r):\n",
            "    out = json.dumps(r)\n",
            "    b = out.encode('utf-8')\n",
            "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
            "    sys.stdout.buffer.write(b)\n",
            "    sys.stdout.buffer.flush()\n",
            "while True:\n",
            "    msg = read_msg()\n",
            "    if msg is None: break\n",
            "    rid = msg.get('id')\n",
            "    m = msg.get('method', '')\n",
            "    if m == 'initialized': continue\n",
            "    if rid is None: continue\n",
            "    if m == 'initialize':\n",
            "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
            "    elif m == 'textDocument/diagnostic':\n",
            "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[]}})\n",
            "    elif m == 'shutdown':\n",
            "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
            "    else:\n",
            "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        );
        let mock_path = project_root.join("mock_lsp.py");
        std::fs::write(&mock_path, mock_lsp).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mock_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        mock_path
    }

    #[test]
    fn reads_line_delimited_json_requests() {
        let mut cursor = Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n");
        let (payload, format) = read_next_stdio_message(&mut cursor)
            .unwrap()
            .expect("message");
        assert_eq!(format, StdioWireFormat::LineDelimitedJson);
        assert!(payload.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn reads_content_length_framed_requests() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut cursor = Cursor::new(framed.into_bytes());
        let (payload, format) = read_next_stdio_message(&mut cursor)
            .unwrap()
            .expect("message");
        assert_eq!(format, StdioWireFormat::ContentLength);
        assert_eq!(payload, body);
    }

    #[test]
    fn writes_content_length_framed_responses() {
        let mut out = Vec::new();
        write_stdio_payload(
            &mut out,
            &json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}}),
            StdioWireFormat::ContentLength,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("Content-Length: "));
        assert!(text.contains("\r\n\r\n{\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn writes_line_delimited_json_responses() {
        let mut out = Vec::new();
        write_stdio_payload(
            &mut out,
            &json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}}),
            StdioWireFormat::LineDelimitedJson,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.ends_with('\n'));
        assert!(text.starts_with("{\"jsonrpc\":\"2.0\""));
    }

    #[test]
    fn single_parse_error_exposes_transport_protocol_diagnostics() {
        let state = test_state();
        let response = process_stdio_payload(&state, "{invalid json").expect("response");
        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["error"]["code"], json!(-32700));
        assert_eq!(
            value["error"]["data"]["error_scope"],
            json!("transport_stdio")
        );
        assert_eq!(value["error"]["data"]["request_stage"], json!("parse"));
        assert_eq!(
            value["error"]["data"]["orchestration_contract"]["host_id"],
            json!("generic-mcp")
        );
    }

    #[test]
    fn batch_parse_error_exposes_transport_protocol_diagnostics() {
        let state = test_state();
        let response = process_stdio_payload(&state, "[{invalid json").expect("response");
        let value = serde_json::to_value(&response).unwrap();
        assert_eq!(value["error"]["code"], json!(-32700));
        assert_eq!(
            value["error"]["data"]["error_scope"],
            json!("transport_stdio")
        );
        assert_eq!(value["error"]["data"]["request_stage"], json!("parse"));
        assert_eq!(
            value["error"]["data"]["orchestration_contract"]["host_id"],
            json!("generic-mcp")
        );
    }

    #[test]
    fn line_delimited_dispatch_error_round_trips_request_aware_diagnostics() {
        let state = test_state();
        state.set_surface(crate::tool_defs::ToolSurface::Profile(
            crate::tool_defs::ToolProfile::ReviewerGraph,
        ));

        let output = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"set_profile","arguments":{"_session_client_name":"CodexHarness"}}}
"#,
        );

        assert!(output.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(output.trim_end()).unwrap();
        assert_eq!(value["error"]["code"], json!(-32602));
        assert_eq!(value["error"]["data"]["error_class"], json!("validation"));
        assert_eq!(
            value["error"]["data"]["tool_name"],
            json!("set_profile")
        );
        assert_eq!(value["error"]["data"]["request_stage"], json!("tool_arguments"));
        assert!(value["error"]["data"].get("orchestration_contract").is_none());
        assert!(value["error"]["data"].get("recommended_next_steps").is_none());
        assert!(value["error"]["data"].get("recovery_actions").is_none());
    }

    #[test]
    fn line_delimited_unknown_tool_error_stays_lean() {
        let state = test_state();

        let output = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"definitely_missing_tool","arguments":{"_session_client_name":"CodexHarness"}}}
"#,
        );

        assert!(output.ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(output.trim_end()).unwrap();
        assert_eq!(value["error"]["code"], json!(-32601));
        assert_eq!(value["error"]["data"]["error_class"], json!("validation"));
        assert_eq!(
            value["error"]["data"]["tool_name"],
            json!("definitely_missing_tool")
        );
        assert_eq!(value["error"]["data"]["request_stage"], json!("tool_selection"));
        assert!(value["error"]["data"].get("orchestration_contract").is_none());
        assert!(value["error"]["data"].get("recommended_next_steps").is_none());
        assert!(value["error"]["data"].get("recovery_actions").is_none());
    }

    #[test]
    fn content_length_batch_router_error_round_trips_request_aware_diagnostics() {
        let state = test_state();
        let body = r#"[{"jsonrpc":"1.0","id":11,"method":"initialize","params":{"clientInfo":{"name":"Claude Code","version":"1.0.0"},"profile":"reviewer-graph"}},{"jsonrpc":"2.0","id":12,"method":"initialize"}]"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        let output = round_trip_stdio(&state, framed.as_bytes());

        assert!(output.starts_with("Content-Length: "));
        let value: serde_json::Value = serde_json::from_str(content_length_body(&output)).unwrap();
        let responses = value.as_array().expect("batch response");
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["error"]["code"], json!(-32600));
        assert_eq!(
            responses[0]["error"]["data"]["error_scope"],
            json!("router")
        );
        assert_eq!(
            responses[0]["error"]["data"]["request_stage"],
            json!("jsonrpc_envelope")
        );
        assert_eq!(
            responses[0]["error"]["data"]["orchestration_contract"]["host_id"],
            json!("claude-code")
        );
        assert_eq!(
            responses[0]["error"]["data"]["orchestration_contract"]["active_surface"],
            json!("reviewer-graph")
        );
        assert_eq!(
            responses[1]["result"]["serverInfo"]["name"],
            json!("codelens-mcp")
        );
    }

    #[test]
    fn stdio_logical_session_persists_profile_across_messages() {
        let state = test_state();

        let set_profile = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"set_profile","arguments":{"profile":"reviewer-graph","_session_id":"stdio-profile"}}}
"#,
        );
        let set_payload = first_tool_payload(&set_profile);
        assert_eq!(set_payload["success"], json!(true));
        assert_eq!(
            set_payload["data"]["active_surface"],
            json!("reviewer-graph")
        );

        let listed = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":22,"method":"tools/list","params":{"_session_id":"stdio-profile"}}
"#,
        );
        let value = response_value(&listed);
        assert_eq!(value["result"]["active_surface"], json!("reviewer-graph"));
    }

    #[test]
    fn stdio_initialize_persists_requested_profile_and_deferred_loading() {
        let state = test_state();

        let init = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":24,"method":"initialize","params":{"_session_id":"stdio-init-profile","clientInfo":{"name":"Claude Code","version":"1.0.0"},"profile":"reviewer-graph","deferredToolLoading":true}}
"#,
        );
        let init_value = response_value(&init);
        assert_eq!(init_value["result"]["serverInfo"]["name"], json!("codelens-mcp"));

        let listed = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":25,"method":"tools/list","params":{"_session_id":"stdio-init-profile"}}
"#,
        );
        let value = response_value(&listed);
        assert_eq!(value["result"]["client_profile"], json!("claude"));
        assert_eq!(value["result"]["active_surface"], json!("reviewer-graph"));
        assert_eq!(value["result"]["deferred_loading_active"], json!(true));
        assert_eq!(value["result"]["loaded_namespaces"], json!([]));
        assert_eq!(value["result"]["loaded_tiers"], json!([]));
    }

    #[test]
    fn stdio_initialize_applies_codex_defaults_for_followup_tools_list() {
        let state = test_state();

        let init = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":26,"method":"initialize","params":{"_session_id":"stdio-init-codex","clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}
"#,
        );
        let init_value = response_value(&init);
        assert_eq!(init_value["result"]["serverInfo"]["name"], json!("codelens-mcp"));

        let listed = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":27,"method":"tools/list","params":{"_session_id":"stdio-init-codex"}}
"#,
        );
        let value = response_value(&listed);
        assert_eq!(value["result"]["client_profile"], json!("codex"));
        assert_eq!(value["result"]["active_surface"], json!("workflow-first"));
        assert_eq!(value["result"]["deferred_loading_active"], json!(true));
        assert_eq!(value["result"]["default_contract_mode"], json!("lean"));
    }

    #[test]
    fn stdio_codex_prepare_harness_session_bootstraps_without_tools_list() {
        let state = test_state();

        let bootstrap = round_trip_stdio(
            &state,
            format!(
                r#"{{"jsonrpc":"2.0","id":30,"method":"tools/call","params":{{"name":"prepare_harness_session","arguments":{{"project":"{}","_session_client_name":"CodexHarness","preferred_entrypoints":["explore_codebase","plan_safe_refactor"]}}}}}}"#,
                state.project().as_path().display()
            )
            .as_bytes(),
        );

        let payload = first_tool_payload(&bootstrap);
        assert_eq!(payload["success"], json!(true));
        assert_eq!(payload["data"]["project"]["auto_surface"], json!("workflow-first"));
        assert_eq!(payload["data"]["active_surface"], json!("workflow-first"));
        assert_eq!(payload["data"]["token_budget"], json!(6000));
        assert_eq!(
            payload["data"]["routing"]["recommended_entrypoint"],
            json!("explore_codebase")
        );
        assert_eq!(payload["orchestration_contract"]["host_id"], json!("codex"));
        assert!(
            payload["suggested_next_tools"]
                .as_array()
                .map(|items| items.iter().any(|item| item == "explore_codebase"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn stdio_codex_surface_blocked_tool_call_returns_bootstrap_recovery_contract() {
        let state = test_state();

        let init = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":30,"method":"initialize","params":{"_session_id":"stdio-codex-blocked","clientInfo":{"name":"CodexHarness","version":"1.0.0"},"profile":"reviewer-graph"}}
"#,
        );
        let init_value = response_value(&init);
        assert_eq!(init_value["result"]["serverInfo"]["name"], json!("codelens-mcp"));

        let blocked = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":31,"method":"tools/call","params":{"name":"read_memory","arguments":{"memory_name":"missing-memory","_session_id":"stdio-codex-blocked"}}}
"#,
        );
        let blocked_payload = first_tool_payload(&blocked);
        assert_eq!(blocked_payload["success"], json!(false));
        assert!(blocked.contains("not available in active surface"));
        assert_eq!(
            blocked_payload["orchestration_contract"]["host_id"],
            json!("codex")
        );
        assert_eq!(
            blocked_payload["orchestration_contract"]["active_surface"],
            json!("reviewer-graph")
        );
        assert!(
            blocked_payload["recovery_actions"]
                .as_array()
                .map(|items| items.iter().any(|item| {
                    item["kind"] == json!("tool_call")
                        && item["target"] == json!("prepare_harness_session")
                }))
                .unwrap_or(false)
        );
        assert!(
            blocked_payload["recovery_actions"]
                .as_array()
                .map(|items| items.iter().any(|item| {
                    item["kind"] == json!("rpc_call")
                        && item["target"] == json!("tools/list")
                        && item["arguments"]["full"] == json!(true)
                }))
                .unwrap_or(false)
        );
        assert!(
            blocked_payload["recommended_next_steps"]
                .as_array()
                .map(|items| items
                    .iter()
                    .any(|item| item["target"] == json!("host_orchestrator")))
                .unwrap_or(false)
        );
    }

    #[test]
    fn stdio_claude_surface_blocked_tool_call_returns_bootstrap_recovery_contract() {
        let state = test_state();

        let init = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":32,"method":"initialize","params":{"_session_id":"stdio-claude-blocked","clientInfo":{"name":"Claude Code","version":"1.0.0"},"profile":"reviewer-graph"}}
"#,
        );
        let init_value = response_value(&init);
        assert_eq!(init_value["result"]["serverInfo"]["name"], json!("codelens-mcp"));

        let blocked = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":33,"method":"tools/call","params":{"name":"read_memory","arguments":{"memory_name":"missing-memory","_session_id":"stdio-claude-blocked"}}}
"#,
        );
        let blocked_payload = first_tool_payload(&blocked);
        assert_eq!(blocked_payload["success"], json!(false));
        assert!(blocked.contains("not available in active surface"));
        assert_eq!(
            blocked_payload["orchestration_contract"]["host_id"],
            json!("claude-code")
        );
        assert_eq!(
            blocked_payload["orchestration_contract"]["active_surface"],
            json!("reviewer-graph")
        );
        assert!(
            blocked_payload["recovery_actions"]
                .as_array()
                .map(|items| items.iter().any(|item| {
                    item["kind"] == json!("tool_call")
                        && item["target"] == json!("prepare_harness_session")
                }))
                .unwrap_or(false)
        );
        assert!(
            blocked_payload["recovery_actions"]
                .as_array()
                .map(|items| items.iter().any(|item| {
                    item["kind"] == json!("rpc_call")
                        && item["target"] == json!("tools/list")
                        && item["arguments"]["full"] == json!(true)
                }))
                .unwrap_or(false)
        );
        assert!(
            blocked_payload["recommended_next_steps"]
                .as_array()
                .map(|items| items
                    .iter()
                    .any(|item| item["target"] == json!("host_orchestrator")))
                .unwrap_or(false)
        );
    }

    #[test]
    fn stdio_logical_session_persists_namespace_expansion_and_allows_hidden_tool_calls() {
        let state = test_state();
        state.set_surface(crate::tool_defs::ToolSurface::Profile(
            crate::tool_defs::ToolProfile::ReviewerGraph,
        ));
        let file_path = state.project().as_path().join("stdio-deferred-hidden.py");
        std::fs::write(&file_path, "def gamma():\n    return 3\n").unwrap();
        let mock_path = write_mock_lsp(state.project().as_path());

        let bootstrap = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":31,"method":"tools/list","params":{"_session_id":"stdio-deferred","_session_client_name":"Claude Code","_session_deferred_tool_loading":true}}
"#,
        );
        let bootstrap_value = response_value(&bootstrap);
        assert_eq!(bootstrap_value["result"]["client_profile"], json!("claude"));
        assert_eq!(
            bootstrap_value["result"]["deferred_loading_active"],
            json!(true)
        );
        assert!(!bootstrap.contains("\"get_file_diagnostics\""));

        let blocked = round_trip_stdio(
            &state,
            format!(
                r#"{{"jsonrpc":"2.0","id":32,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}","_session_id":"stdio-deferred"}}}}}}
"#,
                file_path.display()
            )
            .as_bytes(),
        );
        let blocked_payload = first_tool_payload(&blocked);
        assert_eq!(blocked_payload["success"], json!(false));
        assert!(
            blocked_payload["error"]
                .as_str()
                .unwrap_or("")
                .contains("hidden by deferred loading")
        );
        assert_eq!(
            blocked_payload["orchestration_contract"]["host_id"],
            json!("claude-code")
        );
        assert!(
            blocked_payload["recovery_actions"]
                .as_array()
                .map(|items| {
                    items.iter().any(|item| {
                        item["kind"] == json!("rpc_call")
                            && item["target"] == json!("tools/list")
                            && item["arguments"]["namespace"] == json!("lsp")
                    })
                })
                .unwrap_or(false)
        );

        let tier_expanded = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":33,"method":"tools/list","params":{"_session_id":"stdio-deferred","tier":"primitive"}}
"#,
        );
        let tier_value = response_value(&tier_expanded);
        assert_eq!(tier_value["result"]["selected_tier"], json!("primitive"));
        assert_eq!(tier_value["result"]["loaded_tiers"], json!(["primitive"]));
        assert!(tier_expanded.contains("\"find_symbol\""));

        let expanded = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":34,"method":"tools/list","params":{"_session_id":"stdio-deferred","namespace":"lsp"}}
"#,
        );
        let expanded_value = response_value(&expanded);
        assert_eq!(expanded_value["result"]["selected_namespace"], json!("lsp"));
        assert_eq!(
            expanded_value["result"]["loaded_namespaces"],
            json!(["lsp"])
        );
        assert!(expanded.contains("\"get_file_diagnostics\""));

        let allowed_body = format!(
            r#"{{"jsonrpc":"2.0","id":35,"method":"tools/call","params":{{"name":"get_file_diagnostics","arguments":{{"file_path":"{}","command":"python3","args":["{}"],"_session_id":"stdio-deferred"}}}}}}"#,
            file_path.display(),
            mock_path.display()
        );
        let allowed = round_trip_stdio(&state, &content_length_request(&allowed_body));
        let allowed_payload = first_tool_payload(&allowed);
        assert_eq!(
            allowed_payload["success"],
            json!(true),
            "allowed body: {allowed}"
        );

        let summary = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":36,"method":"tools/list","params":{"_session_id":"stdio-deferred"}}
"#,
        );
        let summary_value = response_value(&summary);
        assert_eq!(summary_value["result"]["loaded_namespaces"], json!(["lsp"]));
        assert_eq!(
            summary_value["result"]["loaded_tiers"],
            json!(["primitive"])
        );
        assert!(summary.contains("\"get_file_diagnostics\""));
    }

    #[test]
    fn stdio_full_tools_list_persists_full_tool_exposure_for_followup_requests() {
        let state = test_state();

        let init = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":40,"method":"initialize","params":{"_session_id":"stdio-full-codex","clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}
"#,
        );
        let init_value = response_value(&init);
        assert_eq!(init_value["result"]["serverInfo"]["name"], json!("codelens-mcp"));

        let initial = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":41,"method":"tools/list","params":{"_session_id":"stdio-full-codex"}}
"#,
        );
        let initial_value = response_value(&initial);
        assert_eq!(initial_value["result"]["client_profile"], json!("codex"));
        assert_eq!(initial_value["result"]["default_contract_mode"], json!("lean"));
        assert_eq!(initial_value["result"]["deferred_loading_active"], json!(true));
        assert!(initial_value["result"].get("full_tool_exposure").is_none());

        let full = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":42,"method":"tools/list","params":{"_session_id":"stdio-full-codex","full":true}}
"#,
        );
        let full_value = response_value(&full);
        assert_eq!(full_value["result"]["client_profile"], json!("codex"));
        assert_eq!(full_value["result"]["default_contract_mode"], json!("lean"));
        assert_eq!(full_value["result"]["deferred_loading_active"], json!(false));
        assert_eq!(full_value["result"]["full_tool_exposure"], json!(true));
        assert!(full.contains("\"description\""));

        let summary = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":43,"method":"tools/list","params":{"_session_id":"stdio-full-codex"}}
"#,
        );
        let summary_value = response_value(&summary);
        assert_eq!(summary_value["result"]["client_profile"], json!("codex"));
        assert_eq!(summary_value["result"]["default_contract_mode"], json!("lean"));
        assert_eq!(summary_value["result"]["deferred_loading_active"], json!(false));
        assert_eq!(summary_value["result"]["full_tool_exposure"], json!(true));
        assert!(summary.contains("\"include_output_schema\":true"));
        assert!(summary.contains("\"include_annotations\":true"));
        assert!(summary.contains("\"outputSchema\""));
        assert!(summary.contains("\"annotations\""));
    }

    #[test]
    fn stdio_full_tools_list_resource_updates_session_resource_state() {
        let state = test_state();

        let init = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":44,"method":"initialize","params":{"_session_id":"stdio-full-resource","clientInfo":{"name":"CodexHarness","version":"1.0.0"}}}
"#,
        );
        let init_value = response_value(&init);
        assert_eq!(init_value["result"]["serverInfo"]["name"], json!("codelens-mcp"));

        let full_resource = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":45,"method":"resources/read","params":{"uri":"codelens://tools/list/full","_session_id":"stdio-full-resource"}}
"#,
        );
        let full_text = first_resource_text(&full_resource);
        assert!(full_text.contains("\"client_profile\": \"codex\""));
        assert!(full_text.contains("\"default_contract_mode\": \"lean\""));
        assert!(full_text.contains("\"full_tool_exposure\": true"));
        assert!(full_text.contains("\"deferred_loading_active\": false"));
        assert!(full_text.contains("\"description\""));

        let session_resource = round_trip_stdio(
            &state,
            br#"{"jsonrpc":"2.0","id":46,"method":"resources/read","params":{"uri":"codelens://session/http","_session_id":"stdio-full-resource"}}
"#,
        );
        let session_text = first_resource_text(&session_resource);
        assert!(session_text.contains("\"client_profile\": \"codex\""));
        assert!(session_text.contains("\"default_tools_list_contract_mode\": \"lean\""));
        assert!(session_text.contains("\"full_tool_exposure\": true"));
        assert!(session_text.contains("\"health_summary\":"));
    }
}
