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

런타임 산출물:
- 각 `run_dir`는 이제 typed manifest `run-manifest.json`과 append-only event log `run-events.jsonl`을 가진다.
- manifest는 `repo/task/agent/mode/scenario` identity를 고정해서 다른 실행이 같은 `run_dir`를 재사용하며 섞이는 것을 막는다.
- expensive post-processing 단계(`mcp_preflight`, `harness_eval`, `routing_policy_refresh`)는 같은 `run_dir`에서 completed checkpoint가 있으면 재사용할 수 있다.
- 이 디렉터리의 truth artifact는 더 이상 개별 `prompt`, `session-entry`, `harness-eval` 파일이 아니라, 그 파일들을 가리키는 `run-manifest.json`이다.
- Codex preflight는 이제 low-level `tools/list -> activate_project -> get_capabilities` choreography를 하네스 밖에서 중복하지 않고, 공식 세션 도구 `prepare_harness_session`을 우선 사용한다. 구버전 서버만 legacy round-trip fallback을 탄다.
