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
    /// `const { imported: local } = require("module")`, not an ESM entry.
    pub is_require: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleExport {
    pub exported_name: String,
    pub local_name: Option<String>,
    pub module: Option<String>,
    pub is_type: bool,
    pub imported_name: Option<String>,
}

/// `member` read off a dynamic-import namespace: in
/// `import("spec").then(m => m.member)` the callback's only parameter is the
/// module namespace, so the `member` token reads export `member` of `spec`.
#[derive(Debug, Clone)]
pub struct DynamicImportMember {
    pub module: String,
    pub member: String,
    pub member_span: ByteSpan,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct DeclarationAnalysis {
    pub declarations: Vec<ExactDeclaration>,
    pub unsupported_members: Vec<UnsupportedDeclaration>,
    pub imports: Vec<ModuleImport>,
    pub exports: Vec<ModuleExport>,
    pub dynamic_import_members: Vec<DynamicImportMember>,
    /// `m.member` read off `const m = require("module")`: the same shape as a
    /// dynamic-import member, with `m` bound by the require declaration.
    pub require_members: Vec<DynamicImportMember>,
    /// Resolved-reference tokens that are the whole callee of a `new`
    /// expression (`new X()`, never `new X.Y()` or `new f(X)`).
    pub new_callees: Vec<ByteSpan>,
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

    /// The verified `require` destructuring entry a binding stands for.
    pub fn require_import(&self, declaration: &ExactDeclaration) -> Option<&ModuleImport> {
        self.imports
            .iter()
            .find(|import| import.is_require && import.local_span == declaration.name_span)
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
    let mut new_callees = Vec::new();
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
                // The parent of a `new X()` callee is the `new` expression,
                // whose span holds only the keyword before the token.
                let parent = semantic.nodes().parent_node(reference.node_id()).span();
                if source
                    .get(parent.start as usize..span.start)
                    .and_then(|head| head.strip_prefix("new"))
                    .is_some_and(|gap| !gap.is_empty() && gap.trim().is_empty())
                {
                    new_callees.push(span);
                }
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
    new_callees.sort_by_key(|span| (span.start, span.end));
    let tokens = module_link_names(source, file_path)?;
    let mut imports = parsed
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
            is_require: false,
        })
        .collect::<Vec<_>>();
    let mut exports = parsed
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
        .collect::<Vec<_>>();
    // Keep a `m.member` token only when OXC binds that `m` to the callback's
    // parameter; a shadowing `m` or another object of the same name is not
    // the namespace.
    let dynamic_import_members = tokens
        .dynamic
        .into_iter()
        .filter(|candidate| {
            declarations.iter().any(|d| {
                d.name_span == candidate.param_span
                    && d.references.iter().any(|r| r.span == candidate.object_span)
            })
        })
        .map(|candidate| {
            let (line, column) = offset_to_line_col(bytes, candidate.member_span.start);
            DynamicImportMember {
                module: candidate.module,
                member: candidate.member,
                member_span: candidate.member_span,
                line,
                column,
            }
        })
        .collect();
    let (cjs_imports, cjs_exports, require_members) =
        commonjs_links(&declarations, tokens.cjs, bytes);
    imports.extend(cjs_imports);
    exports.extend(cjs_exports);
    Ok(DeclarationAnalysis {
        declarations,
        unsupported_members: tokens.members,
        imports,
        exports,
        dynamic_import_members,
        require_members,
        new_callees,
    })
}

