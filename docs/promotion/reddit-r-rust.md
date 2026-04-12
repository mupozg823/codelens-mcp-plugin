# Reddit r/rust Post Draft

## Title

Show r/rust: CodeLens MCP — Pure Rust MCP server that cuts AI agent token costs by 50-87%

## Body

Hi r/rust! I've been working on **CodeLens MCP**, a Pure Rust MCP (Model Context Protocol) server that provides compressed code intelligence for AI coding agents like Claude Code, Cursor, and Codex.

### The problem

Multi-agent coding harnesses burn tokens on repeated file reads, grep cycles, and raw graph expansion. A simple "what breaks if I change X?" costs 4,600 tokens with Read+Grep.

### The solution

CodeLens maintains a live, indexed understanding of your codebase via tree-sitter and exposes bounded, ranked answers:

```
Without CodeLens                           With CodeLens
Read file + grep references → 4,600 tok   get_impact_analysis → 1,500 tok (67% saved)
Read manifest + entry + files → 5,000 tok  onboard_project     →   660 tok (87% saved)
```

### Features

- **89+ tools**, 25 languages (all via statically-linked tree-sitter grammars)
- **Role-based tool surfaces** — each agent role (planner/builder/reviewer/refactor) sees a different tool set
- **Analysis handles** — heavy reports run as async jobs, agents expand only needed sections
- **Mutation gates** — verification required before code changes
- **<12ms cold start**, no LSP boot needed
- **Single binary**, zero runtime dependencies

### Install

```bash
cargo install codelens-mcp
```

### Links

- GitHub: https://github.com/mupozg823/codelens-mcp-plugin
- crates.io: https://crates.io/crates/codelens-mcp
- Engine library: https://crates.io/crates/codelens-engine

The engine crate (`codelens-engine`) is also published separately if you want tree-sitter-based symbol extraction + import graph analysis as a library.

Happy to answer any questions about the architecture or benchmarks!
