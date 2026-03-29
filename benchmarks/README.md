# CodeLens MCP Benchmarks

Measures end-to-end latency for the most performance-sensitive tool calls
using the `--cmd` one-shot CLI mode.

## What is measured

| Metric                            | Description                                           |
| --------------------------------- | ----------------------------------------------------- |
| Cold start + `get_current_config` | Binary startup time with a clean index                |
| `refresh_symbol_index`            | Full tree-sitter parse + SQLite write for the project |
| `get_symbols_overview`            | Read symbols from index for a given path              |
| `find_symbol`                     | Lookup a single symbol by name                        |
| `get_impact_analysis`             | Import-graph traversal from a source file             |

Each metric is run **3 times**; min / avg / max (ms) are reported.

The index cache (`.codelens/index/`) is wiped before every cold-start run so
that result reflects true startup + initialization time, not a warm cache.

## How to run

```bash
# From repo root — uses the current directory as the project under test
./benchmarks/bench.sh

# Against a different project
./benchmarks/bench.sh /path/to/project

# With a custom binary
./benchmarks/bench.sh /path/to/project ./target/debug/codelens-mcp
```

## Requirements

- Rust toolchain (for `cargo build --release`)
- `python3` in PATH (used for millisecond-precision timing; falls back to
  `gdate`/`date` if unavailable)
- The binary must support `--cmd` one-shot mode

## Sample output

```
=== CodeLens MCP Benchmark ===
Project : /Users/you/my-project
Binary  : ./target/release/codelens-mcp

[1/2] Building release binary...
Build OK

[2/2] Running benchmarks (3 runs each)...

  Metric                                        Min       Avg       Max
  ------------------------------------------  --------  --------  --------
  Cold start + get_current_config                42 ms     45 ms     49 ms
  Symbol indexing (refresh_symbol_index)        310 ms    318 ms    330 ms
  get_symbols_overview path=src                  28 ms     30 ms     33 ms
  find_symbol name=main                          15 ms     16 ms     17 ms
  get_impact_analysis src/main.rs                22 ms     24 ms     27 ms

  ------------------------------------------  --------  --------  --------

Binary info
  Size       : 32.1 MB
  Tool count : 49

Done.
```