/// Keep a CommonJS candidate only where OXC agrees: `require`, `module` and
/// `exports` are the free (global) names, an exported value is a root binding,
/// and an imported local is a binding that is never reassigned.
fn commonjs_links(
    declarations: &[ExactDeclaration],
    cjs: CjsTokens,
    bytes: &[u8],
) -> (
    Vec<ModuleImport>,
    Vec<ModuleExport>,
    Vec<DynamicImportMember>,
) {
    let binding = |span: ByteSpan| {
        declarations
            .iter()
            .find(|d| d.references.iter().any(|r| r.span == span))
    };
    let constant = |span: ByteSpan| {
        declarations.iter().find(|d| {
            d.name_span == span
                && d.references
                    .iter()
                    .all(|r| !matches!(r.kind, RefKind::Write))
        })
    };
    let mut imports = Vec::new();
    for call in cjs
        .requires
        .iter()
        .filter(|call| binding(call.require_span).is_none())
    {
        for (imported, local_span) in &call.entries {
            if let Some(local) = constant(*local_span) {
                imports.push(ModuleImport {
                    local_name: local.name.clone(),
                    local_span: *local_span,
                    module: call.module.clone(),
                    is_type: false,
                    is_namespace: false,
                    is_default: false,
                    imported_name: Some(imported.clone()),
                    is_require: true,
                });
            }
        }
    }
    let exports = cjs
        .exports
        .into_iter()
        .filter(|export| export.free.iter().all(|span| binding(*span).is_none()))
        .filter_map(|export| {
            let local = binding(export.value_span).filter(|d| d.is_root)?;
            Some(ModuleExport {
                exported_name: export.name,
                local_name: Some(local.name.clone()),
                module: None,
                is_type: false,
                imported_name: None,
            })
        })
        .collect();
    let namespaces = cjs
        .namespaces
        .iter()
        .filter(|call| binding(call.require_span).is_none())
        .filter_map(|call| {
            let [(_, local_span)] = call.entries.as_slice() else {
                return None;
            };
            constant(*local_span).map(|_| (*local_span, call.module.as_str()))
        })
        .collect::<Vec<_>>();
    let members = cjs
        .member_reads
        .into_iter()
        .filter_map(|read| {
            let object = binding(read.object_span)?;
            let module = namespaces
                .iter()
                .find(|(span, _)| *span == object.name_span)?
                .1;
            let (line, column) = offset_to_line_col(bytes, read.member_span.start);
            Some(DynamicImportMember {
                module: module.to_owned(),
                member: read.member,
                member_span: read.member_span,
                line,
                column,
            })
        })
        .collect();
    (imports, exports, members)
}

type LinkNames = std::collections::HashMap<(usize, usize), String>;

struct ModuleTokens {
    imports: LinkNames,
    exports: LinkNames,
    members: Vec<UnsupportedDeclaration>,
    dynamic: Vec<DynamicCandidate>,
    cjs: CjsTokens,
}

/// CommonJS link candidates from tree-sitter; OXC confirms each binding in
/// `commonjs_links`.
#[derive(Default)]
struct CjsTokens {
    requires: Vec<RequireCall>,
    /// `const m = require("spec")`: one entry, the `m` token.
    namespaces: Vec<RequireCall>,
    exports: Vec<CjsExport>,
    /// `m.member` reads whose object is named like a namespace binding.
    member_reads: Vec<MemberRead>,
}

/// `const { imported: local, … } = require("spec")` with a string-literal
/// specifier: `(imported, local token)` per plain identifier entry.
struct RequireCall {
    module: String,
    require_span: ByteSpan,
    entries: Vec<(String, ByteSpan)>,
}

/// A top-level `module.exports = { name, name: value }`,
/// `module.exports.name = value` or `exports.name = value`; `free` holds the
/// `module`/`exports` tokens that must be the CommonJS globals.
struct CjsExport {
    name: String,
    value_span: ByteSpan,
    free: Vec<ByteSpan>,
}

struct MemberRead {
    object_span: ByteSpan,
    member: String,
    member_span: ByteSpan,
}

struct DynamicCandidate {
    module: String,
    member: String,
    member_span: ByteSpan,
    object_span: ByteSpan,
    param_span: ByteSpan,
}

fn byte_span(node: tree_sitter::Node) -> ByteSpan {
    ByteSpan {
        start: node.start_byte(),
        end: node.end_byte(),
    }
}

/// `import("spec").then(param => … param.member …)` with a string specifier
/// and exactly one identifier parameter. OXC later checks that each `param`
/// token is bound to that parameter.
fn dynamic_import_candidates(
    node: tree_sitter::Node,
    source: &str,
    out: &mut Vec<DynamicCandidate>,
) {
    let Some(callee) = node
        .child_by_field_name("function")
        .filter(|callee| node.kind() == "call_expression" && callee.kind() == "member_expression")
    else {
        return;
    };
    let (Some(object), Some(property)) = (
        callee.child_by_field_name("object"),
        callee.child_by_field_name("property"),
    ) else {
        return;
    };
    if &source[property.byte_range()] != "then" {
        return;
    }
    let Some(module) = import_call_specifier(object, source) else {
        return;
    };
    let Some(callback) = node
        .child_by_field_name("arguments")
        .and_then(|args| args.named_child(0))
    else {
        return;
    };
    let (Some(param), Some(body)) = (
        single_identifier_parameter(callback),
        callback.child_by_field_name("body"),
    ) else {
        return;
    };
    let name = &source[param.byte_range()];
    let mut nodes = vec![body];
    while let Some(node) = nodes.pop() {
        if node.kind() == "member_expression"
            && let (Some(object), Some(member)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("property"),
            )
            && object.kind() == "identifier"
            && &source[object.byte_range()] == name
            && member.kind() == "property_identifier"
        {
            out.push(DynamicCandidate {
                module: module.clone(),
                member: source[member.byte_range()].to_owned(),
                member_span: byte_span(member),
                object_span: byte_span(object),
                param_span: byte_span(param),
            });
        }
        let mut cursor = node.walk();
        nodes.extend(node.children(&mut cursor));
    }
}

