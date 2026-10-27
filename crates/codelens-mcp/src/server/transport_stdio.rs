use super::router::handle_request;
use crate::AppState;
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use anyhow::Result;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

pub(crate) fn run_stdio(state: Arc<AppState>) -> Result<()> {
    state.metrics().record_transport_session("stdio");
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    serve_stdio(&state, stdin.lock(), &mut stdout)
}

/// The stdio JSON-RPC loop over any line reader and writer: one request (or
/// batch) per line, one response line per answered request.
pub(crate) fn serve_stdio(
    state: &AppState,
    input: impl BufRead,
    stdout: &mut impl Write,
) -> Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        // JSON-RPC 2.0 batch support: detect array vs object
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            // Batch request
            match serde_json::from_str::<Vec<JsonRpcRequest>>(trimmed) {
                Ok(requests) => {
                    let responses: Vec<_> = requests
                        .into_iter()
                        .filter_map(|mut req| {
                            strip_client_session_metadata(&mut req);
                            handle_request(state, req)
                        })
                        .collect();
                    if !responses.is_empty() {
                        serde_json::to_writer(&mut *stdout, &responses)?;
                        stdout.write_all(b"\n")?;
                        stdout.flush()?;
                    }
                }
                Err(error) => {
                    let resp =
                        JsonRpcResponse::error(None, -32700, format!("Batch parse error: {error}"));
                    serde_json::to_writer(&mut *stdout, &resp)?;
                    stdout.write_all(b"\n")?;
                    stdout.flush()?;
                }
            }
        } else {
            // Single request
            let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(mut request) => {
                    strip_client_session_metadata(&mut request);
                    handle_request(state, request)
                }
                Err(error) => Some(JsonRpcResponse::error(
                    None,
                    -32700,
                    format!("Parse error: {error}"),
                )),
            };
            if let Some(response) = response {
                serde_json::to_writer(&mut *stdout, &response)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}

/// Drop client-supplied `_session_*` root keys before dispatch, as the HTTP
/// transport does before injecting its own block. `_session_*` is
/// server-owned metadata: dispatch binds `_session_project_path` (creating
/// `.codelens` there) before the tool runs, and no stdio client sends these
/// keys. On stdio the session is the process; an analysis-lite call binds
/// from its `target.file` alone.
fn strip_client_session_metadata(request: &mut JsonRpcRequest) {
    let metadata = match request.method.as_str() {
        "tools/call" => request
            .params
            .as_mut()
            .and_then(|params| params.get_mut("arguments")),
        "tools/list" | "resources/read" => request.params.as_mut(),
        _ => None,
    };
    if let Some(metadata) = metadata.and_then(|value| value.as_object_mut()) {
        metadata.retain(|key, _| !key.starts_with("_session_"));
    }
}
