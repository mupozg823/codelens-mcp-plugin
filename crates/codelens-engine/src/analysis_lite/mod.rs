//! Read-only, candidate-only JS/TS evidence. No legacy fallback can turn a
//! backend error into an empty graph. Generation fencing belongs to the caller;
//! the response separately describes its disk-read source basis.

mod graph;
#[cfg(test)]
mod tests;
mod types;
pub use types::*;

use crate::oxc_analysis::{
    ExactDeclaration, RefKind, UnsupportedDeclaration, analyze_declarations, is_js_ts,
};
use crate::{ProjectRoot, SymbolIndex};
use std::collections::{BTreeMap, HashSet, VecDeque};

pub(super) fn error(
    code: &'static str,
    backend: &'static str,
    file: Option<&str>,
    message: impl ToString,
) -> EvidenceError {
    EvidenceError {
        code,
        backend,
        file: file.map(ToOwned::to_owned),
        message: message.to_string(),
    }
}

pub(super) fn identity(file: &str, declaration: &ExactDeclaration) -> DeclarationIdentity {
    DeclarationIdentity {
        symbol_id: format!(
            "{file}#js_ts_binding:{}@{}:{}",
            declaration.name, declaration.name_span.start, declaration.name_span.end
        ),
        file: file.to_owned(),
        name: declaration.name.clone(),
        name_span: Some(declaration.name_span),
        declaration_span: Some(declaration.declaration_span),
        kind: "lexical_binding",
    }
}

fn empty_response(
    project: &ProjectRoot,
    query: AnalysisQuery,
    generation: u64,
    limits: EvidenceLimits,
    path: Option<&str>,
) -> AnalysisEvidence {
    AnalysisEvidence {
        schema_version: SCHEMA_VERSION,
        capability: match query {
            AnalysisQuery::References => "js_ts_declaration_refs",
            AnalysisQuery::Impact => "js_ts_bounded_reverse_callgraph",
        },
        project: project.as_path().display().to_string(),
        target: None,
        snapshot: EvidenceSnapshot {
            generation,
            source_basis: "per_request_disk_reads; symbol_generation_is_a_separate_fence",
            source_bundle_hash: None,
            files_read: 0,
        },
        scope: EvidenceScope {
            language: "javascript_typescript",
            caller_path: path.map(ToOwned::to_owned),
            backends: vec![
                "oxc_semantic",
                "tree_sitter_callgraph",
                "existing_js_import_resolver",
            ],
            limitations: vec![
                "cross_file_non_call_import_and_type_references_incomplete",
                "static_extracted_edges_include_callbacks; direct_call_detail_unknown",
                "dynamic_dispatch_namespace_members_and_star_exports_not_complete",
                "zero_results_do_not_prove_unused",
                "discovery_respects_project_exclusions_and_gitignore",
            ],
        },
        status: EvidenceStatus::Partial,
        results: EvidenceResults::default(),
        limits,
        truncation: EvidenceTruncation::default(),
        unresolved: UnresolvedSummary {
            scope: "caller_scope_scan",
            observed_count: 0,
            examples: vec![],
            examples_truncated: false,
            total: None,
        },
        freshness: SourceFreshness {
            disk: "not_read",
            generation_is_disk_freshness: false,
            reconciliation: "not_performed_by_read_query",
        },
        errors: vec![],
    }
}

fn member_identity(file: &str, member: &UnsupportedDeclaration) -> DeclarationIdentity {
    DeclarationIdentity {
        symbol_id: format!(
            "{file}#unsupported_member:{}@{}:{}",
            member.name, member.name_span.start, member.name_span.end
        ),
        file: file.to_owned(),
        name: member.name.clone(),
        name_span: Some(member.name_span),
        declaration_span: Some(member.declaration_span),
        kind: "unsupported_member",
    }
}

