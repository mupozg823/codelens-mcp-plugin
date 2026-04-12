# Hacker News Show HN Draft

## Title

Show HN: CodeLens MCP – 50-87% fewer tokens for AI coding agents (Rust, 89 tools, 25 langs)

## URL

https://github.com/mupozg823/codelens-mcp-plugin

## Comment (first comment by author)

Hi HN! I built CodeLens MCP, a Pure Rust MCP server that acts as a compressed context layer for AI coding agents.

The core insight: multi-agent coding harnesses (Claude Code, Cursor, Codex) waste most of their token budget on repeated file reads and grep cycles. A simple "what calls this function?" costs 4,600 tokens with read+grep. CodeLens answers the same question in 1,500 tokens with a bounded, ranked response.

Key design decisions:

1. **tree-sitter-first** — no LSP server needed. Cold start <12ms. AST-level understanding of 25 languages via statically-linked grammars.

2. **Bounded answers** — every tool returns a compressed response with expansion handles. Agents drill down only when needed, instead of dumping raw file content into context.

3. **Role-based surfaces** — a planner agent sees 55 read-only tools. A builder sees 20 focused tools. A reviewer sees the full 89. Same server, different views.

4. **Mutation gates** — refactor tools require a verification step before executing. Prevents the "blind rewrite" failure mode.

5. **Analysis handles** — heavy reports (impact analysis, architecture audit) run as async jobs. The agent polls for completion and expands individual sections.

Measured token savings on real projects: 50-87% reduction vs Read/Grep baselines (tiktoken cl100k_base).

Tech: Pure Rust, SQLite FTS5 for symbol search, petgraph for dependency analysis, optional fastembed ONNX for semantic search. Single binary, ~23MB without ML model, ~76MB with.

Install: `cargo install codelens-mcp`

Happy to discuss architecture decisions or benchmarks.
