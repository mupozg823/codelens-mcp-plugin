use super::{doctor_report_json, ensure_strict_semantic_coverage, strict_failures_from_json};
use anyhow::Result;
use serde_json::{Value, json};
use std::fs;
#[cfg(feature = "http")]
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("codelens-doctor-status-{name}-{nanos}"))
}

#[cfg(feature = "http")]
fn write_codex_config(home: &Path, url: &str) -> io::Result<()> {
    let config_dir = home.join(".codex");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("config.toml"),
        format!(
            r#"[mcp_servers.codelens]
url = "{url}"
"#
        ),
    )
}

#[cfg(feature = "http")]
fn write_claude_project_config(project: &Path, url: &str) -> io::Result<()> {
    fs::write(
        project.join(".mcp.json"),
        format!(r#"{{"mcpServers":{{"codelens":{{"url":"{url}"}}}}}}"#),
    )
}

#[cfg(feature = "http")]
fn write_cursor_project_config(project: &Path, url: &str) -> io::Result<()> {
    let config_dir = project.join(".cursor");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("mcp.json"),
        format!(r#"{{"mcpServers":{{"codelens":{{"url":"{url}"}}}}}}"#),
    )
}

fn write_malformed_cursor_project_config(project: &Path) -> std::io::Result<()> {
    let config_dir = project.join(".cursor");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("mcp.json"),
        r#"{"mcpServers":{"codelens":{}}}"#,
    )
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

#[test]
fn strict_status_json_reports_malformed_cursor_config() -> Result<()> {
    let root = temp_root("malformed-cursor");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_malformed_cursor_project_config(&project)?;

    let payload = doctor_report_json("status", &["cursor"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "cursor")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "invalid_config");
    assert!(
        coverage["detail"]
            .as_str()
            .is_some_and(|detail| detail.contains("missing both url and command"))
    );
    assert_eq!(
        coverage["remediation"],
        "Repair host config, then rerun doctor/status --strict."
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("cursor"));
    assert!(failures[0].contains("Repair host config"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn strict_status_json_allows_unconfigured_codex_host() -> Result<()> {
    let root = temp_root("unconfigured");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;

    let payload = doctor_report_json("status", &["codex"], &home, &project, true)?;
    let coverage = &payload["hosts"][0]["semantic_coverage"];

    assert_eq!(coverage["status"], "not_configured");
    assert!(strict_failures_from_json(&payload).is_empty());
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn strict_status_json_allows_unconfigured_non_codex_hosts() -> Result<()> {
    let root = temp_root("unconfigured-non-codex");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;

    let payload = doctor_report_json("status", &["claude-code", "cursor"], &home, &project, true)?;

    assert_eq!(
        coverage_for_host(&payload, "claude-code")?["status"],
        "not_configured"
    );
    assert_eq!(
        coverage_for_host(&payload, "cursor")?["status"],
        "not_configured"
    );
    assert!(strict_failures_from_json(&payload).is_empty());
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_unreachable_codex_http_config() -> Result<()> {
    let root = temp_root("unreachable-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_codex_config(&home, "http://127.0.0.1:1/mcp")?;

    let payload = doctor_report_json("status", &["codex"], &home, &project, true)?;
    let coverage = &payload["hosts"][0]["semantic_coverage"];
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "unreachable");
    assert_eq!(
        coverage["remediation"],
        "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict."
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("codex"));
    assert!(failures[0].contains("Start the CodeLens HTTP daemon"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_unreachable_claude_http_config() -> Result<()> {
    let root = temp_root("unreachable-claude-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_claude_project_config(&project, "http://127.0.0.1:1/mcp")?;

    let payload = doctor_report_json("status", &["claude-code"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "claude-code")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "unreachable");
    assert_eq!(
        coverage["remediation"],
        "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict."
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("claude-code"));
    assert!(failures[0].contains("Start the CodeLens HTTP daemon"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_unreachable_cursor_http_config() -> Result<()> {
    let root = temp_root("unreachable-cursor-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_cursor_project_config(&project, "http://127.0.0.1:1/mcp")?;

    let payload = doctor_report_json("status", &["cursor"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "cursor")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "unreachable");
    assert_eq!(
        coverage["remediation"],
        "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict."
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("cursor"));
    assert!(failures[0].contains("Start the CodeLens HTTP daemon"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn strict_json_failures_ignore_unconfigured_hosts() {
    let payload = json!({
        "hosts": [
            {
                "host": "codex",
                "semantic_coverage": {
                    "ok": false,
                    "status": "stdio_attach",
                    "detail": "stdio attach `codelens-mcp`; semantic coverage is only probed for HTTP daemons",
                    "remediation": null
                }
            },
            {
                "host": "windsurf",
                "semantic_coverage": {
                    "ok": false,
                    "status": "not_configured",
                    "detail": "no attached machine-readable CodeLens config",
                    "remediation": null
                }
            }
        ]
    });

    let failures = strict_failures_from_json(&payload);
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("codex"));
    assert!(failures[0].contains("stdio attach"));
}

#[test]
fn strict_json_failures_include_stale_and_unreachable_remediation() {
    let payload = json!({
        "hosts": [
            {
                "host": "codex",
                "semantic_coverage": {
                    "ok": false,
                    "status": "stale",
                    "detail": "status=stale, stale_files=3",
                    "remediation": "Run index_embeddings for this project, then rerun doctor/status --strict."
                }
            },
            {
                "host": "claude-code",
                "semantic_coverage": {
                    "ok": false,
                    "status": "unreachable",
                    "detail": "HTTP JSON-RPC request failed: connection refused",
                    "remediation": "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict."
                }
            },
            {
                "host": "windsurf",
                "semantic_coverage": {
                    "ok": false,
                    "status": "not_configured",
                    "detail": "no attached machine-readable CodeLens config",
                    "remediation": null
                }
            }
        ]
    });

    let failures = strict_failures_from_json(&payload);

    assert_eq!(failures.len(), 2);
    assert!(failures[0].contains("codex"));
    assert!(failures[0].contains("Run index_embeddings"));
    assert!(failures[1].contains("claude-code"));
    assert!(failures[1].contains("Start the CodeLens HTTP daemon"));
}

#[test]
fn strict_failure_message_includes_rendered_report() {
    let err = ensure_strict_semantic_coverage(
        "status",
        "{\"strict_semantic_coverage\":true}",
        &["codex: status=stale".to_owned()],
    )
    .expect_err("strict failure");

    let message = err.to_string();
    assert!(message.contains("strict semantic coverage failed"));
    assert!(message.contains("\"strict_semantic_coverage\":true"));
    assert!(message.contains("codex: status=stale"));
}
