# Phase 0 — Mutation Surface Truthing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raw mutation surface(11 primitives + tree-sitter rename)가 응답에 `authority`/`can_preview`/`can_apply`/`edit_authority`를 정직 광고하고, surface-manifest CI가 capability matrix 정합성 위반을 차단하도록 한다.

**Architecture:** `tools/mutation.rs`에 `raw_fs_envelope` 헬퍼를 도입해 11개 primitive 응답에 머지. tree-sitter `rename_symbol`은 `dry_run=false`에서 `Validation` 에러로 강등. capability matrix는 `--print-operation-matrix` CLI flag로 단일 출처화하고, `surface-manifest.py`에 contract A(`verified=false && can_apply=true` reject) + contract B(matrix 내부 정합성)를 추가. CI에서 새 contract test로 회귀 차단.

**Tech Stack:** Rust (codelens-mcp), `serde_json::Value`, Python 3 (`scripts/surface-manifest.py`, fixture tests), GitHub Actions.

**Spec reference:** `docs/superpowers/specs/2026-04-25-codelens-phase0-mutation-surface-design.md` (commit `0094f8f2`).

**Plan refinements vs spec:**

- Spec §2 H "신규 바이너리 `dump-matrix.rs`" → 실제 패턴(`main.rs` `--print-surface-manifest` flag)에 정합화하기 위해 **`--print-operation-matrix` CLI flag**로 변경. 별도 binary 미신설.
- Spec §2 C "matrix↔manifest 1:1" → Phase 0 deliverable: **matrix 내부 정합성**(필수 필드, fail_closed 보존, can_apply→verified, support enum). 완전한 operation→tool mapping은 Phase 1로 명시.

**Branch:** `codex/ide-refactor-substrate` (in-flight, head `0094f8f2`).

---

## File Structure

| 역할                                                 | 경로                                                             | 변경         |
| ---------------------------------------------------- | ---------------------------------------------------------------- | ------------ |
| 11 primitive + tree-sitter rename 응답 envelope 머지 | `crates/codelens-mcp/src/tools/mutation.rs`                      | 수정         |
| envelope contract test (11 + 3)                      | `crates/codelens-mcp/src/integration_tests/mutation_envelope.rs` | 신규         |
| 신규 모듈 등록                                       | `crates/codelens-mcp/src/integration_tests/mod.rs`               | 1줄 추가     |
| `--print-operation-matrix` CLI flag                  | `crates/codelens-mcp/src/main.rs`                                | 약 10줄 추가 |
| contract A + B 검사                                  | `scripts/surface-manifest.py`                                    | 약 60줄 추가 |
| contract A/B fixture test                            | `scripts/test/test-surface-manifest-contracts.py`                | 신규         |
| CI step 추가                                         | `.github/workflows/ci.yml`                                       | 5~8줄 추가   |

**Footprint**: 4 수정 + 2 신규 = 6 파일 (spec AC-7 ≤ 8 만족).

---

### Task 1: `raw_fs_envelope` 헬퍼 + 11 primitive 응답 머지 (TDD)

**Files:**

- Create: `crates/codelens-mcp/src/integration_tests/mutation_envelope.rs`
- Modify: `crates/codelens-mcp/src/integration_tests/mod.rs:24-35`
- Modify: `crates/codelens-mcp/src/tools/mutation.rs`

**Why this batch:** 11 primitive가 모두 동일한 envelope 패턴이라 한 cycle로 묶으면 TDD 비용을 11배 절감. 각 primitive 1 case + 검증 한 번.

- [ ] **Step 1: 신규 파일 생성 — 11개 primitive envelope 실패 test**

`crates/codelens-mcp/src/integration_tests/mutation_envelope.rs`:

```rust
use super::*;

fn assert_raw_fs_envelope(result: &serde_json::Value, expected_op: &str) {
    assert_eq!(
        result["authority"], "syntax",
        "expected authority=syntax for {expected_op}, got {:?}",
        result["authority"]
    );
    assert_eq!(
        result["can_preview"], true,
        "expected can_preview=true for {expected_op}"
    );
    assert_eq!(
        result["can_apply"], true,
        "expected can_apply=true for {expected_op}"
    );
    let edit_authority = &result["edit_authority"];
    assert_eq!(
        edit_authority["kind"], "raw_fs",
        "expected edit_authority.kind=raw_fs for {expected_op}"
    );
    assert_eq!(
        edit_authority["operation"], expected_op,
        "expected edit_authority.operation={expected_op}"
    );
    assert!(
        edit_authority["validator"].is_null(),
        "expected edit_authority.validator=null for {expected_op}"
    );
}

fn seed_lines(project: &codelens_engine::ProjectRoot, name: &str) -> std::path::PathBuf {
    let path = project.as_path().join(name);
    fs::write(&path, "alpha\nbeta\ngamma\ndelta\n").unwrap();
    path
}

#[test]
fn create_text_file_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "envelope_create.txt", "content": "x\n"}),
    );
    assert_raw_fs_envelope(&result, "create_text_file");
}

#[test]
fn delete_lines_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_delete.txt");
    let result = call_tool(
        &state,
        "delete_lines",
        json!({"relative_path": "envelope_delete.txt", "start_line": 1, "end_line": 1}),
    );
    assert_raw_fs_envelope(&result, "delete_lines");
}

#[test]
fn insert_at_line_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_insert_line.txt");
    let result = call_tool(
        &state,
        "insert_at_line",
        json!({"relative_path": "envelope_insert_line.txt", "line": 1, "content": "new\n"}),
    );
    assert_raw_fs_envelope(&result, "insert_at_line");
}

#[test]
fn insert_before_symbol_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_before.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let result = call_tool(
        &state,
        "insert_before_symbol",
        json!({
            "relative_path": "envelope_before.py",
            "symbol_name": "alpha",
            "content": "# leading\n"
        }),
    );
    assert_raw_fs_envelope(&result, "insert_before_symbol");
}

#[test]
fn insert_after_symbol_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_after.py");
    fs::write(&path, "def alpha():\n    pass\n").unwrap();
    let result = call_tool(
        &state,
        "insert_after_symbol",
        json!({
            "relative_path": "envelope_after.py",
            "symbol_name": "alpha",
            "content": "# trailing\n"
        }),
    );
    assert_raw_fs_envelope(&result, "insert_after_symbol");
}

#[test]
fn insert_content_default_dispatches_to_line_envelope() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_insert_content.txt");
    let result = call_tool(
        &state,
        "insert_content",
        json!({"relative_path": "envelope_insert_content.txt", "line": 1, "content": "new\n"}),
    );
    assert_raw_fs_envelope(&result, "insert_at_line");
}

#[test]
fn replace_lines_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_replace_lines.txt");
    let result = call_tool(
        &state,
        "replace_lines",
        json!({
            "relative_path": "envelope_replace_lines.txt",
            "start_line": 2,
            "end_line": 2,
            "new_content": "BETA\n"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_lines");
}

#[test]
fn replace_content_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_replace_content.txt");
    let result = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "envelope_replace_content.txt",
            "old_text": "alpha",
            "new_text": "ALPHA"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_content");
}

#[test]
fn replace_content_unified_default_dispatches_to_text() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "envelope_replace_unified.txt");
    let result = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "envelope_replace_unified.txt",
            "old_text": "alpha",
            "new_text": "ALPHA"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_content");
}

#[test]
fn replace_symbol_body_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_replace_symbol.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();
    let result = call_tool(
        &state,
        "replace_symbol_body",
        json!({
            "relative_path": "envelope_replace_symbol.py",
            "symbol_name": "alpha",
            "new_body": "    return 2\n"
        }),
    );
    assert_raw_fs_envelope(&result, "replace_symbol_body");
}

#[test]
fn add_import_advertises_raw_fs() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("envelope_import.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();
    let result = call_tool(
        &state,
        "add_import",
        json!({
            "file_path": "envelope_import.py",
            "import_statement": "import os"
        }),
    );
    assert_raw_fs_envelope(&result, "add_import");
}
```

