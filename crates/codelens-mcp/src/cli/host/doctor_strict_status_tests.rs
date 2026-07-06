use super::{doctor_report_json, strict_failures_from_json};
use anyhow::Result;
use serde_json::Value;
#[cfg(feature = "http")]
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "http")]
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("codelens-doctor-strict-{name}-{nanos}"))
}

fn coverage_for_host<'a>(payload: &'a Value, host: &str) -> Result<&'a Value> {
    payload["hosts"]
        .as_array()
        .and_then(|hosts| {
            hosts
                .iter()
                .find(|report| report["host"].as_str() == Some(host))
        })
        .map(|report| &report["semantic_coverage"])
        .ok_or_else(|| anyhow::anyhow!("missing semantic coverage for {host}"))
}

fn write_cursor_stdio_config(project: &Path) -> Result<()> {
    let config_dir = project.join(".cursor");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("mcp.json"),
        r#"{"mcpServers":{"codelens":{"command":"codelens-mcp","args":["."]}}}"#,
    )?;
    Ok(())
}

#[cfg(feature = "http")]
fn write_claude_http_config(project: &Path, url: &str) -> Result<()> {
    fs::write(
        project.join(".mcp.json"),
        format!(r#"{{"mcpServers":{{"codelens":{{"url":"{url}"}}}}}}"#),
    )?;
    Ok(())
}

#[test]
fn strict_status_json_reports_stdio_only_cursor_config() -> Result<()> {
    let root = temp_root("cursor-stdio");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_cursor_stdio_config(&project)?;

    let payload = doctor_report_json("status", &["cursor"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "cursor")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "stdio_attach");
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("cursor"));
    assert!(failures[0].contains("stdio attach"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_stale_http_coverage() -> Result<()> {
    let root = temp_root("stale-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    let (url, server) = spawn_embedding_coverage_server(stale_report())?;
    write_claude_http_config(&project, &url)?;

    let payload = doctor_report_json("status", &["claude-code"], &home, &project, true)?;
    let server_result = server
        .join()
        .map_err(|_| anyhow::anyhow!("coverage responder thread panicked"))?;
    server_result?;
    let coverage = coverage_for_host(&payload, "claude-code")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "stale");
    assert_eq!(
        coverage["remediation"],
        "Run index_embeddings for this project, then rerun doctor/status --strict."
    );
    assert!(
        coverage["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("stale_files=3"))
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("claude-code"));
    assert!(failures[0].contains("Run index_embeddings"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_missing_model_http_coverage() -> Result<()> {
    let root = temp_root("missing-model-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    let (url, server) = spawn_embedding_coverage_server(missing_model_report())?;
    write_claude_http_config(&project, &url)?;

    let payload = doctor_report_json("status", &["claude-code"], &home, &project, true)?;
    let server_result = server
        .join()
        .map_err(|_| anyhow::anyhow!("coverage responder thread panicked"))?;
    server_result?;
    let coverage = coverage_for_host(&payload, "claude-code")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "model_assets_unavailable");
    assert_eq!(
        coverage["remediation"],
        "Install CodeLens embedding model assets or set CODELENS_MODEL_DIR, then rerun doctor/status --strict."
    );
    assert!(
        coverage["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("model_assets.available=false"))
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("claude-code"));
    assert!(failures[0].contains("CODELENS_MODEL_DIR"));
    assert!(!failures[0].contains("Run index_embeddings"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
fn stale_report() -> Value {
    json!({
        "status": "stale",
        "compiled": true,
        "model_assets": {"available": true},
        "index": {
            "indexed_symbols": 42,
            "readiness_percent": 80,
            "stale_files": 3,
            "stale_file_reasons": [
                {"file_path": "src/main.rs", "reason": "embedding_keys_changed"}
            ],
            "model_mismatch": false,
            "last_index_sha": "abc123"
        },
        "query_cache": {"entries": 7},
        "remediation": {"action": "refresh_embedding_index"}
    })
}

#[cfg(feature = "http")]
fn missing_model_report() -> Value {
    json!({
        "status": "model_assets_unavailable",
        "compiled": true,
        "model_assets": {"available": false},
        "index": {
            "indexed_symbols": 0,
            "readiness_percent": 0,
            "stale_files": 0,
            "stale_file_reasons": [],
            "model_mismatch": false,
            "last_index_sha": null
        },
        "query_cache": {"entries": 0},
        "remediation": {"action": "install_model_assets"}
    })
}

#[cfg(feature = "http")]
fn spawn_embedding_coverage_server(
    report: Value,
) -> Result<(String, thread::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let url = format!("http://{}/mcp", listener.local_addr()?);
    let handle = thread::spawn(move || -> Result<()> {
        let responses = json_rpc_responses(report);
        for response in responses {
            let start = Instant::now();
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(pair) => break pair,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        if start.elapsed() > Duration::from_secs(5) {
                            anyhow::bail!("timed out waiting for coverage probe request");
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => return Err(err.into()),
                }
            };
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request)?;
            stream.write_all(response.as_bytes())?;
        }
        Ok(())
    });
    Ok((url, handle))
}

#[cfg(feature = "http")]
fn json_rpc_responses(report: Value) -> Vec<String> {
    let initialize = json!({"jsonrpc":"2.0","id":9001,"result":{}});
    let list_tools = json!({"jsonrpc":"2.0","id":9002,"result":{"tools":[]}});
    let tool_payload = json!({"success":true,"data":report});
    let tool_call = json!({
        "jsonrpc": "2.0",
        "id": 9003,
        "result": {
            "content": [{"type": "text", "text": tool_payload.to_string()}]
        }
    });
    vec![
        http_json_response(&initialize, Some("doctor-test-session")),
        http_json_response(&list_tools, None),
        http_json_response(&tool_call, None),
    ]
}

#[cfg(feature = "http")]
fn http_json_response(body: &Value, session_id: Option<&str>) -> String {
    let text = body.to_string();
    let session_header = session_id
        .map(|id| format!("mcp-session-id: {id}\r\n"))
        .unwrap_or_default();
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n{session_header}content-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len()
    )
}
