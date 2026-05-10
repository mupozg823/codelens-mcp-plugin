use serde_json::{Value, json};

pub(crate) fn json_resource(uri: &str, payload: Value) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/json",
            "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
        }]
    })
}

pub(crate) fn schema_resource(uri: &str, payload: Value) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "application/schema+json",
            "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
        }]
    })
}

pub(crate) fn text_resource(uri: &str, text: String) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": "text/plain",
            "text": text
        }]
    })
}