fn import_call_specifier(call: tree_sitter::Node, source: &str) -> Option<String> {
    if call.kind() != "call_expression" || call.child_by_field_name("function")?.kind() != "import"
    {
        return None;
    }
    let args = call.child_by_field_name("arguments")?;
    if args.named_child_count() != 1 {
        return None;
    }
    string_literal(args.named_child(0)?, source)
}

fn string_literal(node: tree_sitter::Node, source: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let quoted = &source[node.byte_range()];
    quoted
        .get(1..quoted.len().checked_sub(1)?)
        .map(str::to_owned)
}

/// `require("spec")` with exactly one string-literal argument: the specifier
/// and the `require` token. Template strings and computed paths are skipped.
fn require_call(call: tree_sitter::Node, source: &str) -> Option<(String, ByteSpan)> {
    let callee = call.child_by_field_name("function")?;
    if call.kind() != "call_expression"
        || callee.kind() != "identifier"
        || &source[callee.byte_range()] != "require"
    {
        return None;
    }
    let args = call.child_by_field_name("arguments")?;
    if args.named_child_count() != 1 {
        return None;
    }
    Some((
        string_literal(args.named_child(0)?, source)?,
        byte_span(callee),
    ))
}

/// `const { X, Y: Z } = require("spec")` adds one entry per shorthand or
/// `key: identifier` property; defaults, rest, nested and computed keys are
/// skipped. `const m = require("spec")` is a namespace.
fn require_declarator(node: tree_sitter::Node, source: &str, cjs: &mut CjsTokens) {
    if node.kind() != "variable_declarator" {
        return;
    }
    let (Some(name), Some((module, require_span))) = (
        node.child_by_field_name("name"),
        node.child_by_field_name("value")
            .and_then(|value| require_call(value, source)),
    ) else {
        return;
    };
    if name.kind() == "identifier" {
        cjs.namespaces.push(RequireCall {
            module,
            require_span,
            entries: vec![("*".to_owned(), byte_span(name))],
        });
        return;
    }
    if name.kind() != "object_pattern" {
        return;
    }
    let mut cursor = name.walk();
    let entries = name
        .named_children(&mut cursor)
        .filter_map(|entry| match entry.kind() {
            "shorthand_property_identifier_pattern" => {
                Some((source[entry.byte_range()].to_owned(), byte_span(entry)))
            }
            "pair_pattern" => {
                let key = entry.child_by_field_name("key")?;
                let value = entry.child_by_field_name("value")?;
                (key.kind() == "property_identifier" && value.kind() == "identifier")
                    .then(|| (source[key.byte_range()].to_owned(), byte_span(value)))
            }
            _ => None,
        })
        .collect();
    cjs.requires.push(RequireCall {
        module,
        require_span,
        entries,
    });
}

/// `module.exports`: the `module` token.
fn module_exports(node: tree_sitter::Node, source: &str) -> Option<ByteSpan> {
    let object = node.child_by_field_name("object")?;
    let property = node.child_by_field_name("property")?;
    (node.kind() == "member_expression"
        && object.kind() == "identifier"
        && &source[object.byte_range()] == "module"
        && &source[property.byte_range()] == "exports")
        .then(|| byte_span(object))
}

