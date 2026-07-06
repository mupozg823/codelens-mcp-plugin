//! Host doctor/status report rendering.

mod coverage;
#[cfg(test)]
#[path = "doctor_editor_status_tests.rs"]
mod editor_status_tests;
#[cfg(test)]
#[path = "doctor_strict_status_tests.rs"]
mod strict_status_tests;
#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;

use super::inspect::{inspect_host_file, inspect_host_file_json};
use super::{
    append_host_adapter_common_metadata, canonical_attach_host, home_dir_from_env,
    host_adapter_common_metadata_json, resolve_host_path, supported_attach_hosts,
};
use crate::surface_manifest::HOST_ADAPTER_HOSTS;
use anyhow::{Context, Result};
use coverage::{coverage_for_inspected_files, strict_semantic_coverage_enabled};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn parse_doctor_hosts(args: &[String]) -> Result<Vec<&'static str>> {
    let filtered = args[2..]
        .iter()
        .filter(|arg| !matches!(arg.as_str(), "--json" | "--strict"))
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        anyhow::bail!(
            "usage: codelens-mcp doctor <host>\n       codelens-mcp doctor --all\nsupported hosts: {}",
            supported_attach_hosts()
        );
    }
    if filtered
        .iter()
        .any(|arg| arg.as_str() == "--all" || arg.as_str() == "all")
    {
        return Ok(HOST_ADAPTER_HOSTS.into_iter().collect());
    }

    let requested = filtered
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str())
        .context(format!(
            "usage: codelens-mcp doctor <host>\n       codelens-mcp doctor --all\nsupported hosts: {}",
            supported_attach_hosts()
        ))?;
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown doctor host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    Ok(vec![canonical])
}

pub(crate) fn render_doctor_report(
    command: &str,
    hosts: &[&str],
    home: &Path,
    cwd: &Path,
) -> Result<String> {
    render_doctor_report_with_options(command, hosts, home, cwd, false)
}

fn render_doctor_report_with_options(
    command: &str,
    hosts: &[&str],
    home: &Path,
    cwd: &Path,
    strict_semantic_coverage: bool,
) -> Result<String> {
    let mut out = String::new();
    let mut strict_failures = Vec::new();
    out.push_str(&format!("CodeLens {command} report\n"));
    out.push_str(
        "Host checks reuse canonical host-adapter metadata plus local filesystem state.\n\n",
    );

    for (index, host) in hosts.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let adapter = crate::surface_manifest::host_adapter_bundle_for_project(host, Some(cwd))
            .context("missing host adapter bundle for doctor report")?;
        let native_files = adapter
            .get("native_files")
            .and_then(Value::as_array)
            .context("host adapter bundle is missing native_files")?;

        out.push_str(&format!("{host}:\n"));
        append_host_adapter_common_metadata(&mut out, &adapter, "- ");
        let mut inspected_files = Vec::new();
        for file in native_files {
            let raw_path = file
                .get("path")
                .and_then(Value::as_str)
                .context("native file entry is missing path")?;
            let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
            let path = resolve_host_path(raw_path, home, cwd);
            let template = file.get("template");
            out.push_str(&inspect_host_file(&path, format, template));
            out.push('\n');
            inspected_files.push(inspect_host_file_json(&path, format, template));
        }
        if strict_semantic_coverage {
            let coverage = coverage_for_inspected_files(&inspected_files, cwd);
            if let Some(issue) = coverage.strict_exit_issue() {
                strict_failures.push(format!("{host}: {issue}"));
            }
            out.push_str(&format!(
                "  semantic coverage: {}\n",
                coverage.render_text()
            ));
        }
    }

    out.push_str("\nInterpretation:\n");
    out.push_str("- `attached` means a machine-readable CodeLens config stanza or section is currently detectable.\n");
    out.push_str("- `present (customized)` means the policy file or managed block exists and still resembles the generated template, but has local edits.\n");
    out.push_str("- `manual review required` means the file exists but the lightweight checker cannot prove alignment.\n");
    ensure_strict_semantic_coverage(command, &out, &strict_failures)?;
    Ok(out)
}

