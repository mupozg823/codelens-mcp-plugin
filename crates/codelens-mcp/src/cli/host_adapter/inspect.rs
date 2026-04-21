use super::{
    json_config::{inspect_json_config_entry, inspect_json_config_entry_json},
    text_policy::{inspect_text_policy_file, inspect_text_policy_file_json},
    toml_config::{inspect_toml_section, inspect_toml_section_json},
};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn inspect_host_file(path: &Path, format: &str, template: Option<&Value>) -> String {
    match format {
        "json" => template
            .map(|template| inspect_json_config_entry(path, template))
            .unwrap_or_else(|| {
                format!(
                    "- {} [json]: present but manual review required",
                    path.display()
                )
            }),
        "toml" => template
            .and_then(Value::as_str)
            .map(|template| inspect_toml_section(path, template))
            .unwrap_or_else(|| {
                format!(
                    "- {} [toml]: present but manual review required",
                    path.display()
                )
            }),
        "markdown" | "mdc" => template
            .and_then(Value::as_str)
            .map(|template| inspect_text_policy_file(path, template, format))
            .unwrap_or_else(|| {
                format!(
                    "- {} [{format}]: present but manual review required",
                    path.display()
                )
            }),
        other => {
            if path.exists() {
                format!("- {} [{other}]: present", path.display())
            } else {
                format!("- {} [{other}]: missing", path.display())
            }
        }
    }
}

pub(super) fn inspect_host_file_json(path: &Path, format: &str, template: Option<&Value>) -> Value {
    match format {
        "json" => template
            .map(|template| inspect_json_config_entry_json(path, template))
            .unwrap_or_else(|| {
                json!({
                    "path": path.display().to_string(),
                    "format": "json",
                    "status": "manual_review_required",
                    "message": "present but manual review required",
                })
            }),
        "toml" => template
            .and_then(Value::as_str)
            .map(|template| inspect_toml_section_json(path, template))
            .unwrap_or_else(|| {
                json!({
                    "path": path.display().to_string(),
                    "format": "toml",
                    "status": "manual_review_required",
                    "message": "present but manual review required",
                })
            }),
        "markdown" | "mdc" => template
            .and_then(Value::as_str)
            .map(|template| inspect_text_policy_file_json(path, template, format))
            .unwrap_or_else(|| {
                json!({
                    "path": path.display().to_string(),
                    "format": format,
                    "status": "manual_review_required",
                    "message": "present but manual review required",
                })
            }),
        other => {
            if path.exists() {
                json!({
                    "path": path.display().to_string(),
                    "format": other,
                    "status": "present",
                    "message": "present",
                })
            } else {
                json!({
                    "path": path.display().to_string(),
                    "format": other,
                    "status": "missing",
                    "message": "missing",
                })
            }
        }
    }
}
