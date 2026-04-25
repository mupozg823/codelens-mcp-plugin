use anyhow::{Context, Result, bail};
use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct CodeActionCandidate {
    pub index: usize,
    pub title: String,
    pub kind: Option<String>,
    pub disabled_reason: Option<String>,
    pub edit: Option<Value>,
    pub command: Option<Value>,
    pub raw: Value,
}

pub(super) fn code_actions_from_response(
    response: Value,
    only: &[String],
) -> Result<Vec<CodeActionCandidate>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    let Some(items) = result.as_array() else {
        return Ok(Vec::new());
    };

    let mut actions = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let Some(title) = item.get("title").and_then(Value::as_str) else {
            continue;
        };
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if !matches_requested_kind(kind.as_deref(), only) {
            continue;
        }
        actions.push(CodeActionCandidate {
            index,
            title: title.to_owned(),
            kind,
            disabled_reason: item
                .get("disabled")
                .and_then(|disabled| disabled.get("reason"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            edit: item.get("edit").cloned(),
            command: item.get("command").cloned(),
            raw: item.clone(),
        });
    }
    Ok(actions)
}

pub(super) fn select_code_action(
    actions: &[CodeActionCandidate],
    action_id: Option<&str>,
) -> Result<CodeActionCandidate> {
    if let Some(action_id) = action_id {
        return actions
            .iter()
            .find(|action| {
                action.title == action_id
                    || action.kind.as_deref() == Some(action_id)
                    || action.index.to_string() == action_id
            })
            .cloned()
            .with_context(|| format!("LSP codeAction `{action_id}` was not returned"));
    }

    match actions {
        [] => bail!("unsupported_semantic_refactor: LSP returned no matching codeAction"),
        [single] => Ok(single.clone()),
        many => bail!(
            "multiple LSP codeActions matched; provide action_id. Candidates: {}",
            many.iter()
                .map(|action| format!("{}:{}", action.index, action.title))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn matches_requested_kind(kind: Option<&str>, only: &[String]) -> bool {
    if only.is_empty() {
        return true;
    }
    let Some(kind) = kind else {
        return false;
    };
    only.iter()
        .any(|requested| kind == requested || kind.starts_with(&format!("{requested}.")))
}
