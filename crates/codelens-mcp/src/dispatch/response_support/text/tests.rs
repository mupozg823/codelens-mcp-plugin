use super::summarize::{TEXT_CHANNEL_MAX_ARRAY_ITEMS, summarize_text_data_for_response};
use serde_json::{Value, json};

#[test]
fn shrinking_array_child_flags_parent_truncated() {
    let payload = json!({
        "references": [1, 2, 3, 4, 5],
        "count": 5,
        "returned_count": 5,
        "sampled": false,
    });
    let summarized = summarize_text_data_for_response(&payload);
    let obj = summarized.as_object().expect("object");
    assert_eq!(
        obj.get("truncated").and_then(Value::as_bool),
        Some(true),
        "parent must be flagged when an array child was shrunk"
    );
    assert_eq!(
        obj.get("references")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(TEXT_CHANNEL_MAX_ARRAY_ITEMS)
    );
    assert_eq!(obj.get("count").and_then(Value::as_i64), Some(5));
    assert_eq!(obj.get("returned_count").and_then(Value::as_i64), Some(5));
}

#[test]
fn short_array_leaves_parent_untruncated() {
    let payload = json!({
        "references": [1, 2],
        "count": 2,
        "returned_count": 2,
    });
    let summarized = summarize_text_data_for_response(&payload);
    let obj = summarized.as_object().expect("object");
    assert!(obj.get("truncated").is_none());
    assert_eq!(
        obj.get("references")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
}

#[test]
fn nested_object_array_shrink_flags_inner_parent() {
    let payload = json!({
        "outer": {
            "items": [1, 2, 3, 4],
            "total": 4,
        }
    });
    let summarized = summarize_text_data_for_response(&payload);
    let inner = summarized
        .get("outer")
        .and_then(Value::as_object)
        .expect("outer object");
    assert_eq!(inner.get("truncated").and_then(Value::as_bool), Some(true));
}