fn doctor_report_json(
    command: &str,
    hosts: &[&str],
    home: &Path,
    cwd: &Path,
    strict_semantic_coverage: bool,
) -> Result<Value> {
    let host_reports = hosts
        .iter()
        .map(|host| {
            let adapter = crate::surface_manifest::host_adapter_bundle_for_project(host, Some(cwd))
                .context("missing host adapter bundle for doctor report")?;
            let native_files = adapter
                .get("native_files")
                .and_then(Value::as_array)
                .context("host adapter bundle is missing native_files")?;
            let files = native_files
                .iter()
                .map(|file| {
                    let raw_path = file
                        .get("path")
                        .and_then(Value::as_str)
                        .context("native file entry is missing path")?;
                    let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
                    let path = resolve_host_path(raw_path, home, cwd);
                    let template = file.get("template");
                    Ok(inspect_host_file_json(&path, format, template))
                })
                .collect::<Result<Vec<_>>>()?;
            let semantic_coverage = strict_semantic_coverage
                .then(|| coverage_for_inspected_files(&files, cwd).to_json());

            Ok(json!({
                "host": host,
                "metadata": host_adapter_common_metadata_json(&adapter),
                "files": files,
                "semantic_coverage": semantic_coverage,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "command": command,
        "strict_semantic_coverage": strict_semantic_coverage,
        "hosts": host_reports,
        "legend": {
            "attached_exact": "machine-readable CodeLens config matches the generated entry exactly",
            "attached_customized": "machine-readable CodeLens config is present but differs from the generated entry",
            "present_exact": "text policy file or managed block matches the generated template exactly",
            "present_customized": "text policy file or managed block exists and still resembles the generated template, but has local edits",
            "manual_review_required": "the lightweight checker cannot prove alignment",
            "missing": "file is absent"
        }
    }))
}

fn strict_failures_from_json(payload: &Value) -> Vec<String> {
    payload
        .get("hosts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|host_report| {
            let host = host_report
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let coverage = host_report.get("semantic_coverage")?;
            let status = coverage
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let ok = coverage.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if ok || status == "not_configured" {
                return None;
            }
            let detail = coverage
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("semantic coverage was not ready");
            let remediation = coverage.get("remediation").and_then(Value::as_str);
            let issue = match remediation {
                Some(remediation) => format!("{host}: {detail}; remediation: {remediation}"),
                None => format!("{host}: {detail}"),
            };
            Some(issue)
        })
        .collect()
}

fn ensure_strict_semantic_coverage(
    command: &str,
    rendered: &str,
    strict_failures: &[String],
) -> Result<()> {
    if strict_failures.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "CodeLens {command} strict semantic coverage failed\n\n{rendered}\nIssues:\n- {}",
        strict_failures.join("\n- ")
    )
}

pub(crate) fn run_doctor_command(args: &[String]) -> Result<String> {
    let command = args.get(1).map(String::as_str).unwrap_or("doctor");
    let hosts = parse_doctor_hosts(args)?;
    let strict_semantic_coverage = strict_semantic_coverage_enabled(args);
    let home = home_dir_from_env()?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    if args[2..].iter().any(|arg| arg == "--json") {
        let payload = doctor_report_json(command, &hosts, &home, &cwd, strict_semantic_coverage)?;
        let rendered = serde_json::to_string_pretty(&payload)
            .context("failed to render doctor report as JSON")?;
        if strict_semantic_coverage {
            ensure_strict_semantic_coverage(
                command,
                &rendered,
                &strict_failures_from_json(&payload),
            )?;
        }
        return Ok(rendered);
    }
    if strict_semantic_coverage {
        render_doctor_report_with_options(command, &hosts, &home, &cwd, true)
    } else {
        render_doctor_report(command, &hosts, &home, &cwd)
    }
}
