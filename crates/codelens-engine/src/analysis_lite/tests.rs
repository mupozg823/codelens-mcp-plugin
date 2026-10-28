use super::*;
use crate::oxc_analysis::ByteSpan;
use std::fs;

#[test]
fn analysis_lite_exact_indexed_ids_require_matching_source_not_just_stable_generation() {
    let (_dir, project, index) = fixture(&[("a.ts", "export function sink() {}\nsink();\n")]);
    index
        .index_files(&[project.as_path().join("a.ts")])
        .unwrap();
    let mut selected = selector("a.ts", "sink");
    selected.name = None;
    selected.symbol_id = Some("a.ts#function:sink".to_owned());
    let response = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selected,
        None,
        limits(),
    );
    assert_eq!(response.results.references.len(), 1, "{response:?}");
    let generation = index.committed_generation();
    fs::write(
        project.as_path().join("a.ts"),
        "// 한국어 😀\nexport function sink() {}\nsink();\n",
    )
    .unwrap();
    let stale = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selected,
        None,
        limits(),
    );
    assert_eq!(stale.status, EvidenceStatus::Error);
    assert_eq!(stale.errors[0].code, "indexed_target_stale");
    assert_eq!(index.committed_generation(), generation);
    let mut exact = selector("a.ts", "sink");
    exact.name_span = Some(ByteSpan { start: 34, end: 38 });
    let current = query(
        &project,
        &index,
        AnalysisQuery::References,
        &exact,
        None,
        limits(),
    );
    assert_eq!(current.results.references.len(), 1);
    exact.name_span = Some(ByteSpan { start: 26, end: 30 });
    let utf16 = query(
        &project,
        &index,
        AnalysisQuery::References,
        &exact,
        None,
        limits(),
    );
    assert_eq!(utf16.status, EvidenceStatus::Error);
    assert_eq!(utf16.errors[0].code, "target_not_found");
}

fn fixture(files: &[(&str, &str)]) -> (tempfile::TempDir, ProjectRoot, SymbolIndex) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("package.json"), "{}").unwrap();
    for (file, source) in files {
        let path = dir.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let project = ProjectRoot::new_exact(dir.path()).unwrap();
    let index = SymbolIndex::new_memory(project.clone());
    (dir, project, index)
}

fn selector(file: &str, name: &str) -> DeclarationSelector {
    DeclarationSelector {
        file: file.to_owned(),
        name: Some(name.to_owned()),
        symbol_id: None,
        name_span: None,
    }
}

fn limits() -> EvidenceLimits {
    EvidenceLimits {
        max_results: 100,
        max_depth: 3,
        max_nodes: 40,
        max_edges: 100,
        max_files: 2000,
    }
}

#[test]
fn analysis_lite_merges_local_and_external_evidence_without_crossing_shadowed_declarations() {
    let (_dir, project, index) = fixture(&[
        (
            "a.ts",
            "export function same() {}\nsame();\nfunction wrap() { function same() {} same(); }\n",
        ),
        (
            "b.ts",
            "import { same as chosen } from './a';\nexport function caller() { chosen(); chosen(); }\nfunction shadow() { function chosen() {} chosen(); }\n",
        ),
        (
            "unrelated.ts",
            "export function same() {}\nexport function other() { same(); }\n",
        ),
    ]);
    let ambiguous = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("a.ts", "same"),
        None,
        limits(),
    );
    assert_eq!(ambiguous.status, EvidenceStatus::Partial);
    assert_eq!(ambiguous.results.selection_candidates.len(), 2);
    assert_eq!(ambiguous.errors[0].code, "ambiguous_target");
    assert!(ambiguous.target.is_none());

    let mut selected = selector("a.ts", "same");
    selected.name_span = Some(ByteSpan { start: 16, end: 20 });
    let response = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selected,
        None,
        limits(),
    );
    assert_eq!(response.status, EvidenceStatus::Partial, "{response:?}");
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(
        response.results.references.len(),
        3,
        "{:?}",
        response.results.references
    );
    assert_eq!(
        response
            .results
            .references
            .iter()
            .filter(|row| row.location.file == "b.ts")
            .count(),
        2
    );
    let local = response
        .results
        .references
        .iter()
        .find(|row| row.location.file == "a.ts")
        .unwrap();
    assert_eq!(
        local.provenance.len(),
        2,
        "same location merges OXC and graph provenance"
    );
    let external = response
        .results
        .references
        .iter()
        .filter(|row| row.location.file == "b.ts")
        .collect::<Vec<_>>();
    assert_eq!(external[0].location.line, external[1].location.line);
    assert_ne!(
        external[0].location.span, external[1].location.span,
        "two same-line callsites survive"
    );
    assert!(
        response
            .results
            .references
            .iter()
            .all(|row| row.target.name_span == Some(ByteSpan { start: 16, end: 20 }))
    );

    selected.symbol_id = response
        .target
        .as_ref()
        .map(|target| target.symbol_id.clone());
    selected.name_span = None;
    assert_eq!(
        query(
            &project,
            &index,
            AnalysisQuery::References,
            &selected,
            None,
            limits()
        )
        .results
        .references
        .len(),
        3
    );
}

