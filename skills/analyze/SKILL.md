---
name: codelens-analyze
description: "Deep architecture analysis — dependencies, coupling, dead code, circular imports"
trigger: "/codelens-analyze"
tools:
  [
    get_symbol_importance,
    find_dead_code,
    find_circular_dependencies,
    get_change_coupling,
    get_blast_radius,
  ]
---

# CodeLens Architecture Analysis

Perform a comprehensive architecture health check on the codebase.

## Workflow

1. **Centrality**: Call `get_symbol_importance` (top_n=30) to identify the most coupled files
2. **Dead code**: Call `find_dead_code` to find unreachable symbols and unused exports
3. **Circular deps**: Call `find_circular_dependencies` to detect import cycles
4. **Change coupling**: Call `get_change_coupling` to find files that always change together (hidden dependencies)
5. **Hot spots**: For the top 5 most important files, call `get_blast_radius` to assess risk

## Usage

```
/codelens-analyze             # Full architecture analysis
```

## Output Format

- Dependency graph summary (most connected nodes)
- Dead code candidates with confidence scores
- Circular dependency cycles (if any)
- Change coupling pairs (co-change frequency)
- Risk hot spots (high blast radius + high centrality)
- Actionable recommendations
