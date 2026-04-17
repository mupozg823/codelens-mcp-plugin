# codelens-engine

Core code intelligence engine for [CodeLens MCP](https://github.com/mupozg823/codelens-mcp-plugin).

Pure Rust library that treats a source repository as an indexed, queryable
dataset. Parses 25 languages with tree-sitter, builds import/call/type
graphs, and optionally runs a local embedding index for semantic search —
all behind synchronous library APIs that other crates consume directly.

## Where it fits

```text
┌────────────────────────────┐
│   codelens-mcp (binary)    │  ← MCP/JSON-RPC tool surface, mutation gates,
│   tool surface + transport │    analysis jobs, coordination, profiles
└──────────────┬─────────────┘
               ▼
┌────────────────────────────┐
│     codelens-engine        │  ← YOU ARE HERE. Pure library. No I/O policy,
│     analysis + storage     │    no MCP protocol, no transport.
└──────────────┬─────────────┘
               ▼
      tree-sitter · SQLite/FTS5 · sqlite-vec · ONNX runtime · optional LSP
```

`codelens-engine` is the data plane. `codelens-mcp` is the control plane.
Keeping them separate lets harness/editor/TUI consumers import the engine
without pulling in the MCP server, and lets the server evolve its tool
contract without disturbing the analysis core.

## What it provides

- **Symbol extraction** — AST-based functions, classes, types, constants,
  parameters. Captures `name_path` for qualified lookups (e.g.
  `Module::Class/method`) and lines/columns for precise navigation.
- **Import graph** — cross-file dependency tracking with blast-radius
  queries (`what breaks if I delete this file?`), dead code detection,
  and Tarjan SCC cycle detection.
- **Call graph** — static caller/callee relationships per language.
- **Scope analysis** — block-level resolution for rename safety and
  unused/shadowed identifier checks.
- **Type hierarchy** — inheritance and trait implementation chains.
- **Hybrid retrieval** — ranked context combining exact lexical hits,
  SQLite FTS5 results, and (opt-in) semantic cosine similarity.
- **Semantic index** — opt-in ONNX-runtime embedding store backed by
  `sqlite-vec`. Ships a bundled CodeSearchNet INT8 model by default.
- **LSP bridge** — opt-in type-aware references/diagnostics when an LSP
  server is installed, falls back to tree-sitter transparently.
- **Safe mutation primitives** — rename, replace-symbol-body, extract/
  inline/move refactors with dry-run preview.

## Storage layout

Per-project state lives entirely under `.codelens/` at the project root:

```text
.codelens/
├── symbols.db            SQLite (+FTS5): symbols, references, imports, scope
├── vec.db                sqlite-vec index (opt-in, feature = "semantic")
├── graph.cache           pre-computed import/call graph
├── bridges.json          optional NL→code query bridges
├── memories/             operator notes, one file per topic
└── audit/                mutation and coordination audit log
```

Nothing here needs a daemon; the engine opens, queries, and closes the
SQLite handles synchronously per request. The vector store uses the
same SQLite connection via the sqlite-vec extension loaded at start.

## Languages

Python, JavaScript, TypeScript, TSX, Go, Java, Kotlin, Rust, C, C++,
PHP, Swift, Scala, Ruby, C#, Dart, Lua, Zig, Elixir, Haskell, OCaml,
Erlang, R, Bash, Julia — plus Clojure/ClojureScript and HTML/CSS/TOML/
YAML config grammars.

## Usage

```toml
[dependencies]
codelens-engine = "1.9"

# With semantic search (adds ~70 MB dependency footprint: fastembed + ort):
codelens-engine = { version = "1.9", features = ["semantic"] }
```

```rust
use codelens_engine::{ProjectRoot, SymbolIndex};

let project = ProjectRoot::discover(".")?;
let index = SymbolIndex::build(&project)?;
for sym in index.symbols_in_file("src/main.rs") {
    println!("{} {}@{}:{}", sym.kind, sym.name, sym.file, sym.line);
}
```

## Feature flags

| Feature         | Default | Adds                                                       |
| --------------- | ------- | ---------------------------------------------------------- |
| `semantic`      | yes     | ONNX runtime + fastembed + sqlite-vec for embedding search |
| `scip-backend`  | no      | SCIP import + precise navigation overlay (enterprise)      |
| `model-bakeoff` | no      | Alternative embedding model harness (benchmark-only)       |

## Design posture

- **Tree-sitter first.** Every core flow works without LSP. LSP is
  opt-in for type-awareness.
- **Bounded outputs.** Every query has a size cap; the library never
  returns unbounded file contents.
- **Incremental indexing.** The file watcher re-parses only changed
  files; full rebuild is a last resort.
- **No surprise I/O.** The engine touches only `.codelens/` and files
  under the project root. No network calls by default.

## License

Apache-2.0. See [LICENSE](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/LICENSE).
