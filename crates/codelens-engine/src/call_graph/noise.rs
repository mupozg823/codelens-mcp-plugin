/// Resolve call graph config via the unified language registry.
/// Only a subset of languages have call graph queries defined.
/// Filter out common std/builtin method calls that add noise to the call graph.
/// Covers Rust std, Python builtins, JS/TS builtins, Go builtins, and Java/Kotlin stdlib.
pub fn is_noise_callee(name: &str) -> bool {
    matches!(
        name,
        // ── cross-language common ──
        "get" | "set" | "push" | "pop" | "len" | "from" | "into"
            | "map" | "filter" | "collect" | "contains" | "insert" | "remove"
            | "format" | "print" | "clone" | "default" | "next" | "read"
            | "write" | "open" | "close" | "keys" | "values" | "sort"
            | "reverse" | "find" | "replace" | "delete" | "add" | "clear"
            | "of" | "size" | "copy"
            // ── Rust std ──
            | "is_empty" | "to_string" | "to_owned" | "as_str" | "as_ref"
            | "unwrap" | "expect" | "ok" | "err" | "and_then" | "or_else"
            | "unwrap_or" | "unwrap_or_else" | "unwrap_or_default"
            | "iter" | "into_iter" | "take" | "skip"
            | "println" | "eprintln" | "drop" | "enter" | "lock" | "cloned"
            // ── Python builtins ──
            | "range" | "enumerate" | "zip" | "sorted" | "reversed"
            | "isinstance" | "issubclass" | "hasattr" | "getattr" | "setattr" | "delattr"
            | "type" | "super" | "str" | "int" | "float" | "bool"
            | "list" | "dict" | "tuple" | "frozenset" | "bytes" | "bytearray"
            | "repr" | "abs" | "min" | "max" | "sum" | "any" | "all"
            | "ord" | "chr" | "hex" | "oct" | "bin" | "hash" | "id"
            | "input" | "vars" | "dir" | "help" | "round"
            | "append" | "extend" | "update" | "items" | "join" | "split"
            | "strip" | "startswith" | "endswith" | "encode" | "decode"
            | "upper" | "lower"
            // ── JS/TS builtins ──
            | "log" | "warn" | "error" | "info" | "debug"
            | "toString" | "valueOf" | "JSON" | "parse" | "stringify" | "assign"
            | "entries" | "forEach" | "reduce" | "findIndex" | "some" | "every"
            | "includes" | "indexOf" | "slice" | "splice" | "concat"
            | "flat" | "flatMap" | "fill" | "isArray"
            | "Promise" | "resolve" | "reject" | "then" | "catch" | "finally"
            | "setTimeout" | "setInterval" | "clearTimeout" | "clearInterval"
            | "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "require"
            // ── Go builtins ──
            | "make" | "cap" | "panic" | "recover" | "real" | "imag" | "complex"
            | "Println" | "Printf" | "Sprintf" | "Fprintf" | "Errorf" | "New"
            // ── Java/Kotlin stdlib ──
            | "equals" | "hashCode" | "compareTo" | "getClass"
            | "notify" | "notifyAll" | "wait" | "isEmpty"
            | "addAll" | "containsKey" | "containsValue" | "put" | "putAll"
            | "entrySet" | "keySet" | "charAt" | "substring" | "trim"
            | "length" | "toArray" | "stream" | "asList"
    )
}

/// Language-aware noise filter. Rust `new` is a constructor, not noise.
pub(crate) fn is_noise_callee_for_lang(name: &str, lang: Option<&str>) -> bool {
    if lang == Some("rs") && name == "new" {
        return false;
    }
    is_noise_callee(name)
}
