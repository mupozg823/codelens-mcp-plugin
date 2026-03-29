---
name: codelens-onboard
description: "Quick project onboarding — understand structure, key symbols, and architecture"
trigger: "/codelens-onboard"
tools:
  [
    activate_project,
    get_current_config,
    get_symbols_overview,
    get_ranked_context,
  ]
---

# CodeLens Project Onboarding

Rapidly understand a codebase's structure, key components, and architecture.

## Workflow

1. **Activate**: Call `activate_project` to initialize the index
2. **Overview**: Call `get_current_config` to see project stats, frameworks, workspace packages
3. **Structure**: Call `get_symbols_overview` on the project root (depth=1) to see top-level modules
4. **Key symbols**: Call `get_ranked_context` with query="main entry" to find the most important entry points
5. **Summarize**: Present the project architecture, key files, and entry points

## Usage

```
/codelens-onboard             # Onboard current project
```

## Output Format

- Project type and framework detection
- File/symbol count and language breakdown
- Top 10 most important symbols (by centrality)
- Entry points and main modules
- Suggested next exploration areas