- [ ] **Step 2: 새 모듈 등록**

`crates/codelens-mcp/src/integration_tests/mod.rs:24` 부근의 `mod` 선언 블록에 `mod mutation_envelope;`를 알파벳 순서로 추가:

```rust
mod coordination;
mod lsp;
mod memory;
mod mutation;
mod mutation_envelope;  // ← 추가
mod protocol;
mod readonly;
```

- [ ] **Step 3: 컴파일/실행 — 모든 새 test가 fail 확인**

```bash
cargo test -p codelens-mcp --no-default-features mutation_envelope:: 2>&1 | tail -40
```

Expected: 11 test 모두 `panicked at 'expected authority=...'` 형태의 fail. (테스트가 응답에 `authority` 필드를 기대하지만 현재 응답에는 없음.)

- [ ] **Step 4: `mutation.rs`에 `raw_fs_envelope` 헬퍼 추가**

`crates/codelens-mcp/src/tools/mutation.rs` 최상단 import 블록 수정:

```rust
use super::{AppState, ToolResult, required_string, success_meta};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    add_import, analyze_missing_imports, create_text_file, delete_lines, insert_after_symbol,
    insert_at_line, insert_before_symbol, rename, replace_content, replace_lines,
    replace_symbol_body,
};
use serde_json::{json, Value};  // ← Value 추가
```

파일 끝(또는 첫 함수 이전)에 헬퍼 추가:

```rust
/// Envelope advertising that this is a raw filesystem mutation with no semantic authority.
/// Agents that read these fields know "syntax-level edit, no LSP/compiler verification".
fn raw_fs_envelope(operation: &str) -> Value {
    json!({
        "authority": "syntax",
        "can_preview": true,
        "can_apply": true,
        "edit_authority": {
            "kind": "raw_fs",
            "operation": operation,
            "validator": Value::Null,
        }
    })
}

/// Merge `raw_fs_envelope(operation)` fields into an existing JSON object.
fn merge_raw_fs_envelope(mut value: Value, operation: &str) -> Value {
    let envelope = raw_fs_envelope(operation);
    if let (Some(target), Some(source)) = (value.as_object_mut(), envelope.as_object()) {
        for (k, v) in source {
            target.insert(k.clone(), v.clone());
        }
    }
    value
}
```

- [ ] **Step 5: 11 primitive 결과에 envelope 머지**

각 primitive의 `Ok(...)`에서 결과 `Value`를 `merge_raw_fs_envelope(value, "<operation_name>")`로 감싼다. operation 이름은 도구 이름과 동일.

`create_text_file_tool`(약 line 55):

```rust
pub fn create_text_file_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let content = required_string(arguments, "content")?;
    let overwrite = arguments
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(
        create_text_file(&state.project(), relative_path, content, overwrite).map(|_| {
            (
                merge_raw_fs_envelope(
                    json!({ "created": relative_path }),
                    "create_text_file",
                ),
                success_meta(BackendKind::Filesystem, 1.0),
            )
        })?,
    )
}
```

`delete_lines_tool`:

```rust
Ok(
    delete_lines(&state.project(), relative_path, start_line, end_line).map(|content| {
        (
            merge_raw_fs_envelope(json!({ "content": content }), "delete_lines"),
            success_meta(BackendKind::Filesystem, 1.0),
        )
    })?,
)
```

`insert_at_line_tool`:

```rust
Ok(
    insert_at_line(&state.project(), relative_path, line, content).map(|modified| {
        (
            merge_raw_fs_envelope(json!({ "content": modified }), "insert_at_line"),
            success_meta(BackendKind::Filesystem, 1.0),
        )
    })?,
)
```

`replace_lines_tool`:

```rust
Ok(replace_lines(
    &state.project(),
    relative_path,
    start_line,
    end_line,
    new_content,
)
.map(|content| {
    (
        merge_raw_fs_envelope(json!({ "content": content }), "replace_lines"),
        success_meta(BackendKind::Filesystem, 1.0),
    )
})?)
```

`replace_content_tool`:

```rust
Ok(replace_content(
    &state.project(),
    relative_path,
    old_text,
    new_text,
    regex_mode,
)
.map(|(content, count)| {
    (
        merge_raw_fs_envelope(
            json!({ "content": content, "replacements": count }),
            "replace_content",
        ),
        success_meta(BackendKind::Filesystem, 1.0),
    )
})?)
```

`replace_symbol_body_tool`:

```rust
Ok(replace_symbol_body(
    &state.project(),
    relative_path,
    symbol_name,
    name_path,
    new_body,
)
.map(|content| {
    (
        merge_raw_fs_envelope(json!({ "content": content }), "replace_symbol_body"),
        success_meta(BackendKind::TreeSitter, 0.95),
    )
})?)
```

`insert_before_symbol_tool`:

```rust
Ok(insert_before_symbol(
    &state.project(),
    relative_path,
    symbol_name,
    name_path,
    content,
)
.map(|modified| {
    (
        merge_raw_fs_envelope(json!({ "content": modified }), "insert_before_symbol"),
        success_meta(BackendKind::TreeSitter, 0.95),
    )
})?)
```

`insert_after_symbol_tool`:

```rust
Ok(insert_after_symbol(
    &state.project(),
    relative_path,
    symbol_name,
    name_path,
    content,
)
.map(|modified| {
    (
        merge_raw_fs_envelope(json!({ "content": modified }), "insert_after_symbol"),
        success_meta(BackendKind::TreeSitter, 0.95),
    )
})?)
```

