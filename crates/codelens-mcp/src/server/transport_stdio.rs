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

    for line in stdin.lock().lines() {
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
                        .filter_map(|req| handle_request(&state, req))
                        .collect();
                    if !responses.is_empty() {
                        serde_json::to_writer(&mut stdout, &responses)?;
                        stdout.write_all(b"\n")?;
                        stdout.flush()?;
                    }
                }
                Err(error) => {
                    let resp =
                        JsonRpcResponse::error(None, -32700, format!("Batch parse error: {error}"));
                    serde_json::to_writer(&mut stdout, &resp)?;
                    stdout.write_all(b"\n")?;
                    stdout.flush()?;
                }
            }
        } else {
            // Single request
            let response = match serde_json::from_str::<JsonRpcRequest>(trimmed) {
                Ok(request) => handle_request(&state, request),
                Err(error) => Some(JsonRpcResponse::error(
                    None,
                    -32700,
                    format!("Parse error: {error}"),
                )),
            };
            if let Some(response) = response {
                serde_json::to_writer(&mut stdout, &response)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}
