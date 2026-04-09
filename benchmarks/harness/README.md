# Harness Evaluation Layer

이 디렉터리는 `CodeLens MCP` 제품 자체가 아니라, `Codex/Claude` 같은 상위 하네스에서 CodeLens를 언제 어떻게 쓰는지가 실제로 이득인지 평가하고 운영 정책으로 내보내는 계층이다.

포함 범위:
- `harness-eval.py`
- `session-eval.py`
- `session-pack.py`
- `session-overhead-benchmark.py`
- `task-bootstrap.py`
- `codex-task-runner.py`
- `claude-task-runner.py`
- `refresh-routing-policy.py`
- `watch-routing-policy.py`
- `coverage-gap-queue.py`
- `coverage-gap-runner.py`
- `apply-routing-policy.py`
- `export-routing-policy.py`

원칙:
- CodeLens MCP 제품 측정은 상위 `benchmarks/` 루트에 둔다.
- 하네스 전용 runner/policy/coverage/promotion 로직은 여기 둔다.
- 루트의 같은 이름 스크립트는 기존 호출 경로를 깨지 않기 위한 호환 wrapper다.
- `agent_registry.py`가 Codex/Claude별 canonical policy, bootstrap 출력, wrapper, instruction 파일 경로의 단일 소스다.
- repo-local contract 문서(`AGENTS.md`, `CLAUDE.md`, `EVAL_CONTRACT.md`, `docs/platform-setup.md`)가 프로젝트별 진실의 원천이고, 이 디렉터리의 출력은 그 계약을 보조하는 하네스 산출물이다.
- canonical routing policy는 더 이상 완전 공용이 아니다.
  - Codex: `~/.codex/harness/policies/codelens-routing-policy.json`
  - Claude: `~/.claude/harness/policies/codelens-routing-policy.json`
  - shared reference: `~/.codex/harness/policies/codelens-routing-policy.shared.json`

Codex harness defaults reflected in this layer:
- non-interactive by default during harness runs
- explicit task tracking for multi-step work
- build -> verify -> fix before completion when runnable verification exists
- completion summaries must say what was done, what evidence was used, what verification ran, and what remains uncertain
- simple local lookup/edit stays native; CodeLens is for multi-file reviewer/planning/preflight paths after bootstrap
