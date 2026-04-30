//! Host doctor/status report rendering.

use super::inspect::{inspect_host_file, inspect_host_file_json};
use super::{
    append_host_adapter_common_metadata, canonical_attach_host, home_dir_from_env,
    host_adapter_common_metadata_json, resolve_host_path, supported_attach_hosts,
};
use crate::surface_manifest::HOST_ADAPTER_HOSTS;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn parse_doctor_hosts(args: &[String]) -> Result<Vec<&'static str>> {
    let filtered = args[2..]
        .iter()
        .filter(|arg| arg.as_str() != "--json")
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
    let mut out = String::new();
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
        }
    }

    out.push_str("\nInterpretation:\n");
    out.push_str("- `attached` means a machine-readable CodeLens config stanza or section is currently detectable.\n");
    out.push_str("- `present (customized)` means the policy file or managed block exists and still resembles the generated template, but has local edits.\n");
    out.push_str("- `manual review required` means the file exists but the lightweight checker cannot prove alignment.\n");
    Ok(out)
}

fn doctor_report_json(command: &str, hosts: &[&str], home: &Path, cwd: &Path) -> Result<Value> {
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

            Ok(json!({
                "host": host,
                "metadata": host_adapter_common_metadata_json(&adapter),
                "files": files,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "command": command,
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

pub(crate) fn run_doctor_command(args: &[String]) -> Result<String> {
    let command = args.get(1).map_or("doctor", String::as_str);
    let hosts = parse_doctor_hosts(args)?;
    let home = home_dir_from_env()?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    if args[2..].iter().any(|arg| arg == "--json") {
        return serde_json::to_string_pretty(&doctor_report_json(command, &hosts, &home, &cwd)?)
            .context("failed to render doctor report as JSON");
    }
    render_doctor_report(command, &hosts, &home, &cwd)
}
