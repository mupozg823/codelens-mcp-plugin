//! Host adapter attach/doctor/detach CLI rendering and inspection helpers.

use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(crate) fn canonical_attach_host(host: &str) -> Option<&'static str> {
    match host.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" | "claudecode" => Some("claude-code"),
        "codex" => Some("codex"),
        "cursor" => Some("cursor"),
        "cline" => Some("cline"),
        "windsurf" | "codeium" => Some("windsurf"),
        _ => None,
    }
}

pub(super) fn supported_attach_hosts() -> &'static str {
    "claude-code, codex, cursor, cline, windsurf"
}

pub(super) fn home_dir_from_env() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot resolve host-native config paths")
}

pub(super) fn resolve_host_path(raw: &str, home: &Path, cwd: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        cwd.join(raw)
    }
}

pub(super) fn json_string_list(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn push_labeled_line(out: &mut String, prefix: &str, label: &str, value: &str) {
    if !value.is_empty() {
        out.push_str(&format!("{prefix}{label}: {value}\n"));
    }
}

fn push_joined_line(out: &mut String, prefix: &str, label: &str, values: &[String]) {
    if !values.is_empty() {
        out.push_str(&format!("{prefix}{label}: {}\n", values.join(", ")));
    }
}

fn push_bulleted_block(out: &mut String, heading: &str, values: &[String]) {
    if !values.is_empty() {
        out.push_str(&format!("{heading}:\n"));
        for value in values {
            out.push_str(&format!("- {value}\n"));
        }
    }
}

pub(super) fn append_host_adapter_common_metadata(out: &mut String, adapter: &Value, prefix: &str) {
    let resource_uri = adapter
        .get("resource_uri")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let preferred_profiles = json_string_list(adapter, "preferred_profiles");
    let native_primitives = json_string_list(adapter, "native_primitives");
    let compiler_targets = json_string_list(adapter, "compiler_targets");

    push_labeled_line(out, prefix, "Adapter resource", resource_uri);
    push_joined_line(out, prefix, "Preferred profiles", &preferred_profiles);
    push_joined_line(out, prefix, "Native host primitives", &native_primitives);
    push_joined_line(out, prefix, "Host-native targets", &compiler_targets);
}

pub(super) fn append_host_adapter_attach_guidance(out: &mut String, adapter: &Value) {
    let best_fit = adapter
        .get("best_fit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let recommended_modes = json_string_list(adapter, "recommended_modes");
    let preferred_codelens_use = json_string_list(adapter, "preferred_codelens_use");
    let avoid = json_string_list(adapter, "avoid");
    let primary_bootstrap_sequence = json_string_list(adapter, "primary_bootstrap_sequence");

    push_labeled_line(out, "", "Best fit", best_fit);
    push_joined_line(out, "", "Recommended modes", &recommended_modes);
    push_bulleted_block(out, "Use CodeLens for", &preferred_codelens_use);
    push_bulleted_block(out, "Avoid", &avoid);
    if !primary_bootstrap_sequence.is_empty() {
        push_labeled_line(
            out,
            "",
            "Primary bootstrap sequence",
            &primary_bootstrap_sequence.join(" -> "),
        );
    }
}

pub(super) fn host_adapter_common_metadata_json(adapter: &Value) -> Value {
    json!({
        "resource_uri": adapter
            .get("resource_uri")
            .cloned()
            .unwrap_or(Value::Null),
        "preferred_profiles": json_string_list(adapter, "preferred_profiles"),
        "native_primitives": json_string_list(adapter, "native_primitives"),
        "compiler_targets": json_string_list(adapter, "compiler_targets"),
    })
}

pub(super) fn render_template(template: &Value) -> Result<String> {
    if let Some(text) = template.as_str() {
        Ok(text.to_owned())
    } else {
        serde_json::to_string_pretty(template).context("failed to render template as JSON")
    }
}

mod attach;
mod detach;
mod doctor;
mod inspect;

pub(crate) use attach::render_attach_instructions;
pub(crate) use detach::run_detach_command;
#[cfg(test)]
pub(crate) use detach::{parse_detach_hosts, render_detach_report};
pub(crate) use doctor::run_doctor_command;
#[cfg(test)]
pub(crate) use doctor::{parse_doctor_hosts, render_doctor_report};
#[cfg(test)]
pub(crate) use inspect::{inspect_text_policy_file, inspect_text_policy_file_json};
