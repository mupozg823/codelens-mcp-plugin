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

/// #347: stamp the project's absolute path into the codelens server
/// entry as an `x-codelens-project` header so every session the host
/// opens binds to this workspace at initialize — no agent round trip.
fn set_codelens_json_template_project_header(template: &mut Value, project_root: &Path) -> bool {
    for pointer in ["/mcpServers/codelens", "/servers/codelens", "/codelens"] {
        if let Some(server) = template.pointer_mut(pointer)
            && let Some(object) = server.as_object_mut()
        {
            let headers = object
                .entry("headers".to_owned())
                .or_insert_with(|| json!({}));
            if let Some(map) = headers.as_object_mut() {
                map.insert(
                    "x-codelens-project".to_owned(),
                    json!(project_root.to_string_lossy()),
                );
                return true;
            }
        }
    }
    false
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

/// #353: stamp the workspace into the codex TOML template as
/// `http_headers = { "x-codelens-project" = ... }` — officially supported
/// by codex `config.toml` for streamable-HTTP MCP servers — so codex
/// sessions bind at initialize and survive eviction exactly like the
/// JSON-template hosts (#347/#351). Replaces an existing `http_headers`
/// line, otherwise inserts one right after `url = ` in the codelens
/// section.
fn set_codelens_toml_template_project_header(template: &str, project_root: &Path) -> String {
    let escaped = project_root
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let header_line = format!(r#"http_headers = {{ "x-codelens-project" = "{escaped}" }}"#);

    let mut in_codelens_section = false;
    let mut stamped = false;
    let mut lines = Vec::new();
    for line in template.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_codelens_section = trimmed == "[mcp_servers.codelens]";
            lines.push(line.to_owned());
            continue;
        }
        if in_codelens_section && trimmed.starts_with("http_headers = ") {
            let indent = &line[..line.len() - line.trim_start().len()];
            lines.push(format!("{indent}{header_line}"));
            stamped = true;
            continue;
        }
        lines.push(line.to_owned());
        if in_codelens_section && !stamped && line.trim().starts_with("url = ") {
            let indent = &line[..line.len() - line.trim_start().len()];
            lines.push(format!("{indent}{header_line}"));
            stamped = true;
        }
    }

    if !stamped {
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
    let url_override = project_host_attach_url(project_root, host);

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
                    if let Some(url) = url_override.as_deref() {
                        let _ = set_codelens_json_template_url(template, url);
                    }
                    // #347: always bind the attach target's workspace.
                    let _ = set_codelens_json_template_project_header(template, project_root);
                }
                "toml" => {
                    if let Some(url) = url_override.as_deref()
                        && let Some(text) = template.as_str()
                    {
                        *template = Value::String(set_codelens_toml_template_url(text, url));
                    }
                    // #353: always bind the attach target's workspace —
                    // same contract as the JSON templates above.
                    if let Some(text) = template.as_str() {
                        *template = Value::String(set_codelens_toml_template_project_header(
                            text,
                            project_root,
                        ));
                    }
                }
                _ => {}
            }
        }
    }

    let Some(url) = url_override else {
        return;
    };
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

#[cfg(test)]
mod project_header_tests {
    #[test]
    fn attach_bundle_stamps_project_header_into_json_templates() {
        let tmp = std::env::temp_dir().join(format!("codelens-attach-hdr-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bundle =
            crate::surface_manifest::host_adapter_bundle_for_project("claude-code", Some(&tmp))
                .expect("claude-code bundle");
        let native_files = bundle["native_files"].as_array().expect("native_files");
        let mcp_json = native_files
            .iter()
            .find(|file| file["path"] == ".mcp.json")
            .expect(".mcp.json template");
        assert_eq!(
            mcp_json["template"]["mcpServers"]["codelens"]["headers"]["x-codelens-project"],
            serde_json::json!(tmp.to_string_lossy()),
            "attach must stamp the workspace into the binding header: {mcp_json}"
        );
    }

    #[test]
    fn attach_bundle_without_project_root_leaves_templates_unstamped() {
        let bundle = crate::surface_manifest::host_adapter_bundle_for_project("claude-code", None)
            .expect("claude-code bundle");
        let native_files = bundle["native_files"].as_array().expect("native_files");
        let mcp_json = native_files
            .iter()
            .find(|file| file["path"] == ".mcp.json")
            .expect(".mcp.json template");
        assert!(
            mcp_json["template"]["mcpServers"]["codelens"]
                .get("headers")
                .is_none(),
            "no project root → no header stamp: {mcp_json}"
        );
    }

    /// #353: the codex TOML template carries the same workspace binding
    /// as the JSON templates — via the officially supported
    /// `http_headers` inline table.
    #[test]
    fn attach_bundle_stamps_project_header_into_codex_toml_template() {
        let tmp = std::env::temp_dir().join(format!("codelens-attach-toml-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let bundle = crate::surface_manifest::host_adapter_bundle_for_project("codex", Some(&tmp))
            .expect("codex bundle");
        let native_files = bundle["native_files"].as_array().expect("native_files");
        let config_toml = native_files
            .iter()
            .find(|file| file["format"] == "toml")
            .expect("config.toml template");
        let template = config_toml["template"].as_str().expect("toml text");
        let expected = format!(
            r#"http_headers = {{ "x-codelens-project" = "{}" }}"#,
            tmp.to_string_lossy()
        );
        assert!(
            template.contains(&expected),
            "codex template must stamp the binding header: {template}"
        );
        assert!(
            template.find("[mcp_servers.codelens]").unwrap()
                < template.find("http_headers").unwrap(),
            "header line belongs to the codelens section: {template}"
        );
    }

    #[test]
    fn attach_bundle_without_project_root_leaves_codex_toml_unstamped() {
        let bundle = crate::surface_manifest::host_adapter_bundle_for_project("codex", None)
            .expect("codex bundle");
        let native_files = bundle["native_files"].as_array().expect("native_files");
        let config_toml = native_files
            .iter()
            .find(|file| file["format"] == "toml")
            .expect("config.toml template");
        assert!(
            !config_toml["template"]
                .as_str()
                .unwrap()
                .contains("http_headers"),
            "no project root → no header stamp"
        );
    }
}