/// Produces a typed response even on source/index failures. `status=error`
/// instructs the MCP adapter to emit the existing error envelope with this
/// payload, rather than a successful zero-impact report.
pub fn query(
    project: &ProjectRoot,
    index: &SymbolIndex,
    query: AnalysisQuery,
    selector: &DeclarationSelector,
    caller_path: Option<&str>,
    limits: EvidenceLimits,
) -> AnalysisEvidence {
    let mut response = empty_response(
        project,
        query,
        index.committed_generation(),
        limits,
        caller_path,
    );
    if let Err(failure) = execute(project, index, query, selector, caller_path, &mut response) {
        response.status = EvidenceStatus::Error;
        response.errors.push(failure);
        // Discard potentially mixed source evidence on an execution failure.
        response.results = EvidenceResults::default();
    }
    response
}

fn execute(
    project: &ProjectRoot,
    index: &SymbolIndex,
    query: AnalysisQuery,
    selector: &DeclarationSelector,
    caller_path: Option<&str>,
    response: &mut AnalysisEvidence,
) -> Result<(), EvidenceError> {
    let limits = &response.limits;
    if !(1..=200).contains(&limits.max_results)
        || !(1..=8).contains(&limits.max_depth)
        || !(1..=100).contains(&limits.max_nodes)
        || !(1..=200).contains(&limits.max_edges)
        || !(1..=10000).contains(&limits.max_files)
    {
        return Err(error(
            "invalid_limits",
            "analysis_lite",
            None,
            "query limits exceed candidate bounds",
        ));
    }
    // A failing index must stay an error even when disk parsing could succeed.
    index
        .file_count()
        .map_err(|e| error("index_read_failed", "symbol_index", None, e))?;
    let target_path = project
        .resolve(&selector.file)
        .map_err(|e| error("invalid_target_path", "project", Some(&selector.file), e))?;
    let file = project.to_relative(&target_path);
    if !is_js_ts(&target_path) {
        response.status = EvidenceStatus::Unsupported;
        response.errors.push(error(
            "unsupported_language",
            "oxc_semantic",
            Some(&file),
            "analysis-lite supports JS/TS declarations",
        ));
        return Ok(());
    }
    let source = std::fs::read_to_string(&target_path)
        .map_err(|e| error("source_read_failed", "disk", Some(&file), e))?;
    let analysis = analyze_declarations(&source, &file)
        .map_err(|e| error("parse_failed", "oxc_semantic", Some(&file), e))?;
    let hash = crate::db::content_hash(source.as_bytes());
    response.snapshot.files_read = 1;
    response.snapshot.source_bundle_hash = Some(hash.clone());
    response.freshness.disk = "target_file_read; project_not_scanned";
    let mut candidates = analysis
        .declarations
        .iter()
        .filter(|d| {
            selector.name.as_ref().is_none_or(|name| d.name == *name)
                && selector.name_span.is_none_or(|span| d.name_span == span)
        })
        .collect::<Vec<_>>();
    let mut members = analysis
        .unsupported_members
        .iter()
        .filter(|member| {
            selector
                .name
                .as_ref()
                .is_none_or(|name| member.name == *name)
                && selector
                    .name_span
                    .is_none_or(|span| member.name_span == span)
        })
        .collect::<Vec<_>>();
    if let Some(id) = &selector.symbol_id {
        if id.contains("#js_ts_binding:") {
            candidates.retain(|d| identity(&file, d).symbol_id == *id);
            members.clear();
        } else if id.contains("#unsupported_member:") {
            candidates.clear();
            members.retain(|member| member_identity(&file, member).symbol_id == *id);
        } else {
            // Indexed IDs are accepted only against unchanged indexed content.
            // A stable generation alone does not establish disk freshness.
            let indexed_hash = index
                .db()
                .get_file(&file)
                .map_err(|e| error("index_read_failed", "symbol_index", Some(&file), e))?
                .map(|row| row.content_hash);
            if indexed_hash.as_deref() != Some(&hash) {
                return Err(error(
                    "indexed_target_stale",
                    "symbol_index",
                    Some(&file),
                    "indexed symbol_id requires matching disk/index source hash; use an exact name-token span",
                ));
            }
            let symbols = index
                .find_symbol_cached(id, Some(&file), false, true, 2)
                .map_err(|e| error("index_read_failed", "symbol_index", Some(&file), e))?;
            let symbols = symbols
                .into_iter()
                .filter(|s| s.id == *id && s.file_path == file)
                .collect::<Vec<_>>();
            if symbols.len() != 1 {
                candidates.clear();
                members.clear();
            } else {
                let symbol = &symbols[0];
                candidates.retain(|d| {
                    d.name == symbol.name
                        && d.name_span.start >= symbol.start_byte as usize
                        && d.name_span.end <= symbol.end_byte as usize
                });
                candidates.sort_by_key(|d| d.name_span.start);
                candidates.truncate(1);
                members.retain(|member| {
                    member.name == symbol.name
                        && member.name_span.start >= symbol.start_byte as usize
                        && member.name_span.end <= symbol.end_byte as usize
                });
                members.sort_by_key(|member| member.name_span.start);
                members.truncate(1);
            }
        }
    }
    let selection_count = candidates.len() + members.len();
    if selection_count == 0 {
        return Err(error(
            "target_not_found",
            "oxc_semantic",
            Some(&file),
            "no declaration matches the exact selector",
        ));
    }
    if selection_count != 1 {
        response.results.selection_candidates = candidates
            .iter()
            .map(|d| identity(&file, d))
            .chain(members.iter().map(|member| member_identity(&file, member)))
            .take(response.limits.max_results)
            .collect();
        response.truncation.results = selection_count > response.limits.max_results;
        response.errors.push(error(
            "ambiguous_target",
            "oxc_semantic",
            Some(&file),
            "select one declaration using its name-token start_byte/end_byte or returned symbol_id",
        ));
        response.snapshot.files_read = 1;
        response.snapshot.source_bundle_hash = Some(hash);
        response.freshness.disk = "target_file_read; project_not_scanned";
        return Ok(());
    }
    if let Some(member) = members.first() {
        response.status = EvidenceStatus::Unsupported;
        response.target = Some(member_identity(&file, member));
        response.errors.push(error("unsupported_member_declaration", "oxc_semantic", Some(&file),
            "property/method declarations have no candidate lexical binding identity; this/qualified/interface dispatch is unresolved"));
        response.snapshot.files_read = 1;
        response.snapshot.source_bundle_hash = Some(hash);
        response.freshness.disk = "target_file_read; project_not_scanned";
        return Ok(());
    }
    let declaration = candidates[0];
    response.target = Some(identity(&file, declaration));
    let graph = graph::DeclarationGraph::build(
        project,
        caller_path,
        &file,
        &source,
        analysis.clone(),
        response.limits.max_files,
    )?;
    // An import binding is an alias: graph edges carry the declaration it
    // resolves to, so the query runs on that identity and adds the binding's
    // own local uses. An unresolved import keeps the binding as the target;
    // its callsites are unresolved evidence.
    let exported = declaration
        .is_import
        .then(|| graph.imported_declaration(project, &file, declaration).ok())
        .flatten();
    let target = exported.as_ref().map_or_else(
        || identity(&file, declaration),
        |(resolved, ..)| resolved.clone(),
    );
    response.target = Some(target.clone());
    response.snapshot.files_read = graph.sources.len();
    response.snapshot.source_bundle_hash = Some(graph.bundle_hash());
    response.truncation.files = graph.files_truncated;
    response.errors.extend(graph.errors.iter().cloned());
    response.unresolved.observed_count = graph.unresolved.len();
    response.unresolved.examples = graph.unresolved.iter().take(20).cloned().collect();
    response.unresolved.examples_truncated = graph.unresolved.len() > 20;

    match query {
        AnalysisQuery::References => {
            let mut references = BTreeMap::new();
            let mut locals = vec![(
                file.as_str(),
                declaration,
                hash.as_str(),
                if exported.is_some() {
                    "verified_esm_import_export_identity"
                } else {
                    "exact_lexical_declaration"
                },
            )];
            if let Some((resolved, exported, exported_hash)) = &exported {
                locals.push((
                    resolved.file.as_str(),
                    exported,
                    exported_hash,
                    "exact_lexical_declaration",
                ));
            }
            for (local_file, local, local_hash, resolution) in locals {
                for reference in &local.references {
                    let location = EvidenceLocation {
                        file: local_file.to_owned(),
                        span: reference.span,
                        line: reference.line,
                        column: reference.column,
                        offset_encoding: "utf8_bytes",
                        line_text: None,
                    };
                    references.insert(
                        (
                            local_file.to_owned(),
                            reference.span.start,
                            reference.span.end,
                        ),
                        ReferenceEvidence {
                            location,
                            target: target.clone(),
                            source: None,
                            provenance: vec![Provenance {
                                backend: "oxc_semantic",
                                relation_kind: "semantic_reference",
                                detail: match reference.kind {
                                    RefKind::Write => "write",
                                    RefKind::Read => "read",
                                    RefKind::Definition => "definition",
                                },
                                source_hash: local_hash.to_owned(),
                                resolution,
                            }],
                        },
                    );
                }
            }
            // No self-only condition: local and all identity-resolved external
            // graph evidence are combined for every selected declaration.
            for edge in graph.callers(&target.call_target()) {
                let location = &edge.location;
                let row = references
                    .entry((
                        location.file.clone(),
                        location.span.start,
                        location.span.end,
                    ))
                    .or_insert_with(|| ReferenceEvidence {
                        location: location.clone(),
                        target: target.clone(),
                        source: edge.source.clone(),
                        provenance: vec![],
                    });
                row.source = row.source.clone().or_else(|| edge.source.clone());
                for provenance in &edge.provenance {
                    if !row.provenance.contains(provenance) {
                        row.provenance.push(provenance.clone());
                    }
                }
            }
            response.truncation.results = references.len() > response.limits.max_results;
            response.results.references = references
                .into_values()
                .take(response.limits.max_results)
                .collect();
        }
        AnalysisQuery::Impact => reverse_impact(&graph, target, response),
    }
    for location in response
        .results
        .references
        .iter_mut()
        .map(|row| &mut row.location)
        .chain(
            response
                .results
                .edges
                .iter_mut()
                .map(|edge| &mut edge.location),
        )
    {
        location.line_text = graph.line_text(&location.file, location.span);
    }
    // Verify the source snapshot independently of the index-generation fence.
    // A file changed while the watcher was stalled still invalidates evidence.
    graph.verify_sources(project)?;
    response.freshness.disk = "source_hashes_unchanged_during_query; later_edits_unverified";
    Ok(())
}

