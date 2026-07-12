use super::*;
use std::io::Write;
use tempfile::NamedTempFile;

// `IntelligenceSource` is needed by the fixtures below; the SCIP navigation
// methods are inherent on `ScipBackend`, so no trait import is required.
use crate::ir::IntelligenceSource;

/// Build a minimal SCIP index in memory for testing.
fn build_test_index() -> Index {
    let mut idx = Index::new();

    let mut doc = scip_types::Document::new();
    doc.relative_path = "src/main.rs".to_owned();

    // Definition occurrence
    let mut def_occ = scip_types::Occurrence::new();
    def_occ.range = vec![10, 4, 18]; // line 10, col 4..18
    def_occ.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
    def_occ.symbol_roles = 1; // Definition
    doc.occurrences.push(def_occ);

    // Reference occurrence
    let mut ref_occ = scip_types::Occurrence::new();
    ref_occ.range = vec![20, 8, 22]; // line 20, col 8..22
    ref_occ.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
    ref_occ.symbol_roles = 0; // Reference (not definition)
    doc.occurrences.push(ref_occ);

    // Symbol info
    let mut info = scip_types::SymbolInformation::new();
    info.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
    info.documentation = vec!["A test struct for unit testing.".to_owned()];
    doc.symbols.push(info);

    // Second file with a reference
    let mut doc2 = scip_types::Document::new();
    doc2.relative_path = "src/lib.rs".to_owned();

    let mut ref_occ2 = scip_types::Occurrence::new();
    ref_occ2.range = vec![5, 0, 8];
    ref_occ2.symbol = "scip-rust cargo test 0.1.0 src/main.rs/MyStruct#".to_owned();
    ref_occ2.symbol_roles = 0;
    doc2.occurrences.push(ref_occ2);

    idx.documents.push(doc);
    idx.documents.push(doc2);
    idx
}

fn write_index_to_file(idx: &Index) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    let bytes = idx.write_to_bytes().unwrap();
    file.write_all(&bytes).unwrap();
    file.flush().unwrap();
    file
}

#[test]
fn test_load_and_file_count() {
    let idx = build_test_index();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();
    assert_eq!(backend.file_count(), 2);
    assert!(backend.symbol_count() >= 1);
}

#[test]
fn test_has_index_for() {
    let idx = build_test_index();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();
    assert!(backend.has_index_for("src/main.rs"));
    assert!(backend.has_index_for("src/lib.rs"));
    assert!(!backend.has_index_for("src/unknown.rs"));
}

#[test]
fn test_find_definitions() {
    let idx = build_test_index();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let defs = backend
        .find_definitions("MyStruct", "src/main.rs", 10)
        .unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "MyStruct");
    assert_eq!(defs[0].file_path, "src/main.rs");
    assert_eq!(defs[0].line, 10);
    // No sibling definition follows in src/main.rs and the
    // occurrence range is single-line, so end_line stays None.
    // Issue #179: the MCP layer must fall back to the 50-line
    // heuristic in that case.
    assert_eq!(defs[0].end_line, None);
    assert!(matches!(defs[0].source, IntelligenceSource::Scip));
}

/// Issue #179: when a same-document sibling definition follows the
/// queried symbol, `find_definitions` reports an `end_line` so the
/// MCP layer can slice the body precisely instead of falling back
/// to the 50-line heuristic.
#[test]
fn test_find_definitions_end_line_from_sibling() {
    let idx = build_callees_fixture();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let defs = backend
        .find_definitions(
            "handle_request",
            "crates/codelens-mcp/src/server/router.rs",
            10,
        )
        .unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].line, 10);
    // Next definition (`other_fn`) starts at line 25, so the body
    // body extends through line 24 inclusive.
    assert_eq!(defs[0].end_line, Some(24));
}

#[test]
fn test_find_references_cross_file() {
    let idx = build_test_index();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let refs = backend
        .find_references("MyStruct", "src/main.rs", 10)
        .unwrap();
    // Should find: 1 def in main.rs + 1 ref in main.rs + 1 ref in lib.rs = 3
    assert_eq!(refs.len(), 3);
    let files: Vec<&str> = refs.iter().map(|r| r.file_path.as_str()).collect();
    assert!(files.contains(&"src/main.rs"));
    assert!(files.contains(&"src/lib.rs"));
}

#[test]
fn test_hover() {
    let idx = build_test_index();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let hover = backend.hover("src/main.rs", 10, 5).unwrap();
    assert!(hover.is_some());
    assert!(hover.unwrap().contains("test struct"));
}

