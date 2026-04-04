# Harness Modes

## Mode A: Native Fast Path

Use for:

- trivial point lookups
- small local edits
- single-file diagnostics

## Mode B: CodeLens Read-Only Assist

Use for:

- multi-file context compression
- impact review
- ranked symbol/context lookup

Preferred tools:

- `find_minimal_context_for_change`
- `get_ranked_context`
- `get_file_diagnostics`

## Mode C: Verifier-First Mutation

Use for:

- rename
- file creation or replacement
- import-affecting refactors
- cross-file changes

Preferred tools:

- `verify_change_readiness`
- `unresolved_reference_check`
- `safe_rename_report`

## Mode D: Async Analysis

Use for:

- large impact reports
- heavyweight review artifacts
- repeated section expansion

Preferred workflow:

- `start_analysis_job`
- `get_analysis_job`
- `get_analysis_section`
