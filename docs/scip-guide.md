# SCIP Precise Navigation Guide

CodeLens supports [SCIP](https://sourcegraph.com/docs/code-intelligence/scip) (Source Code Intelligence Protocol) index files for type-aware definitions, references, hover docs, and diagnostics.

## Why SCIP?

tree-sitter provides fast, zero-config symbol analysis but operates on syntax alone. SCIP adds **type-aware precision**:

| Capability       | tree-sitter                  | SCIP                            |
| ---------------- | ---------------------------- | ------------------------------- |
| Find definitions | Name-based, same file bias   | Type-resolved, cross-package    |
| Find references  | Text grep + scope heuristics | All usages including re-exports |
| Hover docs       | None                         | Full documentation              |
| Diagnostics      | None                         | Compiler-level diagnostics      |

## Setup

### 1. Build with SCIP support

```bash
cargo build --release --features scip-backend
```

### 2. Generate a SCIP index for your project

Use the appropriate SCIP indexer for your language:

```bash
# Rust
cargo install scip-cli
scip-rust            # produces index.scip

# Go
go install github.com/sourcegraph/scip-go/cmd/scip-go@latest
scip-go              # produces index.scip

# TypeScript / JavaScript
npm install -g @anthropic-ai/scip-typescript
scip-typescript index # produces index.scip

# Java
# Use scip-java from Sourcegraph
scip-java index      # produces index.scip

# Python
pip install scip-python
scip-python index    # produces index.scip
```

### 3. Place the index file

CodeLens auto-detects SCIP index files in these locations (checked in order):

1. `index.scip` (project root)
2. `.scip/index.scip`
3. `.codelens/index.scip`

### 4. Verify

```bash
codelens-mcp . --cmd get_capabilities --args '{}'
```

Look for `"scip"` in the `intelligence_sources` array.

## How It Works

When SCIP is available, CodeLens uses it as a **precision layer** before falling back to tree-sitter:

```
find_referencing_symbols("MyStruct", "src/main.rs")
  1. Try oxc_semantic (JS/TS only)
  2. Try SCIP backend (if scip-backend feature + index exists)
  3. Fall back to tree-sitter text search
```

The SCIP backend returns results with `backend: "scip"` and confidence 0.98 (vs 0.85 for tree-sitter).

## Keeping the Index Fresh

SCIP indexes are static snapshots. After significant code changes:

```bash
# Re-run the indexer
scip-rust  # or scip-go, scip-typescript, etc.
```

CodeLens does NOT auto-update the SCIP index. For CI/CD workflows, generate the index as a build step.

## Limitations

- SCIP index must be pre-generated (no on-demand indexing)
- Index file can be large for big projects (10-100MB+)
- Not all languages have mature SCIP indexers
- Hover docs depend on the indexer including documentation in the output
