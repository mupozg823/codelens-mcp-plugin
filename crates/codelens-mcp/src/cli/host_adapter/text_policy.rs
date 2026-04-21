use super::render::normalize_text_for_compare;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

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

pub(super) fn first_significant_template_line(template: &str) -> Option<&str> {
    template
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```") && *line != "---")
}

pub(super) fn extract_managed_text_block(text: &str) -> Option<String> {
    const BEGIN: &str = "<!-- CODELENS_HOST_ROUTING:BEGIN -->";
    const END: &str = "<!-- CODELENS_HOST_ROUTING:END -->";

    let start = text.find(BEGIN)?;
    let end = text[start..].find(END)? + start + END.len();
    Some(text[start..end].to_owned())
}

pub(super) fn inspect_text_policy_file(path: &Path, expected: &str, format: &str) -> String {
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

pub(super) fn inspect_text_policy_file_json(path: &Path, expected: &str, format: &str) -> Value {
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