#[test]
fn analysis_lite_resolves_named_alias_reexports_using_source_tokens_even_with_misleading_comments()
{
    let (_dir, project, index) = fixture(&[
        (
            "a.ts",
            "export function sink() {}\nexport function wrong() {}\n",
        ),
        (
            "barrel.ts",
            "export { sink as forwarded } from './a';\n// export { wrong as forwarded } from './a';\n",
        ),
        (
            "caller.ts",
            "import { forwarded as chosen } from './barrel';\n// import { wrong as chosen } from './a';\nexport function useIt() { chosen(); }\n",
        ),
    ]);
    let response = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert_eq!(response.results.references.len(), 1, "{response:?}");
    let row = &response.results.references[0];
    assert_eq!(row.location.file, "caller.ts");
    assert_eq!(row.location.line, 3);
    assert_eq!(row.target.name, "sink");
    assert_eq!(
        row.provenance[0].resolution,
        "verified_esm_import_export_identity"
    );
    assert_eq!(row.provenance[0].relation_kind, "callgraph_edge");
    assert_eq!(row.provenance[0].detail, "detail_unknown");
}

#[test]
fn analysis_lite_noncall_external_use_and_zero_rows_remain_partial_with_unknown_total() {
    let (_dir, project, index) = fixture(&[
        ("a.ts", "export function sink() {}\n"),
        ("b.ts", "import { sink } from './a';\nconst held = sink;\n"),
    ]);
    let response = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    assert_eq!(response.status, EvidenceStatus::Partial);
    assert!(response.results.references.is_empty());
    assert_eq!(response.results.total, None);
    assert!(
        response
            .scope
            .limitations
            .contains(&"cross_file_non_call_import_and_type_references_incomplete")
    );
    assert!(
        response
            .scope
            .limitations
            .contains(&"zero_results_do_not_prove_unused")
    );
}

#[test]
fn analysis_lite_reverse_impact_preserves_callsites_and_reports_each_bound() {
    let (_dir, project, index) = fixture(&[(
        "a.ts",
        "export function sink() {}\nexport function middle() { sink(); }\nexport function outer() { middle(); middle(); }\n",
    )]);
    let selected = selector("a.ts", "sink");
    let all = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selected,
        None,
        limits(),
    );
    assert_eq!(all.results.nodes.len(), 3, "{all:?}");
    assert_eq!(all.results.edges.len(), 3);
    assert!(
        all.results
            .edges
            .iter()
            .all(|edge| edge.relation_kind == "callgraph_edge" && edge.detail == "detail_unknown")
    );
    let mut bound = limits();
    bound.max_depth = 1;
    let depth = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selected,
        None,
        bound,
    );
    assert!(depth.truncation.depth);
    assert_eq!(depth.results.edges.len(), 1);
    let mut bound = limits();
    bound.max_nodes = 1;
    let nodes = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selected,
        None,
        bound,
    );
    assert!(nodes.truncation.nodes);
    assert_eq!(nodes.results.nodes.len(), 1);
    assert!(nodes.results.edges.is_empty());
    let mut bound = limits();
    bound.max_edges = 2;
    let edges = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selected,
        None,
        bound,
    );
    assert!(edges.truncation.edges);
    assert_eq!(edges.results.edges.len(), 2);
    let mut bound = limits();
    bound.max_results = 1;
    let refs = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("a.ts", "middle"),
        None,
        bound,
    );
    assert!(refs.truncation.results);
    assert_eq!(refs.results.references.len(), 1);
}

