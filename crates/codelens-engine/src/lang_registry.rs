//! Single source of truth for supported language extensions.
//!
//! All extension-to-language dispatch tables across the codebase should derive
//! from this registry to prevent mismatches.

use std::path::Path;

/// Metadata for a supported file extension.
#[derive(Debug, Clone, Copy)]
pub struct ExtEntry {
    pub ext: &'static str,
    /// LSP textDocument/didOpen language identifier.
    pub language_id: &'static str,
    /// Whether import graph analysis is supported for this extension.
    pub supports_imports: bool,
    /// Canonical extension used as key for tree-sitter config lookup.
    /// Multiple extensions (e.g. "cc", "cxx") map to the same canonical ("cpp").
    pub canonical: &'static str,
}

/// Canonical table of all supported extensions.
/// Every extension that tree-sitter can parse should appear here.
static EXTENSIONS: &[ExtEntry] = &[
    ExtEntry {
        ext: "py",
        language_id: "python",
        supports_imports: true,
        canonical: "py",
    },
    ExtEntry {
        ext: "js",
        language_id: "javascript",
        supports_imports: true,
        canonical: "js",
    },
    ExtEntry {
        ext: "mjs",
        language_id: "javascript",
        supports_imports: true,
        canonical: "js",
    },
    ExtEntry {
        ext: "cjs",
        language_id: "javascript",
        supports_imports: true,
        canonical: "js",
    },
    ExtEntry {
        ext: "ts",
        language_id: "typescript",
        supports_imports: true,
        canonical: "ts",
    },
    ExtEntry {
        ext: "tsx",
        language_id: "typescriptreact",
        supports_imports: true,
        canonical: "tsx",
    },
    ExtEntry {
        ext: "jsx",
        language_id: "javascriptreact",
        supports_imports: true,
        canonical: "tsx",
    },
    ExtEntry {
        ext: "go",
        language_id: "go",
        supports_imports: true,
        canonical: "go",
    },
    ExtEntry {
        ext: "java",
        language_id: "java",
        supports_imports: true,
        canonical: "java",
    },
    ExtEntry {
        ext: "kt",
        language_id: "kotlin",
        supports_imports: true,
        canonical: "kt",
    },
    ExtEntry {
        ext: "kts",
        language_id: "kotlin",
        supports_imports: true,
        canonical: "kt",
    },
    ExtEntry {
        ext: "rs",
        language_id: "rust",
        supports_imports: true,
        canonical: "rs",
    },
    ExtEntry {
        ext: "c",
        language_id: "c",
        supports_imports: true,
        canonical: "c",
    },
    ExtEntry {
        ext: "h",
        language_id: "c",
        supports_imports: true,
        canonical: "c",
    },
    ExtEntry {
        ext: "cpp",
        language_id: "cpp",
        supports_imports: true,
        canonical: "cpp",
    },
    ExtEntry {
        ext: "cc",
        language_id: "cpp",
        supports_imports: true,
        canonical: "cpp",
    },
    ExtEntry {
        ext: "cxx",
        language_id: "cpp",
        supports_imports: true,
        canonical: "cpp",
    },
    ExtEntry {
        ext: "hpp",
        language_id: "cpp",
        supports_imports: true,
        canonical: "cpp",
    },
    ExtEntry {
        ext: "hh",
        language_id: "cpp",
        supports_imports: true,
        canonical: "cpp",
    },
    ExtEntry {
        ext: "hxx",
        language_id: "cpp",
        supports_imports: true,
        canonical: "cpp",
    },
    ExtEntry {
        ext: "php",
        language_id: "php",
        supports_imports: true,
        canonical: "php",
    },
    ExtEntry {
        ext: "swift",
        language_id: "swift",
        supports_imports: true,
        canonical: "swift",
    },
    ExtEntry {
        ext: "scala",
        language_id: "scala",
        supports_imports: true,
        canonical: "scala",
    },
    ExtEntry {
        ext: "sc",
        language_id: "scala",
        supports_imports: true,
        canonical: "scala",
    },
    ExtEntry {
        ext: "rb",
        language_id: "ruby",
        supports_imports: true,
        canonical: "rb",
    },
    ExtEntry {
        ext: "cs",
        language_id: "csharp",
        supports_imports: true,
        canonical: "cs",
    },
    ExtEntry {
        ext: "dart",
        language_id: "dart",
        supports_imports: true,
        canonical: "dart",
    },
    // --- Phase 6a: new languages ---
    ExtEntry {
        ext: "lua",
        language_id: "lua",
        supports_imports: false,
        canonical: "lua",
    },
    ExtEntry {
        ext: "zig",
        language_id: "zig",
        supports_imports: false,
        canonical: "zig",
    },
    ExtEntry {
        ext: "ex",
        language_id: "elixir",
        supports_imports: false,
        canonical: "ex",
    },
    ExtEntry {
        ext: "exs",
        language_id: "elixir",
        supports_imports: false,
        canonical: "ex",
    },
    ExtEntry {
        ext: "hs",
        language_id: "haskell",
        supports_imports: false,
        canonical: "hs",
    },
    ExtEntry {
        ext: "ml",
        language_id: "ocaml",
        supports_imports: false,
        canonical: "ml",
    },
    ExtEntry {
        ext: "mli",
        language_id: "ocaml",
        supports_imports: false,
        canonical: "ml",
    },
    ExtEntry {
        ext: "erl",
        language_id: "erlang",
        supports_imports: false,
        canonical: "erl",
    },
    ExtEntry {
        ext: "hrl",
        language_id: "erlang",
        supports_imports: false,
        canonical: "erl",
    },
    ExtEntry {
        ext: "r",
        language_id: "r",
        supports_imports: false,
        canonical: "r",
    },
    ExtEntry {
        ext: "R",
        language_id: "r",
        supports_imports: false,
        canonical: "r",
    },
    ExtEntry {
        ext: "sh",
        language_id: "shellscript",
        supports_imports: false,
        canonical: "sh",
    },
    ExtEntry {
        ext: "bash",
        language_id: "shellscript",
        supports_imports: false,
        canonical: "sh",
    },
    ExtEntry {
        ext: "jl",
        language_id: "julia",
        supports_imports: false,
        canonical: "jl",
    },
    // Phase 3 additions
    ExtEntry {
        ext: "css",
        language_id: "css",
        supports_imports: true,
        canonical: "css",
    },
    ExtEntry {
        ext: "html",
        language_id: "html",
        supports_imports: false,
        canonical: "html",
    },
    ExtEntry {
        ext: "htm",
        language_id: "html",
        supports_imports: false,
        canonical: "html",
    },
    ExtEntry {
        ext: "toml",
        language_id: "toml",
        supports_imports: false,
        canonical: "toml",
    },
    ExtEntry {
        ext: "yaml",
        language_id: "yaml",
        supports_imports: false,
        canonical: "yaml",
    },
    ExtEntry {
        ext: "yml",
        language_id: "yaml",
        supports_imports: false,
        canonical: "yaml",
    },
    ExtEntry {
        ext: "clj",
        language_id: "clojure",
        supports_imports: false,
        canonical: "clj",
    },
    ExtEntry {
        ext: "cljs",
        language_id: "clojurescript",
        supports_imports: false,
        canonical: "clj",
    },
    // dockerfile, make, vim, fsharp — deferred: tree-sitter version conflict
    // Perl deferred until tree-sitter 0.26 upgrade
];

