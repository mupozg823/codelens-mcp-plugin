//! Host config inspection and cleanup helpers.

use serde_json::{Value, json};
use std::fs;
use std::path::Path;

mod text_policy;
use text_policy::{
    inspect_json_config_entry, inspect_json_config_entry_json, inspect_toml_section,
    inspect_toml_section_json,
};
pub(crate) use text_policy::{inspect_text_policy_file, inspect_text_policy_file_json};

pub(super) fn normalize_text_for_compare(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

pub(super) fn parse_json_route_from_template(template: &Value) -> Option<(Vec<String>, String)> {
    let object = template.as_object()?;
    if object
        .get("mcpServers")
        .and_then(Value::as_object)
        .is_some_and(|map| map.contains_key("codelens"))
    {
        return Some((vec!["mcpServers".to_owned()], "codelens".to_owned()));
    }
    if object
        .get("servers")
        .and_then(Value::as_object)
        .is_some_and(|map| map.contains_key("codelens"))
    {
        return Some((vec!["servers".to_owned()], "codelens".to_owned()));
    }
    if object.contains_key("codelens") {
        return Some((Vec::new(), "codelens".to_owned()));
    }
    None
}

pub(super) fn get_json_key<'a>(
    value: &'a Value,
    parent_path: &[String],
    key: &str,
) -> Option<&'a Value> {
    let mut current = value;
    for segment in parent_path {
        current = current.get(segment)?;
    }
    current.get(key)
}

fn remove_json_key(value: &mut Value, parent_path: &[String], key: &str) -> bool {
    if parent_path.is_empty() {
        return value
            .as_object_mut()
            .and_then(|map| map.remove(key))
            .is_some();
    }

    let mut current = value;
    for segment in parent_path {
        let Some(next) = current.get_mut(segment) else {
            return false;
        };
        current = next;
    }

    current
        .as_object_mut()
        .and_then(|map| map.remove(key))
        .is_some()
}

fn prune_empty_json(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let empty_keys = map
                .iter_mut()
                .filter_map(|(key, child)| prune_empty_json(child).then_some(key.clone()))
                .collect::<Vec<_>>();
            for key in empty_keys {
                map.remove(&key);
            }
            map.is_empty()
        }
        Value::Array(items) => {
            items.retain_mut(|item| !prune_empty_json(item));
            items.is_empty()
        }
        _ => false,
    }
}

pub(super) fn remove_json_config_entry(
    path: &Path,
    parent_path: &[String],
    key: &str,
    summary: &str,
    apply_changes: bool,
) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display}: not present");
    };
    let Ok(mut payload) = serde_json::from_str::<Value>(&content) else {
        return format!("- {display}: manual cleanup required ({summary}; invalid JSON)");
    };
    if !remove_json_key(&mut payload, parent_path, key) {
        return format!("- {display}: no CodeLens entry found");
    }
    prune_empty_json(&mut payload);
    if payload.as_object().is_some_and(|map| map.is_empty()) {
        if !apply_changes {
            return format!("- {display}: would remove empty config file");
        }
        match fs::remove_file(path) {
            Ok(()) => format!("- {display}: removed empty config file"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    } else {
        if !apply_changes {
            return format!("- {display}: would remove CodeLens config entry");
        }
        match fs::write(
            path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_owned())
            ),
        ) {
            Ok(()) => format!("- {display}: removed CodeLens config entry"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    }
}

pub(super) fn extract_toml_section_name(template: &str) -> Option<String> {
    template.lines().find_map(|line| {
        let trimmed = line.trim();
        (trimmed.starts_with('[') && trimmed.ends_with(']')).then(|| {
            trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned()
        })
    })
}

pub(super) fn remove_toml_section(path: &Path, section: &str, apply_changes: bool) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display}: not present");
    };
    let header = format!("[{section}]");
    let mut removed = false;
    let mut output = String::new();
    let mut skipping = false;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if !skipping && trimmed == header {
            removed = true;
            skipping = true;
            continue;
        }
        if skipping {
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                skipping = false;
            } else {
                continue;
            }
        }
        output.push_str(line);
    }

    if !removed {
        return format!("- {display}: no CodeLens section found");
    }

    let cleaned = output.trim().to_owned();
    if cleaned.is_empty() {
        if !apply_changes {
            return format!("- {display}: would remove empty config file");
        }
        match fs::remove_file(path) {
            Ok(()) => format!("- {display}: removed empty config file"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    } else {
        if !apply_changes {
            return format!("- {display}: would remove CodeLens TOML section");
        }
        match fs::write(path, format!("{cleaned}\n")) {
            Ok(()) => format!("- {display}: removed CodeLens TOML section"),
            Err(err) => format!("- {display}: manual cleanup required ({err})"),
        }
    }
}

pub(super) fn remove_exact_text_file(
    path: &Path,
    expected: &str,
    label: &str,
    apply_changes: bool,
) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display}: not present");
    };
    if normalize_text_for_compare(&content) != normalize_text_for_compare(expected) {
        return format!(
            "- {display}: manual cleanup required ({label} was modified after generation)"
        );
    }
    if !apply_changes {
        return format!("- {display}: would remove generated {label}");
    }
    match fs::remove_file(path) {
        Ok(()) => format!("- {display}: removed generated {label}"),
        Err(err) => format!("- {display}: manual cleanup required ({err})"),
    }
}

pub(super) fn has_toml_section(content: &str, section: &str) -> bool {
    let header = format!("[{section}]");
    content.lines().any(|line| line.trim() == header)
}

pub(super) fn extract_toml_section_block(content: &str, section: &str) -> Option<String> {
    let header = format!("[{section}]");
    let mut in_section = false;
    let mut lines = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if !in_section {
            if trimmed == header {
                in_section = true;
                lines.push(line.to_owned());
            }
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            break;
        }
        lines.push(line.to_owned());
    }

    if !in_section {
        return None;
    }

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    Some(lines.join("\n"))
}

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
