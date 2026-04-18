//! CLI argument parsing + project-root resolution + HTTP startup banner.
//!
//! Extracted from `main.rs` as of v1.9.32 to keep the binary entry point
//! focused on bootstrap/dispatch. All parsing functions are pure and test
//! co-located below.

use crate::state::RuntimeDaemonMode;
use crate::surface_manifest::HOST_ADAPTER_HOSTS;
use anyhow::{Context, Result};
use codelens_engine::ProjectRoot;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// Where the startup project root came from, in priority order. Used for
/// diagnostic banners and the "refusing to start on `/` without explicit
/// project root" guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StartupProjectSource {
    Cli(String),
    ClaudeEnv(String),
    McpEnv(String),
    Cwd(PathBuf),
}

impl StartupProjectSource {
    pub(crate) fn is_explicit(&self) -> bool {
        !matches!(self, Self::Cwd(_))
    }

    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Cli(_) => "CLI path",
            Self::ClaudeEnv(_) => "CLAUDE_PROJECT_DIR",
            Self::McpEnv(_) => "MCP_PROJECT_DIR",
            Self::Cwd(_) => "current working directory",
        }
    }
}

/// Flags that consume the next argument as their value. Used by the
/// positional-project-arg parser to skip over `--flag value` pairs without
/// treating `value` as the project path.
fn flag_takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "--preset" | "--profile" | "--daemon-mode" | "--cmd" | "--args" | "--transport" | "--port"
    )
}

pub(crate) fn is_attach_subcommand(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("attach"))
}

pub(crate) fn is_detach_subcommand(args: &[String]) -> bool {
    matches!(args.get(1).map(String::as_str), Some("detach"))
}

pub(crate) fn is_doctor_subcommand(args: &[String]) -> bool {
    matches!(
        args.get(1).map(String::as_str),
        Some("doctor") | Some("status")
    )
}

pub(crate) fn attach_host_arg(args: &[String]) -> Option<String> {
    args.get(2).cloned()
}

fn canonical_attach_host(host: &str) -> Option<&'static str> {
    match host.to_ascii_lowercase().as_str() {
        "claude" | "claude-code" | "claude_code" | "claudecode" => Some("claude-code"),
        "codex" => Some("codex"),
        "cursor" => Some("cursor"),
        "cline" => Some("cline"),
        "windsurf" | "codeium" => Some("windsurf"),
        _ => None,
    }
}

fn supported_attach_hosts() -> &'static str {
    "claude-code, codex, cursor, cline, windsurf"
}

fn home_dir_from_env() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set; cannot resolve host-native config paths")
}

fn resolve_host_path(raw: &str, home: &Path, cwd: &Path) -> PathBuf {
    if raw == "~" {
        home.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        home.join(rest)
    } else {
        cwd.join(raw)
    }
}