`add_import_tool`:

```rust
Ok(
    add_import(&state.project(), file_path, import_statement).map(|content| {
        (
            merge_raw_fs_envelope(
                json!({"success": true, "file_path": file_path, "content_length": content.len()}),
                "add_import",
            ),
            success_meta(BackendKind::Filesystem, 1.0),
        )
    })?,
)
```

`insert_content_tool` 및 `replace_content_unified`는 dispatcher라 내부 호출에 의해 envelope이 자동 머지됨 — 추가 변경 불필요.

- [ ] **Step 6: 다시 실행 — 11개 envelope test 통과 확인**

```bash
cargo test -p codelens-mcp --no-default-features mutation_envelope:: 2>&1 | tail -20
```

Expected: `test result: ok. 11 passed`.

- [ ] **Step 7: 기존 mutation test 회귀 0 확인**

```bash
cargo test -p codelens-mcp --no-default-features integration_tests::mutation:: 2>&1 | tail -10
cargo test -p codelens-mcp --features http 2>&1 | tail -5
```

Expected: 모두 PASS, 신규 fail 0.

- [ ] **Step 8: Commit**

```bash
git add crates/codelens-mcp/src/tools/mutation.rs \
        crates/codelens-mcp/src/integration_tests/mutation_envelope.rs \
        crates/codelens-mcp/src/integration_tests/mod.rs
git commit -m "$(cat <<'EOF'
feat(mcp): advertise raw_fs envelope on 11 mutation primitives

raw_fs_envelope helper + merge into create_text_file/delete_lines/
insert_at_line/insert_before_symbol/insert_after_symbol/replace_lines/
replace_content/replace_symbol_body/add_import responses. agents see
authority=syntax / can_apply=true / edit_authority.kind=raw_fs.

Closes Phase 0 G1.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: `BackendKind::Filesystem` confidence 1.0 → 0.7 (TDD)

**Files:**

- Modify: `crates/codelens-mcp/src/integration_tests/mutation_envelope.rs` (test 추가)
- Modify: `crates/codelens-mcp/src/tools/mutation.rs` (모든 `success_meta(BackendKind::Filesystem, 1.0)` → `0.7`)

- [ ] **Step 1: confidence 강등 실패 test 추가**

`mutation_envelope.rs` 끝에 추가:

```rust
fn extract_meta(response: &serde_json::Value) -> serde_json::Value {
    response.get("_meta").cloned().unwrap_or_else(|| json!(null))
}

#[test]
fn create_text_file_filesystem_confidence_is_lowered() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "conf_create.txt", "content": "x\n"}),
    );
    let meta = extract_meta(&result);
    let confidence = meta["confidence"].as_f64().unwrap_or(1.0);
    assert!(
        confidence <= 0.7 + f64::EPSILON,
        "expected Filesystem confidence ≤ 0.7, got {confidence}"
    );
    assert_eq!(meta["backend_used"], "filesystem");
}

#[test]
fn delete_lines_filesystem_confidence_is_lowered() {
    let project = project_root();
    let state = make_state(&project);
    seed_lines(&project, "conf_delete.txt");
    let result = call_tool(
        &state,
        "delete_lines",
        json!({"relative_path": "conf_delete.txt", "start_line": 1, "end_line": 1}),
    );
    let meta = extract_meta(&result);
    let confidence = meta["confidence"].as_f64().unwrap_or(1.0);
    assert!(
        confidence <= 0.7 + f64::EPSILON,
        "expected Filesystem confidence ≤ 0.7, got {confidence}"
    );
}
```

(주의: `_meta` 필드가 응답에 노출되는지 사전 확인. `parse_tool_response`가 `structuredContent`를 `data` key로 머지하므로, `_meta`는 별도 검증 경로일 수 있음. 만약 `_meta`가 직접 응답에 안 보이면 다음 단계에서 노출 경로 확인 후 조정.)

- [ ] **Step 2: 컴파일/실행 — fail 확인**

```bash
cargo test -p codelens-mcp --no-default-features filesystem_confidence_is_lowered 2>&1 | tail -20
```

Expected: 두 test 모두 `expected Filesystem confidence ≤ 0.7, got 1.0` fail.
만약 `_meta` 필드가 응답에 안 보여서 `confidence == 1.0` (default fallback)이 잡히면, `parse_tool_response`나 router가 `_meta`를 어디로 라우팅하는지 확인 후 test에서 올바른 path 사용 (예: `result["data"]["_meta"]`, `result["_meta"]`, 또는 별도 helper 추가). **이 step에서 helper 정확성 자체를 먼저 확정한 뒤 Step 3 진입.**

- [ ] **Step 3: 모든 `BackendKind::Filesystem, 1.0` 호출을 `0.7`로 변경**

`mutation.rs`에서 `success_meta(BackendKind::Filesystem, 1.0)` 5곳을 `success_meta(BackendKind::Filesystem, 0.7)`로 일괄 교체:

```bash
grep -n "BackendKind::Filesystem, 1.0" crates/codelens-mcp/src/tools/mutation.rs
```

해당 라인들을 `0.7`로 수정. (`BackendKind::TreeSitter, 0.95`는 그대로 유지 — TreeSitter는 syntax-aware라 Filesystem보다 confidence 높음.)

- [ ] **Step 4: 다시 실행 — 통과 + 회귀 0 확인**

```bash
cargo test -p codelens-mcp --no-default-features filesystem_confidence_is_lowered 2>&1 | tail -10
cargo test -p codelens-mcp --features http 2>&1 | tail -5
```

Expected: 새 test 2 PASS, 기존 fail 0.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/mutation.rs \
        crates/codelens-mcp/src/integration_tests/mutation_envelope.rs
git commit -m "$(cat <<'EOF'
feat(mcp): lower Filesystem mutation confidence 1.0 -> 0.7

raw fs writes are not authoritative — confidence 1.0 misled agents
into trusting syntax-only edits. lowered to 0.7 across mutation.rs
TreeSitter mutations keep 0.95 (syntax-aware).

Closes Phase 0 G3.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: tree-sitter `rename_symbol` `can_apply=false` 강등 (TDD)

**Files:**

- Modify: `crates/codelens-mcp/src/integration_tests/mutation_envelope.rs` (test 추가)
- Modify: `crates/codelens-mcp/src/tools/mutation.rs:11-53` (`rename_symbol` TreeSitter 분기)

- [ ] **Step 1: 강등 실패 test 3 case 추가**

`mutation_envelope.rs` 끝에 추가:

```rust
#[test]
fn tree_sitter_rename_apply_attempt_returns_validation_error() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("rename_apply.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "rename_symbol",
                "arguments": {
                    "_session_id": default_session_id(&state),
                    "file_path": "rename_apply.py",
                    "symbol_name": "alpha",
                    "new_name": "beta",
                    "semantic_edit_backend": "tree-sitter",
                    "dry_run": false
                }
            })),
        },
    )
    .expect("tools/call should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    let text = value["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        text.contains("preview-only") || text.contains("Validation"),
        "expected validation error mentioning preview-only, got: {text}"
    );
}

