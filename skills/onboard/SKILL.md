---
name: codelens-onboard
description: "Quick project onboarding — understand structure, key symbols, and architecture"
tools:
  [prepare_harness_session, onboard_project, overview, get_ranked_context]
---

# CodeLens Project Onboarding

Rapidly understand a codebase's structure, key components, and architecture.

## Workflow

1. **Bind**: Reuse an existing confirmed project binding, or call `prepare_harness_session` with the absolute project and `detail=compact`.
2. **Onboard**: Call `onboard_project` to get structure, key files (PageRank), and circular deps
3. **Drill down**: Call `overview` with mode=file on key files to see their structure
4. **Key symbols**: Call `get_ranked_context` with query="main entry" to find the most important entry points
5. **Summarize**: Present the project architecture, key files, and entry points

Use only tools available to the host, loading deferred tools through native tool
search when supported. If `onboard_project` is unavailable, use a bounded `overview`
or native manifest/file reads; do not repeatedly bootstrap to discover it.
Drill down only where the first result leaves a concrete question unanswered.

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