fn json_string_list(value: &Value, key: &str) -> Vec<String> {
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

fn append_host_adapter_common_metadata(out: &mut String, adapter: &Value, prefix: &str) {
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

fn append_host_adapter_attach_guidance(out: &mut String, adapter: &Value) {
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

fn host_adapter_common_metadata_json(adapter: &Value) -> Value {
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

fn render_template(template: &Value) -> Result<String> {
    if let Some(text) = template.as_str() {
        Ok(text.to_owned())
    } else {
        serde_json::to_string_pretty(template).context("failed to render template as JSON")
    }
}

fn normalize_text_for_compare(text: &str) -> String {
    text.replace("\r\n", "\n").trim_end().to_owned()
}

fn parse_json_route_from_template(template: &Value) -> Option<(Vec<String>, String)> {
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

fn get_json_key<'a>(value: &'a Value, parent_path: &[String], key: &str) -> Option<&'a Value> {
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

fn remove_json_config_entry(
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

fn extract_toml_section_name(template: &str) -> Option<String> {
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

fn remove_toml_section(path: &Path, section: &str, apply_changes: bool) -> String {
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

fn remove_exact_text_file(path: &Path, expected: &str, label: &str, apply_changes: bool) -> String {
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

fn has_toml_section(content: &str, section: &str) -> bool {
    let header = format!("[{section}]");
    content.lines().any(|line| line.trim() == header)
}

fn first_significant_template_line(template: &str) -> Option<&str> {
    template
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("```") && *line != "---")
}

fn inspect_json_config_entry(path: &Path, template: &Value) -> String {
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

fn inspect_json_config_entry_json(path: &Path, template: &Value) -> Value {
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

fn inspect_toml_section(path: &Path, template: &str) -> String {
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
    if normalize_text_for_compare(&content) == normalize_text_for_compare(template) {
        format!("- {display} [toml]: attached (exact generated file)")
    } else {
        format!("- {display} [toml]: attached (CodeLens section present)")
    }
}

fn inspect_toml_section_json(path: &Path, template: &str) -> Value {
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
    if normalize_text_for_compare(&content) == normalize_text_for_compare(template) {
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

fn inspect_text_policy_file(path: &Path, expected: &str, format: &str) -> String {
    let display = path.display();
    let Ok(content) = fs::read_to_string(path) else {
        return format!("- {display} [{format}]: missing");
    };
    if normalize_text_for_compare(&content) == normalize_text_for_compare(expected) {
        return format!("- {display} [{format}]: present (exact generated file)");
    }
    if first_significant_template_line(expected).is_some_and(|line| content.contains(line)) {
        return format!("- {display} [{format}]: present (customized)");
    }
    format!("- {display} [{format}]: present but manual review required")
}

fn inspect_text_policy_file_json(path: &Path, expected: &str, format: &str) -> Value {
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

fn inspect_host_file(path: &Path, format: &str, template: Option<&Value>) -> String {
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

fn inspect_host_file_json(path: &Path, format: &str, template: Option<&Value>) -> Value {
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

fn detach_host_files(
    host: &str,
    home: &Path,
    cwd: &Path,
    apply_changes: bool,
) -> Result<Vec<String>> {
    let adapter = crate::surface_manifest::host_adapter_bundle(host)
        .context("missing host adapter bundle for detach target")?;
    let native_files = adapter
        .get("native_files")
        .and_then(Value::as_array)
        .context("host adapter bundle is missing native_files")?;

    let mut notes = vec![format!("{host}:")];
    for file in native_files {
        let raw_path = file
            .get("path")
            .and_then(Value::as_str)
            .context("native file entry is missing path")?;
        let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
        let path = resolve_host_path(raw_path, home, cwd);
        let template = file.get("template");

        let note = match format {
            "json" => match template {
                Some(template) => match parse_json_route_from_template(template) {
                    Some((parent_path, key)) => remove_json_config_entry(
                        &path,
                        &parent_path,
                        &key,
                        "unsupported JSON shape",
                        apply_changes,
                    ),
                    None => format!(
                        "- {}: manual cleanup required (unsupported JSON template shape)",
                        path.display()
                    ),
                },
                None => format!(
                    "- {}: manual cleanup required (missing template)",
                    path.display()
                ),
            },
            "toml" => match template
                .and_then(Value::as_str)
                .and_then(extract_toml_section_name)
            {
                Some(section) => remove_toml_section(&path, &section, apply_changes),
                None => format!(
                    "- {}: manual cleanup required (missing TOML section template)",
                    path.display()
                ),
            },
            "markdown" | "mdc" => match template.and_then(Value::as_str) {
                Some(expected) => remove_exact_text_file(&path, expected, format, apply_changes),
                None => format!(
                    "- {}: manual cleanup required (missing text template)",
                    path.display()
                ),
            },
            other => format!(
                "- {}: manual cleanup required (unsupported format `{other}`)",
                path.display()
            ),
        };
        notes.push(note);
    }

    Ok(notes)
}

fn parse_detach_hosts(args: &[String]) -> Result<Vec<&'static str>> {
    let tail = &args[2..];
    if tail.is_empty() {
        anyhow::bail!(
            "usage: codelens-mcp detach <host>\n       codelens-mcp detach --all\nsupported hosts: {}",
            supported_attach_hosts()
        );
    }
    if tail.iter().any(|arg| arg == "--all" || arg == "all") {
        return Ok(HOST_ADAPTER_HOSTS.into_iter().collect());
    }

    let requested = tail[0].as_str();
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown detach host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    Ok(vec![canonical])
}

fn parse_doctor_hosts(args: &[String]) -> Result<Vec<&'static str>> {
    let filtered = args[2..]
        .iter()
        .filter(|arg| arg.as_str() != "--json")
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        anyhow::bail!(
            "usage: codelens-mcp doctor <host>\n       codelens-mcp doctor --all\nsupported hosts: {}",
            supported_attach_hosts()
        );
    }
    if filtered
        .iter()
        .any(|arg| arg.as_str() == "--all" || arg.as_str() == "all")
    {
        return Ok(HOST_ADAPTER_HOSTS.into_iter().collect());
    }

    let requested = filtered
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str())
        .context(format!(
            "usage: codelens-mcp doctor <host>\n       codelens-mcp doctor --all\nsupported hosts: {}",
            supported_attach_hosts()
        ))?;
    let canonical = canonical_attach_host(requested).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown doctor host `{requested}`\nsupported hosts: {}",
            supported_attach_hosts()
        )
    })?;
    Ok(vec![canonical])
}

fn detach_is_dry_run(args: &[String]) -> bool {
    args[2..].iter().any(|arg| arg == "--dry-run")
}

fn render_detach_report(
    hosts: &[&str],
    home: &Path,
    cwd: &Path,
    apply_changes: bool,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("CodeLens detach report\n");
    if apply_changes {
        out.push_str("Machine-editable config files are cleaned automatically.\n");
    } else {
        out.push_str("Dry run only. No files were modified.\n");
    }
    out.push_str(
        "Modified policy markdown files are left in place and reported for manual cleanup.\n\n",
    );

    for (index, host) in hosts.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let adapter = crate::surface_manifest::host_adapter_bundle(host)
            .context("missing host adapter bundle for detach report")?;

        out.push_str(&format!("{host}:\n"));
        append_host_adapter_common_metadata(&mut out, &adapter, "- ");
        for line in detach_host_files(host, home, cwd, apply_changes)? {
            if line == format!("{host}:") {
                continue;
            }
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.push_str("\nManual follow-up:\n");
    out.push_str("- Stop any running `codelens-mcp --transport http` daemons if you no longer want the shared server.\n");
    out.push_str(
        "- Remove repo-local `.codelens/` only if you also want to discard cached runtime state.\n",
    );
    out.push_str("- Remove the binary with your install channel: `brew uninstall codelens-mcp`, `cargo uninstall codelens-mcp`, or delete the installed executable path.\n");
    Ok(out)
}

pub(crate) fn run_detach_command(args: &[String]) -> Result<String> {
    let hosts = parse_detach_hosts(args)?;
    let home = home_dir_from_env()?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    render_detach_report(&hosts, &home, &cwd, !detach_is_dry_run(args))
}

fn render_doctor_report(command: &str, hosts: &[&str], home: &Path, cwd: &Path) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format!("CodeLens {command} report\n"));
    out.push_str(
        "Host checks reuse canonical host-adapter metadata plus local filesystem state.\n\n",
    );

    for (index, host) in hosts.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let adapter = crate::surface_manifest::host_adapter_bundle(host)
            .context("missing host adapter bundle for doctor report")?;
        let native_files = adapter
            .get("native_files")
            .and_then(Value::as_array)
            .context("host adapter bundle is missing native_files")?;

        out.push_str(&format!("{host}:\n"));
        append_host_adapter_common_metadata(&mut out, &adapter, "- ");
        for file in native_files {
            let raw_path = file
                .get("path")
                .and_then(Value::as_str)
                .context("native file entry is missing path")?;
            let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
            let path = resolve_host_path(raw_path, home, cwd);
            let template = file.get("template");
            out.push_str(&inspect_host_file(&path, format, template));
            out.push('\n');
        }
    }

    out.push_str("\nInterpretation:\n");
    out.push_str("- `attached` means a machine-readable CodeLens config stanza or section is currently detectable.\n");
    out.push_str("- `present (customized)` means the policy file exists and still resembles the generated template, but has local edits.\n");
    out.push_str("- `manual review required` means the file exists but the lightweight checker cannot prove alignment.\n");
    Ok(out)
}

fn doctor_report_json(command: &str, hosts: &[&str], home: &Path, cwd: &Path) -> Result<Value> {
    let host_reports = hosts
        .iter()
        .map(|host| {
            let adapter = crate::surface_manifest::host_adapter_bundle(host)
                .context("missing host adapter bundle for doctor report")?;
            let native_files = adapter
                .get("native_files")
                .and_then(Value::as_array)
                .context("host adapter bundle is missing native_files")?;
            let files = native_files
                .iter()
                .map(|file| {
                    let raw_path = file
                        .get("path")
                        .and_then(Value::as_str)
                        .context("native file entry is missing path")?;
                    let format = file.get("format").and_then(Value::as_str).unwrap_or("text");
                    let path = resolve_host_path(raw_path, home, cwd);
                    let template = file.get("template");
                    Ok(inspect_host_file_json(&path, format, template))
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(json!({
                "host": host,
                "metadata": host_adapter_common_metadata_json(&adapter),
                "files": files,
            }))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "command": command,
        "hosts": host_reports,
        "legend": {
            "attached_exact": "machine-readable CodeLens config matches the generated entry exactly",
            "attached_customized": "machine-readable CodeLens config is present but differs from the generated entry",
            "present_exact": "text policy file matches the generated template exactly",
            "present_customized": "text policy file exists and still resembles the generated template, but has local edits",
            "manual_review_required": "the lightweight checker cannot prove alignment",
            "missing": "file is absent"
        }
    }))
}

pub(crate) fn run_doctor_command(args: &[String]) -> Result<String> {
    let command = args.get(1).map(String::as_str).unwrap_or("doctor");
    let hosts = parse_doctor_hosts(args)?;
    let home = home_dir_from_env()?;
    let cwd = std::env::current_dir().context("failed to resolve current working directory")?;
    if args[2..].iter().any(|arg| arg == "--json") {
        return serde_json::to_string_pretty(&doctor_report_json(command, &hosts, &home, &cwd)?)
            .context("failed to render doctor report as JSON");
    }
    render_doctor_report(command, &hosts, &home, &cwd)
}

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
    let adapter = crate::surface_manifest::host_adapter_bundle(canonical)
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
    out.push_str("The default daemon URL assumes `http://127.0.0.1:7837/mcp`.\n");
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

/// Locate the positional project argument, skipping known `--flag value`
/// pairs and `--flag=value` forms. `--` terminates flag parsing.
pub(crate) fn parse_cli_project_arg(args: &[String]) -> Option<String> {
    let mut skip_next = false;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        let value = arg.as_str();
        if skip_next {
            skip_next = false;
            continue;
        }
        if value == "--" {
            return iter.next().map(|entry| entry.to_string());
        }
        if let Some((flag, _)) = value.split_once('=')
            && flag_takes_value(flag)
        {
            continue;
        }
        if flag_takes_value(value) {
            skip_next = true;
            continue;
        }
        if value.starts_with('-') {
            continue;
        }
        return Some(value.to_string());
    }
    None
}

/// Resolve the authoritative project-root *source* in the documented
/// priority order: explicit CLI arg → `CLAUDE_PROJECT_DIR` →
/// `MCP_PROJECT_DIR` → current working directory.
pub(crate) fn select_startup_project_source(
    args: &[String],
    claude_project_dir: Option<String>,
    mcp_project_dir: Option<String>,
    cwd: PathBuf,
) -> StartupProjectSource {
    if let Some(path) = parse_cli_project_arg(args) {
        StartupProjectSource::Cli(path)
    } else if let Some(path) = claude_project_dir {
        StartupProjectSource::ClaudeEnv(path)
    } else if let Some(path) = mcp_project_dir {
        StartupProjectSource::McpEnv(path)
    } else {
        StartupProjectSource::Cwd(cwd)
    }
}

/// Resolve a [`StartupProjectSource`] into a concrete [`ProjectRoot`]. Fails
/// closed when an explicit source points at a path that cannot be resolved.
pub(crate) fn resolve_startup_project(source: &StartupProjectSource) -> Result<ProjectRoot> {
    match source {
        StartupProjectSource::Cli(path)
        | StartupProjectSource::ClaudeEnv(path)
        | StartupProjectSource::McpEnv(path) => ProjectRoot::new(path).with_context(|| {
            format!(
                "failed to resolve explicit project root from {}",
                source.label()
            )
        }),
        StartupProjectSource::Cwd(path) => ProjectRoot::new(path)
            .with_context(|| format!("failed to resolve project root from {}", path.display())),
    }
}

/// Extract the value of `--flag <value>` or `--flag=<value>` from an argv
/// slice. `--` terminates flag scanning. Returns `None` if the flag is
/// absent, or when `--flag` appears as the last argument without a value.
pub(crate) fn cli_option_value(args: &[String], flag: &str) -> Option<String> {
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--" {
            break;
        }
        if let Some(value) = arg.strip_prefix(&format!("{flag}=")) {
            return Some(value.to_owned());
        }
        if arg == flag {
            return iter.next().cloned();
        }
    }
    None
}

/// Phase 4c (§observability): emit a single-line startup marker at
/// `warn` level so append-only log files (e.g. launchd's
/// `~/.codex/codelens-http.log`) have an explicit session boundary
/// between historical noise and the current run. Includes every
/// identity field a debugger might want: `pid`, `transport`, `port`,
/// `project_root`, `project_source` (CLI path / env var / cwd),
/// `surface`, `token_budget`, `daemon_mode`, and the build-time
/// identity fields introduced in Phase 4b (`git_sha`, `build_time`,
/// `git_dirty`) plus the wall-clock `daemon_started_at`.
///
/// `warn!` level is intentional: the default `CODELENS_LOG` filter
/// is `warn`, so session-start markers are visible without users
/// having to opt into `info` logging.
#[cfg_attr(not(feature = "http"), allow(dead_code))]
pub(crate) fn format_http_startup_banner(
    project_root: &std::path::Path,
    project_source: &StartupProjectSource,
    surface_label: &str,
    token_budget: usize,
    daemon_mode: RuntimeDaemonMode,
    port: u16,
    daemon_started_at: &str,
) -> String {
    let escaped_project_root = project_root.display().to_string().replace('"', "\\\"");
    format!(
        "CODELENS_SESSION_START pid={} transport=http port={} project_root=\"{}\" project_source=\"{}\" surface={} token_budget={} daemon_mode={} git_sha={} build_time={} daemon_started_at={} git_dirty={}",
        std::process::id(),
        port,
        escaped_project_root,
        project_source.label(),
        surface_label,
        token_budget,
        daemon_mode.as_str(),
        crate::build_info::BUILD_GIT_SHA,
        crate::build_info::BUILD_TIME,
        daemon_started_at,
        crate::build_info::build_git_dirty()
    )
}

#[cfg(test)]
mod startup_tests {
    use super::{
        StartupProjectSource, canonical_attach_host, parse_cli_project_arg, parse_detach_hosts,
        parse_doctor_hosts, render_attach_instructions, render_detach_report, render_doctor_report,
        resolve_startup_project, run_doctor_command,
    };

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-startup-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cli_project_arg_skips_flag_values() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--transport".to_owned(),
            "http".to_owned(),
            "--profile".to_owned(),
            "reviewer-graph".to_owned(),
            "/tmp/repo".to_owned(),
        ];
        assert_eq!(parse_cli_project_arg(&args).as_deref(), Some("/tmp/repo"));
    }

    #[test]
    fn cli_project_arg_honors_double_dash_separator() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--transport".to_owned(),
            "http".to_owned(),
            "--".to_owned(),
            ".".to_owned(),
        ];
        assert_eq!(parse_cli_project_arg(&args).as_deref(), Some("."));
    }

    #[test]
    fn cli_project_arg_skips_equals_syntax_flags() {
        let args = vec![
            "codelens-mcp".to_owned(),
            "--transport=http".to_owned(),
            "--port=7842".to_owned(),
            "/tmp/repo".to_owned(),
        ];
        assert_eq!(parse_cli_project_arg(&args).as_deref(), Some("/tmp/repo"));
    }

    #[test]
    fn explicit_project_resolution_fails_closed() {
        let missing = temp_dir("missing-parent").join("does-not-exist");
        let source = StartupProjectSource::Cli(missing.to_string_lossy().to_string());
        let error = resolve_startup_project(&source).expect_err("missing explicit path must fail");
        assert!(
            error
                .to_string()
                .contains("failed to resolve explicit project root")
        );
    }

    #[test]
    fn attach_host_aliases_normalize_to_canonical_host_ids() {
        assert_eq!(canonical_attach_host("claude"), Some("claude-code"));
        assert_eq!(canonical_attach_host("claudecode"), Some("claude-code"));
        assert_eq!(canonical_attach_host("codeium"), Some("windsurf"));
    }

    #[test]
    fn render_attach_instructions_for_codex_emits_copy_ready_targets() {
        let rendered = render_attach_instructions(Some("codex")).expect("attach output");
        assert!(rendered.contains("CodeLens attach target: codex"));
        assert!(rendered.contains("Native host primitives:"));
        assert!(rendered.contains("Use CodeLens for:"));
        assert!(rendered.contains("Avoid:"));
        assert!(rendered.contains("Primary bootstrap sequence:"));
        assert!(rendered.contains("Delegate scaffold contract:"));
        assert!(rendered.contains("Compiled overlays:"));
        assert!(rendered.contains("## Compiled Routing Overlays"));
        assert!(rendered.contains("delegate_to_codex_builder"));
        assert!(rendered.contains("handoff_id"));
        assert!(rendered.contains("~/.codex/config.toml"));
        assert!(rendered.contains("AGENTS.md"));
        assert!(rendered.contains("worktrees"));
        assert!(rendered.contains("analysis jobs for CI-facing summaries"));
        assert!(
            rendered
                .contains("copying Claude-specific subagent topology into Codex worktree flows")
        );
        assert!(rendered.contains("Verify the host wiring with `codelens-mcp doctor codex`"));
        assert!(rendered.contains("builder-minimal / editing"));
        assert!(rendered.contains("builder-minimal"));
        assert!(rendered.contains("refactor-full"));
    }

    #[test]
    fn render_attach_instructions_for_cursor_surfaces_delegate_handoff_contract() {
        let rendered = render_attach_instructions(Some("cursor")).expect("attach output");
        assert!(rendered.contains("CodeLens attach target: cursor"));
        assert!(rendered.contains("Native host primitives:"));
        assert!(rendered.contains("background agents"));
        assert!(rendered.contains("Use CodeLens for:"));
        assert!(rendered.contains("analysis jobs for background-agent queues"));
        assert!(rendered.contains("Avoid:"));
        assert!(rendered.contains("shipping the full CodeLens surface into every mode"));
        assert!(rendered.contains("Primary bootstrap sequence:"));
        assert!(rendered.contains("Delegate scaffold contract:"));
        assert!(rendered.contains("Compiled overlays:"));
        assert!(rendered.contains("## Compiled Routing Overlays"));
        assert!(rendered.contains("delegate_to_codex_builder"));
        assert!(rendered.contains("handoff_id"));
        assert!(rendered.contains(".cursor/rules/codelens-routing.mdc"));
    }

    #[test]
    fn render_attach_instructions_accepts_windsurf_aliases() {
        let rendered = render_attach_instructions(Some("codeium")).expect("attach output");
        assert!(rendered.contains("CodeLens attach target: windsurf"));
        assert!(rendered.contains("Requested alias: codeium -> windsurf"));
        assert!(rendered.contains("~/.codeium/windsurf/mcp_config.json"));
    }

    #[test]
    fn render_attach_instructions_rejects_unknown_hosts() {
        let error =
            render_attach_instructions(Some("openhands")).expect_err("unknown host must fail");
        assert!(error.to_string().contains("unknown attach host"));
    }

    #[test]
    fn detach_report_removes_codelens_json_entry_and_keeps_other_servers() {
        let root = temp_dir("detach-json");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" },
                    "other": { "type": "http", "url": "http://127.0.0.1:9999/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let report = render_detach_report(&["cursor"], &home, &cwd, true).expect("detach report");
        let updated = std::fs::read_to_string(cwd.join(".cursor/mcp.json")).unwrap();
        assert!(report.contains("Adapter resource: codelens://host-adapters/cursor"));
        assert!(report.contains("Preferred profiles: planner-readonly, reviewer-graph, ci-audit"));
        assert!(report.contains("Host-native targets: .cursor/rules, AGENTS.md, .cursor/mcp.json, background-agent environment.json"));
        assert!(report.contains("removed CodeLens config entry"));
        assert!(!updated.contains("\"codelens\""));
        assert!(updated.contains("\"other\""));
    }

    #[test]
    fn detach_report_removes_codelens_toml_section_only() {
        let root = temp_dir("detach-toml");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            home.join(".codex/config.toml"),
            r#"[mcp_servers.codelens]
url = "http://127.0.0.1:7837/mcp"

[mcp_servers.other]
url = "http://127.0.0.1:9999/mcp"
"#,
        )
        .unwrap();

        let report = render_detach_report(&["codex"], &home, &cwd, true).expect("detach report");
        let updated = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(report.contains("Adapter resource: codelens://host-adapters/codex"));
        assert!(report.contains("Native host primitives: AGENTS.md, skills, worktrees, shared MCP config, CLI, app, and IDE continuity"));
        assert!(report.contains(
            "Host-native targets: AGENTS.md, ~/.codex/config.toml, repo-local skill files"
        ));
        assert!(report.contains("removed CodeLens TOML section"));
        assert!(!updated.contains("[mcp_servers.codelens]"));
        assert!(updated.contains("[mcp_servers.other]"));
    }

    #[test]
    fn detach_report_requires_manual_cleanup_for_modified_policy_file() {
        let root = temp_dir("detach-manual");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(cwd.join("AGENTS.md"), "# CodeLens Routing\n\ncustomized\n").unwrap();

        let report = render_detach_report(&["codex"], &home, &cwd, true).expect("detach report");
        assert!(report.contains("manual cleanup required"));
        assert!(cwd.join("AGENTS.md").exists());
    }

    #[test]
    fn detach_cli_accepts_all_flag() {
        let hosts = parse_detach_hosts(&[
            "codelens-mcp".to_owned(),
            "detach".to_owned(),
            "--all".to_owned(),
        ])
        .expect("detach hosts");

        assert!(hosts.contains(&"claude-code"));
        assert!(hosts.contains(&"codex"));
        assert!(hosts.contains(&"windsurf"));
    }

    #[test]
    fn doctor_report_detects_machine_attachment_and_customized_policy_file() {
        let root = temp_dir("doctor-host");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(cwd.join(".cursor/rules")).unwrap();
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" },
                    "other": { "type": "http", "url": "http://127.0.0.1:9999/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            cwd.join(".cursor/rules/codelens-routing.mdc"),
            "---\ndescription: Route CodeLens usage by task risk and phase\nalwaysApply: true\n---\n\nCustomized locally.\n",
        )
        .unwrap();

        let report =
            render_doctor_report("doctor", &["cursor"], &home, &cwd).expect("doctor report");
        assert!(report.contains("CodeLens doctor report"));
        assert!(report.contains("Adapter resource: codelens://host-adapters/cursor"));
        assert!(report.contains(".cursor/mcp.json [json]: attached (exact CodeLens entry)"));
        assert!(report.contains(".cursor/rules/codelens-routing.mdc [mdc]: present (customized)"));
        assert!(report.contains("Interpretation:"));
    }

    #[test]
    fn doctor_report_marks_missing_codex_files() {
        let root = temp_dir("doctor-missing");
        let home = root.join("home");
        let cwd = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        let report =
            render_doctor_report("doctor", &["codex"], &home, &cwd).expect("doctor report");
        assert!(report.contains(".codex/config.toml [toml]: missing"));
        assert!(report.contains("AGENTS.md [markdown]: missing"));
    }

    #[test]
    fn doctor_cli_accepts_all_flag() {
        let hosts = parse_doctor_hosts(&[
            "codelens-mcp".to_owned(),
            "doctor".to_owned(),
            "--all".to_owned(),
        ])
        .expect("doctor hosts");

        assert!(hosts.contains(&"claude-code"));
        assert!(hosts.contains(&"codex"));
        assert!(hosts.contains(&"windsurf"));
    }

    #[test]
    fn doctor_cli_accepts_json_flag_before_host() {
        let hosts = parse_doctor_hosts(&[
            "codelens-mcp".to_owned(),
            "doctor".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("doctor hosts");
        assert_eq!(hosts, vec!["cursor"]);
    }

    #[test]
    fn run_doctor_command_renders_json_report() {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = temp_dir("doctor-json");
        let home = root.join("home");
        let cwd = root.join("repo");
        let previous_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", &home);
        }
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        let rendered = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "doctor".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("doctor json");
        std::env::set_current_dir(previous).unwrap();
        unsafe {
            match previous_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }

        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("valid doctor json");
        assert_eq!(payload["command"], serde_json::json!("doctor"));
        assert_eq!(payload["hosts"][0]["host"], serde_json::json!("cursor"));
        assert_eq!(
            payload["hosts"][0]["metadata"]["resource_uri"],
            serde_json::json!("codelens://host-adapters/cursor")
        );
        assert_eq!(
            payload["hosts"][0]["files"][0]["status"],
            serde_json::json!("attached_exact")
        );
    }

    #[test]
    fn run_status_command_renders_status_alias_in_text_and_json() {
        let _guard = crate::env_compat::TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let root = temp_dir("status-json");
        let home = root.join("home");
        let cwd = root.join("repo");
        let previous_home = std::env::var("HOME").ok();
        unsafe {
            std::env::set_var("HOME", &home);
        }
        std::fs::create_dir_all(cwd.join(".cursor")).unwrap();
        std::fs::write(
            cwd.join(".cursor/mcp.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&cwd).unwrap();
        let text = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "status".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("status text");
        let rendered = run_doctor_command(&[
            "codelens-mcp".to_owned(),
            "status".to_owned(),
            "--json".to_owned(),
            "cursor".to_owned(),
        ])
        .expect("status json");
        std::env::set_current_dir(previous).unwrap();
        unsafe {
            match previous_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }

        assert!(text.contains("CodeLens status report"));
        let payload: serde_json::Value =
            serde_json::from_str(&rendered).expect("valid status json");
        assert_eq!(payload["command"], serde_json::json!("status"));
    }

    /// Phase 4c (§observability): the startup banner must carry
    /// every identity field a debugger might want in a single line,
    /// so append-only log tails can pinpoint "which build, which
    /// process, which project" without cross-referencing other
    /// state. Guards the format string against accidental field
    /// removal.
    #[test]
    fn http_startup_banner_includes_runtime_identity_fields() {
        let banner = super::format_http_startup_banner(
            std::path::Path::new("/tmp/repo"),
            &StartupProjectSource::McpEnv("/tmp/repo".to_owned()),
            "builder-minimal",
            2400,
            crate::state::RuntimeDaemonMode::Standard,
            7837,
            "2026-04-11T19:49:55Z",
        );
        assert!(banner.starts_with("CODELENS_SESSION_START pid="));
        assert!(banner.contains("transport=http"));
        assert!(banner.contains("port=7837"));
        assert!(banner.contains("project_root=\"/tmp/repo\""));
        assert!(banner.contains("project_source=\"MCP_PROJECT_DIR\""));
        assert!(banner.contains("surface=builder-minimal"));
        assert!(banner.contains("token_budget=2400"));
        assert!(banner.contains("daemon_mode=standard"));
        assert!(banner.contains("daemon_started_at=2026-04-11T19:49:55Z"));
        assert!(banner.contains("git_sha="));
        assert!(banner.contains("build_time="));
        assert!(banner.contains("git_dirty="));
    }
}