#[test]
fn analysis_lite_errors_are_not_successful_empty_impact() {
    let (_dir, project, index) = fixture(&[
        ("a.ts", "export function target() {}\n"),
        ("bad.ts", "function broken( {"),
        ("foreign.py", "def target(): pass\n"),
    ]);
    for (selected, scope, want) in [
        (selector("missing.ts", "target"), None, "source_read_failed"),
        (selector("bad.ts", "broken"), None, "parse_failed"),
        (selector("a.ts", "missing"), None, "target_not_found"),
        (
            selector("a.ts", "target"),
            Some("missing-scope"),
            "scope_not_found",
        ),
    ] {
        let response = query(
            &project,
            &index,
            AnalysisQuery::Impact,
            &selected,
            scope,
            limits(),
        );
        assert_eq!(response.status, EvidenceStatus::Error, "{response:?}");
        assert_eq!(response.errors[0].code, want);
        assert_eq!(response.results.total, None);
        assert!(response.results.edges.is_empty());
    }
    let unsupported = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("foreign.py", "target"),
        None,
        limits(),
    );
    assert_eq!(unsupported.status, EvidenceStatus::Unsupported);
    let partial = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("a.ts", "target"),
        None,
        limits(),
    );
    assert_eq!(partial.status, EvidenceStatus::Partial);
    assert_eq!(
        partial.errors[0].code, "parse_failed",
        "other-file backend errors stay visible"
    );
}

#[test]
fn analysis_lite_callback_edges_never_become_proven_direct_calls() {
    let (_dir, project, index) = fixture(&[(
        "a.ts",
        "export function target() {}\nexport function callbackUser() { register(target); }\n",
    )]);
    let response = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("a.ts", "target"),
        None,
        limits(),
    );
    assert_eq!(response.results.edges.len(), 1);
    assert_eq!(response.results.edges[0].detail, "detail_unknown");
    assert_eq!(
        response.unresolved.observed_count, 1,
        "unresolved register remains visible"
    );
    assert_eq!(response.unresolved.examples[0].name, "register");
}

#[test]
fn analysis_lite_unsupported_member_callers_never_borrow_an_enclosing_function_identity() {
    let mut failures = Vec::new();
    for member in [
        "class Holder { same() { sink(); } }",
        "class Holder { ['same']() { sink(); } }",
        "class Holder { 'same'() { sink(); } }",
        "class Holder { same = () => { sink(); }; }",
        "const holder = { same() { sink(); } };",
        "const holder = { same: function () { sink(); } };",
        "const holder = { same: () => { sink(); } };",
    ] {
        let source = format!(
            "export function sink() {{}}\nexport function same() {{ {member} }}\nexport function upstream() {{ same(); }}\n"
        );
        let (_dir, project, index) = fixture(&[("a.ts", &source)]);
        let response = query(
            &project,
            &index,
            AnalysisQuery::Impact,
            &selector("a.ts", "sink"),
            None,
            limits(),
        );
        if response.results.edges.len() != 1
            || response.results.edges[0].source.is_some()
            || response.results.nodes.len() != 1
            || response.unresolved.observed_count == 0
        {
            failures.push((member, response));
        }
    }
    assert!(
        failures.is_empty(),
        "wrong member caller identities: {failures:#?}"
    );
}

