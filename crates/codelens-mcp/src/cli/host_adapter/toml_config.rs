use super::render::normalize_text_for_compare;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

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
