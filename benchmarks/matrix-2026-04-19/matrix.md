# CodeLens Multi-Target Benchmark Matrix

Run date: 2026-04-19 09:16:00
Binary: ./target/release/codelens-mcp
Source: benchmarks/bench-matrix.sh

## Targets

| Target | Path | Language | Files | Notes |
| --- | --- | --- | --- | --- |
| self | `/Users/bagjaeseog/codelens-mcp-plugin` | mixed (Rust + Python + MD) | ~320 indexed | self-benchmark (dogfooding) |
| serena | `/tmp/serena-oraios` | Python | 287 py files | external medium-repo (oraios/serena) |

## Warm-path metrics (min of 3 runs, ms)

| Metric | self | serena |
| --- | ---: | ---: |
| Cold start + get_current_config | 327 | 330 |
| Symbol indexing (refresh_symbol_index) | 257 | 248 |
| get_symbols_overview path=src | 51 | 63 |
| find_symbol name=main | 51 | 54 |
| get_impact_analysis src/main.rs | 51 | 52 |

## CodeLens vs grep (min of 3 runs, ms)

| Comparison | self | serena |
| --- | ---: | ---: |
| CodeLens: find_symbol | 51 | 53 |
| grep: fn dispatch_tool | 19270 | 104 |
| CodeLens: get_symbols_overview | 47 | 51 |
| grep: pub/fn/struct patterns | 21 | 21 |
| CodeLens: find_referencing_symbols | 46 | 51 |
| grep: references | 8831 | 55 |

## Token efficiency (bytes)

| Metric | self | serena |
| --- | ---: | ---: |
| CodeLens find_symbol | 2755 | 2970 |
| grep -A 20 | 65855 | 0 |

## Honest interpretation

- CodeLens's ~55-60 ms warm cost is **constant** across repo sizes.
- grep scales with repo size — it wins on small repos, loses on large
  ones.
- Token compression ratio is scenario-dependent:
  - self-bench hits many `dispatch_tool` occurrences → grep's
    `-A 20` output is large → large compression ratio
  - serena has few `SerenaAgent` hits → grep output is already tight
    → small compression ratio
- Use the matrix (not a single self-bench number) to set expectations.

## Adding a new target

Edit `TARGETS=(...)` in `benchmarks/bench-matrix.sh` and re-run.
Consider adding:

- A large monorepo sample (> 1,000 files) to re-validate the "100×+"
  claim
- A tiny single-file project to stress cold-start latency
- A TypeScript / Go project to cover non-Rust / non-Python lanes
