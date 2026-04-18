# CodeLens MCP Benchmark — v1.9.46

Run date: 2026-04-19
Host: darwin (macOS)
Project: codelens-mcp-plugin (self-benchmark)
Binary: `target/release/codelens-mcp` (74.0 MB)
Runs per metric: 3 (reporting min / avg / max)

## Cold start + core operations

| Metric                                   |    Min |    Avg |     Max |
| ---------------------------------------- | -----: | -----: | ------: |
| Cold start + `get_current_config`        | 328 ms | 842 ms | 1837 ms |
| Symbol indexing (`refresh_symbol_index`) | 263 ms | 269 ms |  276 ms |
| `get_symbols_overview` (path=src)        |  52 ms |  54 ms |   55 ms |
| `find_symbol` (name=main)                |  51 ms |  52 ms |   54 ms |
| `get_impact_analysis` (src/main.rs)      |  51 ms |  51 ms |   52 ms |

Cold start variance reflects file-cache state; warm runs are
sub-100 ms for every primitive operation.

## CodeLens vs `grep` comparison

| Operation                          |  CodeLens |    `grep` |                                                          Speedup |
| ---------------------------------- | --------: | --------: | ---------------------------------------------------------------: |
| Find symbol `dispatch_tool`        | **53 ms** | 30,155 ms |                                                  **568×** faster |
| Find references to `dispatch_tool` | **54 ms** | 21,836 ms |                                                  **405×** faster |
| Symbols overview (structural)      |     77 ms |     22 ms | grep wins on this narrow case — CodeLens returns richer AST data |

## Token efficiency

| Tool                   |   Output size | Token estimate |
| ---------------------- | ------------: | -------------: |
| CodeLens `find_symbol` |   2,755 bytes |    ~688 tokens |
| `grep -A 20`           | 186,463 bytes | ~46,615 tokens |

**CodeLens is 67.8× more token-efficient** for the same semantic lookup.

## Interpretation

- Symbol-aware retrieval (`find_symbol`, `find_referencing_symbols`)
  dominates naive grep by 2-3 orders of magnitude on real repositories.
- Symbol overview is the one case where grep can beat CodeLens —
  but CodeLens trades that latency for AST-accurate, private-inclusive
  structure that grep cannot produce.
- Token efficiency is the more impactful axis for agent harnesses —
  67.8× compression keeps context windows sustainable across long
  sessions.

## Session context (for correlation with measurement memory)

- Commit under test: `e80e04e` (post Phase 1 landing, 17 commits ahead
  of previous `origin/main`).
- Test suite: 437/437 green on `cargo test -p codelens-mcp --features http`.
- Architecture: `onboard_project.has_cycles=false`, all scaffold modules
  (`backend.rs`, `registry.rs`) wired and exercised by 25 new tests.

Raw terminal capture lives alongside this file at
`benchmarks/bench-v1.9.46-result.txt`.