fn reverse_impact(
    graph: &graph::DeclarationGraph,
    target: DeclarationIdentity,
    response: &mut AnalysisEvidence,
) {
    let mut nodes = BTreeMap::from([(target.symbol_id.clone(), target.clone())]);
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([(target, 0)]);
    let mut edge_keys = HashSet::new();
    while let Some((current, depth)) = queue.pop_front() {
        if !visited.insert(current.symbol_id.clone()) {
            continue;
        }
        for edge in graph.callers(&current.call_target()) {
            let key = (
                edge.location.file.clone(),
                edge.location.span,
                edge.target.symbol_id.clone(),
            );
            if edge_keys.contains(&key) {
                continue;
            }
            if depth >= response.limits.max_depth {
                response.truncation.depth = true;
                continue;
            }
            if response.results.edges.len() >= response.limits.max_edges {
                response.truncation.edges = true;
                continue;
            }
            if let Some(source) = &edge.source {
                if !nodes.contains_key(&source.symbol_id)
                    && nodes.len() >= response.limits.max_nodes
                {
                    response.truncation.nodes = true;
                    continue;
                }
                if nodes
                    .insert(source.symbol_id.clone(), source.clone())
                    .is_none()
                {
                    queue.push_back((source.clone(), depth + 1));
                }
            }
            edge_keys.insert(key);
            response.results.edges.push(edge.clone());
        }
    }
    response.results.nodes = nodes.into_values().collect();
}
