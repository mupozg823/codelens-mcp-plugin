---
name: codelens-analyze
description: "Deep architecture analysis — dependencies, coupling, dead code, circular imports"
tools: [prepare_harness_session, review, graph]
---

# CodeLens Architecture Analysis

Perform a comprehensive architecture health check on the codebase.

## Workflow

1. **Bind and summarize**: Reuse an existing confirmed project binding, or call `prepare_harness_session` with the absolute project and `detail=compact`. Use `review` mode=architecture for the structure summary and key files.
2. **Dead code**: Call `review` with mode=dead to find unreachable symbols and unused exports
3. **Circular deps**: Call `review` with mode=boundary to detect import cycles
4. **Hot spots**: For the top 5 most important files, call `graph` with mode=impact to assess risk

Check host tool availability before calling; a host allowlist may narrow the server
surface. Load a deferred tool with native tool search when supported, or use native
evidence and report the gap. `review` mode=dead / mode=boundary and
`graph` mode=impact route to the same `dead_code_report` / `module_boundary_report` /
`impact_report` handlers as before. For finer-grained centrality, `get_symbol_importance`
remains an optional Full-preset follow-up. Discover it through native tool search,
or request the graph namespace only if the host exposes namespace expansion.
Otherwise continue with the available evidence instead of retrying discovery.

## Usage

```
/codelens-analyze             # Full architecture analysis
```

## Output Format

- Dependency graph summary (most connected nodes)
- Dead code candidates with confidence scores
- Circular dependency cycles (if any)
- Risk hot spots (high blast radius + high centrality)
- Actionable recommendations
