//! Shared programmatic-read envelope fields (I2.3): the batch/cursor/
//! snapshot layer (`symbol_query::batch`) adds these to the three read
//! tools' payloads, so their output schemas advertise them here in one
//! place. All fields are optional — singular non-paged calls omit them
//! except `index_snapshot`, which is always present.

use serde_json::json;

/// Inject the programmatic-read properties into an object schema.
pub(crate) fn with_programmatic_read_properties(
    mut schema: serde_json::Value,
) -> serde_json::Value {
    let Some(properties) = schema
        .get_mut("properties")
        .and_then(|value| value.as_object_mut())
    else {
        return schema;
    };
    properties.insert(
        "index_snapshot".to_owned(),
        json!({
            "type": "string",
            "description": "Index snapshot token (gen:<n>); pass back as `snapshot` to pin replays"
        }),
    );
    properties.insert(
        "batch".to_owned(),
        json!({
            "type": "array",
            "items": {"type": "object"},
            "description": "Per-item results for array-input calls; failed items carry {ok:false, error}"
        }),
    );
    properties.insert("batch_count".to_owned(), json!({"type": "integer"}));
    properties.insert("ok_count".to_owned(), json!({"type": "integer"}));
    properties.insert("error_count".to_owned(), json!({"type": "integer"}));
    properties.insert(
        "page".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "offset": {"type": "integer"},
                "returned": {"type": "integer"},
                "total": {"type": "integer"}
            }
        }),
    );
    properties.insert(
        "next_cursor".to_owned(),
        json!({
            "type": "string",
            "description": "Opaque continuation cursor, present only when results remain"
        }),
    );
    schema
}
