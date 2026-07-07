use super::*;

#[test]
fn ranked_context_reports_body_omission_when_bodies_not_requested() {
    let project = project_root();
    fs::write(
        project.as_path().join("ranked_coverage.py"),
        "def search_users(query):\n    return query\n\ndef delete_user(uid):\n    return uid\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({"query": "search users", "disable_semantic": true}),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["coverage"]["mode"], json!("symbol_cards"));
    assert_eq!(
        payload["data"]["coverage"]["bodies_requested"],
        json!(false)
    );
    assert_eq!(payload["data"]["coverage"]["bodies_returned"], json!(0));
    assert_eq!(payload["data"]["coverage"]["body_scope"], json!("omitted"));
    assert_eq!(payload["data"]["coverage"]["flow_complete"], json!(false));
    assert!(
        payload["data"]["coverage"]["gaps"]
            .as_array()
            .expect("coverage gaps")
            .contains(&json!("bodies_not_requested"))
    );
}

#[test]
fn ranked_context_reports_actual_body_coverage_when_bodies_requested() {
    let project = project_root();
    fs::write(
        project.as_path().join("ranked_body_coverage.py"),
        "def search_users(query):\n    return query\n\ndef delete_user(uid):\n    return uid\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({
            "query": "search users",
            "path": "ranked_body_coverage.py",
            "include_body": true,
            "disable_semantic": true,
            "max_tokens": 12000
        }),
    );

    assert_eq!(payload["success"], json!(true));
    let symbols = payload["data"]["symbols"].as_array().expect("symbols");
    let bodies_returned = symbols
        .iter()
        .filter(|symbol| symbol.get("body").is_some())
        .count();

    assert_eq!(payload["data"]["coverage"]["bodies_requested"], json!(true));
    assert_eq!(
        payload["data"]["coverage"]["bodies_returned"],
        json!(bodies_returned)
    );
    assert_eq!(payload["data"]["coverage"]["flow_complete"], json!(false));
    assert_eq!(
        payload["data"]["coverage"]["body_scope"],
        if bodies_returned == 0 {
            json!("none")
        } else {
            json!("symbol_span")
        }
    );
    assert!(
        payload["data"]["coverage"]["gaps"]
            .as_array()
            .expect("coverage gaps")
            .contains(&json!("not_flow_complete"))
    );
}