#[test]
fn test_short_name() {
    assert_eq!(
        super::parse::short_name("scip-rust cargo pkg 0.1.0 src/main.rs/MyStruct#"),
        "MyStruct"
    );
    assert_eq!(
        super::parse::short_name("scip-go gomod example.com/pkg src/handler.go/HandleRequest."),
        "HandleRequest"
    );
    assert_eq!(super::parse::short_name("simple_name"), "simple_name");
}

#[test]
fn test_source() {
    let idx = build_test_index();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();
    assert!(matches!(backend.source(), IntelligenceSource::Scip));
}

/// Build a SCIP fixture with two functions in router.rs:
///   line 10: `fn handle_request(...)` — definition
///   line 12: `dispatch_tool(...)` — reference inside handle_request body
///   line 14: `read_resource(...)` — reference inside handle_request body
///   line 25: `fn other_fn(...)` — next definition (closes handle_request body at 25)
///   line 27: `read_resource(...)` — reference in other_fn (must NOT be a callee)
/// Plus dispatch_tool's def in dispatch/mod.rs:5 and read_resource's def in
/// resources.rs:8 so callees can be resolved to their files.
fn build_callees_fixture() -> Index {
    let mut idx = Index::new();

    let dispatch_tool = "scip-rust cargo codelens-mcp 1.9 dispatch/mod/dispatch_tool().";
    let read_resource = "scip-rust cargo codelens-mcp 1.9 resources/read_resource().";
    let handle_request = "scip-rust cargo codelens-mcp 1.9 server/router/handle_request().";
    let other_fn = "scip-rust cargo codelens-mcp 1.9 server/router/other_fn().";

    let mut router = scip_types::Document::new();
    router.relative_path = "crates/codelens-mcp/src/server/router.rs".to_owned();
    // handle_request def @ line 10
    let mut def = scip_types::Occurrence::new();
    def.range = vec![10, 4, 18];
    def.symbol = handle_request.to_owned();
    def.symbol_roles = 1;
    router.occurrences.push(def);
    // call to dispatch_tool @ line 12
    let mut c1 = scip_types::Occurrence::new();
    c1.range = vec![12, 8, 21];
    c1.symbol = dispatch_tool.to_owned();
    c1.symbol_roles = 0;
    router.occurrences.push(c1);
    // call to read_resource @ line 14
    let mut c2 = scip_types::Occurrence::new();
    c2.range = vec![14, 8, 21];
    c2.symbol = read_resource.to_owned();
    c2.symbol_roles = 0;
    router.occurrences.push(c2);
    // other_fn def @ line 25 (closes handle_request body)
    let mut def2 = scip_types::Occurrence::new();
    def2.range = vec![25, 4, 12];
    def2.symbol = other_fn.to_owned();
    def2.symbol_roles = 1;
    router.occurrences.push(def2);
    // call to read_resource @ line 27 — inside other_fn, NOT handle_request
    let mut c3 = scip_types::Occurrence::new();
    c3.range = vec![27, 8, 21];
    c3.symbol = read_resource.to_owned();
    c3.symbol_roles = 0;
    router.occurrences.push(c3);
    idx.documents.push(router);

    let mut dispatch_doc = scip_types::Document::new();
    dispatch_doc.relative_path = "crates/codelens-mcp/src/dispatch/mod.rs".to_owned();
    let mut d_def = scip_types::Occurrence::new();
    d_def.range = vec![5, 4, 17];
    d_def.symbol = dispatch_tool.to_owned();
    d_def.symbol_roles = 1;
    dispatch_doc.occurrences.push(d_def);
    idx.documents.push(dispatch_doc);

    let mut resources_doc = scip_types::Document::new();
    resources_doc.relative_path = "crates/codelens-mcp/src/resources.rs".to_owned();
    let mut r_def = scip_types::Occurrence::new();
    r_def.range = vec![8, 4, 17];
    r_def.symbol = read_resource.to_owned();
    r_def.symbol_roles = 1;
    resources_doc.occurrences.push(r_def);
    idx.documents.push(resources_doc);

    idx
}

