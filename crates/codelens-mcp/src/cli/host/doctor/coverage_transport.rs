use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Transport {
    Http {
        url: String,
        headers: BTreeMap<String, String>,
    },
    Stdio {
        command: String,
    },
}

pub(super) fn parse_transport(path: &Path, format: &str) -> Result<Transport, String> {
    let content = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    match format {
        "json" => parse_json_transport(&content),
        "toml" => parse_toml_transport(&content),
        other => Err(format!("unsupported machine-readable format `{other}`")),
    }
}

pub(super) fn parse_json_transport(content: &str) -> Result<Transport, String> {
    let payload = serde_json::from_str::<Value>(content)
        .map_err(|err| format!("failed to parse JSON config: {err}"))?;
    let entry = payload
        .get("mcpServers")
        .and_then(|servers| servers.get("codelens"))
        .or_else(|| {
            payload
                .get("servers")
                .and_then(|servers| servers.get("codelens"))
        })
        .or_else(|| payload.get("codelens"))
        .and_then(Value::as_object)
        .ok_or_else(|| "CodeLens entry not found in JSON config".to_owned())?;
    transport_from_json_entry(entry)
}

fn transport_from_json_entry(entry: &serde_json::Map<String, Value>) -> Result<Transport, String> {
    if let Some(url) = entry.get("url").and_then(Value::as_str) {
        return Ok(Transport::Http {
            url: url.to_owned(),
            headers: headers_from_json_entry(entry),
        });
    }
    if let Some(command) = entry.get("command").and_then(Value::as_str) {
        return Ok(Transport::Stdio {
            command: command.to_owned(),
        });
    }
    Err("CodeLens entry is missing both url and command".to_owned())
}

fn headers_from_json_entry(entry: &serde_json::Map<String, Value>) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for key in ["headers", "http_headers"] {
        if let Some(raw_headers) = entry.get(key).and_then(Value::as_object) {
            for (header, value) in raw_headers {
                if let Some(text) = value.as_str() {
                    headers.insert(header.to_owned(), text.to_owned());
                }
            }
        }
    }
    headers
}

pub(super) fn parse_toml_transport(content: &str) -> Result<Transport, String> {
    let payload = content
        .parse::<toml::Value>()
        .map_err(|err| format!("failed to parse TOML config: {err}"))?;
    let entry = payload
        .get("mcp_servers")
        .and_then(|servers| servers.get("codelens"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "CodeLens entry not found in TOML config".to_owned())?;
    if let Some(url) = entry.get("url").and_then(toml::Value::as_str) {
        return Ok(Transport::Http {
            url: url.to_owned(),
            headers: headers_from_toml_entry(entry),
        });
    }
    if let Some(command) = entry.get("command").and_then(toml::Value::as_str) {
        return Ok(Transport::Stdio {
            command: command.to_owned(),
        });
    }
    Err("CodeLens TOML entry is missing both url and command".to_owned())
}

fn headers_from_toml_entry(
    entry: &toml::map::Map<String, toml::Value>,
) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    for key in ["headers", "http_headers"] {
        if let Some(raw_headers) = entry.get(key).and_then(toml::Value::as_table) {
            for (header, value) in raw_headers {
                if let Some(text) = value.as_str() {
                    headers.insert(header.to_owned(), text.to_owned());
                }
            }
        }
    }
    headers
}