#[test]
fn tree_sitter_rename_dry_run_advertises_preview_only() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("rename_dry.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();

    let result = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_dry.py",
            "symbol_name": "alpha",
            "new_name": "beta",
            "semantic_edit_backend": "tree-sitter",
            "dry_run": true
        }),
    );
    assert_eq!(result["authority"], "syntax");
    assert_eq!(result["can_preview"], true);
    assert_eq!(result["can_apply"], false);
    assert_eq!(result["support"], "syntax_preview");
    assert!(
        result["blocker_reason"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "expected non-empty blocker_reason"
    );
}

#[test]
fn unset_backend_apply_attempt_returns_validation_error() {
    let project = project_root();
    let state = make_state(&project);
    let path = project.as_path().join("rename_unset.py");
    fs::write(&path, "def alpha():\n    return 1\n").unwrap();

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "rename_symbol",
                "arguments": {
                    "_session_id": default_session_id(&state),
                    "file_path": "rename_unset.py",
                    "symbol_name": "alpha",
                    "new_name": "beta",
                    "dry_run": false
                }
            })),
        },
    )
    .expect("tools/call should return a response");

    let value = serde_json::to_value(&response).expect("serialize");
    let text = value["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    assert!(
        text.contains("preview-only") || text.contains("Validation"),
        "expected validation error when backend unset and dry_run=false, got: {text}"
    );
}
```

상단에 import 추가:

```rust
use crate::server::router::handle_request;
```

(`super::*` 가 이미 `handle_request`를 가져오면 생략. 확인: `mutation.rs` test 파일에 `super::*` 만으로 충분한지 점검 후 미사용 import는 빼기.)

- [ ] **Step 2: 컴파일/실행 — fail 확인**

```bash
cargo test -p codelens-mcp --no-default-features tree_sitter_rename 2>&1 | tail -20
cargo test -p codelens-mcp --no-default-features unset_backend_apply 2>&1 | tail -10
```

Expected: 3 test 모두 fail (현재 응답에 `can_apply: false`/`blocker_reason`/`Validation` 없음).

- [ ] **Step 3: `mutation.rs::rename_symbol`의 TreeSitter 분기 강등**

기존 (`mutation.rs:11-53` 부근):

```rust
pub fn rename_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    match crate::tools::semantic_edit::selected_backend(arguments)? {
        crate::tools::semantic_edit::SemanticEditBackendSelection::Lsp => { ... }
        crate::tools::semantic_edit::SemanticEditBackendSelection::JetBrains
        | crate::tools::semantic_edit::SemanticEditBackendSelection::Roslyn => { ... }
        crate::tools::semantic_edit::SemanticEditBackendSelection::TreeSitter => {}
    }
    // 기존: tree-sitter rename 호출
    ...
}
```

수정:

```rust
pub fn rename_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    match crate::tools::semantic_edit::selected_backend(arguments)? {
        crate::tools::semantic_edit::SemanticEditBackendSelection::Lsp => {
            return crate::tools::semantic_edit::rename_symbol_with_lsp_backend(state, arguments);
        }
        crate::tools::semantic_edit::SemanticEditBackendSelection::JetBrains
        | crate::tools::semantic_edit::SemanticEditBackendSelection::Roslyn => {
            return crate::tools::semantic_adapter::rename_with_local_adapter(
                state,
                arguments,
                crate::tools::semantic_edit::selected_backend(arguments)?,
            );
        }
        crate::tools::semantic_edit::SemanticEditBackendSelection::TreeSitter => {}
    }

    let file_path = required_string(arguments, "file_path")?;
    let symbol_name = arguments
        .get("symbol_name")
        .or_else(|| arguments.get("name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("symbol_name or name".into()))?;
    let new_name = required_string(arguments, "new_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let scope = match arguments.get("scope").and_then(|v| v.as_str()) {
        Some("file") => rename::RenameScope::File,
        _ => rename::RenameScope::Project,
    };
    let dry_run_requested = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Phase 0 강등: tree-sitter rename은 preview-only.
    // dry_run=false면 즉시 Validation으로 거부 — fail-closed.
    if !dry_run_requested {
        return Err(CodeLensError::Validation(
            "tree-sitter rename is preview-only; \
             select semantic_edit_backend=lsp (or jetbrains/roslyn) to apply".into()
        ));
    }

    let preview = rename::rename_symbol(
        &state.project(),
        file_path,
        symbol_name,
        new_name,
        name_path,
        scope,
        true, // 강제 dry_run=true
    )?;

    let mut value = json!(preview);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("authority".to_owned(), json!("syntax"));
        obj.insert("can_preview".to_owned(), json!(true));
        obj.insert("can_apply".to_owned(), json!(false));
        obj.insert("support".to_owned(), json!("syntax_preview"));
        obj.insert(
            "blocker_reason".to_owned(),
            json!(
                "tree-sitter rename is preview-only; \
                 select semantic_edit_backend=lsp (or jetbrains/roslyn) to apply"
            ),
        );
        obj.insert(
            "edit_authority".to_owned(),
            json!({
                "kind": "raw_fs",
                "operation": "rename_symbol",
                "validator": Value::Null,
            }),
        );
        obj.insert(
            "suggested_next_tools".to_owned(),
            json!([
                "rename_symbol with semantic_edit_backend=lsp",
                "verify_change_readiness"
            ]),
        );
    }

    Ok((value, success_meta(BackendKind::TreeSitter, 0.90)))
}
```

- [ ] **Step 4: 다시 실행 — 3 test 통과 + 회귀 0**

```bash
cargo test -p codelens-mcp --no-default-features tree_sitter_rename 2>&1 | tail -10
cargo test -p codelens-mcp --no-default-features unset_backend_apply 2>&1 | tail -5
cargo test -p codelens-mcp --no-default-features 2>&1 | tail -5
cargo test -p codelens-mcp --features http 2>&1 | tail -5
```

Expected: 3 신규 PASS. 기존 mutation/integration test에서 `rename_symbol` apply 시도하는 케이스가 있으면 해당 테스트가 새 contract와 충돌할 수 있음 — 발견 시 해당 test에 `dry_run=true` 또는 `semantic_edit_backend=lsp` 명시 보강.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/mutation.rs \
        crates/codelens-mcp/src/integration_tests/mutation_envelope.rs
git commit -m "$(cat <<'EOF'
feat(mcp): downgrade tree-sitter rename to preview-only

dry_run=false (or unset) on tree-sitter backend now returns
Validation error pointing to semantic_edit_backend=lsp. dry_run=true
returns preview envelope with can_apply=false, support=syntax_preview,
blocker_reason. semantic LSP/JetBrains/Roslyn paths unchanged.

Closes Phase 0 G2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: `--print-operation-matrix` CLI flag (단일 출처)

**Files:**

- Modify: `crates/codelens-mcp/src/main.rs:226` 부근

- [ ] **Step 1: smoke test 작성 (Bash, plan 검증용)**

작성할 검증 명령:

```bash
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
test -s /tmp/operation-matrix.json
python3 -c "import json; d=json.load(open('/tmp/operation-matrix.json')); assert d['schema']=='codelens-semantic-operation-matrix-v1', d['schema']; assert isinstance(d['operations'], list); assert len(d['operations']) >= 13, len(d['operations'])"
```

이 명령은 implementation 후 Step 4에서 사용.

- [ ] **Step 2: `--print-operation-matrix` flag 추가**

`crates/codelens-mcp/src/main.rs:226-233`의 `--print-surface-manifest` 블록 바로 아래에 추가:

```rust
    if args.iter().any(|arg| arg == "--print-surface-manifest") {
        let surface = profile
            .map(ToolSurface::Profile)
            .unwrap_or_else(|| ToolSurface::Preset(preset));
        let manifest = surface_manifest::build_surface_manifest(surface, daemon_mode);
        println!("{}", serde_json::to_string_pretty(&manifest)?);
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--print-operation-matrix") {
        let matrix =
            crate::backend_operation_matrix::semantic_edit_operation_matrix();
        println!("{}", serde_json::to_string_pretty(&matrix)?);
        return Ok(());
    }
