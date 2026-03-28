# CodeLens Rust Core

This workspace is the first step toward an editor-agnostic CodeLens runtime.

Current scope:

- pure Rust project-root abstraction
- filesystem-backed read/search tools
- tree-sitter-backed symbol overview and symbol lookup for Python, JavaScript, TypeScript, and TSX
- cached project-wide symbol index with mtime freshness checks
- on-disk symbol cache at `.codelens/index/symbols-v1.json`
- minimal stdio LSP reference lookup path with explicit or inferred server command
- pooled stdio LSP sessions for repeated semantic requests
- pull-based `textDocument/diagnostic` support exposed through MCP
- minimal MCP-compatible stdio server

Not yet implemented:

- richer semantic tools on top of LSP (hierarchy, rename planning, workspace symbols)
- IntelliJ adapter bridge

The intended migration path is:

1. move editor-independent tools into Rust
2. add persistent index + symbol engine
3. add LSP semantic backend
4. keep IntelliJ as an adapter, not the core runtime
