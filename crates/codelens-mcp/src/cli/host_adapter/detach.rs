use super::{
    json_config::{parse_json_route_from_template, remove_json_config_entry},
    render::append_host_adapter_common_metadata,
    resolve::{
        canonical_attach_host, home_dir_from_env, resolve_host_path, supported_attach_hosts,
    },
    text_policy::remove_exact_text_file,
    toml_config::{extract_toml_section_name, remove_toml_section},
};
use crate::surface_manifest::HOST_ADAPTER_HOSTS;
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::Path;

pub(super) fn detach_host_files(
    host: &str,
    home: &Path,
    cwd: &Path,
    apply_changes: bool,
) -> Result<Vec<String>> {
    let adapter = crate::surface_manifest::host_adapter_bundle_for_project(host, Some(cwd))
        .context("missing host adapter bundle for detach target")?;
    let native_files = adapter
        .get("native_files")
        .and_then(Value::as_array)
        .context("host adapter bundle is missing native_files")?;

    let mut notes = vec![format!("{host}:")];
    for file in native_files {
        let raw_path = file
            .get("path")
            .and_then(Value::as_str)
            .context("native file entry is missing path")?;
        let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
        let path = resolve_host_path(raw_path, home, cwd);
        let template = file.get("template");

        let note = match format {
            "json" => match template {
                Some(template) => match parse_json_route_from_template(template) {
                    Some((parent_path, key)) => remove_json_config_entry(
                        &path,
                        &parent_path,
                        &key,
                        "unsupported JSON shape",
                        apply_changes,
                    ),
                    None => format!(
                        "- {}: manual cleanup required (unsupported JSON template shape)",
                        path.display()
                    ),
                },
                None => format!(
                    "- {}: manual cleanup required (missing template)",
                    path.display()
                ),
            },
            "toml" => match template
                .and_then(Value::as_str)
                .and_then(extract_toml_section_name)
            {
                Some(section) => remove_toml_section(&path, &section, apply_changes),
                None => format!(
                    "- {}: manual cleanup required (missing TOML section template)",
                    path.display()
                ),
            },
            "markdown" | "mdc" => match template.and_then(Value::as_str) {
                Some(expected) => remove_exact_text_file(&path, expected, format, apply_changes),
                None => format!(
                    "- {}: manual cleanup required (missing text template)",
                    path.display()
                ),
            },
            other => format!(
                "- {}: manual cleanup required (unsupported format `{other}`)",
                path.display()
            ),
        };
        notes.push(note);
    }

    Ok(notes)
}

pub(super) fn parse_detach_hosts(args: &[String]) -> Result<Vec<&'static str>> {
    let tail = &args[2..];
    if tail.is_empty() {
        anyhow::bail!(
            "usage: codelens-mcp detach <host>\n       codelens-mcp detach --all\nsupported hosts: {}",
            supported_attach_hosts()
        );
    }
    if tail.iter().any(|arg| arg == "--all" || arg == "all") {
        return Ok(HOST_ADAPTER_HOSTS.into_iter().collect());
    }

    let requested = tail[0].as_str();
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown detach host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    Ok(vec![canonical])
}

pub(super) fn detach_is_dry_run(args: &[String]) -> bool {
    args[2..].iter().any(|arg| arg == "--dry-run")
}

pub(super) fn render_detach_report(
    hosts: &[&str],
    home: &Path,
    cwd: &Path,
    apply_changes: bool,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("CodeLens detach report\n");
    if apply_changes {
        out.push_str("Machine-editable config files are cleaned automatically.\n");
    } else {
        out.push_str("Dry run only. No files were modified.\n");
    }
    out.push_str(
        "Modified policy markdown files are left in place and reported for manual cleanup.\n\n",
    );

    for (index, host) in hosts.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let adapter = crate::surface_manifest::host_adapter_bundle_for_project(host, Some(cwd))
            .context("missing host adapter bundle for detach report")?;

        out.push_str(&format!("{host}:\n"));
        append_host_adapter_common_metadata(&mut out, &adapter, "- ");
        for line in detach_host_files(host, home, cwd, apply_changes)? {
            if line == format!("{host}:") {
                continue;
            }
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.push_str("\nManual follow-up:\n");
    out.push_str("- Stop any running `codelens-mcp --transport http` daemons if you no longer want the shared server.\n");
    out.push_str(
        "- Remove repo-local `.codelens/` only if you also want to discard cached runtime state.\n",
    );
    out.push_str("- Remove the binary with your install channel: `brew uninstall codelens-mcp`, `cargo uninstall codelens-mcp`, or delete the installed executable path.\n");
    Ok(out)
}

pub(crate) fn run_detach_command(args: &[String]) -> Result<String> {
    let hosts = parse_detach_hosts(args)?;
    let home = home_dir_from_env()?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    render_detach_report(&hosts, &home, &cwd, !detach_is_dry_run(args))
}