```

`backend_operation_matrix` 모듈이 `pub` 또는 `pub(crate)`로 노출됐는지 확인. 노출돼있지 않다면 `crates/codelens-mcp/src/lib.rs`(또는 `main.rs`의 `mod` 선언부)에서 가시성 조정. 현재 `backend_operation_matrix.rs` 내부 함수는 `pub(crate)`이므로 같은 crate 안의 `main.rs`에서 호출 가능.

- [ ] **Step 3: 컴파일 + flag 동작 확인**

```bash
cargo check -p codelens-mcp --features http
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
python3 -c "import json; d=json.load(open('/tmp/operation-matrix.json')); print('schema:', d['schema']); print('ops:', len(d['operations'])); print('first:', d['operations'][0]['operation'], d['operations'][0]['backend'])"
```

Expected:

```
schema: codelens-semantic-operation-matrix-v1
ops: 14
first: rename tree-sitter
```

- [ ] **Step 4: 회귀 가드**

```bash
cargo run -q -p codelens-mcp --features http -- --print-surface-manifest 2>&1 | head -3
```

Expected: 기존 surface manifest 출력 그대로.

```bash
cargo test -p codelens-mcp --features http 2>&1 | tail -5
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/main.rs
git commit -m "$(cat <<'EOF'
feat(mcp): expose --print-operation-matrix CLI flag

single source of truth for the semantic operation/backend/authority
matrix consumed by surface-manifest.py contract checks. mirrors the
existing --print-surface-manifest flag pattern.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: `surface-manifest.py` contract A — `verified=false && can_apply=true` reject (TDD)

**Files:**

- Create: `scripts/test/test-surface-manifest-contracts.py`
- Modify: `scripts/surface-manifest.py`

- [ ] **Step 1: fixture test 작성**

`scripts/test/test-surface-manifest-contracts.py` 신규:

```python
#!/usr/bin/env python3
"""Tests for surface-manifest.py contract A and B (Phase 0 mutation surface truthing)."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SURFACE_MANIFEST = REPO_ROOT / "scripts" / "surface-manifest.py"

VALID_MATRIX = {
    "schema": "codelens-semantic-operation-matrix-v1",
    "tier1_languages": ["rust", "typescript", "javascript", "java"],
    "operations": [
        {
            "operation": "rename",
            "backend": "tree-sitter",
            "languages": ["rust"],
            "support": "syntax_preview",
            "authority": "syntax",
            "can_preview": True,
            "can_apply": False,
            "verified": True,
            "blocker_reason": "tree-sitter rename is syntax-scoped evidence",
            "required_methods": [],
            "failure_policy": "fail_closed",
        },
        {
            "operation": "rename",
            "backend": "lsp",
            "languages": ["rust", "typescript", "javascript", "java"],
            "support": "authoritative_apply",
            "authority": "workspace_edit",
            "can_preview": True,
            "can_apply": True,
            "verified": True,
            "blocker_reason": None,
            "required_methods": ["textDocument/rename"],
            "failure_policy": "fail_closed",
        },
    ],
}


def run_contract_check(matrix: dict) -> subprocess.CompletedProcess:
    """Run surface-manifest.py --check-operation-matrix against a temp matrix file."""
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".json", delete=False, encoding="utf-8"
    ) as f:
        json.dump(matrix, f)
        matrix_path = f.name
    try:
        return subprocess.run(
            [
                sys.executable,
                str(SURFACE_MANIFEST),
                "--check-operation-matrix",
                matrix_path,
            ],
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        Path(matrix_path).unlink(missing_ok=True)


def test_valid_matrix_passes() -> None:
    proc = run_contract_check(VALID_MATRIX)
    assert proc.returncode == 0, f"valid matrix should pass: stderr={proc.stderr}"


def test_contract_a_verified_false_can_apply_true_rejected() -> None:
    bad = json.loads(json.dumps(VALID_MATRIX))
    bad["operations"].append(
        {
            "operation": "extract_function",
            "backend": "lsp",
            "languages": ["rust"],
            "support": "conditional_authoritative_apply",
            "authority": "workspace_edit",
            "can_preview": True,
            "can_apply": True,  # contract A 위반
            "verified": False,  # ↑
            "blocker_reason": "fixture coverage missing",
            "required_methods": [],
            "failure_policy": "fail_closed",
        }
    )
    proc = run_contract_check(bad)
    assert proc.returncode == 1, (
        f"expected exit 1 for contract A violation, got {proc.returncode}"
    )
    assert "extract_function" in proc.stderr or "extract_function" in proc.stdout, (
        f"expected violation to enumerate extract_function: {proc.stderr} {proc.stdout}"
    )


def main() -> int:
    failures: list[str] = []
    for name, fn in [
        ("valid_matrix_passes", test_valid_matrix_passes),
        ("contract_a_violation_rejected", test_contract_a_verified_false_can_apply_true_rejected),
    ]:
        try:
            fn()
            print(f"PASS  {name}")
        except AssertionError as exc:
            print(f"FAIL  {name}: {exc}")
            failures.append(name)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: 실행 — fail 확인 (`--check-operation-matrix` 미구현)**

```bash
python3 scripts/test/test-surface-manifest-contracts.py
```

Expected: 두 test 모두 `proc.returncode != 0` 형태로 fail (flag 미인식).

- [ ] **Step 3: `surface-manifest.py`에 contract A 추가**

`scripts/surface-manifest.py` 끝의 `def main()` 위에 추가:

```python
OPERATION_MATRIX_REQUIRED_FIELDS = [
    "operation",
    "backend",
    "languages",
    "support",
    "authority",
    "can_preview",
    "can_apply",
    "verified",
    "required_methods",
    "failure_policy",
]


