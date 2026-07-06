//! Raw host-native adapter template routing.

mod claude_code;
mod cline;
mod codex;
mod cursor;
mod windsurf;

use super::overlays::augment_host_adapter_bundle;
use serde_json::Value;

pub(super) fn raw_host_adapter_bundle(host: &str) -> Option<Value> {
    let mut bundle = match host {
        "claude-code" => claude_code::bundle(),
        "codex" => codex::bundle(),
        "cursor" => cursor::bundle(),
        "cline" => cline::bundle(),
        "windsurf" => windsurf::bundle(),
        _ => return None,
    };

    augment_host_adapter_bundle(host, &mut bundle);
    Some(bundle)
}
