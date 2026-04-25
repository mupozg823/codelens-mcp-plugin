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

pub(super) fn inspect_json_config_entry(path: &Path, template: &Value) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display} [json]: missing");
    };
    let Ok(payload) = serde_json::from_str::<Value>(&content) else {
        return format!("- {display} [json]: present but invalid JSON");
    };
    let Some((parent_path, key)) = parse_json_route_from_template(template) else {
        return format!("- {display} [json]: present but manual review required");
    };
    let Some(actual) = get_json_key(&payload, &parent_path, &key) else {
        return format!("- {display} [json]: present but missing CodeLens entry");
    };
    let expected = get_json_key(template, &parent_path, &key);
    if expected.is_some_and(|value| value == actual) {
        format!("- {display} [json]: attached (exact CodeLens entry)")
    } else {
        format!("- {display} [json]: attached (customized CodeLens entry)")
    }
}

pub(super) fn inspect_json_config_entry_json(path: &Path, template: &Value) -> Value {
    let path_text = path.display().to_string();
    let Ok(content) = fs::read_to_string(path) else {
        return json!({
            "path": path_text,
            "format": "json",
            "status": "missing",
            "message": "missing",
        });
    };
    let Ok(payload) = serde_json::from_str::<Value>(&content) else {
        return json!({
            "path": path_text,
            "format": "json",
            "status": "invalid_json",
            "message": "present but invalid JSON",
        });
    };
    let Some((parent_path, key)) = parse_json_route_from_template(template) else {
        return json!({
            "path": path_text,
            "format": "json",
            "status": "manual_review_required",
            "message": "present but manual review required",
        });
    };
    let Some(actual) = get_json_key(&payload, &parent_path, &key) else {
        return json!({
            "path": path_text,
            "format": "json",
            "status": "missing_codelens_entry",
            "message": "present but missing CodeLens entry",
        });
    };
    let expected = get_json_key(template, &parent_path, &key);
    if expected.is_some_and(|value| value == actual) {
        json!({
            "path": path_text,
            "format": "json",
            "status": "attached_exact",
            "message": "attached (exact CodeLens entry)",
        })
    } else {
        json!({
            "path": path_text,
            "format": "json",
            "status": "attached_customized",
            "message": "attached (customized CodeLens entry)",
        })
    }
}

pub(super) fn inspect_toml_section(path: &Path, template: &str) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display} [toml]: missing");
    };
    let Some(section) = extract_toml_section_name(template) else {
        return format!("- {display} [toml]: present but manual review required");
    };
    if !has_toml_section(&content, &section) {
        return format!("- {display} [toml]: present but missing CodeLens section");
    }
    let actual_section =
        extract_toml_section_block(&content, &section).unwrap_or_else(|| content.clone());
    if normalize_text_for_compare(&actual_section) == normalize_text_for_compare(template) {
        format!("- {display} [toml]: attached (exact generated file)")
    } else {
        format!("- {display} [toml]: attached (CodeLens section present)")
    }
}

pub(super) fn inspect_toml_section_json(path: &Path, template: &str) -> Value {
    let path_text = path.display().to_string();
    let Ok(content) = fs::read_to_string(path) else {
        return json!({
            "path": path_text,
            "format": "toml",
            "status": "missing",
            "message": "missing",
        });
    };
    let Some(section) = extract_toml_section_name(template) else {
        return json!({
            "path": path_text,
            "format": "toml",
            "status": "manual_review_required",
            "message": "present but manual review required",
        });
    };
    if !has_toml_section(&content, &section) {
        return json!({
            "path": path_text,
            "format": "toml",
            "status": "missing_codelens_section",
            "message": "present but missing CodeLens section",
        });
    }
    let actual_section =
        extract_toml_section_block(&content, &section).unwrap_or_else(|| content.clone());
    if normalize_text_for_compare(&actual_section) == normalize_text_for_compare(template) {
        json!({
            "path": path_text,
            "format": "toml",
            "status": "attached_exact",
            "message": "attached (exact generated file)",
        })
    } else {
        json!({
            "path": path_text,
            "format": "toml",
            "status": "attached_customized",
            "message": "attached (CodeLens section present)",
        })
    }
}

pub(crate) fn inspect_text_policy_file(path: &Path, expected: &str, format: &str) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display} [{format}]: missing");
    };
    if normalize_text_for_compare(&content) == normalize_text_for_compare(expected) {
        return format!("- {display} [{format}]: present (exact generated file)");
    }
    if let Some(expected_block) = extract_managed_text_block(expected)
        && let Some(actual_block) = extract_managed_text_block(&content)
    {
        if normalize_text_for_compare(&actual_block) == normalize_text_for_compare(&expected_block)
        {
            return format!("- {display} [{format}]: present (exact managed block)");
        }
        return format!("- {display} [{format}]: present (customized managed block)");
    }
    if first_significant_template_line(expected).is_some_and(|line| content.contains(line)) {
        return format!("- {display} [{format}]: present (customized)");
    }
    format!("- {display} [{format}]: present but manual review required")
}

pub(crate) fn inspect_text_policy_file_json(path: &Path, expected: &str, format: &str) -> Value {
    let path_text = path.display().to_string();
    let Ok(content) = fs::read_to_string(path) else {
        return json!({
            "path": path_text,
            "format": format,
            "status": "missing",
            "message": "missing",
        });
    };
    if normalize_text_for_compare(&content) == normalize_text_for_compare(expected) {
        return json!({
            "path": path_text,
            "format": format,
            "status": "present_exact",
            "message": "present (exact generated file)",
        });
    }
    if let Some(expected_block) = extract_managed_text_block(expected)
        && let Some(actual_block) = extract_managed_text_block(&content)
    {
        if normalize_text_for_compare(&actual_block) == normalize_text_for_compare(&expected_block)
        {
            return json!({
                "path": path_text,
                "format": format,
                "status": "present_exact",
                "message": "present (exact managed block)",
            });
        }
        return json!({
            "path": path_text,
            "format": format,
            "status": "present_customized",
            "message": "present (customized managed block)",
        });
    }
    if first_significant_template_line(expected).is_some_and(|line| content.contains(line)) {
        return json!({
            "path": path_text,
            "format": format,
            "status": "present_customized",
            "message": "present (customized)",
        });
    }
    json!({
        "path": path_text,
        "format": format,
        "status": "manual_review_required",
        "message": "present but manual review required",
    })
}
