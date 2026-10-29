//! Precise JS/TS scope analysis using oxc_semantic.
//! Provides compiler-grade reference resolution without LSP.

use anyhow::Result;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType};
use serde::Serialize;
use std::path::Path;

mod declarations;
pub use declarations::{
    ByteSpan, DeclarationAnalysis, ExactDeclaration, ExactReference, ModuleExport, ModuleImport,
    UnsupportedDeclaration, analyze_declarations,
};

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedReference {
    pub symbol_name: String,
    pub kind: RefKind,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    Definition,
    Read,
    Write,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeSymbol {
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub is_exported: bool,
    pub reference_count: usize,
    pub is_mutated: bool,
}

/// Check if a file is JS/TS (supported by oxc).
pub fn is_js_ts(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts")
    )
}

/// Get all symbols in a JS/TS file with scope-aware metadata.
pub fn get_scope_symbols(source: &str, file_path: &str) -> Result<Vec<ScopeSymbol>> {
    let alloc = Allocator::default();
    let source_type = SourceType::from_path(file_path)
        .map_err(|_| anyhow::anyhow!("unsupported file type: {}", file_path))?;
    let parsed = Parser::new(&alloc, source, source_type).parse();

    if parsed.panicked {
        anyhow::bail!("oxc parser panicked on {}", file_path);
    }

    let built = SemanticBuilder::new().build(&parsed.program);
    let semantic = &built.semantic;
    let scoping = semantic.scoping();

    let lines = LineIndex::new(source.as_bytes());
    let mut symbols = Vec::new();

    for symbol_id in scoping.symbol_ids() {
        let name = scoping.symbol_name(symbol_id).to_string();
        let node_id = scoping.symbol_declaration(symbol_id);
        let node = semantic.nodes().get_node(node_id);
        let span = node.span();
        let (line, column) = lines.line_col(span.start as usize);

        let ref_count = scoping.get_resolved_references(symbol_id).count();
        let is_mutated = scoping.symbol_is_mutated(symbol_id);
        let flags = scoping.symbol_flags(symbol_id);
        let is_exported = format!("{:?}", flags).contains("Export");

        symbols.push(ScopeSymbol {
            name,
            line,
            column,
            is_exported,
            reference_count: ref_count,
            is_mutated,
        });
    }

    Ok(symbols)
}

/// Find all references to a symbol by name in a JS/TS file.
/// Scope-aware: distinguishes definitions, reads, and writes.
pub fn find_references_precise(
    source: &str,
    file_path: &str,
    symbol_name: &str,
) -> Result<Vec<ResolvedReference>> {
    let alloc = Allocator::default();
    let source_type = SourceType::from_path(file_path)
        .map_err(|_| anyhow::anyhow!("unsupported file type: {}", file_path))?;
    let parsed = Parser::new(&alloc, source, source_type).parse();

    if parsed.panicked {
        anyhow::bail!("oxc parser panicked on {}", file_path);
    }

    let built = SemanticBuilder::new().build(&parsed.program);
    let semantic = &built.semantic;
    let scoping = semantic.scoping();

    let lines = LineIndex::new(source.as_bytes());
    let mut refs = Vec::new();

    for symbol_id in scoping.symbol_ids() {
        let name = scoping.symbol_name(symbol_id);
        if name != symbol_name {
            continue;
        }

        // Declaration
        let node_id = scoping.symbol_declaration(symbol_id);
        let decl_span = semantic.nodes().get_node(node_id).span();
        let (line, col) = lines.line_col(decl_span.start as usize);
        refs.push(ResolvedReference {
            symbol_name: symbol_name.to_string(),
            kind: RefKind::Definition,
            line,
            column: col,
        });

        // All resolved references
        for reference in scoping.get_resolved_references(symbol_id) {
            let span = semantic.reference_span(reference);
            let (line, col) = lines.line_col(span.start as usize);
            let kind = if reference.is_write() {
                RefKind::Write
            } else {
                RefKind::Read
            };
            refs.push(ResolvedReference {
                symbol_name: symbol_name.to_string(),
                kind,
                line,
                column: col,
            });
        }
    }

    Ok(refs)
}

/// Find unresolved references (potential missing imports or globals).
pub fn find_unresolved(source: &str, file_path: &str) -> Result<Vec<String>> {
    let alloc = Allocator::default();
    let source_type = SourceType::from_path(file_path)
        .map_err(|_| anyhow::anyhow!("unsupported file type: {}", file_path))?;
    let parsed = Parser::new(&alloc, source, source_type).parse();

    if parsed.panicked {
        anyhow::bail!("oxc parser panicked on {}", file_path);
    }

    let built = SemanticBuilder::new().build(&parsed.program);
    let scoping = built.semantic.scoping();

    let mut unresolved: Vec<String> = scoping
        .root_unresolved_references()
        .keys()
        .map(|name| name.to_string())
        .collect();

    unresolved.sort();
    unresolved.dedup();
    Ok(unresolved)
}

