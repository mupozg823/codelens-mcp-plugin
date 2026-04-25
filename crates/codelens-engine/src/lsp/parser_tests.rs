use super::parsers::{rename_edits_from_workspace_edit_response, rename_plan_from_response};
use super::workspace_edit::workspace_edit_transaction_from_response;
use crate::ProjectRoot;
use serde_json::json;
use std::fs;
use url::Url;

#[test]
fn rename_edits_reject_outside_project_uri_before_reading() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-parser-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("mkdir project");
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let outside = dir
        .parent()
        .expect("parent")
        .join(format!("outside-{}.py", std::process::id()));
    fs::write(&outside, "old_name()\n").expect("write outside file");
    let uri = Url::from_file_path(&outside).expect("file uri").to_string();
    let response = json!({
        "result": {
            "changes": {
                uri: [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 8}
                    },
                    "newText": "new_name"
                }]
            }
        }
    });

    let error = rename_edits_from_workspace_edit_response(&project, response)
        .expect_err("outside URI must be rejected");
    assert!(error.to_string().contains("escapes project root"));
}

#[test]
fn rename_edits_translate_lsp_utf16_offsets_before_apply() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-parser-utf16-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("mkdir project");
    let path = dir.join("sample.py");
    fs::write(&path, "🙂 old_name()\n").expect("write sample");
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let uri = Url::from_file_path(&path).expect("file uri").to_string();
    let response = json!({
        "result": {
            "changes": {
                uri: [{
                    "range": {
                        "start": {"line": 0, "character": 3},
                        "end": {"line": 0, "character": 11}
                    },
                    "newText": "new_name"
                }]
            }
        }
    });

    let edits = rename_edits_from_workspace_edit_response(&project, response).expect("utf16 edit");

    assert_eq!(edits[0].old_text, "old_name");
    assert_eq!(edits[0].column, "🙂 ".len() + 1);
    crate::rename::apply_edits(&project, &edits).expect("apply edit");
    let updated = fs::read_to_string(path).expect("read updated");
    assert_eq!(updated, "🙂 new_name()\n");
}

#[test]
fn workspace_edit_transaction_keeps_text_edits_and_resource_ops_separate() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-parser-transaction-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("mkdir project");
    let path = dir.join("sample.py");
    fs::write(&path, "old_name()\n").expect("write sample");
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let uri = Url::from_file_path(&path).expect("file uri").to_string();
    let created_uri = Url::from_file_path(dir.join("created.py"))
        .expect("created uri")
        .to_string();
    let response = json!({
        "result": {
            "documentChanges": [
                {
                    "textDocument": {"uri": uri},
                    "edits": [{
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 8}
                        },
                        "newText": "new_name"
                    }]
                },
                {"kind": "create", "uri": created_uri}
            ]
        }
    });

    let transaction =
        workspace_edit_transaction_from_response(&project, response).expect("parse transaction");

    assert_eq!(transaction.edit_count, 1);
    assert_eq!(transaction.modified_files, 1);
    assert_eq!(transaction.resource_ops.len(), 1);
    assert_eq!(transaction.resource_ops[0].kind, "create");
    assert_eq!(transaction.resource_ops[0].file_path, "created.py");
    assert!(transaction.rollback_available);
}

#[test]
fn workspace_edit_transaction_rejects_full_file_replacement() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-parser-full-file-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("mkdir project");
    let path = dir.join("sample.ts");
    fs::write(&path, "const oldName = 1;\nconsole.log(oldName);\n").expect("write sample");
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let uri = Url::from_file_path(&path).expect("file uri").to_string();
    let response = json!({
        "result": {
            "changes": {
                uri: [{
                    "range": {
                        "start": {"line": 0, "character": 0},
                        "end": {"line": 1, "character": 21}
                    },
                    "newText": "const newName = 1;\nconsole.log(newName);\n"
                }]
            }
        }
    });

    let error = workspace_edit_transaction_from_response(&project, response)
        .expect_err("full-file replacement must fail closed");

    assert!(
        error.to_string().contains("full-file WorkspaceEdit"),
        "{error}"
    );
}

#[test]
fn rename_plan_rejects_outside_project_uri() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-parser-plan-outside-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("mkdir project");
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let outside = dir
        .parent()
        .expect("parent")
        .join(format!("outside-plan-{}.py", std::process::id()));
    fs::write(&outside, "old_name()\n").expect("write outside file");
    let uri = Url::from_file_path(&outside).expect("file uri").to_string();
    let response = json!({
        "result": {
            "range": {
                "start": {"line": 0, "character": 0},
                "end": {"line": 0, "character": 8}
            },
            "textDocument": {"uri": uri},
            "placeholder": "old_name"
        }
    });

    let error = rename_plan_from_response(&project, "sample.py", "old_name()\n", response, None)
        .expect_err("outside prepareRename URI must be rejected");
    assert!(error.to_string().contains("escapes project root"));
}

#[test]
fn rename_plan_translates_lsp_utf16_offsets() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-lsp-parser-plan-utf16-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("mkdir project");
    let path = dir.join("sample.py");
    let source = "🙂 old_name()\n";
    fs::write(&path, source).expect("write sample");
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let uri = Url::from_file_path(&path).expect("file uri").to_string();
    let response = json!({
        "result": {
            "range": {
                "start": {"line": 0, "character": 3},
                "end": {"line": 0, "character": 11}
            },
            "textDocument": {"uri": uri}
        }
    });

    let plan = rename_plan_from_response(&project, "sample.py", source, response, None)
        .expect("utf16 prepareRename should parse");

    assert_eq!(plan.file_path, "sample.py");
    assert_eq!(plan.column, "🙂 ".len() + 1);
    assert_eq!(plan.end_column, "🙂 old_name".len() + 1);
    assert_eq!(plan.current_name, "old_name");
}
