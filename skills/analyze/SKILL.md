---
name: codelens-analyze
description: "Deep architecture analysis — dependencies, coupling, dead code, circular imports"
trigger: "/codelens-analyze"
tools: [review, graph]
---

# CodeLens Architecture Analysis

Perform a comprehensive architecture health check on the codebase.

## Workflow

1. **Centrality**: Call `review` with mode=architecture for the structure summary and the PageRank key files (the most coupled ones)
2. **Dead code**: Call `review` with mode=dead to find unreachable symbols and unused exports
3. **Circular deps**: Call `review` with mode=boundary to detect import cycles
4. **Hot spots**: For the top 5 most important files, call `graph` with mode=impact to assess risk

Every step stays on the CORE-20 default surface (ADR-0016), so each one is callable
as a session's first tool call on any host. `review` mode=dead / mode=boundary and
`graph` mode=impact route to the same `dead_code_report` / `module_boundary_report` /
`impact_report` handlers as before. For finer-grained centrality, `get_symbol_importance`
remains a Full-preset follow-up: expand it with `tools/list {"namespace":"graph"}` first.

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