/// Look up an extension entry by lowercase extension string.
pub fn for_extension(ext: &str) -> Option<&'static ExtEntry> {
    EXTENSIONS.iter().find(|e| e.ext == ext)
}

/// Whether tree-sitter symbol parsing is supported for this extension.
/// All registered extensions support symbols.
pub fn supports_symbols(ext: &str) -> bool {
    for_extension(ext).is_some()
}

/// Whether import graph analysis is supported for this extension.
pub fn supports_imports(ext: &str) -> bool {
    for_extension(ext).is_some_and(|e| e.supports_imports)
}

/// Whether import graph analysis is supported for a file path.
pub fn supports_imports_for_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| supports_imports(&ext.to_ascii_lowercase()))
}

/// Return the LSP language identifier for an extension.
pub fn language_id(ext: &str) -> Option<&'static str> {
    for_extension(ext).map(|e| e.language_id)
}

/// Return all extensions that support import analysis.
pub fn import_extensions() -> impl Iterator<Item = &'static str> {
    EXTENSIONS
        .iter()
        .filter(|e| e.supports_imports)
        .map(|e| e.ext)
}

/// Return all supported language registry entries.
pub fn all_entries() -> impl Iterator<Item = &'static ExtEntry> {
    EXTENSIONS.iter()
}

/// Return all supported extensions.
pub fn all_extensions() -> impl Iterator<Item = &'static str> {
    EXTENSIONS.iter().map(|e| e.ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_extensions_unique() {
        let mut seen = std::collections::HashSet::new();
        for entry in EXTENSIONS {
            assert!(seen.insert(entry.ext), "duplicate extension: {}", entry.ext);
        }
    }

    #[test]
    fn kts_supports_imports() {
        assert!(
            supports_imports("kts"),
            "kts should support imports (Kotlin scripts)"
        );
    }

    #[test]
    fn swift_scala_support_imports() {
        assert!(supports_imports("swift"));
        assert!(supports_imports("scala"));
        assert!(supports_imports("sc"));
    }

    #[test]
    fn hh_hxx_have_language_id() {
        assert_eq!(language_id("hh"), Some("cpp"));
        assert_eq!(language_id("hxx"), Some("cpp"));
    }

    #[test]
    fn jsx_tsx_distinct_language_ids() {
        assert_eq!(language_id("tsx"), Some("typescriptreact"));
        assert_eq!(language_id("jsx"), Some("javascriptreact"));
    }
}
