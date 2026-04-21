use serde_json::{Value, json};
use std::fs;
use std::path::Path;

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

pub(super) fn remove_json_key(value: &mut Value, parent_path: &[String], key: &str) -> bool {
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

pub(super) fn prune_empty_json(value: &mut Value) -> bool {
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