def check_operation_matrix(matrix_path: Path) -> list[str]:
    """Return a list of violation messages. Empty list = pass."""
    violations: list[str] = []
    try:
        matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        return [f"failed to load operation matrix from {matrix_path}: {exc}"]

    operations = matrix.get("operations")
    if not isinstance(operations, list):
        return [f"operation matrix missing 'operations' list (got {type(operations).__name__})"]

    for index, op in enumerate(operations):
        if not isinstance(op, dict):
            violations.append(f"operations[{index}] is not an object")
            continue

        # Contract A: verified=false && can_apply=true must never coexist
        if op.get("can_apply") is True and op.get("verified") is False:
            ident = f"{op.get('operation')}/{op.get('backend')}"
            violations.append(
                f"contract A violation: {ident} (operations[{index}]) "
                "advertises can_apply=true but verified=false — "
                "fail_closed requires verified evidence before can_apply"
            )

    return violations
```

`main()`을 contract sub-mode 지원하도록 확장:

```python
def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="write docs/generated/surface-manifest.json and refresh generated doc blocks",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="check docs/generated/surface-manifest.json and generated doc blocks for drift",
    )
    parser.add_argument(
        "--check-operation-matrix",
        type=Path,
        default=None,
        metavar="PATH",
        help="check the semantic operation matrix JSON for contract violations",
    )
    args = parser.parse_args()

    if args.check_operation_matrix is not None:
        violations = check_operation_matrix(args.check_operation_matrix)
        if violations:
            print("operation matrix contract violations:")
            for violation in violations:
                print(f"- {violation}", file=sys.stderr)
            raise SystemExit(1)
        return

    manifest = load_manifest()
    expected = expected_files(manifest)
    # ... 기존 로직 그대로
```

(`def main()` 안의 기존 manifest drift 검사 로직은 그대로 유지. `--check-operation-matrix`는 별도 sub-mode로 동작 후 즉시 return.)

- [ ] **Step 4: 다시 실행 — 통과**

```bash
python3 scripts/test/test-surface-manifest-contracts.py
```

Expected:

```
PASS  valid_matrix_passes
PASS  contract_a_violation_rejected
```

- [ ] **Step 5: production matrix 회귀 가드**

```bash
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json
```

Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add scripts/surface-manifest.py scripts/test/test-surface-manifest-contracts.py
git commit -m "$(cat <<'EOF'
feat(scripts): add operation matrix contract A (verified-or-can-apply)

surface-manifest.py --check-operation-matrix mode rejects matrix rows
where can_apply=true && verified=false — fail-closed requires verified
evidence before authoritative apply. fixture suite added under
scripts/test/test-surface-manifest-contracts.py.

Closes Phase 0 G6 (contract A).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: contract B — matrix 내부 정합성 (TDD)

**Goal**: matrix 자체의 well-formedness 검사. 각 row가 필수 필드 보유, `failure_policy=fail_closed`, `support` enum 안에 있음, `authoritative_apply`인데 `can_apply=false` 같은 모순 차단.

(Note: spec §2 C는 "matrix↔manifest 1:1"을 언급했지만, Phase 0에서 operation→tool mapping을 정식 정의하기 어려움. Phase 0은 matrix 자체 정합성으로 먼저 닫고, 1:1 mapping은 Phase 1에서 G5와 함께 정식 도입.)

**Files:**

- Modify: `scripts/test/test-surface-manifest-contracts.py`
- Modify: `scripts/surface-manifest.py`

- [ ] **Step 1: contract B 실패 fixture test 추가**

`test-surface-manifest-contracts.py` 끝에 추가 (main() 직전):

```python
def test_contract_b_missing_required_field_rejected() -> None:
    bad = json.loads(json.dumps(VALID_MATRIX))
    del bad["operations"][0]["failure_policy"]
    proc = run_contract_check(bad)
    assert proc.returncode == 1, (
        f"expected exit 1 for missing failure_policy, got {proc.returncode}"
    )
    assert "failure_policy" in proc.stderr, (
        f"expected violation to mention failure_policy: {proc.stderr}"
    )


def test_contract_b_failure_policy_must_be_fail_closed() -> None:
    bad = json.loads(json.dumps(VALID_MATRIX))
    bad["operations"][0]["failure_policy"] = "best_effort"
    proc = run_contract_check(bad)
    assert proc.returncode == 1, (
        f"expected exit 1 for non-fail-closed policy, got {proc.returncode}"
    )
    assert "fail_closed" in proc.stderr, (
        f"expected violation to mention fail_closed: {proc.stderr}"
    )


def test_contract_b_authoritative_apply_implies_can_apply_true() -> None:
    bad = json.loads(json.dumps(VALID_MATRIX))
    # support=authoritative_apply 인데 can_apply=false 모순
    bad["operations"][1]["can_apply"] = False
    proc = run_contract_check(bad)
    assert proc.returncode == 1, (
        f"expected exit 1 for authoritative_apply contradiction, got {proc.returncode}"
    )
    assert "authoritative_apply" in proc.stderr, (
        f"expected violation to mention authoritative_apply: {proc.stderr}"
    )
```

`main()`의 test 리스트에 3개 등록:

```python
    for name, fn in [
        ("valid_matrix_passes", test_valid_matrix_passes),
        ("contract_a_violation_rejected", test_contract_a_verified_false_can_apply_true_rejected),
        ("contract_b_missing_field_rejected", test_contract_b_missing_required_field_rejected),
        ("contract_b_failure_policy_enum", test_contract_b_failure_policy_must_be_fail_closed),
        ("contract_b_authoritative_apply_consistency", test_contract_b_authoritative_apply_implies_can_apply_true),
    ]:
