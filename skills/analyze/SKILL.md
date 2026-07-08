---
name: codelens-analyze
description: "Deep architecture analysis — dependencies, coupling, dead code, circular imports"
trigger: "/codelens-analyze"
tools:
  [
    get_symbol_importance,
    dead_code_report,
    module_boundary_report,
    impact_report,
  ]
---

# CodeLens Architecture Analysis

Perform a comprehensive architecture health check on the codebase.

## Workflow

1. **Centrality**: Call `get_symbol_importance` (top_n=30) to identify the most coupled files
2. **Dead code**: Call `dead_code_report` to find unreachable symbols and unused exports
3. **Circular deps**: Call `module_boundary_report` to detect import cycles
4. **Hot spots**: For the top 5 most important files, call `impact_report` to assess risk

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