#[test]
fn analysis_lite_caller_identity_preserves_exact_functions_and_lexical_callback_containment() {
    let source = "export function sink() {}\n\
        export function same() {}\n\
        function outer() { function same() { sink(); } same(); }\n\
        const expression = function named() { sink(); };\n\
        const arrow = () => { sink(); };\n\
        function callbackOuter() { register(() => sink()); }\n\
        function upstream() { outer(); expression(); arrow(); callbackOuter(); }\n";
    let (_dir, project, index) = fixture(&[("a.ts", source)]);
    let response = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    let analysis = analyze_declarations(source, "a.ts").unwrap();
    let direct = response
        .results
        .edges
        .iter()
        .filter(|edge| edge.target.name == "sink")
        .map(|edge| edge.source.as_ref().expect("supported exact caller"))
        .collect::<Vec<_>>();
    assert_eq!(direct.len(), 4, "{response:?}");
    for caller in &direct {
        let matching = analysis
            .named(&caller.name)
            .into_iter()
            .find(|declaration| {
                Some(declaration.name_span) == caller.name_span
                    && Some(declaration.declaration_span) == caller.declaration_span
            });
        assert!(matching.is_some(), "{caller:?}");
    }
    let inner_same = analysis.named("same")[1];
    assert!(
        direct.iter().any(|caller| {
            caller.name == "same" && caller.name_span == Some(inner_same.name_span)
        })
    );
    for name in ["expression", "arrow", "callbackOuter"] {
        assert!(direct.iter().any(|caller| caller.name == name), "{name}");
    }
    assert!(
        response
            .results
            .nodes
            .iter()
            .any(|node| node.name == "upstream")
    );
    assert!(
        !response
            .results
            .nodes
            .iter()
            .any(|node| { node.name_span == Some(analysis.named("same")[0].name_span) })
    );
}

#[test]
fn analysis_lite_disk_hashes_detect_change_without_a_generation_commit() {
    let (_dir, project, _index) = fixture(&[("a.ts", "export function target() {}\n")]);
    let source = fs::read_to_string(project.as_path().join("a.ts")).unwrap();
    let analysis = analyze_declarations(&source, "a.ts").unwrap();
    let graph =
        graph::DeclarationGraph::build(&project, None, "a.ts", &source, analysis, 10).unwrap();
    fs::write(
        project.as_path().join("a.ts"),
        "export function changed() {}\n",
    )
    .unwrap();
    assert_eq!(
        graph.verify_sources(&project).unwrap_err().code,
        "source_changed"
    );
}

#[test]
fn analysis_lite_source_file_cap_is_explicit_and_keeps_the_exact_target() {
    let (_dir, project, index) = fixture(&[
        ("a.ts", "export function ignored() {}\n"),
        ("z.ts", "export function target() {}\n"),
    ]);
    let mut bound = limits();
    bound.max_files = 1;
    let response = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("z.ts", "target"),
        None,
        bound,
    );
    assert_eq!(response.status, EvidenceStatus::Partial);
    assert!(response.truncation.files);
    assert_eq!(response.snapshot.files_read, 1);
    assert_eq!(response.target.unwrap().file, "z.ts");
}

#[test]
fn analysis_lite_class_and_interface_members_are_explicitly_unsupported_without_name_guessing() {
    let (_dir, project, index) = fixture(&[(
        "members.ts",
        "class A { same() { this.same(); this.other.same(); } other!: B; }\nclass B { same() {} }\ninterface I { same(): void; }\nexport function same() {}\nexport function unknown(x: I) { x.same(); }\n",
    )]);
    let response = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("members.ts", "same"),
        None,
        limits(),
    );
    assert!(
        response.results.references.is_empty(),
        "name-only selection must not prefer the lexical function over same-name methods"
    );
    assert!(response.target.is_none());
    assert_eq!(response.errors[0].code, "ambiguous_target");
    assert!(response.results.selection_candidates.len() >= 3);
    let mut method = selector("members.ts", "same");
    method.name_span = Some(ByteSpan { start: 10, end: 14 });
    let unsupported = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &method,
        None,
        limits(),
    );
    assert_eq!(unsupported.status, EvidenceStatus::Unsupported);
    assert_eq!(unsupported.errors[0].code, "unsupported_member_declaration");
    assert!(unsupported.results.edges.is_empty());

    let function_start = "class A { same() { this.same(); this.other.same(); } other!: B; }\nclass B { same() {} }\ninterface I { same(): void; }\nexport function ".len();
    let mut lexical = selector("members.ts", "same");
    lexical.name_span = Some(ByteSpan {
        start: function_start,
        end: function_start + 4,
    });
    let safe = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &lexical,
        None,
        limits(),
    );
    assert!(
        safe.results.edges.is_empty(),
        "this.same, this.other.same and interface calls are never attributed to a same-name free function"
    );
    assert!(safe.unresolved.observed_count >= 3);
}

