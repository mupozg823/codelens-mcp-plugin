use crate::import_graph::GraphCache;
use crate::project::ProjectRoot;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use super::extract::extract_calls;
use super::js_imports::{build_js_import_binding_index, filter_external_import_edges};
use super::resolve::{collect_candidate_files, maybe_import_graph, resolve_call_edges};
use super::types::{
    CallEdge, CallTargetIdentity, CalleeEntry, CallerEntry, ResolvedCalleeEntry,
    ResolvedCallerEntry,
};

fn collect_scope_files(project: &ProjectRoot, path: Option<&str>) -> Result<Vec<PathBuf>> {
    let Some(path) = path else {
        return collect_candidate_files(project.as_path());
    };

    let resolved = project.resolve(path)?;
    if resolved.is_dir() {
        collect_candidate_files(&resolved)
    } else {
        Ok(vec![resolved])
    }
}

struct CallGraphSnapshot {
    edges: Vec<CallEdge>,
    files: HashSet<String>,
}

impl CallGraphSnapshot {
    fn build(
        project: &ProjectRoot,
        path: Option<&str>,
        graph_cache: Option<&GraphCache>,
    ) -> Result<Self> {
        let files = collect_scope_files(project, path)?;
        let scoped_files = files.iter().map(|file| project.to_relative(file)).collect();
        let mut edges = Vec::new();

        for file in &files {
            let mut file_edges = extract_calls(file);
            for edge in &mut file_edges {
                edge.caller_file = project.to_relative(file);
            }
            edges.extend(file_edges);
        }

        let import_bindings = build_js_import_binding_index(project, &files);
        filter_external_import_edges(&mut edges, &import_bindings);
        let import_graph = maybe_import_graph(project, &files, graph_cache);
        resolve_call_edges(
            &mut edges,
            project,
            import_graph.as_deref(),
            Some(&import_bindings),
        );

        Ok(Self {
            edges,
            files: scoped_files,
        })
    }

    fn get_callers_for_target(
        &self,
        function_name: &str,
        resolved_file: Option<&str>,
        max_results: usize,
    ) -> Vec<ResolvedCallerEntry> {
        self.get_callers_for_identity(
            &CallTargetIdentity {
                canonical_name: function_name.to_owned(),
                resolved_file: resolved_file.map(ToOwned::to_owned),
                declaration_path: None,
            },
            max_results,
        )
    }

    fn get_callers_for_identity(
        &self,
        target: &CallTargetIdentity,
        max_results: usize,
    ) -> Vec<ResolvedCallerEntry> {
        let mut seen = HashSet::new();
        let mut results = Vec::new();

        for edge in &self.edges {
            let canonical_callee_name = edge
                .canonical_callee_name
                .as_deref()
                .unwrap_or(&edge.callee_name);
            let name_matches = target.resolved_file.as_deref().map_or_else(
                || {
                    edge.callee_name == target.canonical_name
                        || canonical_callee_name == target.canonical_name
                },
                |_| canonical_callee_name == target.canonical_name,
            );
            let file_matches = target
                .resolved_file
                .as_deref()
                .is_none_or(|file| edge.resolved_file.as_deref() == Some(file));
            let declaration_matches =
                target
                    .declaration_path
                    .as_deref()
                    .is_none_or(|declaration_path| {
                        edge.target_declaration_path.as_deref() == Some(declaration_path)
                    });
            let identity_matches = name_matches && file_matches && declaration_matches;
            if identity_matches {
                let key = (
                    edge.caller_file.clone(),
                    edge.caller_name.clone(),
                    edge.line,
                );
                if seen.insert(key) {
                    results.push(ResolvedCallerEntry {
                        caller: CallerEntry {
                            file: edge.caller_file.clone(),
                            function: edge.caller_name.clone(),
                            line: edge.line,
                            confidence: edge.confidence,
                            resolution: edge.resolution_strategy,
                        },
                        caller_identity: CallTargetIdentity {
                            canonical_name: edge.caller_name.clone(),
                            resolved_file: Some(edge.caller_file.clone()),
                            declaration_path: edge.caller_declaration_path.clone(),
                        },
                        target: CallTargetIdentity {
                            canonical_name: canonical_callee_name.to_owned(),
                            resolved_file: edge.resolved_file.clone(),
                            declaration_path: edge.target_declaration_path.clone(),
                        },
                    });
                }
            }
        }

        sort_and_truncate(&mut results, max_results, |entry| entry.caller.confidence);
        results
    }

    fn contains_file(&self, file: &str) -> bool {
        self.files.contains(file)
    }

    fn get_callees(
        &self,
        function_name: &str,
        caller_file: Option<&str>,
        max_results: usize,
    ) -> Vec<ResolvedCalleeEntry> {
        self.get_callees_for_source(
            &CallTargetIdentity {
                canonical_name: function_name.to_owned(),
                resolved_file: caller_file.map(ToOwned::to_owned),
                declaration_path: None,
            },
            max_results,
        )
    }

