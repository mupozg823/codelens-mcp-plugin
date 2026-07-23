use super::{AppState, ToolResult, required_string, success_meta};
use crate::protocol::BackendKind;
use codelens_engine::call_graph::api::ResolvedCallGraph;
use codelens_engine::call_graph::types::CallTargetIdentity;
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TraceNode {
    // Canonical name plus definition file keeps aliases and same-named
    // functions on distinct BFS paths.
    file: Option<String>,
    symbol: String,
    declaration_path: Option<String>,
}

impl TraceNode {
    fn identity(&self) -> CallTargetIdentity {
        CallTargetIdentity {
            canonical_name: self.symbol.clone(),
            resolved_file: self.file.clone(),
            declaration_path: self.declaration_path.clone(),
        }
    }
}

fn scoped_file(project: &codelens_engine::ProjectRoot, path: Option<&str>) -> Option<String> {
    path.and_then(|path| project.resolve(path).ok())
        .filter(|resolved| resolved.is_file())
        .map(|resolved| project.to_relative(resolved))
}

fn is_root_node(node: &TraceNode, root: &TraceNode) -> bool {
    node.symbol == root.symbol
        && (root.file.is_none() || node.file.as_ref() == root.file.as_ref())
        && (root.declaration_path.is_none()
            || node.declaration_path.as_ref() == root.declaration_path.as_ref())
}

fn reached_limit(result_count: usize, max_results: usize) -> bool {
    max_results > 0 && result_count >= max_results
}

pub fn call_graph_flow(state: &AppState, arguments: &Value) -> ToolResult {
    let function_name = required_string(arguments, "function_name")?;
    let path = arguments.get("path").and_then(Value::as_str);
    let max_depth = arguments
        .get("max_depth")
        .and_then(Value::as_u64)
        .unwrap_or(3) as usize;
    let max_results = arguments
        .get("max_results")
        .and_then(Value::as_u64)
        .unwrap_or(20) as usize;

    let project = state.project();
    let graph_cache = state.graph_cache();
    let mut call_graph = ResolvedCallGraph::build(&project, path, Some(graph_cache.as_ref()))?;
    let root_file = scoped_file(&project, path);
    let root_node = TraceNode {
        file: root_file.clone(),
        symbol: function_name.to_owned(),
        declaration_path: None,
    };

    let mut callers = Vec::new();
    let mut caller_queue = VecDeque::from([(root_node.clone(), 0usize)]);
    let mut visited_caller_nodes = HashSet::from([root_node.clone()]);
    let mut seen_callers = HashSet::new();
    while let Some((node, depth)) = caller_queue.pop_front() {
        if depth >= max_depth || reached_limit(callers.len(), max_results) {
            continue;
        }

        let next_depth = depth + 1;
        let entries = call_graph.get_callers_for_identity(&node.identity(), 0);
        for entry in entries {
            let caller_identity = entry.caller_identity;
            let entry = entry.caller;
            let next_node = TraceNode {
                file: caller_identity.resolved_file,
                symbol: caller_identity.canonical_name,
                declaration_path: caller_identity.declaration_path,
            };
            if is_root_node(&next_node, &root_node) {
                continue;
            }
            if seen_callers.insert((entry.file.clone(), entry.function.clone())) {
                callers.push(json!({
                    "name": entry.function,
                    "file": entry.file,
                    "line": entry.line,
                    "depth": next_depth,
                    "confidence": entry.confidence,
                    "resolution": entry.resolution,
                }));
            }
            if next_depth < max_depth && visited_caller_nodes.insert(next_node.clone()) {
                caller_queue.push_back((next_node, next_depth));
            }
            if reached_limit(callers.len(), max_results) {
                break;
            }
        }
    }

    let mut callees = Vec::new();
    let mut callee_queue = VecDeque::from([(root_node.clone(), 0usize)]);
    let mut visited_callees = HashSet::from([root_node.clone()]);
    let mut seen_callees = HashSet::new();
    while let Some((node, depth)) = callee_queue.pop_front() {
        if depth >= max_depth || reached_limit(callees.len(), max_results) {
            continue;
        }

        let next_depth = depth + 1;
        let entries = call_graph.get_callees_for_source(&node.identity(), 0)?;
        for entry in entries {
            let target = entry.target;
            let entry = entry.callee;
            let next_node = TraceNode {
                // An unresolved callee is still tied to the current
                // definition file, preventing unresolved same-name calls in
                // another file from being merged into this node.
                file: target.resolved_file.clone().or_else(|| node.file.clone()),
                symbol: target.canonical_name,
                declaration_path: target.declaration_path,
            };
            if is_root_node(&next_node, &root_node) {
                continue;
            }
            if seen_callees.insert(next_node.clone()) {
                callees.push(json!({
                    "name": entry.name,
                    "file": entry.resolved_file,
                    "line": entry.line,
                    "depth": next_depth,
                    "confidence": entry.confidence,
                    "resolution": entry.resolution,
                }));
            }
            if next_depth < max_depth
                && (path.is_some() || next_node.file.is_some())
                && visited_callees.insert(next_node.clone())
            {
                callee_queue.push_back((next_node, next_depth));
            }
            if reached_limit(callees.len(), max_results) {
                break;
            }
        }
    }

    Ok((
        json!({
            "function": function_name,
            "path": path,
            "max_depth": max_depth,
            "max_results": max_results,
            "callers": callers,
            "caller_count": callers.len(),
            "callees": callees,
            "callee_count": callees.len(),
            "flow_summary": format!(
                "{} has {} caller(s) and {} callee(s) within depth {}",
                function_name,
                callers.len(),
                callees.len(),
                max_depth
            )
        }),
        success_meta(BackendKind::Hybrid, 0.90),
    ))
}