// The legacy name-only noise list (`format`, `read`, `log`, …) guesses at
// builtins. Exact identity resolves a project declaration of that name; only
// an unresolvable noise token is discarded, and silently, as before.
#[test]
fn analysis_lite_project_declarations_with_noise_names_keep_their_callers() {
    let (_dir, project, index) = fixture(&[
        (
            "fmt.ts",
            "export function format() {}\nexport function local() { format(); }\n",
        ),
        (
            "b.ts",
            "import { format } from './fmt';\nexport function caller(items: number[]) { format(); console.log(items); items.push(1); }\n",
        ),
    ]);
    let impact = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("fmt.ts", "format"),
        None,
        limits(),
    );
    let callers = impact
        .results
        .edges
        .iter()
        .filter_map(|edge| edge.source.as_ref().map(|source| source.name.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        callers,
        ["caller", "local"].into_iter().collect(),
        "{impact:?}"
    );
    let refs = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("fmt.ts", "format"),
        None,
        limits(),
    );
    assert_eq!(
        refs.results
            .references
            .iter()
            .filter(|row| row.location.file == "b.ts")
            .count(),
        1,
        "{refs:?}"
    );
    assert!(
        impact
            .unresolved
            .examples
            .iter()
            .all(|row| row.name != "log" && row.name != "push"),
        "unresolvable noise tokens stay discarded: {:?}",
        impact.unresolved.examples
    );
}

// OXC accepts `.mts`/`.cts` (`is_js_ts`); call extraction must cover them too
// instead of returning a silently empty graph.
#[test]
fn analysis_lite_mts_and_cts_files_have_call_edges() {
    for (file, importer) in [("a.mts", "b.mts"), ("a.cts", "b.cts")] {
        let (_dir, project, index) = fixture(&[
            (
                file,
                "export function target() {}\nexport function caller() { target(); }\n",
            ),
            (
                importer,
                "import { target } from './a';\nexport function other() { target(); }\n",
            ),
        ]);
        let impact = query(
            &project,
            &index,
            AnalysisQuery::Impact,
            &selector(file, "target"),
            None,
            limits(),
        );
        assert!(impact.errors.is_empty(), "{file}: {:?}", impact.errors);
        assert_eq!(impact.results.edges.len(), 1, "{file}: {impact:?}");
        assert_eq!(
            impact.results.edges[0]
                .source
                .as_ref()
                .map(|source| source.name.as_str()),
            Some("caller"),
            "{file}"
        );
        // The module resolver does not cover these extensions: the import is
        // reported unresolved, never dropped.
        assert!(
            impact.unresolved.examples.iter().any(|row| {
                row.name == "target"
                    && row.location.as_ref().is_some_and(|at| at.file == importer)
                    && row
                        .reason
                        .contains("no verified project-local import target")
            }),
            "{importer}: {:?}",
            impact.unresolved.examples
        );
    }
}

// An import binding is an alias: selecting it queries the declaration it
// resolves to, the identity graph edges carry, instead of an empty graph.
#[test]
fn analysis_lite_import_binding_selector_queries_the_declaration_it_resolves_to() {
    let (_dir, project, index) = fixture(&[
        (
            "a.ts",
            "export function sink() {}\nexport function near() { sink(); }\n",
        ),
        (
            "b.ts",
            "import { sink as local } from './a';\nexport function caller() { local(); }\nexport const held = local;\n",
        ),
    ]);
    let impact = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("b.ts", "local"),
        None,
        limits(),
    );
    assert!(impact.errors.is_empty(), "{:?}", impact.errors);
    let direct = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    assert_eq!(
        impact.target.as_ref().map(|target| &target.symbol_id),
        direct.target.as_ref().map(|target| &target.symbol_id),
        "{impact:?}"
    );
    let callers = impact
        .results
        .edges
        .iter()
        .filter_map(|edge| edge.source.as_ref().map(|source| source.name.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(callers, ["caller", "near"].into_iter().collect());

    let refs = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("b.ts", "local"),
        None,
        limits(),
    );
    let rows = refs
        .results
        .references
        .iter()
        .map(|row| (row.location.file.as_str(), row.location.line))
        .collect::<Vec<_>>();
    // a.ts call in `near`, b.ts call in `caller` and the non-call read `held`.
    assert_eq!(rows, [("a.ts", 2), ("b.ts", 2), ("b.ts", 3)], "{refs:?}");
    assert!(
        refs.results
            .references
            .iter()
            .all(|row| Some(&row.target) == refs.target.as_ref())
    );
}

