---
name: codelens-review
description: "Analyze code changes for impact, quality, and safety using CodeLens MCP tools"
tools:
  [
    prepare_harness_session,
    get_changed_files,
    graph,
    find_referencing_symbols,
    get_file_diagnostics,
    overview,
  ]
---

# CodeLens Code Review

Analyze the impact and safety of code changes using structural analysis.

## Workflow

1. **Bind and identify changes**: Reuse a confirmed project binding, or call `prepare_harness_session` with the absolute project and `detail=compact`. Identify the requested diff with native Git; use `get_changed_files` only if exposed to this host.
2. **Assess impact**: Where downstream effects remain uncertain, call `graph` with mode=impact on the relevant changed paths
3. **Check references**: For modified symbols with uncertain callers, call `find_referencing_symbols` and retain the relevant source locations
4. **Run diagnostics**: Call `get_file_diagnostics` on changed files to detect type errors or warnings
5. **Summarize**: Report the blast radius, breaking changes risk, and diagnostic issues

Load deferred tools with native tool search when supported. If a tool or language
diagnostic is unavailable, use the project's native checks and report the gap.
Expand impact or references only where the initial evidence leaves uncertainty;
avoid repeating a full scan for every file when one bounded report answers it.

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
