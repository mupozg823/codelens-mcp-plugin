use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{WorkspaceModuleEdge, WorkspaceModuleGraph, WorkspaceModuleNode};

impl WorkspaceModuleGraph {
    pub(super) fn from_impact(workspace_members: Vec<String>, impact: &Value) -> Self {
        let mut modules = BTreeMap::new();
        for member in &workspace_members {
            modules.insert(
                member.clone(),
                WorkspaceModuleNode {
                    path: member.clone(),
                    workspace_member: true,
                    file_count: 0,
                },
            );
        }

        for file in strings_from_array(impact.get("in_scope_files")) {
            if let Some(module_path) = module_for_file(file, &workspace_members) {
                ensure_module(&mut modules, &workspace_members, &module_path).file_count += 1;
            }
        }

        let mut edge_pairs: BTreeMap<(String, String), BTreeSet<(String, String)>> =
            BTreeMap::new();
        collect_importer_edges(impact, &workspace_members, &mut modules, &mut edge_pairs);
        collect_blast_radius_edges(impact, &workspace_members, &mut modules, &mut edge_pairs);

        let mut modules = modules.into_values().collect::<Vec<_>>();
        modules.sort_by(|left, right| {
            right
                .workspace_member
                .cmp(&left.workspace_member)
                .then_with(|| left.path.cmp(&right.path))
        });

        let edges = edge_pairs
            .into_iter()
            .map(|((source, target), file_pairs)| WorkspaceModuleEdge {
                source,
                target,
                file_pair_count: file_pairs.len(),
            })
            .collect();

        Self::new(workspace_members.len(), modules, edges)
    }
}

pub(super) fn read_workspace_members(project_root: &Path) -> Option<Vec<String>> {
    let manifest = std::fs::read_to_string(project_root.join("Cargo.toml")).ok()?;
    let parsed = manifest.parse::<toml::Value>().ok()?;
    let members = parsed
        .get("workspace")?
        .get("members")?
        .as_array()?
        .iter()
        .filter_map(|member| member.as_str())
        .map(normalize_relative_path)
        .filter(|member| !member.is_empty() && !member.contains('*'))
        .collect::<BTreeSet<_>>();
    if members.is_empty() {
        return None;
    }
    Some(members.into_iter().collect())
}

fn collect_importer_edges(
    impact: &Value,
    workspace_members: &[String],
    modules: &mut BTreeMap<String, WorkspaceModuleNode>,
    edge_pairs: &mut BTreeMap<(String, String), BTreeSet<(String, String)>>,
) {
    for entry in value_array(impact.get("direct_importers")) {
        let Some(source_file) = entry.get("file").and_then(Value::as_str) else {
            continue;
        };
        for target_file in strings_from_array(entry.get("target_files")) {
            record_edge(
                workspace_members,
                modules,
                edge_pairs,
                source_file,
                target_file,
            );
        }
    }
}

fn collect_blast_radius_edges(
    impact: &Value,
    workspace_members: &[String],
    modules: &mut BTreeMap<String, WorkspaceModuleNode>,
    edge_pairs: &mut BTreeMap<(String, String), BTreeSet<(String, String)>>,
) {
    for entry in value_array(impact.get("blast_radius")) {
        let Some(target_file) = entry.get("file").and_then(Value::as_str) else {
            continue;
        };
        for source_file in strings_from_array(entry.get("source_files")) {
            record_edge(
                workspace_members,
                modules,
                edge_pairs,
                source_file,
                target_file,
            );
        }
    }
}

fn record_edge(
    workspace_members: &[String],
    modules: &mut BTreeMap<String, WorkspaceModuleNode>,
    edge_pairs: &mut BTreeMap<(String, String), BTreeSet<(String, String)>>,
    source_file: &str,
    target_file: &str,
) {
    let Some(source_module) = module_for_file(source_file, workspace_members) else {
        return;
    };
    let Some(target_module) = module_for_file(target_file, workspace_members) else {
        return;
    };
    if source_module == target_module {
        return;
    }
    ensure_module(modules, workspace_members, &source_module);
    ensure_module(modules, workspace_members, &target_module);
    edge_pairs
        .entry((source_module, target_module))
        .or_default()
        .insert((
            normalize_relative_path(source_file),
            normalize_relative_path(target_file),
        ));
}

fn ensure_module<'a>(
    modules: &'a mut BTreeMap<String, WorkspaceModuleNode>,
    workspace_members: &[String],
    module_path: &str,
) -> &'a mut WorkspaceModuleNode {
    modules
        .entry(module_path.to_owned())
        .or_insert_with(|| WorkspaceModuleNode {
            path: module_path.to_owned(),
            workspace_member: workspace_members.iter().any(|member| member == module_path),
            file_count: 0,
        })
}

pub(super) fn module_for_file(file_path: &str, workspace_members: &[String]) -> Option<String> {
    let normalized = normalize_relative_path(file_path);
    if normalized.is_empty() || normalized == "<unknown>" {
        return None;
    }
    let workspace_member = workspace_members
        .iter()
        .filter(|member| path_is_inside_module(&normalized, member))
        .max_by_key(|member| member.len());
    if let Some(member) = workspace_member {
        return Some(member.clone());
    }
    normalized
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
}

fn path_is_inside_module(path: &str, module_path: &str) -> bool {
    path == module_path
        || path
            .strip_prefix(module_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn strings_from_array(value: Option<&Value>) -> Vec<&str> {
    value_array(value)
        .into_iter()
        .filter_map(Value::as_str)
        .collect()
}

fn value_array(value: Option<&Value>) -> Vec<&Value> {
    value
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |values| values.iter().collect())
}

fn normalize_relative_path(path: &str) -> String {
    path.replace('\\', "/")
        .trim_start_matches("./")
        .trim_matches('/')
        .to_owned()
}
