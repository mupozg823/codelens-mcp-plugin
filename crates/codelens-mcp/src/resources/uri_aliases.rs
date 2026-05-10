use serde_json::{Value, json};

pub(crate) fn symbiote_alias_entries(entries: &[Value]) -> Vec<Value> {
    entries
        .iter()
        .filter_map(|entry| {
            let uri = entry.get("uri").and_then(|value| value.as_str())?;
            let rest = uri.strip_prefix("codelens://")?;
            let mut alias = entry.clone();
            let object = alias.as_object_mut()?;
            object.insert("uri".to_owned(), json!(format!("symbiote://{rest}")));

            let alias_name = object
                .get("name")
                .and_then(|value| value.as_str())
                .map(|name| format!("{name} (Symbiote Alias)"))
                .unwrap_or_else(|| format!("symbiote://{rest}"));
            object.insert("name".to_owned(), json!(alias_name));

            let alias_description = object
                .get("description")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(|description| format!("{description} [Symbiote URI alias for `{uri}`]"))
                .unwrap_or_else(|| format!("Symbiote URI alias for `{uri}`"));
            object.insert("description".to_owned(), json!(alias_description));
            Some(alias)
        })
        .collect()
}

/// ADR-0007 Phase 2: accept `symbiote://<rest>` as an alias of
/// `codelens://<rest>`. Dispatch logic remains pinned to the canonical
/// `codelens://` form; this normalizer is the single rewrite site.
pub(crate) fn normalize_resource_uri(uri: &str) -> std::borrow::Cow<'_, str> {
    if let Some(rest) = uri.strip_prefix("symbiote://") {
        std::borrow::Cow::Owned(format!("codelens://{}", rest))
    } else {
        std::borrow::Cow::Borrowed(uri)
    }
}
