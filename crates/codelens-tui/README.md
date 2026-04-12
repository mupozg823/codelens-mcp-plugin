# codelens-tui

Terminal dashboard for [CodeLens MCP](https://github.com/mupozg823/codelens-mcp-plugin).

4-panel ratatui-based code intelligence viewer: file tree, symbol list, import graph, and metrics.

## Install

```bash
cargo install codelens-tui
```

## Usage

```bash
# Launch dashboard for current directory
codelens-tui

# Launch for specific project
codelens-tui /path/to/project

# Non-interactive health check
codelens-tui --check

# Machine-readable operator health report
codelens-tui --check --json

# Fail CI if the local index is degraded
codelens-tui --check --strict
```

## Keys

| Key | Action        |
| --- | ------------- |
| Tab | Switch panel  |
| ↑↓  | Navigate      |
| /   | File search   |
| s   | Symbol search |
| q   | Quit          |

## License

Apache-2.0
