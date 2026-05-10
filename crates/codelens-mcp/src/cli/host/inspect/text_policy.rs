//! Text policy file inspection helpers.

use super::{
    extract_toml_section_block, extract_toml_section_name, get_json_key, has_toml_section,
    normalize_text_for_compare, parse_json_route_from_template,
};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

fn first_significant_template_line(template: &str) -> Option<&str> {
    template
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```") && *line != "---")
}

fn extract_managed_text_block(text: &str) -> Option<String> {
    const BEGIN: &str = "<!-- CODELENS_HOST_ROUTING:BEGIN -->";
    const END: &str = "<!-- CODELENS_HOST_ROUTING:END -->";

    let start = text.find(BEGIN)?;
    let end = text[start..].find(END)? + start + END.len();
    Some(text[start..end].to_owned())
}

/// Single source of truth for `inspect_*` results — `(status_slug, message)`.
/// The text and JSON wrappers below format this tuple differently but never
/// disagree on the underlying classification.
type InspectVerdict = (&'static str, &'static str);

fn classify_json_config_entry(path: &Path, template: &Value) -> InspectVerdict {
    let Ok(content) = fs::read_to_string(path) else {
        return ("missing", "missing");
    };
    let Ok(payload) = serde_json::from_str::<Value>(&content) else {
        return ("invalid_json", "present but invalid JSON");
    };
    let Some((parent_path, key)) = parse_json_route_from_template(template) else {
        return (
            "manual_review_required",
            "present but manual review required",
        );
    };
    let Some(actual) = get_json_key(&payload, &parent_path, &key) else {
        return (
            "missing_codelens_entry",
            "present but missing CodeLens entry",
        );
    };
    let expected = get_json_key(template, &parent_path, &key);
    if expected.is_some_and(|value| value == actual) {
        ("attached_exact", "attached (exact CodeLens entry)")
    } else {
        (
            "attached_customized",
            "attached (customized CodeLens entry)",
        )
    }
}

pub(super) fn inspect_json_config_entry(path: &Path, template: &Value) -> String {
    let (_, message) = classify_json_config_entry(path, template);
    format!("- {} [json]: {}", path.display(), message)
}

pub(super) fn inspect_json_config_entry_json(path: &Path, template: &Value) -> Value {
    let (status, message) = classify_json_config_entry(path, template);
    json!({
        "path": path.display().to_string(),
        "format": "json",
        "status": status,
        "message": message,
    })
}

fn classify_toml_section(path: &Path, template: &str) -> InspectVerdict {
    let Ok(content) = fs::read_to_string(path) else {
        return ("missing", "missing");
    };
    let Some(section) = extract_toml_section_name(template) else {
        return (
            "manual_review_required",
            "present but manual review required",
        );
    };
    if !has_toml_section(&content, &section) {
        return (
            "missing_codelens_section",
            "present but missing CodeLens section",
        );
    }
    let actual_section =
        extract_toml_section_block(&content, &section).unwrap_or_else(|| content.clone());
    if normalize_text_for_compare(&actual_section) == normalize_text_for_compare(template) {
        ("attached_exact", "attached (exact generated file)")
    } else {
        ("attached_customized", "attached (CodeLens section present)")
    }
}

pub(super) fn inspect_toml_section(path: &Path, template: &str) -> String {
    let (_, message) = classify_toml_section(path, template);
    format!("- {} [toml]: {}", path.display(), message)
}

pub(super) fn inspect_toml_section_json(path: &Path, template: &str) -> Value {
    let (status, message) = classify_toml_section(path, template);
    json!({
        "path": path.display().to_string(),
        "format": "toml",
        "status": status,
        "message": message,
    })
}

fn classify_text_policy_file(path: &Path, expected: &str) -> InspectVerdict {
    let Ok(content) = fs::read_to_string(path) else {
        return ("missing", "missing");
    };
    if normalize_text_for_compare(&content) == normalize_text_for_compare(expected) {
        return ("present_exact", "present (exact generated file)");
    }
    if let Some(expected_block) = extract_managed_text_block(expected)
        && let Some(actual_block) = extract_managed_text_block(&content)
    {
        if normalize_text_for_compare(&actual_block) == normalize_text_for_compare(&expected_block)
        {
            return ("present_exact", "present (exact managed block)");
        }
        return ("present_customized", "present (customized managed block)");
    }
    if first_significant_template_line(expected).is_some_and(|line| content.contains(line)) {
        return ("present_customized", "present (customized)");
    }
    (
        "manual_review_required",
        "present but manual review required",
    )
}

pub(crate) fn inspect_text_policy_file(path: &Path, expected: &str, format: &str) -> String {
    let (_, message) = classify_text_policy_file(path, expected);
    format!("- {} [{format}]: {}", path.display(), message)
}

pub(crate) fn inspect_text_policy_file_json(path: &Path, expected: &str, format: &str) -> Value {
    let (status, message) = classify_text_policy_file(path, expected);
    json!({
        "path": path.display().to_string(),
        "format": format,
        "status": status,
        "message": message,
    })
}
