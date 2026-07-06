use serde_json::Value;

const MAX_SNAPSHOT_ITEMS: usize = 64;

pub(super) fn first_string_array(
    arguments: &Value,
    primary_key: &str,
    session_key: &str,
) -> Vec<String> {
    string_array_field(arguments, primary_key)
        .or_else(|| string_array_field(arguments, session_key))
        .unwrap_or_default()
}

pub(super) fn first_string_field(
    arguments: &Value,
    primary_key: &str,
    session_key: &str,
) -> Option<String> {
    string_field(arguments, primary_key).or_else(|| string_field(arguments, session_key))
}

fn string_array_field(arguments: &Value, key: &str) -> Option<Vec<String>> {
    let items = arguments.get(key)?.as_array()?;
    let mut values = Vec::new();
    for item in items {
        let Some(value) = item
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !values.iter().any(|existing| existing == value) {
            values.push(value.to_owned());
        }
        if values.len() >= MAX_SNAPSHOT_ITEMS {
            break;
        }
    }
    Some(values)
}

fn string_field(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
