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
