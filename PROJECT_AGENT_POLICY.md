# Project Agent Policy

## Roles

- Codex: implementation, local refactor, direct test execution
- Claude: orchestration, review, evaluation, harness supervision

## Shared Rules

- Prefer minimal diffs.
- Prefer existing modules over new wrappers.
- Use verifier-first flow for risky mutations.
- Keep fast paths fast; do not add heavy analysis to every turn.
- Treat CodeLens as an external coprocessor, not embedded runtime logic.

## Routing

- Simple local lookup/edit: native first
- Multi-file impact/review/refactor: escalate to CodeLens
- Heavy analysis: async handle/job path
- If CodeLens times out or fails: fall back to native path

## Non-Goals

- Do not unify Codex and Claude global prompts.
- Do not duplicate repo policy into global config.
- Do not make CodeLens the default path for every trivial request.
