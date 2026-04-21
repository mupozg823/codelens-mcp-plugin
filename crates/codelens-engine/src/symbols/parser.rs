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
        let raw_name = node_text(name_node, source_bytes).trim().to_owned();
        if raw_name.is_empty() {
            continue;
        }

        // Phase 3-2 Serena-parity: Rust `impl Type { fn method() }`
        // blocks are captured as `@impl.def` so the method ends up
        // nested under an `impl Type` container (disambiguation with
        // `[N]` indices is applied after `nest_symbols`). Without
        // this, methods of the same type name across multiple impl
        // blocks collapsed into sibling `function.def` captures at
        // the file root, and `find_referencing_symbols` could not
        // report an enclosing impl block at all.
        let is_impl_capture = capture_name.starts_with("impl.");
        let name = if is_impl_capture {
            format!("impl {raw_name}")
        } else {
            raw_name.clone()
        };

        let body = include_body.then(|| node_text(def_node, source_bytes).to_owned());
        symbols.push(ParsedSymbol {
            name: name.clone(),
            kind: capture_name_to_kind(capture_name),
            file_path: file_path_owned.clone(),
            line: def_node.start_position().row + 1,
            column: name_node.start_position().column + 1,
            start_byte: def_node.start_byte() as u32,
            end_byte: def_node.end_byte() as u32,
            end_line: def_node.end_position().row + 1,
            signature: build_signature(def_node, source_bytes, &name),
            body,
            name_path: name,
            children: Vec::new(),
        });
    }

    // Phase 3-2: assign `[N]` indices to Rust `impl Type` blocks in
    // source order so repeated `impl ToolMetricsRegistry { … }`
    // entries stay distinguishable. Matches Serena's
    // `impl ToolMetricsRegistry[0]/method_name` format.
    index_impl_blocks(&mut symbols);

    Ok(nest_symbols(dedup_symbols(symbols)))
}

fn index_impl_blocks(symbols: &mut [ParsedSymbol]) {
    let mut order: Vec<usize> = symbols
        .iter()
        .enumerate()
        .filter_map(|(idx, sym)| {
            if sym.name.starts_with("impl ") {
                Some(idx)
            } else {
                None
            }
        })
        .collect();
    // Deterministic source order — the parser iterates query matches
    // in arbitrary order otherwise.
    order.sort_by_key(|&idx| symbols[idx].start_byte);
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for idx in order {
        let key = symbols[idx].name.clone();
        let n = counts.entry(key).or_insert(0);
        let suffix = format!("[{n}]");
        symbols[idx].name.push_str(&suffix);
        symbols[idx].name_path.push_str(&suffix);
        *n += 1;
    }
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
    to_symbol_info_with_source(symbol, depth, None)
}

pub(crate) fn to_symbol_info_with_source(
    symbol: ParsedSymbol,
    depth: usize,
    source: Option<&str>,
) -> SymbolInfo {
    let children = if depth == 0 || depth > 1 {
        symbol
            .children
            .into_iter()
            .map(|child| to_symbol_info_with_source(child, depth.saturating_sub(1), source))
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
        body: source
            .map(|source| slice_source(source, symbol.start_byte, symbol.end_byte))
            .or(symbol.body),
        children,
        start_byte: symbol.start_byte,
        end_byte: symbol.end_byte,
        end_line: symbol.end_line,
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

/// Walk backward from the symbol's `start_byte` to absorb any
/// immediately-preceding documentation comment block (Rust `///`/`//!`,
/// JS/TS `//` line-comments, Python/Shell `#`, Lua `--`). Tree-sitter's
/// `start_byte` points at the signature's first token, so without this
/// walk the returned body drops the docstring — a visible regression
/// against Serena, whose `body_location` already spans the doc run.
///
/// Semantics:
/// - Walk lines upward from the byte just before `start_byte`.
/// - Include any line whose trimmed form begins with a language-agnostic
///   line-comment marker (`///`, `//!`, `//`, `#`, `--`).
/// - Tolerate at most one fully-blank separator line between the doc
///   block and the signature; stop on any other non-comment line.
/// - Block comments (`/** … */`) aren't handled here — they require
///   forward scanning and are left for a dedicated pass.
///
/// Returns the byte offset of the first character on the earliest doc
/// line (or the original `start_byte` if no doc block precedes it).
pub(crate) fn extend_start_to_doc_comments(source: &str, start_byte: u32) -> u32 {
    let start = start_byte as usize;
    if start == 0 || start > source.len() {
        return start_byte;
    }
    let bytes = source.as_bytes();
    // Position `cursor` at the end of the line that precedes the
    // signature's start. If `start` already sits at a line start
    // (common — tree-sitter aligns to token start), back up one byte
    // to land inside the previous line.
    let mut cursor = start;
    if cursor > 0 && bytes[cursor - 1] == b'\n' {
        cursor -= 1;
    }
    let mut doc_block_start: Option<usize> = None;
    let mut tolerated_blank = false;
    loop {
        // Find the start of the line that contains `cursor`.
        let line_start = bytes[..cursor]
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|nl| nl + 1)
            .unwrap_or(0);
        let line_end = cursor;
        let line_bytes = &bytes[line_start..line_end];
        let line_str = match std::str::from_utf8(line_bytes) {
            Ok(s) => s,
            Err(_) => break,
        };
        let trimmed = line_str.trim();
        let is_doc = trimmed.starts_with("///")
            || trimmed.starts_with("//!")
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("--");
        let is_blank = trimmed.is_empty();
        if is_doc {
            doc_block_start = Some(line_start);
            tolerated_blank = false;
        } else if is_blank && doc_block_start.is_some() && !tolerated_blank {
            // Allow a single blank line between the doc block and the
            // signature (common Rust/JSDoc style).
            tolerated_blank = true;
        } else {
            break;
        }
        if line_start == 0 {
            break;
        }
        // Step to the end of the previous line (just before the '\n').
        cursor = line_start.saturating_sub(1);
    }
    doc_block_start.map(|s| s as u32).unwrap_or(start_byte)
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
    } else if capture_name.starts_with("impl") {
        // Rust impl blocks surface as Class so downstream consumers
        // (ranking, surface filters) treat them like any other type
        // container; the `impl ` prefix on the name distinguishes
        // them from plain `struct`/`enum` entries.
        SymbolKind::Class
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
