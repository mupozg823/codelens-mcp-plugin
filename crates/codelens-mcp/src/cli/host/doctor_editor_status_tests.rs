use super::{doctor_report_json, strict_failures_from_json};
use anyhow::Result;
use serde_json::Value;
use std::fs;
#[cfg(feature = "http")]
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("codelens-doctor-editor-{name}-{nanos}"))
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

#[cfg(feature = "http")]
fn write_cline_project_config(project: &Path, url: &str) -> io::Result<()> {
    fs::write(
        project.join("mcp_servers.json"),
        format!(r#"{{"codelens":{{"url":"{url}"}}}}"#),
    )
}

fn write_malformed_cline_project_config(project: &Path) -> std::io::Result<()> {
    fs::write(project.join("mcp_servers.json"), r#"{"codelens":{}}"#)
}

#[cfg(feature = "http")]
fn write_windsurf_config(home: &Path, url: &str) -> io::Result<()> {
    let config_dir = home.join(".codeium/windsurf");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("mcp_config.json"),
        format!(r#"{{"mcpServers":{{"codelens":{{"url":"{url}"}}}}}}"#),
    )
}

fn write_malformed_windsurf_config(home: &Path) -> std::io::Result<()> {
    let config_dir = home.join(".codeium/windsurf");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("mcp_config.json"),
        r#"{"mcpServers":{"codelens":{}}}"#,
    )
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_unreachable_cline_http_config() -> Result<()> {
    let root = temp_root("unreachable-cline-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_cline_project_config(&project, "http://127.0.0.1:1/mcp")?;

    let payload = doctor_report_json("status", &["cline"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "cline")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "unreachable");
    assert_eq!(
        coverage["remediation"],
        "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict."
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("cline"));
    assert!(failures[0].contains("Start the CodeLens HTTP daemon"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[cfg(feature = "http")]
#[test]
fn strict_status_json_reports_unreachable_windsurf_http_config() -> Result<()> {
    let root = temp_root("unreachable-windsurf-http");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_windsurf_config(&home, "http://127.0.0.1:1/mcp")?;

    let payload = doctor_report_json("status", &["windsurf"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "windsurf")?;
    let failures = strict_failures_from_json(&payload);

    assert_eq!(coverage["status"], "unreachable");
    assert_eq!(
        coverage["remediation"],
        "Start the CodeLens HTTP daemon or repair the configured URL/headers, then rerun doctor/status --strict."
    );
    assert_eq!(failures.len(), 1);
    assert!(failures[0].contains("windsurf"));
    assert!(failures[0].contains("Start the CodeLens HTTP daemon"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn strict_status_json_reports_malformed_cline_config() -> Result<()> {
    let root = temp_root("malformed-cline");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_malformed_cline_project_config(&project)?;

    let payload = doctor_report_json("status", &["cline"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "cline")?;
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
    assert!(failures[0].contains("cline"));
    assert!(failures[0].contains("Repair host config"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn strict_status_json_reports_malformed_windsurf_config() -> Result<()> {
    let root = temp_root("malformed-windsurf");
    let home = root.join("home");
    let project = root.join("project");
    fs::create_dir_all(&project)?;
    write_malformed_windsurf_config(&home)?;

    let payload = doctor_report_json("status", &["windsurf"], &home, &project, true)?;
    let coverage = coverage_for_host(&payload, "windsurf")?;
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
    assert!(failures[0].contains("windsurf"));
    assert!(failures[0].contains("Repair host config"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}
