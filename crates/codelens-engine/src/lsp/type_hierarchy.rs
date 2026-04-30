use super::parsers::symbol_kind_label;
use super::types::LspTypeHierarchyNode;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) fn type_hierarchy_node_from_item(item: &Value) -> Result<LspTypeHierarchyNode> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .context("type hierarchy item missing name")?;
    let detail = item
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or(name)
        .to_owned();
    let kind = item
        .get("kind")
        .and_then(Value::as_u64).map_or_else(|| "unknown".to_owned(), symbol_kind_label);
    Ok(LspTypeHierarchyNode {
        name: name.to_owned(),
        fully_qualified_name: detail,
        kind,
        members: HashMap::from([
            ("methods".to_owned(), Vec::new()),
            ("fields".to_owned(), Vec::new()),
            ("properties".to_owned(), Vec::new()),
        ]),
        type_parameters: Vec::new(),
        supertypes: Vec::new(),
        subtypes: Vec::new(),
    })
}

pub(super) fn type_hierarchy_to_map(node: &LspTypeHierarchyNode) -> HashMap<String, Value> {
    let mut result = HashMap::from([
        ("class_name".to_owned(), Value::String(node.name.clone())),
        (
            "fully_qualified_name".to_owned(),
            Value::String(node.fully_qualified_name.clone()),
        ),
        ("kind".to_owned(), Value::String(node.kind.clone())),
        (
            "members".to_owned(),
            serde_json::to_value(&node.members).unwrap_or_else(|_| json!({})),
        ),
        (
            "type_parameters".to_owned(),
            serde_json::to_value(&node.type_parameters).unwrap_or_else(|_| json!([])),
        ),
    ]);
    if !node.supertypes.is_empty() {
        result.insert(
            "supertypes".to_owned(),
            serde_json::to_value(
                node.supertypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    if !node.subtypes.is_empty() {
        result.insert(
            "subtypes".to_owned(),
            serde_json::to_value(
                node.subtypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    result
}

pub(super) fn method_suffix_to_hierarchy(method_suffix: &str) -> &str {
    match method_suffix {
        "supertypes" => "super",
        "subtypes" => "sub",
        _ => "both",
    }
}

fn type_hierarchy_child_to_map(node: &LspTypeHierarchyNode) -> HashMap<String, Value> {
    let mut result = HashMap::from([
        ("name".to_owned(), Value::String(node.name.clone())),
        (
            "qualified_name".to_owned(),
            Value::String(node.fully_qualified_name.clone()),
        ),
        ("kind".to_owned(), Value::String(node.kind.clone())),
    ]);
    if !node.supertypes.is_empty() {
        result.insert(
            "supertypes".to_owned(),
            serde_json::to_value(
                node.supertypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    if !node.subtypes.is_empty() {
        result.insert(
            "subtypes".to_owned(),
            serde_json::to_value(
                node.subtypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    result
}