    fn get_callees_for_source(
        &self,
        source: &CallTargetIdentity,
        max_results: usize,
    ) -> Vec<ResolvedCalleeEntry> {
        let mut seen: HashMap<(String, usize), ()> = HashMap::new();
        let mut results = Vec::new();

        for edge in &self.edges {
            if edge.caller_name == source.canonical_name
                && source
                    .resolved_file
                    .as_deref()
                    .is_none_or(|file| edge.caller_file == file)
                && source
                    .declaration_path
                    .as_deref()
                    .is_none_or(|declaration_path| {
                        edge.caller_declaration_path.as_deref() == Some(declaration_path)
                    })
            {
                let key = (edge.callee_name.clone(), edge.line);
                if seen.insert(key, ()).is_none() {
                    results.push(ResolvedCalleeEntry {
                        callee: CalleeEntry {
                            name: edge.callee_name.clone(),
                            line: edge.line,
                            resolved_file: edge.resolved_file.clone(),
                            confidence: edge.confidence,
                            resolution: edge.resolution_strategy,
                        },
                        target: CallTargetIdentity {
                            canonical_name: edge
                                .canonical_callee_name
                                .clone()
                                .unwrap_or_else(|| edge.callee_name.clone()),
                            resolved_file: edge.resolved_file.clone(),
                            declaration_path: edge.target_declaration_path.clone(),
                        },
                    });
                }
            }
        }

        sort_and_truncate(&mut results, max_results, |entry| entry.callee.confidence);
        results
    }
}

/// Reuses one resolved base scope and materializes escaped callee files at most once each.
pub struct ResolvedCallGraph<'a> {
    project: &'a ProjectRoot,
    graph_cache: Option<&'a GraphCache>,
    base: CallGraphSnapshot,
    escaped_files: HashMap<String, CallGraphSnapshot>,
    #[cfg(test)]
    materialization_count: usize,
}

impl<'a> ResolvedCallGraph<'a> {
    pub fn build(
        project: &'a ProjectRoot,
        path: Option<&str>,
        graph_cache: Option<&'a GraphCache>,
    ) -> Result<Self> {
        Ok(Self {
            project,
            graph_cache,
            base: CallGraphSnapshot::build(project, path, graph_cache)?,
            escaped_files: HashMap::new(),
            #[cfg(test)]
            materialization_count: 1,
        })
    }

    /// Query callers with canonical target identity preserved for traversal.
    pub fn get_callers_for_target(
        &self,
        function_name: &str,
        resolved_file: Option<&str>,
        max_results: usize,
    ) -> Vec<ResolvedCallerEntry> {
        self.base
            .get_callers_for_target(function_name, resolved_file, max_results)
    }

    /// Query callers using the full traversal-only declaration identity.
    pub fn get_callers_for_identity(
        &self,
        target: &CallTargetIdentity,
        max_results: usize,
    ) -> Vec<ResolvedCallerEntry> {
        self.base.get_callers_for_identity(target, max_results)
    }

    /// Query callees, lazily materializing an escaped caller file once if needed.
    pub fn get_callees(
        &mut self,
        function_name: &str,
        caller_file: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<ResolvedCalleeEntry>> {
        self.get_callees_for_source(
            &CallTargetIdentity {
                canonical_name: function_name.to_owned(),
                resolved_file: caller_file.map(ToOwned::to_owned),
                declaration_path: None,
            },
            max_results,
        )
    }

    /// Query callees using the caller's full traversal-only declaration identity.
    pub fn get_callees_for_source(
        &mut self,
        source: &CallTargetIdentity,
        max_results: usize,
    ) -> Result<Vec<ResolvedCalleeEntry>> {
        let Some(caller_file) = source.resolved_file.as_deref() else {
            return Ok(self.base.get_callees_for_source(source, max_results));
        };
        if self.base.contains_file(caller_file) {
            return Ok(self.base.get_callees_for_source(source, max_results));
        }

        match self.escaped_files.entry(caller_file.to_owned()) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                Ok(entry.get().get_callees_for_source(source, max_results))
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                let snapshot =
                    CallGraphSnapshot::build(self.project, Some(caller_file), self.graph_cache)?;
                let snapshot = entry.insert(snapshot);
                #[cfg(test)]
                {
                    self.materialization_count += 1;
                }
                Ok(snapshot.get_callees_for_source(source, max_results))
            }
        }
    }

    #[cfg(test)]
    pub fn materialization_count(&self) -> usize {
        self.materialization_count
    }
}

fn sort_and_truncate<T>(results: &mut Vec<T>, max_results: usize, confidence: impl Fn(&T) -> f64) {
    results.sort_by(|a, b| {
        confidence(b)
            .partial_cmp(&confidence(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
}

/// Find all functions that call `function_name` across the project.
/// Edges are resolved via the 6-stage confidence cascade when an import graph is available.
pub fn get_callers(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<CallerEntry>> {
    Ok(get_callers_for_target(
        project,
        function_name,
        None,
        file_path,
        max_results,
        graph_cache,
    )?
    .into_iter()
    .map(|entry| entry.caller)
    .collect())
}

/// Find callers of a raw or canonical target name, optionally restricted to its definition file.
/// Target identity is filtered before sorting and applying `max_results`.
pub fn get_callers_for_target(
    project: &ProjectRoot,
    function_name: &str,
    resolved_file: Option<&str>,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<ResolvedCallerEntry>> {
    let snapshot = CallGraphSnapshot::build(project, file_path, graph_cache)?;
    Ok(snapshot.get_callers_for_target(function_name, resolved_file, max_results))
}

/// Find all functions called by `function_name` (optionally restricted to a file).
/// Callee names are resolved to their definition files via the 6-stage cascade.
pub fn get_callees(
    project: &ProjectRoot,
    function_name: &str,
    file_path: Option<&str>,
    max_results: usize,
    graph_cache: Option<&GraphCache>,
) -> Result<Vec<CalleeEntry>> {
    let snapshot = CallGraphSnapshot::build(project, file_path, graph_cache)?;
    Ok(snapshot
        .get_callees(function_name, None, max_results)
        .into_iter()
        .map(|entry| entry.callee)
        .collect())
}
