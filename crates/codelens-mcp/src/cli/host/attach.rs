//! Host attach instruction rendering.

use super::{
    append_host_adapter_attach_guidance, append_host_adapter_common_metadata,
    canonical_attach_host, json_string_list, render_template, supported_attach_hosts,
};
use anyhow::{Context, Result};
use serde_json::Value;

pub(crate) fn render_attach_instructions(host: Option<&str>) -> Result<String> {
    let requested = host.context(format!(
        "usage: codelens-mcp attach <host>\nsupported hosts: {}",
        supported_attach_hosts()
    ))?;
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown attach host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    let adapter = crate::surface_manifest::host_adapter_bundle_for_project(canonical, Some(&cwd))
        .context("missing host adapter bundle for attach target")?;

    let execution_rules = json_string_list(&adapter, "execution_rules");
    let routing_defaults = adapter
        .get("routing_defaults")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let native_files = adapter
        .get("native_files")
        .and_then(Value::as_array)
        .context("host adapter bundle is missing native_files")?;

    let mut out = String::new();
    out.push_str(&format!("CodeLens attach target: {canonical}\n"));
    if requested != canonical {
        out.push_str(&format!("Requested alias: {requested} -> {canonical}\n"));
    }
    append_host_adapter_common_metadata(&mut out, &adapter, "");
    append_host_adapter_attach_guidance(&mut out, &adapter);

    if !routing_defaults.is_empty() {
        out.push_str("Routing defaults:\n");
        for (key, value) in routing_defaults {
            let value = value.as_str().unwrap_or("<non-string-routing-default>");
            out.push_str(&format!("- {key}: {value}\n"));
        }
    }

    if !execution_rules.is_empty() {
        out.push_str("Execution contract:\n");
        for rule in execution_rules {
            out.push_str(&format!("- {rule}\n"));
        }
    }

    out.push_str("\nCopy the following templates into the listed host-native files.\n");
    if let Some(url) = adapter.get("resolved_mcp_url").and_then(Value::as_str) {
        let source = adapter
            .get("resolved_mcp_url_source")
            .and_then(Value::as_str)
            .unwrap_or(".codelens/config.json");
        out.push_str(&format!(
            "Project-local daemon URL override from `{source}`: `{url}`.\n"
        ));
    } else {
        out.push_str("The default daemon URL assumes `http://127.0.0.1:7837/mcp`.\n");
    }
    out.push_str(&format!(
        "Verify the host wiring with `codelens-mcp doctor {canonical}` after applying the config.\n"
    ));

    for file in native_files {
        let path = file
            .get("path")
            .and_then(Value::as_str)
            .context("native file entry is missing path")?;
        let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
        let purpose = file
            .get("purpose")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let template = file
            .get("template")
            .context("native file entry is missing template")?;

        out.push_str(&format!("\nPath: {path}\n"));
        out.push_str(&format!("Format: {format}\n"));
        if !purpose.is_empty() {
            out.push_str(&format!("Purpose: {purpose}\n"));
        }
        out.push_str(&format!(
            "```{format}\n{}\n```\n",
            render_template(template)?
        ));
    }

    Ok(out)
}