/// 1-based line and byte column lookups for one source, built once per file.
/// Rescanning from byte 0 for every reference made a bundled file quadratic.
struct LineIndex {
    /// Byte offset where each line starts; `starts[0] == 0`.
    starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    fn new(source: &[u8]) -> Self {
        let mut starts = vec![0];
        starts.extend(
            source
                .iter()
                .enumerate()
                .filter(|&(_, &b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        Self {
            starts,
            len: source.len(),
        }
    }

    /// Offsets past the end clamp to the end.
    fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.len);
        let line = self.starts.partition_point(|&start| start <= offset);
        (line, offset - self.starts[line - 1] + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The byte scan `LineIndex` replaced, kept as the oracle.
    fn scan_line_col(source: &[u8], offset: usize) -> (usize, usize) {
        let offset = offset.min(source.len());
        let mut line = 1;
        let mut col = 1;
        for &b in &source[..offset] {
            if b == b'\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    #[test]
    fn line_index_matches_the_byte_scan_at_every_offset() {
        for source in [
            "",
            "\n",
            "\n\n",
            "a",
            "ab\ncd",
            "ab\r\ncd\r\n",
            "x\n\ny\n",
            "// 한국어 😀\nexport function t() {}\nt();\n",
        ] {
            let bytes = source.as_bytes();
            let lines = LineIndex::new(bytes);
            for offset in 0..=bytes.len() + 2 {
                assert_eq!(
                    lines.line_col(offset),
                    scan_line_col(bytes, offset),
                    "{source:?} at {offset}"
                );
            }
        }
    }

    #[test]
    fn analysis_lite_selects_one_same_name_declaration() {
        let source =
            "function same() {}\nsame();\nfunction wrapper() { function same() {} same(); }\n";
        let analysis = analyze_declarations(source, "test.ts").unwrap();
        let declarations = analysis.named("same");
        assert_eq!(declarations.len(), 2);
        assert_eq!(declarations[0].name_span, ByteSpan { start: 9, end: 13 });
        assert_eq!(declarations[0].references.len(), 1);
        assert_eq!(declarations[0].references[0].span.start, 19);
        assert_eq!(declarations[1].references.len(), 1);
        // Compatibility: the legacy name-only API still merges both scopes.
        assert_eq!(
            find_references_precise(source, "test.ts", "same")
                .unwrap()
                .len(),
            4
        );
    }

    #[test]
    fn test_get_scope_symbols() {
        let source = r#"
const name = "hello";
function greet(msg) {
    console.log(msg);
    return msg.toUpperCase();
}
let x = greet(name);
"#;
        let symbols = get_scope_symbols(source, "test.js").unwrap();
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"x"));
    }

    #[test]
    fn test_find_references() {
        let source = r#"
function add(a, b) { return a + b; }
const result = add(1, 2);
console.log(add(3, 4));
"#;
        let refs = find_references_precise(source, "test.js", "add").unwrap();
        assert!(refs.len() >= 3);
        assert!(refs.iter().any(|r| matches!(r.kind, RefKind::Definition)));
    }

    #[test]
    fn test_typescript_support() {
        let source = r#"
interface User { name: string; }
function getUser(id: number): User {
    return { name: "test" };
}
const user: User = getUser(1);
"#;
        let refs = find_references_precise(source, "test.ts", "getUser").unwrap();
        assert!(refs.len() >= 2);
    }

    #[test]
    fn test_mutation_detection() {
        let source = "let counter = 0;\ncounter++;\ncounter = counter + 1;\n";
        let symbols = get_scope_symbols(source, "test.js").unwrap();
        let counter = symbols.iter().find(|s| s.name == "counter").unwrap();
        assert!(counter.is_mutated);
    }

    #[test]
    fn test_unresolved() {
        let source = "console.log(unknownVar);\nfetch('/api');\n";
        let unresolved = find_unresolved(source, "test.js").unwrap();
        assert!(unresolved.contains(&"console".to_string()));
        assert!(unresolved.contains(&"unknownVar".to_string()));
    }

    #[test]
    fn test_is_js_ts() {
        assert!(is_js_ts(Path::new("app.ts")));
        assert!(is_js_ts(Path::new("index.jsx")));
        assert!(!is_js_ts(Path::new("main.py")));
    }
}
