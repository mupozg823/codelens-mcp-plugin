---
name: codelens-review
description: "Analyze code changes for impact, quality, and safety using CodeLens MCP tools"
trigger: "/codelens-review"
tools:
  [
    get_changed_files,
    get_blast_radius,
    find_referencing_symbols,
    get_file_diagnostics,
    get_symbols_overview,
  ]
---

# CodeLens Code Review

Analyze the impact and safety of code changes using structural analysis.

## Workflow

1. **Identify changes**: Call `get_changed_files` with the target ref (default: HEAD~1)
2. **Assess impact**: For each changed file, call `get_blast_radius` to find affected downstream files
3. **Check references**: For modified symbols, call `find_referencing_symbols` to find callers that may break
4. **Run diagnostics**: Call `get_file_diagnostics` on changed files to detect type errors or warnings
5. **Summarize**: Report the blast radius, breaking changes risk, and diagnostic issues

## Usage

```
/codelens-review              # Review HEAD~1 changes
/codelens-review main         # Review changes vs main branch
```

## Output Format

For each changed file, report:

- File path and change status (M/A/D)
- Symbol count and types affected
- Blast radius (number of downstream files)
- Diagnostic issues (errors/warnings)
- Risk assessment (low/medium/high)
