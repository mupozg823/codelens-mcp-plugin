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

    let delegate_scaffold_rules = json_string_list(&adapter, "delegate_scaffold_rules");
    let overlay_previews = adapter
        .get("overlay_previews")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

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

    if !delegate_scaffold_rules.is_empty() {
        out.push_str("Delegate scaffold contract:\n");
        for rule in delegate_scaffold_rules {
            out.push_str(&format!("- {rule}\n"));
        }
    }

    if !overlay_previews.is_empty() {
        out.push_str("Compiled overlays:\n");
        for preview in overlay_previews {
            let profile = preview
                .get("profile")
                .and_then(Value::as_str)
                .unwrap_or("<unknown-profile>");
            let task_overlay = preview
                .get("task_overlay")
                .and_then(Value::as_str)
                .unwrap_or("<unknown-overlay>");
            let preferred_executor_bias = preview
                .get("preferred_executor_bias")
                .and_then(Value::as_str)
                .unwrap_or("any");
            let bootstrap_sequence = json_string_list(&preview, "bootstrap_sequence");
            let avoid_tools = json_string_list(&preview, "avoid_tools");
            out.push_str(&format!(
                "- {profile} / {task_overlay}: {} [bias: {preferred_executor_bias}]\n",
                if bootstrap_sequence.is_empty() {
                    "prepare_harness_session".to_owned()
                } else {
                    bootstrap_sequence.join(" -> ")
                }
            ));
            if !avoid_tools.is_empty() {
                out.push_str(&format!("  avoid: {}\n", avoid_tools.join(", ")));
            }
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
