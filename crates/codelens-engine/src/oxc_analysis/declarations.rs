//! Candidate-only declaration identity. All offsets and columns are UTF-8 bytes.
//! The legacy name-only API intentionally keeps its previous behavior.

use super::{RefKind, offset_to_line_col};
use anyhow::{Result, bail};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::{GetSpan, SourceType};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
}

impl ByteSpan {
    pub fn contains(self, other: Self) -> bool {
        self.start <= other.start && self.end >= other.end
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExactReference {
    pub span: ByteSpan,
    /// One-based line and UTF-8 byte column (never a UTF-16 position).
    pub line: usize,
    pub column: usize,
    pub kind: RefKind,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExactDeclaration {
    pub name: String,
    pub name_span: ByteSpan,
    pub declaration_span: ByteSpan,
    pub line: usize,
    pub column: usize,
    pub is_root: bool,
    pub is_import: bool,
    pub is_type: bool,
    pub references: Vec<ExactReference>,
}

#[derive(Debug, Clone)]
pub struct ModuleImport {
    pub local_name: String,
    pub local_span: ByteSpan,
    pub module: String,
    pub is_type: bool,
    pub is_namespace: bool,
    pub is_default: bool,
    pub imported_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleExport {
    pub exported_name: String,
    pub local_name: Option<String>,
    pub module: Option<String>,
    pub is_type: bool,
    pub imported_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeclarationAnalysis {
    pub declarations: Vec<ExactDeclaration>,
    pub unsupported_members: Vec<UnsupportedDeclaration>,
    pub imports: Vec<ModuleImport>,
    pub exports: Vec<ModuleExport>,
}

#[derive(Debug, Clone)]
pub struct UnsupportedDeclaration {
    pub name: String,
    pub name_span: ByteSpan,
    pub declaration_span: ByteSpan,
}

impl DeclarationAnalysis {
    pub fn named(&self, name: &str) -> Vec<&ExactDeclaration> {
        self.declarations
            .iter()
            .filter(|d| d.name == name)
            .collect()
    }

    /// Exact lexical binding for a source token, including shadowed imports.
    pub fn binding_at(&self, span: ByteSpan) -> Option<&ExactDeclaration> {
        self.declarations
            .iter()
            .find(|d| d.references.iter().any(|reference| reference.span == span))
    }
}

pub fn analyze_declarations(source: &str, file_path: &str) -> Result<DeclarationAnalysis> {
    let alloc = Allocator::default();
    let source_type = SourceType::from_path(file_path)?;
    let parsed = Parser::new(&alloc, source, source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        bail!("oxc parse errors in {file_path}: {:?}", parsed.errors);
    }
    let built = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !built.errors.is_empty() {
        bail!("oxc semantic errors in {file_path}: {:?}", built.errors);
    }
    let semantic = &built.semantic;
    let scoping = semantic.scoping();
    let bytes = source.as_bytes();
    let mut declarations = Vec::new();
    for id in scoping.symbol_ids() {
        let name = scoping.symbol_name(id).to_owned();
        let token = scoping.symbol_span(id);
        let name_span = ByteSpan {
            start: token.start as usize,
            end: token.end as usize,
        };
        // Refuse an unexpected declaration span instead of silently substituting
        // the enclosing function's start (the legacy API uses that start).
        if source.get(name_span.start..name_span.end) != Some(name.as_str()) {
            bail!("oxc name-token span mismatch for {name} in {file_path}");
        }
        let declaration = semantic
            .nodes()
            .get_node(scoping.symbol_declaration(id))
            .span();
        let (line, column) = offset_to_line_col(bytes, name_span.start);
        let flags = scoping.symbol_flags(id);
        let mut references = scoping
            .get_resolved_references(id)
            .map(|reference| {
                let span = semantic.reference_span(reference);
                let span = ByteSpan {
                    start: span.start as usize,
                    end: span.end as usize,
                };
                let (line, column) = offset_to_line_col(bytes, span.start);
                ExactReference {
                    span,
                    line,
                    column,
                    kind: if reference.is_write() {
                        RefKind::Write
                    } else {
                        RefKind::Read
                    },
                }
            })
            .collect::<Vec<_>>();
        references.sort_by_key(|reference| (reference.span.start, reference.span.end));
        declarations.push(ExactDeclaration {
            name,
            name_span,
            declaration_span: ByteSpan {
                start: declaration.start as usize,
                end: declaration.end as usize,
            },
            line,
            column,
            is_root: scoping.symbol_scope_id(id) == scoping.root_scope_id(),
            is_import: flags.is_import(),
            is_type: flags.is_type(),
            references,
        });
    }
    declarations.sort_by_key(|d| (d.name_span.start, d.name_span.end));
    let tokens = module_link_names(source, file_path)?;
    let imports = parsed
        .module_record
        .import_entries
        .iter()
        .map(|entry| ModuleImport {
            local_name: entry.local_name.name.to_string(),
            local_span: ByteSpan {
                start: entry.local_name.span.start as usize,
                end: entry.local_name.span.end as usize,
            },
            module: entry.module_request.name.to_string(),
            is_type: entry.is_type,
            is_namespace: entry.import_name.is_namespace_object(),
            is_default: entry.import_name.is_default(),
            imported_name: if entry.import_name.is_default() {
                Some("default".to_owned())
            } else {
                tokens
                    .imports
                    .get(&(
                        entry.local_name.span.start as usize,
                        entry.local_name.span.end as usize,
                    ))
                    .cloned()
            },
        })
        .collect();
    let exports = parsed
        .module_record
        .local_export_entries
        .iter()
        .chain(parsed.module_record.indirect_export_entries.iter())
        .filter_map(|entry| {
            let span = entry.export_name.span()?;
            let exported_name = if entry.export_name.is_default() {
                "default".to_owned()
            } else {
                source
                    .get(span.start as usize..span.end as usize)?
                    .trim_matches(['\'', '"'])
                    .to_owned()
            };
            Some(ModuleExport {
                exported_name,
                local_name: entry.local_name.name().map(|name| name.to_string()),
                module: entry
                    .module_request
                    .as_ref()
                    .map(|module| module.name.to_string()),
                is_type: entry.is_type,
                imported_name: tokens
                    .exports
                    .get(&(span.start as usize, span.end as usize))
                    .cloned(),
            })
        })
        .collect();
    Ok(DeclarationAnalysis {
        declarations,
        unsupported_members: tokens.members,
        imports,
        exports,
    })
}

type LinkNames = std::collections::HashMap<(usize, usize), String>;

struct ModuleTokens {
    imports: LinkNames,
    exports: LinkNames,
    members: Vec<UnsupportedDeclaration>,
}

/// Tree-sitter retains the imported-name token that OXC's public module-entry
/// enum does not expose through an accessor. Key by OXC-verified local/export
/// token spans, never by a regex match or same-name guess in a comment.
fn module_link_names(source: &str, path: &str) -> Result<ModuleTokens> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str());
    let language = match extension {
        Some("tsx") => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Some("ts" | "mts" | "cts") => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        _ => tree_sitter_javascript::LANGUAGE.into(),
    };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language)?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| anyhow::anyhow!("module token parser returned no tree"))?;
    let mut imports = LinkNames::new();
    let mut exports = LinkNames::new();
    let mut members = Vec::new();
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        if matches!(
            node.kind(),
            "method_definition"
                | "method_signature"
                | "abstract_method_signature"
                | "public_field_definition"
                | "property_signature"
                | "abstract_property_signature"
        ) && let Some(name) = node.child_by_field_name("name")
            && matches!(name.kind(), "property_identifier" | "identifier")
        {
            members.push(UnsupportedDeclaration {
                name: source[name.start_byte()..name.end_byte()].to_owned(),
                name_span: ByteSpan {
                    start: name.start_byte(),
                    end: name.end_byte(),
                },
                declaration_span: ByteSpan {
                    start: node.start_byte(),
                    end: node.end_byte(),
                },
            });
        }
        if matches!(node.kind(), "import_specifier" | "export_specifier")
            && let Some(name) = node.child_by_field_name("name")
            && name.kind() == "identifier"
        {
            let local = node.child_by_field_name("alias").unwrap_or(name);
            if local.kind() == "identifier" {
                let map = if node.kind() == "import_specifier" {
                    &mut imports
                } else {
                    &mut exports
                };
                map.insert(
                    (local.start_byte(), local.end_byte()),
                    source[name.start_byte()..name.end_byte()].to_owned(),
                );
            }
        }
        let mut cursor = node.walk();
        nodes.extend(node.children(&mut cursor));
    }
    members.sort_by_key(|member| member.name_span.start);
    Ok(ModuleTokens {
        imports,
        exports,
        members,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_lite_utf8_tokens_do_not_use_utf16_offsets() {
        let source = "// 한국어 😀\nexport function target() {}\ntarget();\n";
        let analysis = analyze_declarations(source, "unicode.ts").unwrap();
        let decl = analysis.named("target")[0];
        assert_eq!(decl.name_span.start, 34);
        assert_eq!(&source[decl.name_span.start..decl.name_span.end], "target");
        assert_eq!(decl.references[0].line, 3);
        assert_eq!(decl.references[0].span.start, 46);
        assert_ne!(decl.name_span.start, source[..34].encode_utf16().count());
    }

    #[test]
    fn analysis_lite_invalid_source_is_an_error_not_no_references() {
        assert!(analyze_declarations("export function broken( {", "broken.ts").is_err());
    }
}