/// Named exports from top-level CommonJS assignments. Reassigning
/// `module.exports` replaces the exported object, so an object literal counts
/// only when it is the one such assignment, `module.exports.X` only after the
/// last one, and `exports.X` only when there is none (it detaches `exports`).
/// `module.exports = value` exports no name.
fn commonjs_exports(root: tree_sitter::Node, source: &str) -> Vec<CjsExport> {
    let mut wholes = Vec::new();
    let mut members = Vec::new();
    let mut cursor = root.walk();
    for statement in root.named_children(&mut cursor) {
        let Some(assignment) = statement.named_child(0).filter(|n| {
            statement.kind() == "expression_statement" && n.kind() == "assignment_expression"
        }) else {
            continue;
        };
        let (Some(left), Some(right)) = (
            assignment.child_by_field_name("left"),
            assignment.child_by_field_name("right"),
        ) else {
            continue;
        };
        if let Some(module) = module_exports(left, source) {
            wholes.push((statement.start_byte(), module, right));
            continue;
        }
        let (Some(object), Some(property)) = (
            left.child_by_field_name("object"),
            left.child_by_field_name("property"),
        ) else {
            continue;
        };
        if left.kind() != "member_expression"
            || property.kind() != "property_identifier"
            || right.kind() != "identifier"
        {
            continue;
        }
        let (via_module, free) = match module_exports(object, source) {
            Some(module) => (true, module),
            None if object.kind() == "identifier" && &source[object.byte_range()] == "exports" => {
                (false, byte_span(object))
            }
            None => continue,
        };
        members.push((
            statement.start_byte(),
            via_module,
            CjsExport {
                name: source[property.byte_range()].to_owned(),
                value_span: byte_span(right),
                free: vec![free],
            },
        ));
    }
    let mut out = Vec::new();
    if let [(_, module, object)] = wholes.as_slice()
        && object.kind() == "object"
    {
        let mut cursor = object.walk();
        for entry in object.named_children(&mut cursor) {
            let named = match entry.kind() {
                "shorthand_property_identifier" => Some((entry, entry)),
                "pair" => entry
                    .child_by_field_name("key")
                    .zip(entry.child_by_field_name("value"))
                    .filter(|(key, value)| {
                        key.kind() == "property_identifier" && value.kind() == "identifier"
                    }),
                _ => None,
            };
            if let Some((key, value)) = named {
                out.push(CjsExport {
                    name: source[key.byte_range()].to_owned(),
                    value_span: byte_span(value),
                    free: vec![*module],
                });
            }
        }
    }
    let last_whole = wholes.last().map(|(start, ..)| *start);
    out.extend(
        members
            .into_iter()
            .filter(|(start, via_module, _)| match last_whole {
                None => true,
                Some(whole) => *via_module && *start > whole,
            })
            .map(|(.., export)| export),
    );
    out
}

/// `name.member` reads (not assignment targets) for the given object names.
fn member_reads(
    root: tree_sitter::Node,
    source: &str,
    names: &std::collections::HashSet<&str>,
) -> Vec<MemberRead> {
    let mut out = Vec::new();
    let mut nodes = vec![root];
    while let Some(node) = nodes.pop() {
        if node.kind() == "member_expression"
            && let (Some(object), Some(member)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("property"),
            )
            && object.kind() == "identifier"
            && member.kind() == "property_identifier"
            && names.contains(&source[object.byte_range()])
            && !node.parent().is_some_and(|parent| {
                matches!(
                    parent.kind(),
                    "assignment_expression" | "augmented_assignment_expression"
                ) && parent.child_by_field_name("left") == Some(node)
            })
        {
            out.push(MemberRead {
                object_span: byte_span(object),
                member: source[member.byte_range()].to_owned(),
                member_span: byte_span(member),
            });
        }
        let mut cursor = node.walk();
        nodes.extend(node.children(&mut cursor));
    }
    out.sort_by_key(|read| read.member_span.start);
    out
}

fn single_identifier_parameter(callback: tree_sitter::Node) -> Option<tree_sitter::Node> {
    if !matches!(
        callback.kind(),
        "arrow_function" | "function_expression" | "function"
    ) {
        return None;
    }
    if let Some(param) = callback.child_by_field_name("parameter") {
        return (param.kind() == "identifier").then_some(param);
    }
    let params = callback.child_by_field_name("parameters")?;
    if params.named_child_count() != 1 {
        return None;
    }
    let param = params.named_child(0)?;
    match param.kind() {
        "identifier" => Some(param),
        "required_parameter" => param.child_by_field_name("pattern").filter(|pattern| {
            pattern.kind() == "identifier" && param.child_by_field_name("value").is_none()
        }),
        _ => None,
    }
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
    let mut dynamic = Vec::new();
    let mut cjs = CjsTokens::default();
    let mut nodes = vec![tree.root_node()];
    while let Some(node) = nodes.pop() {
        dynamic_import_candidates(node, source, &mut dynamic);
        require_declarator(node, source, &mut cjs);
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
    dynamic.sort_by_key(|candidate| candidate.member_span.start);
    cjs.exports = commonjs_exports(tree.root_node(), source);
    if !cjs.namespaces.is_empty() {
        let names = cjs
            .namespaces
            .iter()
            .flat_map(|call| &call.entries)
            .map(|(_, span)| &source[span.start..span.end])
            .collect();
        cjs.member_reads = member_reads(tree.root_node(), source, &names);
    }
    Ok(ModuleTokens {
        imports,
        exports,
        members,
        dynamic,
        cjs,
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
