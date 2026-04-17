# codelens-tui

Terminal dashboard + operator health CLI for
[CodeLens MCP](https://github.com/mupozg823/codelens-mcp-plugin).

`codelens-tui` reads the same `.codelens/` state that the MCP server
writes, so you can inspect a project's index, symbols, import graph,
and watcher/queue health without attaching an MCP client. Useful as:

- a quick-look TUI during development,
- a non-interactive `--check` for local scripts and CI,
- an operator debug tool when the MCP server behaves unexpectedly.

## Install

```bash
cargo install codelens-tui
```

## Modes

| Mode            | Command                         | Output                 |
| --------------- | ------------------------------- | ---------------------- |
| Interactive TUI | `codelens-tui [PATH]`           | 4-panel ratatui view   |
| Health snapshot | `codelens-tui --check`          | Human-readable summary |
| Health snapshot | `codelens-tui --check --json`   | Machine-readable JSON  |
| CI gate         | `codelens-tui --check --strict` | Exit 1 on degradation  |

The health snapshot covers watcher failure health, index freshness, LSP
recipe reachability, and embedding model availability. Good signal for
a nightly cron that wants to catch index rot before the next session.

## Interactive panels

1. **File tree** — directory view scoped to the project root, with
   symbol counts per directory sourced from the symbol index.
2. **Symbol list** — symbols in the currently selected file, sortable
   by line or name.
3. **Import graph** — upstream/downstream neighbours of the selected
   symbol, matching the MCP `impact_report` evidence.
4. **Metrics** — tool invocation metrics (if `.codelens/telemetry/tool_usage.jsonl`
   has been written by the MCP server), watcher freshness, analysis
   job queue depth.

## Keys

| Key | Action        |
| --- | ------------- |
| Tab | Switch panel  |
| ↑↓  | Navigate      |
| /   | File search   |
| s   | Symbol search |
| q   | Quit          |

## Where it fits

```text
codelens-mcp (MCP server) ──writes──▶ .codelens/ ◀──reads── codelens-tui
                                           ▲
                                           └── codelens-engine (library)
```

This crate shares `codelens-engine` for parsing and symbol reads, but it
does not embed the MCP server. You can run the TUI against a project
the server has never seen — it will build the index itself on first
launch — or against a project the daemon already indexed, in which case
it just attaches to the existing SQLite files read-only.

## License

Apache-2.0. See [LICENSE](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/LICENSE).