#[test]
fn analysis_lite_unresolved_import_binding_stays_the_target_with_unresolved_callsites() {
    let (_dir, project, index) = fixture(&[(
        "b.ts",
        "import { render } from 'some-package';\nexport function caller() { render(); }\n",
    )]);
    let impact = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("b.ts", "render"),
        None,
        limits(),
    );
    assert_eq!(
        impact
            .target
            .as_ref()
            .map(|target| (target.file.as_str(), target.name.as_str())),
        Some(("b.ts", "render"))
    );
    assert!(impact.results.edges.is_empty());
    assert!(
        impact
            .unresolved
            .examples
            .iter()
            .any(|row| row.name == "render"
                && row
                    .reason
                    .contains("no verified project-local import target")),
        "the empty graph is explained: {:?}",
        impact.unresolved.examples
    );
}

#[test]
fn analysis_lite_reused_parses_follow_edits_between_queries_in_one_process() {
    let (_dir, project, index) = fixture(&[
        ("a.ts", "export function sink() {}\n"),
        ("b.ts", "import { sink } from './a';\nsink();\n"),
        ("c.ts", "export const unrelated = 1;\n"),
    ]);
    let refs = |label: &str| {
        let response = query(
            &project,
            &index,
            AnalysisQuery::References,
            &selector("a.ts", "sink"),
            None,
            limits(),
        );
        assert_ne!(
            response.status,
            EvidenceStatus::Error,
            "{label}: {response:?}"
        );
        let mut files = response
            .results
            .references
            .iter()
            .map(|r| (r.location.file.clone(), r.location.line))
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    let first = refs("first");
    assert!(first.contains(&("b.ts".to_owned(), 2)), "{first:?}");
    // Same content again: served from the parse caches, identical evidence.
    assert_eq!(refs("repeat"), first);
    // An edited caller and a file that newly calls the target must both show
    // up: a changed content hash is a cache miss, never a stale hit.
    fs::write(
        project.as_path().join("b.ts"),
        "import { sink } from './a';\n\nsink();\nsink();\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("c.ts"),
        "import { sink } from './a';\nexport const unrelated = sink();\n",
    )
    .unwrap();
    let edited = refs("edited");
    for expected in [("b.ts", 3), ("b.ts", 4), ("c.ts", 2)] {
        assert!(
            edited.contains(&(expected.0.to_owned(), expected.1)),
            "missing {expected:?} in {edited:?}"
        );
    }
    assert!(!edited.contains(&("b.ts".to_owned(), 2)), "{edited:?}");
}

#[test]
fn analysis_lite_rows_carry_their_trimmed_capped_source_line() {
    let long_tail = "가".repeat(80);
    let (_dir, project, index) = fixture(&[
        ("a.ts", "export function sink() {}\n"),
        (
            "b.ts",
            &format!(
                "import {{ sink }} from './a';\n    sink(); // short\nconst x = [sink()]; // {long_tail}\n"
            ),
        ),
    ]);
    let refs = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    let line_text = |line: usize| {
        refs.results
            .references
            .iter()
            .find(|row| row.location.file == "b.ts" && row.location.line == line)
            .and_then(|row| row.location.line_text.clone())
            .unwrap_or_else(|| panic!("no b.ts:{line} row in {refs:?}"))
    };
    assert_eq!(line_text(2), "sink(); // short");
    let capped = line_text(3);
    assert!(capped.starts_with("const x = [sink()];"), "{capped}");
    assert!(capped.ends_with('…'), "{capped}");
    assert!(
        capped.len() <= LINE_TEXT_MAX_BYTES + '…'.len_utf8(),
        "{capped}"
    );
    let impact = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    assert!(
        impact
            .results
            .edges
            .iter()
            .all(|edge| edge.location.line_text.is_some()),
        "{impact:?}"
    );
    assert!(impact.unresolved.examples.iter().all(|example| {
        example
            .location
            .as_ref()
            .is_none_or(|l| l.line_text.is_none())
    }));
}

#[test]
fn analysis_lite_parse_caches_are_released_only_after_an_idle_period() {
    let (_dir, project, index) = fixture(&[
        ("a.ts", "export function sink() {}\n"),
        ("b.ts", "import { sink } from './a';\nsink();\n"),
    ]);
    let response = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("a.ts", "sink"),
        None,
        limits(),
    );
    assert_ne!(response.status, EvidenceStatus::Error, "{response:?}");
    // Just used: not idle for an hour, so nothing is released.
    assert!(!release_parse_caches_if_idle(
        std::time::Duration::from_secs(3600)
    ));
    // Idle for zero time: the entries this query cached are dropped.
    assert!(release_parse_caches_if_idle(std::time::Duration::ZERO));
}

