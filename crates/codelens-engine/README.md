# codelens-engine

Core code intelligence engine for [CodeLens MCP](https://github.com/mupozg823/codelens-mcp-plugin).

Pure Rust library providing tree-sitter-based symbol extraction, import graph analysis, and optional embedding-based semantic search across 25 programming languages.

## Features

- **Symbol extraction** — AST-based parsing for functions, classes, types, variables
- **Import graph** — cross-file dependency tracking with cycle detection
- **Scope analysis** — block-level scope resolution for rename safety
- **Call graph** — static call relationship extraction
- **Type hierarchy** — inheritance/implementation chain analysis
- **LSP integration** — optional Language Server Protocol bridge
- **Semantic search** — embedding-based code search (feature-gated: `semantic`)

## Language Support

Rust, TypeScript, JavaScript, Python, Go, Java, Kotlin, C, C++, PHP, Swift, Scala, Ruby, C#, Dart, Lua, Zig, Elixir, Haskell, OCaml, Erlang, R, Bash, Julia, Clojure

## Usage

```toml
[dependencies]
codelens-engine = "1.7"

# With semantic search:
codelens-engine = { version = "1.7", features = ["semantic"] }
```

```rust
use codelens_engine::{ProjectRoot, SymbolIndex};

let project = ProjectRoot::discover(".").unwrap();
let index = SymbolIndex::build(&project).unwrap();
let symbols = index.symbols_in_file("src/main.rs");
```

## Feature Flags

| Feature    | Default | Description                                       |
| ---------- | ------- | ------------------------------------------------- |
| `semantic` | yes     | Embedding-based search via fastembed + sqlite-vec |

## License

Apache-2.0
