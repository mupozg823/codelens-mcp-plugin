//! Canonical resolved-operation identity used by routing guidance and metrics.

use serde::Serialize;

use crate::protocol::ToolTier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationWorkClass {
    Unresolved,
    Primitive,
    Composite,
}

impl OperationWorkClass {
    pub(crate) fn is_composite(self) -> bool {
        matches!(self, Self::Composite)
    }

    pub(crate) fn is_primitive(self) -> bool {
        matches!(self, Self::Primitive)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unresolved => "unresolved",
            Self::Primitive => "primitive",
            Self::Composite => "composite",
        }
    }
}

/// Facts retained after a public tool name is resolved to its executable target.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResolvedOperation<'a> {
    pub(crate) target: Option<&'a str>,
    pub(crate) mode: Option<&'a str>,
    pub(crate) work_class: OperationWorkClass,
    /// Number of resolved target-handler entries for this outer request.
    pub(crate) downstream_call_count: u64,
}

impl<'a> ResolvedOperation<'a> {
    pub(crate) fn resolved(target: &'a str, mode: Option<&'a str>) -> Self {
        Self {
            target: Some(target),
            mode,
            work_class: operation_work_class(target),
            downstream_call_count: 0,
        }
    }

    pub(crate) fn direct(tool: &'a str) -> Self {
        Self::resolved(tool, None)
    }

    pub(crate) fn unresolved(mode: Option<&'a str>) -> Self {
        Self {
            target: None,
            mode,
            work_class: OperationWorkClass::Unresolved,
            downstream_call_count: 0,
        }
    }

    /// Resolve request metadata without dispatching or rewriting its arguments.
    pub(crate) fn from_request(public_tool: &'a str, arguments: &'a serde_json::Value) -> Self {
        match crate::tools::verbs::resolve_verb_operation(public_tool, arguments) {
            Ok(Some((target, mode))) => Self::resolved(target, Some(mode)),
            Ok(None) => Self::direct(public_tool),
            Err(_) => Self::unresolved(arguments.get("mode").and_then(serde_json::Value::as_str)),
        }
    }

    pub(crate) fn dispatched(mut self) -> Self {
        self.downstream_call_count = 1;
        self
    }
}

/// The single composite/primitive classifier.
///
/// Callers must pass the resolved executable target, never a facade spelling.
pub(crate) fn operation_work_class(target: &str) -> OperationWorkClass {
    match crate::tool_defs::tool_tier(target) {
        ToolTier::Workflow => OperationWorkClass::Composite,
        ToolTier::Primitive | ToolTier::Analysis => OperationWorkClass::Primitive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_metadata_inherits_the_resolved_target_class() {
        let primitive = ResolvedOperation::resolved("find_symbol", Some("symbol"));
        assert_eq!(primitive.target, Some("find_symbol"));
        assert_eq!(primitive.mode, Some("symbol"));
        assert_eq!(primitive.work_class, OperationWorkClass::Primitive);
        assert_eq!(primitive.downstream_call_count, 0);

        let composite = ResolvedOperation::resolved("explore_codebase", Some("explore"));
        assert_eq!(composite.work_class, OperationWorkClass::Composite);
        assert_eq!(composite.dispatched().downstream_call_count, 1);
    }

    #[test]
    fn request_metadata_distinguishes_resolved_and_unresolved_facades() {
        let resolved_args = serde_json::json!({"mode": "symbol", "name": "target"});
        let resolved = ResolvedOperation::from_request("search", &resolved_args);
        assert_eq!(resolved.target, Some("find_symbol"));
        assert_eq!(resolved.mode, Some("symbol"));
        assert_eq!(resolved.work_class, OperationWorkClass::Primitive);
        assert_eq!(resolved.downstream_call_count, 0);

        let unresolved_args = serde_json::json!({"mode": "unknown"});
        let unresolved = ResolvedOperation::from_request("search", &unresolved_args);
        assert_eq!(unresolved.target, None);
        assert_eq!(unresolved.mode, Some("unknown"));
        assert_eq!(unresolved.work_class, OperationWorkClass::Unresolved);
        assert_eq!(unresolved.downstream_call_count, 0);
    }

    #[test]
    fn operation_work_class_comes_from_the_target_registry_tier() {
        for primitive in [
            "find_symbol",
            "get_callers",
            "get_analysis_section",
            "tools/list",
        ] {
            assert_eq!(
                operation_work_class(primitive),
                OperationWorkClass::Primitive,
                "{primitive} must remain primitive"
            );
        }
        for composite in ["explore_codebase", "onboard_project"] {
            assert_eq!(
                operation_work_class(composite),
                OperationWorkClass::Composite,
                "{composite} must remain composite"
            );
        }

        assert_eq!(
            ResolvedOperation::resolved("find_symbol", Some("symbol")).work_class,
            OperationWorkClass::Primitive,
            "the search facade's workflow tier must not override find_symbol"
        );
    }
}
