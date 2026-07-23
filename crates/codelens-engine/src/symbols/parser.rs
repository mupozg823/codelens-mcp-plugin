use super::LanguageConfig;
use super::types::{ParsedSymbol, SymbolInfo, SymbolKind, SymbolProvenance, make_symbol_id};
use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, LazyLock, Mutex};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Query, QueryCursor};

/// Cached compiled tree-sitter Query per language extension.
static QUERY_CACHE: LazyLock<Mutex<std::collections::HashMap<&'static str, Arc<Query>>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub(crate) fn cached_query(config: &LanguageConfig) -> Result<Arc<Query>> {
    let mut cache = QUERY_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(q) = cache.get(config.extension) {
        return Ok(Arc::clone(q));
    }
    let q = Query::new(&config.language, config.query)
        .with_context(|| format!("invalid query for {}", config.extension))?;
    let q = Arc::new(q);
    cache.insert(config.extension, Arc::clone(&q));
    Ok(q)
}

pub(crate) fn parse_symbols(
    config: &LanguageConfig,
    file_path: &str,
    source: &str,
    include_body: bool,
) -> Result<Vec<ParsedSymbol>> {
    let mut parser = Parser::new();
    parser.set_language(&config.language).with_context(|| {
        format!(
            "failed to set tree-sitter language for {}",
            config.extension
        )
    })?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("failed to parse source"))?;
    let query = cached_query(config)?;
    let source_bytes = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut symbols = Vec::new();
    let file_path_owned = file_path.to_owned();

    let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);
    while let Some(matched) = matches.next() {
        let mut def_capture: Option<(&tree_sitter::QueryCapture<'_>, &str)> = None;
        let mut name_capture: Option<(&tree_sitter::QueryCapture<'_>, &str)> = None;

        for capture in matched.captures.iter() {
            let capture_name = &query.capture_names()[capture.index as usize];
            if capture_name.ends_with(".def") && def_capture.is_none() {
                def_capture = Some((capture, capture_name));
            }
            if capture_name.ends_with(".name") && name_capture.is_none() {
                name_capture = Some((capture, capture_name));
            }
        }

        let Some((def_capture, capture_name)) = def_capture else {
            continue;
        };
        let Some((name_capture, _)) = name_capture else {
            continue;
        };

        let def_node = def_capture.node;
        let name_node = name_capture.node;
        // #349: one canonical form for identifier matching — NFD jamo in
        // source (e.g. pasted from macOS filenames) must equal the NFC
        // query an agent types. Signature/body stay byte-faithful.
        let name =
            crate::unicode::nfc_identifier(node_text(name_node, source_bytes).trim()).into_owned();
        if name.is_empty() {
            continue;
        }

        let rust_impl_owner = (config.extension == "rs")
            .then(|| rust_impl_owner(def_node, source_bytes))
            .flatten();
        let body = include_body.then(|| node_text(def_node, source_bytes).to_owned());
        symbols.push(ParsedSymbol {
            name: name.clone(),
            kind: rust_impl_owner.as_ref().map_or_else(
                || capture_name_to_kind(capture_name),
                |_| SymbolKind::Method,
            ),
            file_path: file_path_owned.clone(),
            line: def_node.start_position().row + 1,
            column: name_node.start_position().column + 1,
            start_byte: def_node.start_byte() as u32,
            end_byte: def_node.end_byte() as u32,
            signature: build_signature(def_node, source_bytes, &name),
            body,
            name_path: rust_impl_owner
                .map_or_else(|| name.clone(), |owner| format!("{owner}/{name}")),
            children: Vec::new(),
        });
    }

    Ok(nest_symbols(dedup_symbols(symbols)))
}

pub(crate) fn flatten_symbols(symbols: Vec<ParsedSymbol>) -> Vec<ParsedSymbol> {
    let mut queue: VecDeque<ParsedSymbol> = symbols.into();
    let mut flat = Vec::new();

    while let Some(mut symbol) = queue.pop_front() {
        let children = std::mem::take(&mut symbol.children);
        queue.extend(children);
        flat.push(symbol);
    }

    flat
}

pub(crate) fn flatten_symbol_infos(mut symbol: SymbolInfo) -> Vec<SymbolInfo> {
    let children = std::mem::take(&mut symbol.children);
    let mut flattened = vec![symbol];
    for child in children {
        flattened.extend(flatten_symbol_infos(child));
    }
    flattened
}

pub(crate) fn to_symbol_info(symbol: ParsedSymbol, depth: usize) -> SymbolInfo {
    let children = if depth == 0 || depth > 1 {
        symbol
            .children
            .into_iter()
            .map(|child| to_symbol_info(child, depth.saturating_sub(1)))
            .collect()
    } else {
        Vec::new()
    };

    let id = make_symbol_id(&symbol.file_path, &symbol.kind, &symbol.name_path);
    let provenance = SymbolProvenance::from_path(&symbol.file_path);
    SymbolInfo {
        name: symbol.name,
        kind: symbol.kind,
        file_path: symbol.file_path,
        line: symbol.line,
        column: symbol.column,
        signature: symbol.signature,
        name_path: symbol.name_path,
        id,
        provenance,
        body: symbol.body,
        children,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
    }
}

pub(crate) fn slice_source(source: &str, start_byte: u32, end_byte: u32) -> String {
    let start_byte = start_byte as usize;
    let end_byte = end_byte as usize;
    source
        .as_bytes()
        .get(start_byte..end_byte)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or_default()
        .to_owned()
}

fn nest_symbols(symbols: Vec<ParsedSymbol>) -> Vec<ParsedSymbol> {
    let mut sorted = symbols;
    sorted.sort_by_key(|symbol| symbol.start_byte);

    let mut roots = Vec::new();
    for symbol in sorted {
        insert_symbol(&mut roots, symbol);
    }
    roots
}

fn dedup_symbols(symbols: Vec<ParsedSymbol>) -> Vec<ParsedSymbol> {
    let mut seen_range = HashSet::new();
    let mut seen_identity = HashSet::new();
    let mut deduped = Vec::new();

    for symbol in symbols {
        let range_key = (symbol.start_byte, symbol.end_byte);
        let identity_key = (symbol.name.clone(), symbol.line, symbol.kind.clone());
        if seen_range.insert(range_key) && seen_identity.insert(identity_key) {
            deduped.push(symbol);
        }
    }

    deduped
}

fn insert_symbol(container: &mut Vec<ParsedSymbol>, mut symbol: ParsedSymbol) {
    if let Some(parent) = container.iter_mut().rev().find(|candidate| {
        candidate.start_byte <= symbol.start_byte && candidate.end_byte >= symbol.end_byte
    }) {
        symbol.name_path = format!("{}/{}", parent.name_path, symbol.name);
        insert_symbol(&mut parent.children, symbol);
    } else {
        container.push(symbol);
    }
}

fn capture_name_to_kind(capture_name: &str) -> SymbolKind {
    if capture_name.starts_with("class") {
        SymbolKind::Class
    } else if capture_name.starts_with("interface") {
        SymbolKind::Interface
    } else if capture_name.starts_with("enum") {
        SymbolKind::Enum
    } else if capture_name.starts_with("module") {
        SymbolKind::Module
    } else if capture_name.starts_with("method") {
        SymbolKind::Method
    } else if capture_name.starts_with("function") {
        SymbolKind::Function
    } else if capture_name.starts_with("property") {
        SymbolKind::Property
    } else if capture_name.starts_with("variable") {
        SymbolKind::Variable
    } else if capture_name.starts_with("type_alias") {
        SymbolKind::TypeAlias
    } else {
        SymbolKind::Unknown
    }
}

fn build_signature(node: Node<'_>, source_bytes: &[u8], fallback: &str) -> String {
    let first_line = node_text(node, source_bytes)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or(fallback);

    if first_line.len() > 200 {
        // Find a char boundary at or before byte 200
        let truncate_at = first_line
            .char_indices()
            .take_while(|(i, _)| *i <= 200)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(200);
        format!("{}...", &first_line[..truncate_at])
    } else {
        first_line.to_owned()
    }
}

fn node_text<'a>(node: Node<'_>, source_bytes: &'a [u8]) -> &'a str {
    let start = node.start_byte();
    let end = node.end_byte();
    std::str::from_utf8(&source_bytes[start..end]).unwrap_or_default()
}

fn rust_impl_owner(def_node: Node<'_>, source_bytes: &[u8]) -> Option<String> {
    let mut ancestor = def_node.parent();
    while let Some(node) = ancestor {
        if node.kind() == "impl_item" {
            return node
                .child_by_field_name("type")
                .map(|owner| node_text(owner, source_bytes).trim().to_owned())
                .filter(|owner| !owner.is_empty());
        }
        ancestor = node.parent();
    }
    None
}

#[cfg(test)]
mod nfc_extraction_tests {
    use super::parse_symbols;
    use crate::lang_config::language_for_path;
    use std::path::Path;

    /// #349: extraction is the canonical-form choke point — an NFD jamo
    /// identifier in source comes out of the parser as NFC, so the
    /// index, the overview payloads, and the BM25F corpus all agree
    /// with the NFC queries agents type.
    #[test]
    fn nfd_identifier_extracts_as_nfc() {
        let nfd_fn = "\u{1112}\u{116e}\u{110b}\u{116f}\u{11ab}\u{110c}\u{1161}_\u{1111}\u{1161}\u{1109}\u{1165}"; // "후원자_파서" decomposed
        let source = format!("pub fn {nfd_fn}() -> i64 {{ 42 }}\n");
        let config = language_for_path(Path::new("lib.rs")).expect("rust config");
        let parsed = parse_symbols(&config, "lib.rs", &source, false).expect("parse");
        let names: Vec<&str> = parsed.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["후원자_파서"],
            "extracted name must be NFC (3+1+2 syllables), got codepoint counts {:?}",
            parsed
                .iter()
                .map(|s| s.name.chars().count())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rust_impl_method_name_path_includes_owner() {
        // Given: two homonymous methods declared by distinct Rust impl owners.
        let source = "struct Alpha;\nimpl Alpha { fn new() -> Self { Self } }\nstruct Beta;\nimpl Beta { fn new() -> Self { Self } }\n";
        let config = language_for_path(Path::new("lib.rs")).expect("rust config");

        // When: symbols are parsed for indexing.
        let parsed = parse_symbols(&config, "lib.rs", source, false).expect("parse");
        let method_paths: Vec<&str> = parsed
            .iter()
            .filter(|symbol| symbol.name == "new")
            .map(|symbol| symbol.name_path.as_str())
            .collect();

        // Then: declaration identity retains each impl owner.
        assert_eq!(method_paths, vec!["Alpha/new", "Beta/new"]);
    }
}
