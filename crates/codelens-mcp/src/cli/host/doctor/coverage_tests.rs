use super::transport::{Transport, parse_json_transport, parse_toml_transport};
use super::*;
use anyhow::Result;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("codelens-doctor-{name}-{nanos}"))
}

#[test]
fn strict_semantic_coverage_flag_is_explicit() {
    assert!(strict_semantic_coverage_enabled(&[
        "codelens-mcp".to_owned(),
        "status".to_owned(),
        "--strict".to_owned(),
        "codex".to_owned(),
    ]));
    assert!(!strict_semantic_coverage_enabled(&[
        "codelens-mcp".to_owned(),
        "status".to_owned(),
        "codex".to_owned(),
    ]));
}

#[test]
fn parse_json_transport_reads_http_headers() -> Result<()> {
    let transport = parse_json_transport(
        r#"{
          "mcpServers": {
            "codelens": {
              "url": "http://127.0.0.1:7839/mcp",
              "headers": {"x-codelens-project": "/tmp/repo"}
            }
          }
        }"#,
    )
    .map_err(anyhow::Error::msg)?;

    assert_eq!(
        transport,
        Transport::Http {
            url: "http://127.0.0.1:7839/mcp".to_owned(),
            headers: BTreeMap::from([("x-codelens-project".to_owned(), "/tmp/repo".to_owned())]),
        }
    );
    Ok(())
}

#[test]
fn parse_toml_transport_reads_http_headers() -> Result<()> {
    let transport = parse_toml_transport(
        r#"
          [mcp_servers.codelens]
          url = "http://127.0.0.1:7839/mcp"
          http_headers = { "x-codelens-project" = "/tmp/repo" }
        "#,
    )
    .map_err(anyhow::Error::msg)?;

    assert_eq!(
        transport,
        Transport::Http {
            url: "http://127.0.0.1:7839/mcp".to_owned(),
            headers: BTreeMap::from([("x-codelens-project".to_owned(), "/tmp/repo".to_owned())]),
        }
    );
    Ok(())
}

#[test]
fn coverage_for_stdio_attach_is_skipped() -> Result<()> {
    let path = temp_path("stdio.json");
    fs::write(
        &path,
        r#"{"mcpServers":{"codelens":{"command":"codelens-mcp","args":[]}}}"#,
    )?;
    let report = coverage_for_inspected_files(
        &[json!({
            "path": path.display().to_string(),
            "format": "json",
            "status": "attached_customized",
        })],
        Path::new("/tmp/repo"),
    );
    let _ = fs::remove_file(&path);

    assert_eq!(report.status, "stdio_attach");
    assert!(!report.checked);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn coverage_for_unreachable_http_attach_points_at_daemon_recovery() -> Result<()> {
    let path = temp_path("unreachable-http.json");
    fs::write(
        &path,
        r#"{"mcpServers":{"codelens":{"url":"http://127.0.0.1:1/mcp"}}}"#,
    )?;
    let report = coverage_for_inspected_files(
        &[json!({
            "path": path.display().to_string(),
            "format": "json",
            "status": "attached_customized",
        })],
        Path::new("/tmp/repo"),
    );
    let _ = fs::remove_file(&path);

    assert_eq!(report.status, "unreachable");
    assert!(report.checked);
    assert!(!report.ok);
    let issue = report
        .strict_exit_issue()
        .ok_or_else(|| anyhow::anyhow!("strict issue"))?;
    assert!(issue.contains("HTTP JSON-RPC request failed"));
    assert!(issue.contains("Start the CodeLens HTTP daemon"));
    Ok(())
}

#[test]
fn strict_exit_issue_allows_unconfigured_hosts_only() -> Result<()> {
    let unconfigured = SemanticCoverage::skipped(
        "not_configured",
        "no attached machine-readable CodeLens config",
    );
    let stdio = SemanticCoverage::skipped(
        "stdio_attach",
        "stdio attach `codelens-mcp`; semantic coverage is only probed for HTTP daemons",
    );

    assert!(unconfigured.strict_exit_issue().is_none());
    let issue = stdio
        .strict_exit_issue()
        .ok_or_else(|| anyhow::anyhow!("strict issue"))?;
    assert!(issue.contains("stdio attach"));
    Ok(())
}

#[test]
fn semantic_coverage_compacts_ready_report() {
    let report = SemanticCoverage::from_report(json!({
        "status": "ready",
        "compiled": true,
        "model_assets": {"available": true},
        "index": {
            "indexed_symbols": 42,
            "readiness_percent": 100,
            "stale_files": 0,
            "stale_file_reasons": [],
            "model_mismatch": false,
            "last_index_sha": "abc123"
        },
        "query_cache": {"entries": 7},
        "remediation": {"action": "none"}
    }));

    assert!(report.ok);
    assert!(report.render_text().contains("status=ready"));
    assert!(report.render_text().contains("indexed_symbols=42"));
    assert!(report.render_text().contains("readiness_percent=100%"));
    assert!(report.render_text().contains("stale_reason=none"));
    assert!(report.render_text().contains("remediation.action=none"));
}

#[test]
fn semantic_coverage_compacts_stale_report_with_index_recovery() -> Result<()> {
    let report = SemanticCoverage::from_report(json!({
        "status": "stale",
        "compiled": true,
        "model_assets": {"available": true},
        "index": {
            "indexed_symbols": 42,
            "readiness_percent": 80,
            "stale_files": 3,
            "stale_file_reasons": [
                {
                    "file_path": "src/main.rs",
                    "reason": "embedding_keys_changed"
                }
            ],
            "model_mismatch": false,
            "last_index_sha": "abc123"
        },
        "query_cache": {"entries": 7},
        "remediation": {"action": "refresh_embedding_index"}
    }));

    assert!(!report.ok);
    assert_eq!(report.status, "stale");
    let text = report.render_text();
    assert!(text.contains("stale_files=3"));
    assert!(text.contains("stale_reason=src/main.rs:embedding_keys_changed"));
    assert!(text.contains("remediation.action=refresh_embedding_index"));
    assert!(text.contains("Run index_embeddings"));
    let issue = report
        .strict_exit_issue()
        .ok_or_else(|| anyhow::anyhow!("strict issue"))?;
    assert!(issue.contains("status=stale"));
    assert!(issue.contains("Run index_embeddings"));
    Ok(())
}

#[test]
fn semantic_coverage_compacts_missing_model_report_with_asset_recovery() -> Result<()> {
    let report = SemanticCoverage::from_report(json!({
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
    }));

    assert!(!report.ok);
    assert_eq!(report.status, "model_assets_unavailable");
    let text = report.render_text();
    assert!(text.contains("model_assets.available=false"));
    assert!(text.contains("remediation.action=install_model_assets"));
    assert!(text.contains("CODELENS_MODEL_DIR"));
    assert!(!text.contains("Run index_embeddings"));
    let issue = report
        .strict_exit_issue()
        .ok_or_else(|| anyhow::anyhow!("strict issue"))?;
    assert!(issue.contains("model_assets_unavailable"));
    assert!(issue.contains("embedding model assets"));
    Ok(())
}
