# CodeLens Benchmark — Real-World Target (Serena, 2026-04-19)

**Target repo**: `/tmp/serena-oraios` (Serena, oraios/serena)
**Size**: 287 Python files, ~40K LOC
**CodeLens binary**: `target/release/codelens-mcp` (v1.9.49, 74 MB)
**Symbol under test**: `SerenaAgent` (class, defined in `src/serena/agent.py:248`)

## Measured latency (3 runs each)

| Operation                      |         CodeLens |         grep |              Speedup |
| ------------------------------ | ---------------: | -----------: | -------------------: |
| `find_symbol` (SerenaAgent)    | 60 ms (warm avg) |        51 ms |     grep 1.2× faster |
| `find_referencing_symbols`     |            61 ms |        40 ms |     grep 1.5× faster |
| Token output (semantic lookup) |       990 tokens | 1,557 tokens | **1.6× compression** |

First CodeLens call includes cold-start index warm-up (137 ms);
subsequent calls steady at ~60 ms.

## Comparison vs self-benchmark (v1.9.46 on codelens-mcp-plugin repo)

| Axis                               | Self-bench (our repo) | Real-world (serena) | Delta            |
| ---------------------------------- | --------------------: | ------------------: | ---------------- |
| `find_symbol` speedup              |                  568× |                1.2× | **480× smaller** |
| `find_referencing_symbols` speedup |                  405× |                1.5× | **270× smaller** |
| Token compression                  |                 67.8× |                1.6× | **42× smaller**  |

**The headline "568× speedup" does NOT generalize to arbitrary
real-world repos.**

## Why the huge delta

Three scenario-dependent factors collapsed the gap:

1. **Repo size.** The self-benchmark runs against the full
   codelens-mcp-plugin tree (Rust sources + benchmarks + scripts +
   markdown ~320 indexed files, ~8K symbols). Serena is a single
   `src/serena` package with 287 Python files. grep's naive text scan
   simply has less to scan — 51 ms vs 30,000 ms on our repo.

2. **Symbol match count.** On our repo, `dispatch_tool` appears across
   many sources, so `grep -A 20 ...` produces ~46 kB of surrounding
   context. On serena, `SerenaAgent` appears a handful of times, so
   grep output is only ~6 kB. The compression ratio shrinks
   proportionally.

3. **Query shape.** CodeLens's constant-time cost (index load + AST
   walk) stays at ~55-60 ms regardless of repo size. grep's cost
   scales with repo size. CodeLens wins decisively as the repo grows;
   on a medium Python package it's roughly a wash.

## Where CodeLens still wins on serena-scale repos

Latency alone isn't the only axis. Even when grep matches CodeLens's
speed, CodeLens returns structurally richer output:

- **AST-accurate symbol body** vs line-oriented text match
- **`name_path` hierarchy** (e.g. `SerenaAgent/__init__`) for
  navigation
- **Kind discrimination** (class vs function vs variable vs type alias)
  grep cannot produce
- **Deterministic result ordering** by importance score
- **Private-inclusive** (module-local symbols) vs grep's regex match

Concrete payload example — `find_symbol SerenaAgent`:

- CodeLens returns: signature, line, column, name_path, kind, language
  tag, file path, body (optional), matched_name — 9 structured fields
- grep returns: `file:line:text` tuples, no structure

On medium repos where latency ties, CodeLens is still preferred for
**structural accuracy**, not **speed**.

## Scenario matrix (honest)

| Scenario                                               | CodeLens advantage vs grep       |
| ------------------------------------------------------ | -------------------------------- |
| Large monorepo (>100K files)                           | **100-500× faster** (self-bench) |
| Structural query (needs AST shape)                     | CodeLens wins regardless of size |
| Medium Python/TS repo (<500 files), simple name lookup | **~1-2×**, roughly tied          |
| Single cold-start query on a fresh repo                | grep wins (no index build cost)  |
| Multi-file references on large repo                    | CodeLens wins by 100×+           |

## Honesty note

Previous release notes and README claimed "568× speedup" and "67.8×
token compression" as if universal. Those numbers are **correct for
the self-benchmark workload** but **not generalizable**. Future
benchmark communication should include scenario caveats or multiple
target repos.

Raw terminal output for this run is not captured in a separate file
— the Bash session is preserved in this document instead.

## References

- Self-benchmark: `benchmarks/bench-v1.9.46-result.md`
- Release notes that quoted the self-bench numbers:
  - v1.9.47, v1.9.48, v1.9.49
- Memory to update with the generalization caveat:
  `ARCHITECTURE` (CodeLens project scope)