#[test]
fn analysis_lite_dynamic_import_member_counts_as_a_reference_to_the_export() {
    // next/dynamic loads every winnation overlay as
    // `dynamic(() => import("@/components/X").then((mod) => mod.X))`, and refs on
    // X missed the renderer that draws it (K-0065).
    let (_dir, project, index) = fixture(&[
        (
            "components/Overlay.tsx",
            "export function Overlay() { return null; }\nexport function Other() { return null; }\n",
        ),
        (
            "renderer.tsx",
            "function load(f: unknown) { return f; }\n\
             const Lazy = load(() => import('./components/Overlay').then((mod) => mod.Overlay));\n\
             const Bare = load(() => import('./components/Overlay').then(m => m.Overlay));\n\
             const Block = load(() => import('./components/Overlay').then(function (mod) { return mod.Overlay; }));\n\
             const Wrong = load(() => import('./components/Overlay').then((mod) => mod.Other));\n\
             const other = { Overlay: 1 };\n\
             const Unbound = load(() => import('./components/Overlay').then((mod) => other.Overlay));\n\
             const Shadow = load(() => import('./components/Overlay').then((mod) => [1].map((mod) => mod.Overlay)));\n\
             export function Page() { return [Lazy, Bare, Block, Wrong, Unbound, Shadow]; }\n",
        ),
    ]);
    let refs = query(
        &project,
        &index,
        AnalysisQuery::References,
        &selector("components/Overlay.tsx", "Overlay"),
        None,
        limits(),
    );
    let rows = refs
        .results
        .references
        .iter()
        .filter(|row| row.location.file == "renderer.tsx")
        .collect::<Vec<_>>();
    assert_eq!(
        rows.iter().map(|row| row.location.line).collect::<Vec<_>>(),
        vec![2, 3, 4],
        "{refs:?}"
    );
    let source = fs::read_to_string(project.as_path().join("renderer.tsx")).unwrap();
    for row in &rows {
        assert_eq!(
            &source[row.location.span.start..row.location.span.end],
            "Overlay"
        );
        assert_eq!(
            row.provenance[0].resolution,
            "verified_esm_dynamic_import_export_identity"
        );
        assert_eq!(row.provenance[0].relation_kind, "callgraph_edge");
    }
    let impact = query(
        &project,
        &index,
        AnalysisQuery::Impact,
        &selector("components/Overlay.tsx", "Overlay"),
        None,
        limits(),
    );
    let callers = impact
        .results
        .edges
        .iter()
        .filter_map(|edge| edge.source.as_ref().map(|source| source.name.as_str()))
        .collect::<std::collections::HashSet<_>>();
    assert!(
        callers.contains("Lazy") && callers.contains("Bare") && callers.contains("Block"),
        "{impact:?}"
    );
    assert!(
        !callers.contains("Wrong") && !callers.contains("Unbound") && !callers.contains("Shadow"),
        "{impact:?}"
    );
}