```

- [ ] **Step 2: 실행 — 3 신규 test fail 확인**

```bash
python3 scripts/test/test-surface-manifest-contracts.py
```

Expected: 기존 2 PASS + 신규 3 FAIL.

- [ ] **Step 3: `surface-manifest.py` `check_operation_matrix`에 contract B 추가**

`SUPPORT_ENUM = {"syntax_preview", "authoritative_apply", "authoritative_check", "conditional_authoritative_apply", "evidence_only", "guarded_syntax_apply"}` 상수를 `OPERATION_MATRIX_REQUIRED_FIELDS` 옆에 추가.

`check_operation_matrix` 안의 for loop에 contract B 검사 추가:

```python
    for index, op in enumerate(operations):
        if not isinstance(op, dict):
            violations.append(f"operations[{index}] is not an object")
            continue

        ident = f"{op.get('operation')}/{op.get('backend')}"

        # Contract B-1: 모든 필수 필드 존재
        for field in OPERATION_MATRIX_REQUIRED_FIELDS:
            if field not in op:
                violations.append(
                    f"contract B violation: {ident} (operations[{index}]) "
                    f"missing required field {field!r}"
                )

        # Contract B-2: failure_policy == "fail_closed"
        if op.get("failure_policy") not in (None, "fail_closed"):
            violations.append(
                f"contract B violation: {ident} (operations[{index}]) "
                f"has failure_policy={op.get('failure_policy')!r} but "
                f"only 'fail_closed' is allowed"
            )

        # Contract B-3: support 값이 enum 안에 있음
        support = op.get("support")
        if support is not None and support not in SUPPORT_ENUM:
            violations.append(
                f"contract B violation: {ident} (operations[{index}]) "
                f"has support={support!r} not in {sorted(SUPPORT_ENUM)}"
            )

        # Contract B-4: support="authoritative_apply" → can_apply=true && verified=true
        if support == "authoritative_apply":
            if op.get("can_apply") is not True:
                violations.append(
                    f"contract B violation: {ident} (operations[{index}]) "
                    f"support=authoritative_apply but can_apply!=true"
                )
            if op.get("verified") is not True:
                violations.append(
                    f"contract B violation: {ident} (operations[{index}]) "
                    f"support=authoritative_apply but verified!=true"
                )

        # Contract B-5: languages 리스트 비어있지 않음
        languages = op.get("languages")
        if isinstance(languages, list) and not languages:
            violations.append(
                f"contract B violation: {ident} (operations[{index}]) "
                f"languages list is empty"
            )

        # Contract A (재배치): verified=false && can_apply=true must never coexist
        if op.get("can_apply") is True and op.get("verified") is False:
            violations.append(
                f"contract A violation: {ident} (operations[{index}]) "
                "advertises can_apply=true but verified=false — "
                "fail_closed requires verified evidence before can_apply"
            )

    return violations
```

(주의: contract A 검사는 기존에 있던 것을 새 위치로 이동, 중복 제거.)

- [ ] **Step 4: 다시 실행 — 5 test 모두 통과**

```bash
python3 scripts/test/test-surface-manifest-contracts.py
```

Expected:

```
PASS  valid_matrix_passes
PASS  contract_a_violation_rejected
PASS  contract_b_missing_field_rejected
PASS  contract_b_failure_policy_enum
PASS  contract_b_authoritative_apply_consistency
```

- [ ] **Step 5: production matrix 회귀 가드**

```bash
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json
echo "exit=$?"
```

Expected: `exit=0`. (production matrix가 contract B를 위반하지 않는지 확인 — 위반 발견 시 matrix 데이터 자체를 손볼 게 아니라 contract 정의가 너무 엄격한 것은 아닌지 재검토.)

- [ ] **Step 6: Commit**

```bash
git add scripts/surface-manifest.py scripts/test/test-surface-manifest-contracts.py
git commit -m "$(cat <<'EOF'
feat(scripts): add operation matrix contract B (internal consistency)

contract B enforces matrix well-formedness: required fields,
failure_policy=fail_closed, support enum, authoritative_apply
implies can_apply=true && verified=true, non-empty languages.
Phase 0 scope; full matrix<->manifest 1:1 mapping deferred to
Phase 1 alongside runtime capability probing (G5).

Closes Phase 0 G6 (contract B internal consistency).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: CI integration — 새 contract test step

**Files:**

- Modify: `.github/workflows/ci.yml:75-78` 부근 (`surface manifest drift check` 다음에 새 step 2개 추가)

- [ ] **Step 1: ci.yml 수정**

`surface manifest drift check` step(line 75-78) 직후, `cargo test (mcp + http)` step 직전에 추가:

```yaml
- name: surface manifest drift check
  if: matrix.os == 'ubuntu-latest'
  shell: bash
  run: python3 scripts/surface-manifest.py

- name: operation matrix dump and contract check
  if: matrix.os == 'ubuntu-latest'
  shell: bash
  run: |
    cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
    python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json

- name: surface manifest contract fixtures
  if: matrix.os == 'ubuntu-latest'
  shell: bash
  run: python3 scripts/test/test-surface-manifest-contracts.py

- name: cargo test (mcp + http)
  if: matrix.os == 'ubuntu-latest'
  run: cargo test -p codelens-mcp --features http
```

- [ ] **Step 2: 로컬에서 CI step 시뮬레이션**

```bash
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json
echo "matrix-check-exit=$?"
python3 scripts/test/test-surface-manifest-contracts.py
echo "fixtures-exit=$?"
```

Expected: 둘 다 `exit=0`.

