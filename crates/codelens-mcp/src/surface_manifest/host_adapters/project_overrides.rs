//! Project-local host attach URL overrides.

use serde_json::{Value, json};
use std::path::Path;

fn load_project_host_attach_config(project_root: &Path) -> Option<Value> {
    let path = project_root.join(".codelens/config.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn project_host_attach_url(project_root: &Path, host: &str) -> Option<String> {
    load_project_host_attach_config(project_root)?
        .get("host_attach")
        .and_then(|value| value.get("per_host_urls"))
        .and_then(|value| value.get(host))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn set_codelens_json_template_url(template: &mut Value, url: &str) -> bool {
    for pointer in ["/mcpServers/codelens", "/servers/codelens", "/codelens"] {
        if let Some(server) = template.pointer_mut(pointer)
            && let Some(object) = server.as_object_mut()
        {
            object.insert("url".to_owned(), json!(url));
            return true;
        }
    }
    false
}

fn set_codelens_toml_template_url(template: &str, url: &str) -> String {
    let mut in_codelens_section = false;
    let mut updated = false;
    let mut lines = Vec::new();

    for line in template.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_codelens_section = trimmed == "[mcp_servers.codelens]";
            lines.push(line.to_owned());
            continue;
        }

        if in_codelens_section && trimmed.starts_with("url = ") {
            let indent = &line[..line.len() - line.trim_start().len()];
            lines.push(format!(r#"{indent}url = "{url}""#));
            updated = true;
            continue;
        }

        lines.push(line.to_owned());
    }

    if !updated {
        return template.to_owned();
    }

    let mut rewritten = lines.join("\n");
    if template.ends_with('\n') {
        rewritten.push('\n');
    }
    rewritten
}

pub(super) fn apply_host_attach_project_overrides(
    host: &str,
    bundle: &mut Value,
    project_root: Option<&Path>,
) {
    let Some(project_root) = project_root else {
        return;
    };
    let Some(url) = project_host_attach_url(project_root, host) else {
        return;
    };

    if let Some(native_files) = bundle.get_mut("native_files").and_then(Value::as_array_mut) {
        for file in native_files {
            let format = file
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let Some(template) = file.get_mut("template") else {
                continue;
            };
            match format.as_str() {
                "json" => {
                    let _ = set_codelens_json_template_url(template, &url);
                }
                "toml" => {
                    if let Some(text) = template.as_str() {
                        *template = Value::String(set_codelens_toml_template_url(text, &url));
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(object) = bundle.as_object_mut() {
        object.insert("resolved_mcp_url".to_owned(), json!(url));
        object.insert(
            "resolved_mcp_url_source".to_owned(),
            json!(format!(
                ".codelens/config.json host_attach.per_host_urls.{host}"
            )),
        );
    }
}
