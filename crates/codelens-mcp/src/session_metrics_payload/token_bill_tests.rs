use super::build_token_bill_payload;
use crate::telemetry::{SessionMetrics, ToolInvocation};
use serde_json::json;

#[test]
fn ranks_token_tools_and_emits_tools_list_tax_when_share_is_high() {
    let session = SessionMetrics {
        core: crate::telemetry::CoreMetrics {
            total_calls: 3,
            total_tokens: 3000,
            ..Default::default()
        },
        token: crate::telemetry::TokenMetrics {
            tools_list_tokens: 900,
        },
        timeline: vec![
            invocation("find_symbol", 12, 500),
            invocation("tools/list", 30, 900),
            invocation("find_symbol", 18, 600),
        ],
        ..Default::default()
    };

    let bill = build_token_bill_payload(&session);

    assert_eq!(bill["tools_list_share"], json!(0.3));
    assert_eq!(bill["top_token_tools"][0]["tool"], json!("find_symbol"));
    assert_eq!(bill["top_token_tools"][0]["tokens"], json!(1100));
    assert_eq!(
        bill["waste_signals"][0]["code"],
        json!("tools_list_token_tax")
    );
}

fn invocation(tool: &str, elapsed_ms: u64, tokens: usize) -> ToolInvocation {
    ToolInvocation {
        tool: tool.to_owned(),
        resolved_target: Some(tool.to_owned()),
        mode: None,
        work_class: crate::operation::operation_work_class(tool),
        downstream_call_count: u64::from(tool != "tools/list"),
        surface: "builder-minimal".to_owned(),
        elapsed_ms,
        tokens,
        success: true,
        truncated: false,
        phase: None,
        target_paths: Vec::new(),
    }
}
