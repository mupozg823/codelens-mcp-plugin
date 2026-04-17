# External 3-arm benchmark runbook

P1-5 from the 2026-04-17 planning session. Measures whether CodeLens's
hybrid retrieval wins outside the self-repository baseline, and whether
the NL→code bridge layer (generic + project-specific) contributes.

## Ablation arms

| Arm        | `CODELENS_GENERIC_BRIDGES_OFF` | `.codelens/bridges.json`                             | What it measures                    |
| ---------- | ------------------------------ | ---------------------------------------------------- | ----------------------------------- |
| bridge-off | `1`                            | absent                                               | lexical + embedding only            |
| generic-on | unset                          | absent                                               | adds the hard-coded GENERIC_BRIDGES |
| repo-on    | unset                          | copied from `benchmarks/bridges/{slug}.bridges.json` | generic + project-specific          |

## Pre-flight

1. Build the release binary with the `semantic` feature:

   ```bash
   cargo build --release --features semantic
   ```

2. Prepare an `external-repos/` directory with one worktree per slug:

   ```text
   external-repos/
     axum/
     ripgrep/
     django/
     typescript/
     bridges/
       axum.bridges.json       # optional; controls the repo-on arm
       ripgrep.bridges.json
   ```

3. (Optional) Author per-repo bridge overrides under
   `benchmarks/bridges/{slug}.bridges.json`. See `benchmarks/bridges/README.md`.

## Local run

```bash
python3 benchmarks/external-3arm.py \
    --repos-dir external-repos \
    --datasets axum:benchmarks/embedding-quality-dataset-axum.json \
               ripgrep:benchmarks/embedding-quality-dataset-ripgrep.json \
               django:benchmarks/embedding-quality-dataset-django.json \
               typescript:benchmarks/embedding-quality-dataset-typescript.json \
    --output benchmarks/external-3arm-results.json
```

Output: a JSON matrix + Markdown table at the `--output` path
(`.json` / `.md`). The Markdown table is what the CI job appends to
`$GITHUB_STEP_SUMMARY`.

## CI run

GitHub Actions workflow at `.github/workflows/external-3arm.yml` clones
the four default repos, applies any committed bridges, and runs the
matrix. Currently manual-trigger only (`workflow_dispatch`). A
commented `schedule:` block is provided for nightly runs once the job
is proven stable.

Manual trigger:

```bash
gh workflow run external-3arm.yml
gh workflow run external-3arm.yml -f repos=axum,ripgrep   # subset
```

## Interpreting the matrix

Row pattern per (repo, arm):

```
| repo | arm | hybrid | semantic | lexical |
```

Decision table:

| Observation                                      | Action                                                                |
| ------------------------------------------------ | --------------------------------------------------------------------- |
| `generic-on.hybrid` > `bridge-off.hybrid`        | generic bridges pay off on this repo; keep default-on.                |
| `generic-on.hybrid` ≤ `bridge-off.hybrid`        | generic bridges may be over-fit to CodeLens's self-repo. Investigate. |
| `repo-on.hybrid` > `generic-on.hybrid` by ≥ 0.01 | the per-repo override is paying for itself; keep it.                  |
| `repo-on.hybrid` ≤ `generic-on.hybrid`           | delete the override; dead entries just waste tokens at runtime.       |
| `semantic` > `hybrid` on any arm                 | ranking blend is underweighting semantic on this repo; tune.          |

## Known limits

- Cold indexing dominates per-run time on large repos. The runner
  re-indexes the isolated copy each arm; do not interpret latency
  numbers from this job as steady-state.
- The dataset for each repo is curated, not machine-generated. Low MRR
  could reflect stale ground-truth rather than retrieval regression —
  cross-check with `benchmarks/lint-datasets.py`.
- `repo-on` arm is skipped when no override file exists. Rows in the
  output table will show `-` and an `error` note, which is expected.
- No client-side sampling; OTel traces (if enabled) will include a
  span per tool call per query. Disable the exporter for local
  benchmark runs or use a dedicated collector.
