use serde_json::{json, Value};

pub(crate) const TREE_SITTER_RENAME_BLOCKER_REASON: &str =
    "tree-sitter rename is preview-only; select semantic_edit_backend=lsp (or jetbrains/roslyn) to apply";

pub(crate) struct ProductCapabilityRegistry;

impl ProductCapabilityRegistry {
    pub(crate) fn operation_matrix(&self) -> Value {
        json!({
            "schema": "codelens-semantic-operation-matrix-v1",
            "tier1_languages": ["rust", "typescript", "javascript", "java"],
            "operations": semantic_edit_operation_descriptors()
        })
    }
}

pub(crate) fn product_capability_registry() -> ProductCapabilityRegistry {
    ProductCapabilityRegistry
}

pub(crate) fn semantic_edit_operation_matrix() -> Value {
    product_capability_registry().operation_matrix()
}

fn semantic_edit_operation_descriptors() -> Vec<Value> {
    let tier1 = json!(["rust", "typescript", "javascript", "java"]);
    vec![
        operation_descriptor(OperationDescriptorSpec {
            operation: "rename",
            backend: "tree-sitter",
            languages: json!(["rust", "typescript", "javascript", "java", "python", "go"]),
            support: "syntax_preview",
            authority: "syntax",
            can_preview: true,
            can_apply: false,
            verified: true,
            required_methods: json!([]),
            blocker_reason: TREE_SITTER_RENAME_BLOCKER_REASON,
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "rename",
            backend: "lsp",
            languages: tier1.clone(),
            support: "authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: true,
            verified: true,
            required_methods: json!(["textDocument/prepareRename", "textDocument/rename"]),
            blocker_reason: "",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "safe_delete_check",
            backend: "lsp",
            languages: tier1.clone(),
            support: "authoritative_check",
            authority: "semantic_readonly",
            can_preview: true,
            can_apply: false,
            verified: true,
            required_methods: json!(["textDocument/references"]),
            blocker_reason: "",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "safe_delete_apply",
            backend: "lsp",
            languages: tier1.clone(),
            support: "conditional_authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: false,
            verified: false,
            required_methods: json!(["textDocument/references", "tree_sitter_symbol_range"]),
            blocker_reason: "guarded apply needs per-language fixture coverage before it can be advertised as authoritative_apply",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "declaration",
            backend: "lsp",
            languages: tier1.clone(),
            support: "authoritative_check",
            authority: "semantic_readonly",
            can_preview: true,
            can_apply: false,
            verified: true,
            required_methods: json!(["textDocument/declaration"]),
            blocker_reason: "",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "definition",
            backend: "lsp",
            languages: tier1.clone(),
            support: "authoritative_check",
            authority: "semantic_readonly",
            can_preview: true,
            can_apply: false,
            verified: true,
            required_methods: json!(["textDocument/definition"]),
            blocker_reason: "",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "implementation",
            backend: "lsp",
            languages: tier1.clone(),
            support: "authoritative_check",
            authority: "semantic_readonly",
            can_preview: true,
            can_apply: false,
            verified: true,
            required_methods: json!(["textDocument/implementation"]),
            blocker_reason: "",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "type_definition",
            backend: "lsp",
            languages: tier1.clone(),
            support: "authoritative_check",
            authority: "semantic_readonly",
            can_preview: true,
            can_apply: false,
            verified: true,
            required_methods: json!(["textDocument/typeDefinition"]),
            blocker_reason: "",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "extract_function",
            backend: "lsp",
            languages: tier1.clone(),
            support: "conditional_authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: false,
            verified: false,
            required_methods: json!(["textDocument/codeAction", "codeAction/resolve"]),
            blocker_reason: "fixture-green inspectable WorkspaceEdit coverage is required before advertising authoritative_apply",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "inline_function",
            backend: "lsp",
            languages: tier1.clone(),
            support: "conditional_authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: false,
            verified: false,
            required_methods: json!(["textDocument/codeAction", "codeAction/resolve"]),
            blocker_reason: "fixture-green inspectable WorkspaceEdit coverage is required before advertising authoritative_apply",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "move_symbol",
            backend: "lsp",
            languages: tier1.clone(),
            support: "conditional_authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: false,
            verified: false,
            required_methods: json!(["textDocument/codeAction", "codeAction/resolve"]),
            blocker_reason: "fixture-green inspectable WorkspaceEdit coverage is required before advertising authoritative_apply",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "change_signature",
            backend: "lsp",
            languages: tier1.clone(),
            support: "conditional_authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: false,
            verified: false,
            required_methods: json!(["textDocument/codeAction", "codeAction/resolve"]),
            blocker_reason: "fixture-green inspectable WorkspaceEdit coverage is required before advertising authoritative_apply",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "rename",
            backend: "roslyn",
            languages: json!(["csharp"]),
            support: "conditional_authoritative_apply",
            authority: "workspace_edit",
            can_preview: true,
            can_apply: false,
            verified: false,
            required_methods: json!([
                "roslyn_workspace_adapter",
                "Microsoft.CodeAnalysis.Rename.Renamer"
            ]),
            blocker_reason: "Roslyn sidecar rename must pass release-artifact fixture gate before static matrix advertises authoritative_apply",
        }),
        operation_descriptor(OperationDescriptorSpec {
            operation: "references",
            backend: "scip",
            languages: json!(["rust", "typescript", "javascript", "java"]),
            support: "evidence_only",
            authority: "semantic_readonly",
            can_preview: false,
            can_apply: false,
            verified: true,
            required_methods: json!(["scip_index"]),
            blocker_reason: "SCIP is evidence only and never edit authority",
        }),
    ]
}

struct OperationDescriptorSpec {
    operation: &'static str,
    backend: &'static str,
    languages: Value,
    support: &'static str,
    authority: &'static str,
    can_preview: bool,
    can_apply: bool,
    verified: bool,
    required_methods: Value,
    blocker_reason: &'static str,
}

fn operation_descriptor(spec: OperationDescriptorSpec) -> Value {
    json!({
        "operation": spec.operation,
        "backend": spec.backend,
        "languages": spec.languages,
        "support": spec.support,
        "authority": spec.authority,
        "can_preview": spec.can_preview,
        "can_apply": spec.can_apply,
        "verified": spec.verified,
        "blocker_reason": if spec.blocker_reason.is_empty() {
            Value::Null
        } else {
            json!(spec.blocker_reason)
        },
        "required_methods": spec.required_methods,
        "failure_policy": "fail_closed"
    })
}