- [ ] **Step 3: yaml syntax 검증**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"
```

Expected: parse 성공, 출력 없음.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci: gate operation matrix contract A+B in CI

new ci.yml steps run --print-operation-matrix and pipe through
surface-manifest.py --check-operation-matrix, plus the contract
fixture suite under scripts/test. matches local pre-merge gate.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Final verification + evaluator dispatch

**Files:** none (verification only)

- [ ] **Step 1: 전체 cargo test + clippy**

```bash
cargo check --workspace
cargo test -p codelens-engine 2>&1 | tail -5
cargo test -p codelens-mcp --no-default-features 2>&1 | tail -5
cargo test -p codelens-mcp 2>&1 | tail -5
cargo test -p codelens-mcp --features http 2>&1 | tail -5
cargo clippy -- -W clippy::all 2>&1 | tail -10
```

Expected:

- engine ≥ 295 PASS, fail 0
- mcp default ≥ 501 PASS, fail 0 (실제 baseline은 commit 시점 측정값으로 대체)
- mcp http PASS
- clippy 신규 warning 0

- [ ] **Step 2: production surface-manifest + contract 게이트**

```bash
python3 scripts/surface-manifest.py
echo "drift-exit=$?"
cargo run -q -p codelens-mcp --features http -- --print-operation-matrix > /tmp/operation-matrix.json
python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json
echo "matrix-exit=$?"
python3 scripts/test/test-surface-manifest-contracts.py
echo "fixtures-exit=$?"
```

Expected: 모든 exit code 0.

- [ ] **Step 3: lint-datasets + agent-contract-check**

```bash
python3 benchmarks/lint-datasets.py --project .
python3 scripts/agent-contract-check.py --project . --strict
```

Expected: exit 0.

- [ ] **Step 4: AC checklist 자기 점검**

각 AC를 직접 grep/확인:

```bash
# AC-1: 11 raw primitive 4필드
grep -n "raw_fs_envelope\|merge_raw_fs_envelope" crates/codelens-mcp/src/tools/mutation.rs | wc -l
# AC-2: confidence 0.7
grep -n "BackendKind::Filesystem, 1.0" crates/codelens-mcp/src/tools/mutation.rs
# (출력 없어야 함)
grep -n "BackendKind::Filesystem, 0.7" crates/codelens-mcp/src/tools/mutation.rs | wc -l
# (≥ 5 라인)
# AC-3: tree-sitter rename 강등
grep -n "tree-sitter rename is preview-only" crates/codelens-mcp/src/tools/mutation.rs
# AC-4: contract A+B
grep -n "check_operation_matrix\|--check-operation-matrix" scripts/surface-manifest.py
# AC-5: --print-operation-matrix flag
grep -n "print-operation-matrix" crates/codelens-mcp/src/main.rs
# AC-7: 변경 파일 수
git diff --stat 0094f8f2..HEAD --name-only | wc -l
# (≤ 8)
```

각 출력이 기대값과 일치하는지 확인.

- [ ] **Step 5: evaluator(opus) 채점 dispatch**

evaluator agent를 spec §6 acceptance criteria 대비로 채점.

```text
Agent dispatch:
  subagent_type: evaluator
  model: opus
  description: "Phase 0 mutation surface PR acceptance scoring"
  prompt: |
    Spec: docs/superpowers/specs/2026-04-25-codelens-phase0-mutation-surface-design.md (§6)
    Plan: docs/superpowers/plans/2026-04-25-codelens-phase0-mutation-surface.md
    Branch HEAD vs base 0094f8f2의 diff와 test 결과를 종합해 AC-1~AC-8 각각을 PASS/FAIL/PARTIAL 채점.
    AC-1~AC-7 중 1개라도 FAIL이면 전체 FAIL — 머지 보류.
    출력 형식: 각 AC별 1줄 판정 + 증거 + 전체 verdict.
```

evaluator 결과가 PASS이면 PR 준비 완료, FAIL이면 갭 패치 후 재채점.

- [ ] **Step 6: PR 본문 작성 (선택)**

evaluator PASS 후 사용자 승인 시:

```bash
gh pr create --base main --head codex/ide-refactor-substrate \
  --title "Phase 0: mutation surface truthing (option B)" \
  --body "$(cat <<'EOF'
## Summary
- raw mutation 11 primitives now advertise `authority`/`can_preview`/`can_apply`/`edit_authority`
- tree-sitter `rename_symbol` downgraded to preview-only (Validation on apply)
- `--print-operation-matrix` CLI flag (single source of truth)
- `surface-manifest.py` contract A (verified-or-can-apply) + B (matrix internal consistency)
- CI gates: contract A/B + fixture suite

Spec: `docs/superpowers/specs/2026-04-25-codelens-phase0-mutation-surface-design.md`
Plan: `docs/superpowers/plans/2026-04-25-codelens-phase0-mutation-surface.md`

Closes Phase 0 gaps G1, G2, G3, G6. G4/G5/G7 deferred to Phase 1.

## Test plan
- [x] `cargo test -p codelens-engine`
- [x] `cargo test -p codelens-mcp --features http`
- [x] `python3 scripts/surface-manifest.py`
- [x] `python3 scripts/surface-manifest.py --check-operation-matrix /tmp/operation-matrix.json`
- [x] `python3 scripts/test/test-surface-manifest-contracts.py`
- [x] evaluator(opus) AC-1~AC-8 PASS

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

(사용자가 명시 승인 전에는 push/pr 만들지 말 것 — 글로벌 CLAUDE.md "actions visible to others" 정책.)

---

## Self-Review (writing-plans skill 강제)

**1. Spec coverage**:

| Spec 항목                                             | Plan task             |
| ----------------------------------------------------- | --------------------- |
| §1 사용자 변경 — 11 primitive 4필드                   | Task 1                |
| §1 사용자 변경 — tree-sitter rename `can_apply=false` | Task 3                |
| §1 사용자 변경 — surface-manifest CI fail             | Task 5, 6, 7          |
| §2 A `mutation.rs` raw_fs_envelope                    | Task 1                |
| §2 B confidence 강등                                  | Task 2                |
| §2 C surface-manifest contract A+B                    | Task 5, 6             |
| §2 D mutation_envelope.rs                             | Task 1, 2, 3          |
| §2 E mod.rs 등록                                      | Task 1 Step 2         |
| §2 F test-surface-manifest-contracts.py               | Task 5, 6             |
| §2 G ci.yml                                           | Task 7                |
| §2 H matrix 단일 출처 (refined to flag)               | Task 4                |
| §6 AC-1~AC-8                                          | Task 8 (verification) |

전 항목 커버됨.

**2. Placeholder scan**: TBD/TODO/"add appropriate" 패턴 0건. 모든 step에 실제 명령·코드·예상 출력 포함.

**3. Type consistency**:

- `raw_fs_envelope`/`merge_raw_fs_envelope` 시그니처 Task 1 Step 4에 정의, Step 5에서 그대로 사용. ✅
- `success_meta(BackendKind::Filesystem, 0.7)` Task 2 Step 3에서 통일. ✅
- `--print-operation-matrix` flag 이름 Task 4·5·6·7에서 동일 사용. ✅
- `--check-operation-matrix` argparse 인자 Task 5·6·7 동일. ✅

**4. 알려진 trade-off**:

- spec §2 C "matrix↔manifest 1:1"을 Phase 0에서 "matrix 내부 정합성"으로 좁힘. Plan §0 refinements + Task 6 Note에 명시. 완전한 1:1 mapping은 Phase 1.
- spec §2 H "신규 binary `dump-matrix.rs`" → CLI flag로 변경. Plan §0 refinements + Task 4에 명시.

---

## Execution Handoff

**Plan complete and saved to `docs/superpowers/plans/2026-04-25-codelens-phase0-mutation-surface.md`. Two execution options:**

**1. Subagent-Driven (recommended)** — Task별 builder agent 분기, 각 Task 완료 시 평가/리뷰. 8 task = 약 6~10 dispatch (verification은 별도). worktree 격리.

**2. Inline Execution** — 이 세션에서 executing-plans 스킬로 batch 실행, 중간 checkpoint에서 사용자 리뷰.

**Which approach?**
