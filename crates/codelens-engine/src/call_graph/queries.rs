pub(super) const PYTHON_FUNC_QUERY: &str = r#"
(function_definition name: (identifier) @func.name) @func.def
"#;

pub(super) const PYTHON_CALL_QUERY: &str = r#"
(call function: (identifier) @callee)
(call function: (attribute attribute: (identifier) @callee))
(decorator (identifier) @callee)
(decorator (call function: (identifier) @callee))
(decorator (attribute attribute: (identifier) @callee))
(decorator (call function: (attribute attribute: (identifier) @callee)))
;; v1.11.1 (F1 follow-up): function-reference arguments. Python
;; callback patterns include `register("evt", handler)`,
;; `dispatcher.on(name, callback)`, `signal.connect(slot)`, plus
;; decorator factories like `@retry(handler)`. The 6-stage
;; resolution cascade filters identifier-arg captures against the
;; project symbol DB; variable arguments fall to `unresolved` and
;; genuine function references resolve via Stage 5 (`unique_name`)
;; at confidence 0.5.
(call arguments: (argument_list (identifier) @callee))
(call arguments: (argument_list (attribute attribute: (identifier) @callee)))
"#;

pub(super) const JS_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
(method_definition name: (property_identifier) @func.name) @func.def
(lexical_declaration
    (variable_declarator
    name: (identifier) @func.name
    value: [(arrow_function) (function_expression)] @func.def))
(variable_declaration
  (variable_declarator
    name: (identifier) @func.name
    value: [(arrow_function) (function_expression)] @func.def))
"#;

pub(super) const JS_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (member_expression property: (property_identifier) @callee))
;; v1.11.1 (F1 follow-up): function-reference arguments. JS/TS frequently
;; pass functions as callbacks — `setTimeout(handler, 100)`,
;; `arr.map(parseLine)`, `bus.on("evt", onEvent)`, `.then(success)`.
;; The 6-stage resolution cascade in `resolve_call_edges` filters these
;; against the symbol DB, so variable arguments fall to `unresolved`
;; while genuine function references resolve via Stage 5
;; (`unique_name`) at confidence 0.5.
(arguments (identifier) @callee)
(arguments (member_expression property: (property_identifier) @callee))
"#;

// JSX/TSX adds React-style component usage (`<Foo />`, `<Foo>`) as caller→callee
// edges. Plain TypeScript (.ts) has no JSX node types — keep this off the JS/TS
// path. tree-sitter-javascript also supports JSX, so .jsx files share this set.
pub(super) const JS_JSX_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (member_expression property: (property_identifier) @callee))
(jsx_self_closing_element name: (identifier) @callee)
(jsx_opening_element name: (identifier) @callee)
(jsx_self_closing_element name: (member_expression property: (property_identifier) @callee))
(jsx_opening_element name: (member_expression property: (property_identifier) @callee))
;; v1.11.1: same function-reference patterns as JS_CALL_QUERY.
(arguments (identifier) @callee)
(arguments (member_expression property: (property_identifier) @callee))
"#;

pub(super) const GO_FUNC_QUERY: &str = r#"
(function_declaration name: (identifier) @func.name) @func.def
(method_declaration name: (field_identifier) @func.name) @func.def
"#;

pub(super) const GO_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (selector_expression field: (field_identifier) @callee))
;; v1.11.2 (F1 follow-up): function-reference arguments in Go.
;; Catches `http.HandleFunc("/", handler)`, `time.AfterFunc(d, callback)`,
;; `runtime.SetFinalizer(p, finalizer)`, and worker-pool dispatch
;; patterns where a function value is passed by name. Same resolution
;; cascade gating: variable arguments fall to `unresolved`, named
;; functions resolve via Stage 5 (`unique_name`) at confidence 0.5.
(argument_list (identifier) @callee)
(argument_list (selector_expression field: (field_identifier) @callee))
"#;

pub(super) const JAVA_FUNC_QUERY: &str = r#"
(method_declaration name: (identifier) @func.name) @func.def
(constructor_declaration name: (identifier) @func.name) @func.def
"#;

pub(super) const JAVA_CALL_QUERY: &str = r#"
(method_invocation name: (identifier) @callee)
(object_creation_expression type: (type_identifier) @callee)
(method_reference (identifier) @callee)
;; v1.11.2 (F1 follow-up): function-reference arguments in Java/Kotlin
;; that are passed as bare identifiers (callbacks, executor.submit
;; targets) rather than the explicit `Class::method` reference syntax
;; already covered above. The same query is shared with Kotlin via
;; the `KOTLIN_FUNC_QUERY` mapping; tree-sitter-kotlin reuses
;; `argument_list` node names for the call grammar so the pattern
;; below applies to Kotlin call sites as well.
(method_invocation arguments: (argument_list (identifier) @callee))
(method_invocation arguments: (argument_list (field_access field: (identifier) @callee)))
"#;

pub(super) const KOTLIN_FUNC_QUERY: &str = r#"
(function_declaration (identifier) @func.name) @func.def
"#;

pub(super) const KOTLIN_CALL_QUERY: &str = r#"
;; Direct call: prepare()
(call_expression (identifier) @callee)

;; Method/navigation call: exec.submit(...) — last identifier in
;; navigation_expression is the method name (anchor `.` selects last child).
(call_expression
  (navigation_expression
    (identifier) @callee .))

;; v1.12.3: function-reference arguments — submit(onTick),
;; register("err", onError). Same noise-filter behavior as Rust:
;; non-function identifiers (variables) are dropped at resolution time.
(call_expression
  (value_arguments
    (value_argument
      (identifier) @callee)))

;; v1.12.4 (Codex P1): Kotlin callable references.
;; - bare form `::onTick` parses as
;;     value_argument > callable_reference > identifier.
;; - qualified form `this::onTick` parses as
;;     value_argument > navigation_expression(`::`) > identifier
;;   (tree-sitter-kotlin-ng folds the `::` token into a
;;   navigation_expression rather than a dedicated callable_reference
;;   node). Both shapes are common in Executor / event-bus callbacks.
(call_expression
  (value_arguments
    (value_argument
      (callable_reference (identifier) @callee))))

(call_expression
  (value_arguments
    (value_argument
      (navigation_expression (identifier) @callee .))))
"#;

pub(super) const RUST_FUNC_QUERY: &str = r#"
(function_item name: (identifier) @func.name) @func.def
"#;

pub(super) const RUST_CALL_QUERY: &str = r#"
(call_expression function: (identifier) @callee)
(call_expression function: (field_expression field: (field_identifier) @callee))
(call_expression function: (scoped_identifier name: (identifier) @callee))
(macro_invocation macro: (identifier) @callee)
(macro_invocation macro: (scoped_identifier name: (identifier) @callee))
;; v1.11.0 (F1): function-reference patterns. A function passed as an
;; argument (closure construction, callback registration, builder
;; accumulators) is a real caller→callee edge that the call_expression
;; rules above miss. Examples:
;;   LazyLock::new(build_tools)
;;   OnceCell::get_or_init(make_state)
;;   iter.map(parse_line).collect()
;;   bus.register("evt", on_event)
;; Many argument identifiers are variables, not functions. The
;; resolution cascade in `resolve_call_edges` filters those: the name
;; must exist in the symbol DB or the edge is dropped as `unresolved`
;; (confidence 0). Genuine function references resolve via Stage 5
;; (unique_name) at confidence 0.5 — honest, lower than import_map but
;; higher than nothing.
(arguments (identifier) @callee)
(arguments (scoped_identifier name: (identifier) @callee))
"#;