#[test]
fn find_callees_within_function_body_resolves_files() {
    // L1 acceptance — `find_callees(handle_request, router.rs)` must
    // surface the two callees inside the body (dispatch_tool,
    // read_resource) with correct resolved files, and must NOT return
    // the read_resource call that lives in the *next* function.
    let idx = build_callees_fixture();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let callees =
        backend.find_callees("handle_request", "crates/codelens-mcp/src/server/router.rs");
    let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"dispatch_tool"),
        "dispatch_tool missing: {names:?}"
    );
    assert!(
        names.contains(&"read_resource"),
        "read_resource missing: {names:?}"
    );
    // Body extent must exclude the call in the next function.
    let read_lines: Vec<usize> = callees
        .iter()
        .filter(|c| c.name == "read_resource")
        .map(|c| c.line)
        .collect();
    assert_eq!(
        read_lines,
        vec![14],
        "read_resource at line 27 belongs to other_fn, not handle_request"
    );

    let dispatch = callees
        .iter()
        .find(|c| c.name == "dispatch_tool")
        .expect("dispatch_tool entry");
    assert_eq!(
        dispatch.resolved_file.as_deref(),
        Some("crates/codelens-mcp/src/dispatch/mod.rs"),
        "callee def file must be resolved via SCIP"
    );
    assert_eq!(dispatch.resolution, Some("scip"));
    assert!(dispatch.confidence >= 0.9);
}

#[test]
fn find_callees_returns_empty_when_function_absent() {
    // Negative case: when the requested function has no def
    // occurrence in the given file, return empty so the MCP layer
    // cleanly falls through to tree-sitter without claiming a
    // false-positive resolution.
    let idx = build_callees_fixture();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let callees = backend.find_callees(
        "no_such_function",
        "crates/codelens-mcp/src/server/router.rs",
    );
    assert!(callees.is_empty());

    let callees_wrong_file = backend.find_callees("handle_request", "src/unknown.rs");
    assert!(callees_wrong_file.is_empty());
}

#[test]
fn find_callers_resolves_enclosing_function_via_next_def() {
    // L1 slice 2 acceptance — `find_callers(dispatch_tool)` must
    // attribute the call site at router.rs:12 to handle_request (the
    // function whose body contains line 12) and the call at line 27
    // to other_fn. Top-level references (outside any fn body) are
    // skipped. The fixture covers both happy paths and the
    // "outside-body" rejection case.
    let idx = build_callees_fixture();
    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    let callers = backend.find_callers("read_resource");
    // read_resource is referenced at router.rs:14 (in handle_request)
    // AND at router.rs:27 (in other_fn). Both are valid callers.
    let mut caller_pairs: Vec<(String, usize)> = callers
        .iter()
        .map(|c| (c.function.clone(), c.line))
        .collect();
    caller_pairs.sort();
    assert_eq!(
        caller_pairs,
        vec![
            ("handle_request".to_owned(), 14),
            ("other_fn".to_owned(), 27),
        ],
        "callers should attribute occurrences to their enclosing fn"
    );

    for c in &callers {
        assert_eq!(c.resolution, Some("scip"));
        assert!(c.confidence >= 0.9);
        assert_eq!(c.file, "crates/codelens-mcp/src/server/router.rs");
    }
}

#[test]
fn find_callers_returns_empty_for_unknown_or_non_function() {
    // Negative cases:
    //   1. Unknown name → empty (caller falls through to tree-sitter).
    //   2. A symbol that exists but is not function-like (no `()` in
    //      its descriptor — a struct or field) must not be reported
    //      as having callers; otherwise `get_callers("MyStruct")`
    //      would return everywhere the type is mentioned, which is
    //      not the call-graph contract.
    let mut idx = build_callees_fixture();
    // Append a struct-like symbol "Config#" referenced from inside
    // handle_request body. find_callers("Config") must NOT pick it up.
    let struct_sym = "scip-rust cargo codelens-mcp 1.9 server/router/Config#".to_owned();
    let mut struct_def = scip_types::Occurrence::new();
    struct_def.range = vec![18, 4, 10];
    struct_def.symbol = struct_sym.clone();
    struct_def.symbol_roles = 1; // definition (struct)
    idx.documents[0].occurrences.push(struct_def);
    let mut struct_ref = scip_types::Occurrence::new();
    struct_ref.range = vec![13, 8, 14];
    struct_ref.symbol = struct_sym.clone();
    struct_ref.symbol_roles = 0;
    idx.documents[0].occurrences.push(struct_ref);

    let file = write_index_to_file(&idx);
    let backend = ScipBackend::load(file.path()).unwrap();

    assert!(backend.find_callers("no_such_function").is_empty());
    assert!(
        backend.find_callers("Config").is_empty(),
        "non-function symbols must be filtered by is_function_like_symbol"
    );
}
