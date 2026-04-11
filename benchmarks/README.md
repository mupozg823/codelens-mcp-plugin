# CodeLens MCP — Benchmark & Quality Tracking

벤치마크 숫자가 아니라 **"실제로 동작하는가"**를 추적합니다.

`benchmarks/`는 두 층으로 나뉩니다.

- product-core benchmark
  - `token-efficiency.py`
  - `embedding-quality.py`
  - `embedding-runtime.py`
  - `render-summary.py`
  - 여기서는 CodeLens MCP 자체의 토큰/지연/quality contract를 본다
- harness evaluation layer
  - 실제 구현은 [`benchmarks/harness/README.md`](/Users/bagjaeseog/codelens-mcp-plugin/benchmarks/harness/README.md)에 둡니다
  - 루트의 `harness-eval.py`, `codex-task-runner.py`, `claude-task-runner.py` 등은 기존 경로 호환용 wrapper입니다
  - 소비자 하네스 bootstrap/session overhead는 여기서만 측정합니다

## 디렉토리 구조

```
benchmarks/
├── README.md                  ← 이 파일
├── run-benchmark.sh           ← 성능 벤치마크 (자동, 재현 가능)
├── collect-session.sh         ← 세션 텔레메트리 수집 (실제 사용 기록)
├── compare.sh                 ← 두 결과 파일 비교 (변화율)
├── bench.sh                   ← 레거시 벤치마크 (간단)
├── BUGS.md                    ← 실사용 시나리오 테스트 (15개 케이스)
└── results/                   ← 모든 측정 결과 (날짜별)
    ├── 2026-03-30-phase8-fixes.md       ← 수동 상세 분석
    ├── 2026-03-30-baseline-v1.1.0.md    ← 자동 baseline
    └── 2026-03-30-session-telemetry.md  ← 실제 Claude Code 세션
```

## 3가지 측정 축

### 1. 성능 벤치마크 (run-benchmark.sh)

```bash
# 코드 변경 후 벤치마크
cargo build --release
./benchmarks/run-benchmark.sh . my-optimization

# 이전 결과와 비교
./benchmarks/compare.sh results/old.md results/new.md
```

**측정 항목:**

- 핵심 도구 성능 (warm, 3회 평균 ms)
- 제로 프로젝트 첫 호출 (auto-index 포함)
- grep 대비 속도

### 1-1. 토큰 효율 / workflow benchmark (token-efficiency.py)

```bash
CODELENS_BIN=target/debug/codelens-mcp python3 benchmarks/token-efficiency.py .
python3 benchmarks/render-summary.py --output benchmarks/token-efficiency-summary.md
```

측정 항목:

- 기존 baseline 대비 CodeLens output token 절감
- `preset:balanced` low-level chain vs role-profile composite workflow 비교
- scenario별 tool call count / low-level chain count / p95 latency / retry count
- CI에서는 markdown summary를 `GITHUB_STEP_SUMMARY`와 artifact로 함께 저장

### 1-2. 임베드 런타임 benchmark (embedding-runtime.py)

```bash
python3 benchmarks/embedding-runtime.py . --isolated-copy
python3 benchmarks/embedding-runtime.py . --isolated-copy --output benchmarks/embedding-runtime-results.json
```

측정 항목:

- 현재 기본 임베드 모델 이름
- `index_embeddings` 실제 경과 시간
- `semantic_search` warm query 평균/최대 지연
- `get_ranked_context` hybrid warm query 평균/최대 지연
- 임베딩 인덱스 심볼 수
- `onboard_project`가 기존 인덱스를 재사용하는지 여부

주의:

- 하드웨어와 프로젝트 크기에 따라 숫자가 크게 달라진다
- 이 스크립트 결과를 README의 고정 성능 수치보다 우선한다
- 재현 가능한 Codex 평가용 런은 `--isolated-copy`를 권장한다. 이 모드는 `.codelens/index` 충돌을 피하기 위해 임시 워크스페이스에서 인덱싱한다.
- `index_embeddings` 또는 측정 대상 tool call이 실패하면 즉시 종료한다. 부분 결과를 정상 벤치로 취급하지 않는다.

### 1-3. 임베드 품질 benchmark (embedding-quality.py)

```bash
python3 benchmarks/embedding-quality.py . --isolated-copy
python3 benchmarks/embedding-quality.py . \
  --isolated-copy \
  --output benchmarks/embedding-quality-results.json \
  --markdown-output benchmarks/embedding-quality-summary.md
```

측정 항목:

- `semantic_search` 자연어 질의 MRR / Acc@k
- `get_ranked_context` hybrid MRR / Acc@k
- `get_ranked_context disable_semantic=true` 대비 hybrid uplift
- query별 miss / wrong-top-hit

현재 재현 가능한 로컬 기준선 (`embedding-quality-results.json`, sequential + `--isolated-copy`):

- `semantic_search`: `MRR 0.502`, `Acc@1 44%`, `Acc@3 56%`, `Acc@5 62%`
- `get_ranked_context` lexical-only: `MRR 0.407`, `Acc@1 28%`, `Acc@3 47%`, `Acc@5 53%`
- `get_ranked_context` hybrid: `MRR 0.654`, `Acc@1 53%`, `Acc@3 69%`, `Acc@5 78%`
- overall uplift: `+0.246 MRR`, `+25% Acc@1`, `+22% Acc@3`, `+25% Acc@5`
- identifier-like queries: neutral uplift by design (`get_ranked_context` lexical-first)

데이터셋:

- `benchmarks/embedding-quality-dataset.json`
- 현재는 CodeLens 자체 코드베이스용 혼합 질의셋
  - `identifier`
  - `short_phrase`
  - `natural_language`

외부 저장소 follow-up 데이터셋 (v1.5 / v1.6 phase 측정용):

- `benchmarks/embedding-quality-dataset-typescript.json`
  - `microsoft/typescript` 34-query TS/JS dataset (Phase 3d, landed)
  - 4-arm A/B 측정 완료 — 전체 결과와 해석은 `docs/benchmarks.md` §8.15 참조
  - arm별 결과 파일: `benchmarks/embedding-quality-v1.6-phase3d-typescript-{baseline,2e-only,2b2c-only,stacked}.json`
- `benchmarks/embedding-quality-dataset-next-js.json`
  - `vercel/next.js` 34-query TS/JS typical-app dataset (Phase 3e, landed)
  - 4-arm A/B 측정 완료 — 전체 결과와 해석은 `docs/benchmarks.md` §8.16 참조
  - arm별 결과 파일: `benchmarks/embedding-quality-v1.6-phase3e-next-js-{baseline,2e-only,2b2c-only,stacked}.json`
- `benchmarks/embedding-quality-dataset-react-core.json`
  - `facebook/react` production subtree 34-query JS runtime dataset (Phase 3f, landed)
  - 4-arm A/B 측정 완료 — 전체 결과와 해석은 `docs/benchmarks.md` §8.17 참조
  - arm별 결과 파일: `benchmarks/embedding-quality-v1.6-phase3f-react-core-{baseline,2e-only,2b2c-only,stacked}.json`
- `benchmarks/embedding-quality-dataset-django.json`
  - `django/django` 34-query Python framework dataset (Phase 3g, landed)
  - 4-arm A/B 측정 완료 — 전체 결과와 해석은 `docs/benchmarks.md` §8.18 참조
  - arm별 결과 파일: `benchmarks/embedding-quality-v1.6-phase3g-django-{baseline,2e-only,2b2c-only,stacked}.json`

외부 phase matrix 자동 집계:

```bash
python3 benchmarks/embedding-quality-matrix.py \
  --require-datasets ripgrep,requests,jest,typescript,next-js,react-core,django,axum
```

- JSON 요약: `benchmarks/embedding-quality-phase3-matrix.json`
- Markdown 요약: `benchmarks/embedding-quality-phase3-matrix.md`
- 목적: ripgrep / requests / jest / typescript / next-js / react-core / django / axum 같은 **registry에 등록된 landed external datasets**를 수동 표 대신 artefact에서 직접 집계
- completeness gate: `--require-datasets ...` 를 주면 registry에 등록된 canonical dataset 누락 시 즉시 실패
- exploratory phase artefact까지 포함해 보려면 `--include-unregistered` 추가

리포트는 전체 평균 외에 질의 유형별 MRR / Acc@k와 hybrid uplift도 같이 보여준다.

현재 정책:

- `get_ranked_context`는 identifier-like query에서 semantic blending을 자동으로 약화한다
- `semantic_search`는 여전히 pure semantic baseline으로 유지한다

### 2. 세션 텔레메트리 (collect-session.sh)

```bash
# Claude Code 세션 종료 전에 실행
./benchmarks/collect-session.sh . session-name
```

**측정 항목:**

- Claude Code가 어떤 도구를 몇 번 호출했는지
- 서브에이전트가 CodeLens를 얼마나 활용했는지
- 도구당 평균 응답 시간 + 토큰 사용량
- 호출되지 않은 도구 (도구 표면 축소 근거)
- 토큰 효율 (CodeLens vs Read/Grep 추정)

### 2-1. 하네스 유의미성 평가 (harness-eval.py / session-eval.py)

`CodeLens를 항상 더 많이 쓰게 하는 것`이 목적이 아니다.  
이 경로는 **Codex/Claude 같은 하네스에서 어떤 작업군에 CodeLens가 실제로 유의미한지**를 분리해서 본다.

평가 모드:

- `baseline`
  - Codex native loop proxy
  - `rg/read/test` 또는 balanced low-level proxy
- `naive-on`
  - CodeLens 연결은 켰지만 routing rule 없이 직접 호출
- `routed-on`
  - deferred loading + workflow-first + verifier-first

대표 저장소:

- [`/Users/bagjaeseog/codelens-mcp-plugin`](/Users/bagjaeseog/codelens-mcp-plugin)
- [`/Users/bagjaeseog/Downloads/_방송도구/stream-admin`](/Users/bagjaeseog/Downloads/_방송도구/stream-admin)
- [`/Users/bagjaeseog/Downloads/SignatureStudio`](/Users/bagjaeseog/Downloads/SignatureStudio)

표준 출력:

- JSON: `~/.codex/harness/reports/<date>-codelens-eval.json`
- Markdown: `~/.codex/harness/reports/<date>-codelens-eval.md`

기본 동작:

- 같은 날짜 파일은 기본적으로 덮어쓴다
- 다른 이름으로 남기려면 `--label` 또는 `--output-json` / `--output-md` 사용

synthetic benchmark만 실행:

```bash
python3 benchmarks/harness-eval.py \
  --repo /Users/bagjaeseog/codelens-mcp-plugin \
  --skip-real-sessions
```

real session entry 정규화:

```bash
target/debug/codelens-mcp . --profile planner-readonly --cmd get_tool_metrics --args '{}' > /tmp/tool-metrics.json

python3 benchmarks/session-eval.py \
  --input /tmp/tool-metrics.json \
  --repo /Users/bagjaeseog/codelens-mcp-plugin \
  --task-kind "impact/reviewer" \
  --mode routed-on \
  --agent codex \
  --acceptance-passed true \
  --verify-passed true \
  --quality-score 0.82 \
  --notes "reviewer task with evidence expansion"
```

real session pack 생성:

```bash
python3 benchmarks/session-pack.py \
  --repo codelens-mcp-plugin \
  --task-kind 'impact/reviewer' \
  --mode routed-on
```

이 pack은 아래를 같이 만든다.

- `~/.codex/harness/reports/session-packs/<date>-*.json`
- `~/.codex/harness/reports/session-packs/<date>-*.md`

pack에는:

- repo/task/mode별 goal
- evaluation task mode (`read-only-eval`, `bounded-local-eval`)
- acceptance criteria
- execution quality checks
- verify commands
- 실제 Codex/Claude에 줄 prompt
- 이후 `session-eval.py`로 정규화할 `scenario_id`

scenario-aware 정규화:

```bash
python3 benchmarks/session-eval.py \
  --scenario-file ~/.codex/harness/reports/session-packs/<pack>.json \
  --scenario-id 'codelens-mcp-plugin::impact/reviewer::routed-on' \
  --input /tmp/tool-metrics.json \
  --agent codex \
  --acceptance-passed true \
  --verify-passed true \
  --quality-score 0.84
```

synthetic + real session 합산 보고서:

```bash
python3 benchmarks/harness-eval.py \
  --repo /Users/bagjaeseog/codelens-mcp-plugin \
  --session-entry-glob '~/.codex/harness/reports/session-entries/*.json'
```

### 2-2. 논문용 대표 지표 집계 (paper-benchmark.py)

하네스/에이전트 논문 기준의 대표 숫자는 `retrieval-only`가 아니라 아래 조합으로 본다.

- 주 지표: `Task Success Rate`
- 보조 retrieval 지표: `get_ranked_context MRR@10` (또는 현재 `embedding-quality.py` cutoff)
- 운영 지표:
  - `Tokens per Successful Task`
  - `Latency per Successful Task`

실행:

```bash
python3 benchmarks/paper-benchmark.py \
  --harness-report ~/.codex/harness/reports/<report>.json \
  --retrieval-report benchmarks/embedding-quality-results.json
```

promotion/gate용 real-session evidence를 fail-closed로 만들려면:

```bash
python3 benchmarks/harness/real-session-evidence.py \
  --retrieval-report benchmarks/embedding-quality-results.json \
  --output-json /tmp/real-session-evidence.json \
  --output-md /tmp/real-session-evidence.md
```

이 스크립트는:

- fresh `real-session` entry만 기준으로 harness evidence를 재계산하고
- stale synthetic-only harness report가 있으면 현재 session-entry archive로 자동 refresh하며
- measured task 수가 부족하면 coverage gap queue와 scenario pack을 같이 생성한다

즉 promotion gate는 이제 `synthetic fallback`이 아니라 `real-session evidence + gap queue`를 기본 경로로 사용한다.

기본 정책:

- 하네스 코호트는 `mode=routed-on`
- `real-session` entry가 있으면 그것을 우선 사용
- real-session이 없으면 `synthetic` entry로 대체하되, 이 결과는 reporting-only다
- retrieval 보조 지표는 `embedding-quality.py`의 `get_ranked_context` 결과를 사용
- `promotion_eligibility`는 별도 필드로 노출되며, synthetic fallback은 승격 근거가 되지 않는다

출력:

- JSON: `benchmarks/paper-benchmark-results.json`
- Markdown: `benchmarks/paper-benchmark-summary.md`

이 스크립트는 기존 `harness-eval.py`와 `embedding-quality.py`를 대체하지 않는다.
둘의 결과를 논문/발표용 대표 지표로 정렬해 주는 얇은 집계 레이어다.

### 2-3. 모델 승격 게이트 (promotion_gate.py)

임베드 모델 승격은 내부 validation 점수로 결정하지 않는다.
반드시 현재 배포 모델과 같은 바이너리/같은 프로젝트에서 fresh A/B를 돌린다.

```bash
python3 scripts/finetune/promotion_gate.py \
  --candidate-onnx-dir scripts/finetune/output/<candidate>/onnx \
  --candidate-label <candidate>
```

기본 하드 게이트:

- `semantic_search` MRR non-regression
- `get_ranked_context` MRR non-regression
- `get_ranked_context` Acc@1 non-regression
- `real-session` harness task success non-regression
- `external-retrieval.py` exact-label non-regression
- `role-retrieval.py` exact-label non-regression
- `contamination_audit.py` pass

주의:

- synthetic-only harness는 smoke check로는 유효하지만, promotion pass/fail에는 쓰지 않는다.
- external 검증은 keyword-hit heuristic이 아니라 exact `expected_symbol` / `expected_file_suffix` gold label을 쓴다.
- role benchmark는 `entrypoint vs helper/predicate` 혼동을 따로 측정한다.

routing policy export:

```bash
python3 benchmarks/export-routing-policy.py \
  --input ~/.codex/harness/reports/2026-04-03-codelens-eval-cross-repo-release.json
```

출력:

- dated policy
  - `~/.codex/harness/policies/<date>-*.json`
  - `~/.codex/harness/policies/<date>-*.md`
- canonical latest policies
  - shared reference
    - `~/.codex/harness/policies/codelens-routing-policy.shared.json`
    - `~/.codex/harness/policies/codelens-routing-policy.shared.md`
  - Codex
  - `~/.codex/harness/policies/codelens-routing-policy.json`
  - `~/.codex/harness/policies/codelens-routing-policy.md`
  - Claude
    - `~/.claude/harness/policies/codelens-routing-policy.json`
    - `~/.claude/harness/policies/codelens-routing-policy.md`

이 policy는:

- global task-kind rule
- repo-specific override
- agent-specific optimization split
- AGENTS/snippet용 설명

을 함께 담는다.

global AGENTS / repo override 적용:

```bash
python3 benchmarks/apply-routing-policy.py
```

이 명령은:

- `~/.codex/AGENTS.md` 에 Codex용 generated `CodeLens Routing Policy` section을 주입/갱신
- `~/.claude/CLAUDE.md` 에 Claude용 generated `CodeLens Routing Policy` section을 주입/갱신
- `~/.codex/harness/policies/repo-overrides/*-codex.md`, `*-claude.md` 에 repo별 override snippet 생성
- `harness-eval-config.json` 에 등록된 repo 중 기존 `AGENTS.md` / `CLAUDE.md` 가 있는 repo에는 project-local `CodeLens Repo Routing Policy` section도 각각 주입/갱신

대표 repo 중 `AGENTS.md` 가 없는 저장소까지 bootstrap 하려면:

```bash
python3 benchmarks/apply-routing-policy.py --bootstrap-missing-agents
```

이 모드는:

- `harness-eval-config.json` 의 representative repo에 대해 최소 `AGENTS.md` 를 생성
- verify commands / stack / local guidance를 넣고
- 같은 파일에 generated repo-local routing policy section을 주입

task 시작 시 routing decision brief를 뽑으려면:

```bash
python3 benchmarks/task-bootstrap.py \
  --repo /absolute/repo/path \
  --task-kind "impact/reviewer" \
  --task "review the modal blast radius before editing"
```

Claude 하네스용 brief:

```bash
python3 benchmarks/task-bootstrap.py \
  --platform claude \
  --repo /absolute/repo/path \
  --task-kind "refactor preflight" \
  --task "check rename safety before editing imports"
```

또는 전역 wrapper:

```bash
~/.codex/harness/bin/codelens-task-bootstrap \
  --repo /absolute/repo/path \
  --task-kind "refactor preflight"
```

이 명령은:

- canonical routing policy + repo override를 읽고
- 해당 task에 CodeLens를 써야 하는지, native로 시작해야 하는지, deferred loading이 필요한지를 판정하고
- first actions / preferred entrypoints / verify commands를 담은 JSON/Markdown brief를 `~/.codex/harness/bootstrap/` 아래 생성

Codex 실행용 prompt까지 만들려면:

```bash
python3 benchmarks/codex-task-runner.py \
  --repo /absolute/repo/path \
  --task-kind "impact/reviewer" \
  --task "review blast radius before changing modal imports"
```

전역 wrapper:

```bash
~/.codex/harness/bin/codex-harness-task \
  --repo /absolute/repo/path \
  --task-kind "impact/reviewer" \
  --task "review blast radius before changing modal imports"
```

기본값은 dry-run이다. 이 모드는:

- bootstrap brief를 갱신하고
- Codex에 전달할 prompt markdown을 `~/.codex/harness/bootstrap/prompts/` 아래 생성하고
- 가능하면 MCP preflight를 수행해 `run_dir/mcp-preflight.json`에 현재 surface / index 상태를 기록하고
- MCP가 불안하면 prompt에 native fallback 지침을 같이 넣고
- 실제 `codex exec` 명령 배열을 JSON으로 출력한다

Claude 실행용 prompt와 command를 만들려면:

```bash
python3 benchmarks/claude-task-runner.py \
  --repo /absolute/repo/path \
  --task-kind "impact/reviewer" \
  --task "review blast radius before changing modal imports"
```

이 모드는:

- Claude platform bootstrap brief를 `~/.claude/harness/bootstrap/` 아래 생성하고
- Claude prompt markdown을 `~/.claude/harness/bootstrap/prompts/` 아래 생성하고
- 실제 `claude -p ...` 명령 배열을 JSON으로 출력한다

실제로 실행하려면 `--exec` 를 붙인다.

대표 coverage task인 `impact/reviewer`, `onboarding/planning`, `refactor preflight`는 session pack 기준으로 기본 `read-only-eval`이다.

- Codex runner는 이런 scenario를 실행할 때 기본 sandbox를 `read-only`로 고정한다
- Claude runner는 이런 scenario를 실행할 때 기본 permission mode를 `plan`으로 낮추고, evaluation timeout을 기본 적용한다
- prompt도 수정/패치 대신 evidence, reviewer verdict, bounded verification 중심으로 바뀐다
- session pack이 `workflow_budget`, `result_budget`, `stop_rule`, agent hint까지 내려주므로 reviewer 수집은 `한 번의 workflow report + 최소 evidence 확장` 쪽으로 더 강하게 제한된다
- 즉 mixed-agent coverage 수집은 generic coding loop가 아니라 read-only evaluation loop로 운영된다

실행 결과를 evaluation entry까지 회수하려면:

```bash
python3 benchmarks/codex-task-runner.py \
  --repo /absolute/repo/path \
  --task-kind "impact/reviewer" \
  --task "review blast radius before changing modal imports" \
  --exec \
  --capture-eval \
  --acceptance-passed true \
  --verify-passed true \
  --quality-score 0.82
```

이 모드는 run bundle 디렉터리 하나를 만들고:

- `metrics-before.json`
- `metrics-after.json`
- `metrics-delta.json`
- `last-message.md`
- `session-entry.json`
- `session-entry.md`

를 함께 남긴다.

그리고:

- `get_tool_metrics` capture self-overhead를 delta에서 제거하고
- archived `session-entry` 를 `~/.codex/harness/reports/session-entries/` 에 저장하고
- 같은 run bundle 안에 refreshed `harness-eval.json` / `harness-eval.md` 를 생성한다
- cross-repo preview `routing policy refresh` 를 실행하고, coverage가 충분하면 canonical policy/AGENTS 적용까지 승격한다

즉 task 실행 결과가 다시 harness-eval에 바로 합칠 수 있는 real-session entry + repo-local 재집계 결과로 이어진다.

Claude에서도 같은 closed-loop를 쓰려면:

```bash
python3 benchmarks/claude-task-runner.py \
  --repo /absolute/repo/path \
  --task-kind "impact/reviewer" \
  --task "review blast radius before changing modal imports" \
  --exec \
  --capture-eval \
  --acceptance-passed true \
  --verify-passed true \
  --quality-score 0.82
```

이 명령은:

- `claude -p` 로 task를 실행하고
- `last-message.md` 를 run bundle에 저장하고
- shared session-entry archive에 real-session entry를 추가하고
- refreshed harness-eval과 routing policy refresh preview까지 생성한다

archived real-session 기준으로 preview refresh만 다시 돌리려면:

```bash
python3 benchmarks/refresh-routing-policy.py
```

이 명령은:

- base synthetic report + archived `session-entry` 를 합쳐 preview hybrid report를 만들고
- preview routing policy를 `~/.codex/harness/policies/previews/` 아래 생성하고
- canonical 대비 drift artifact를 `~/.codex/harness/reports/drift/` 아래 생성하고
- 대표 repo × required task kind coverage가 충분하고, 각 required task kind에 대해 `codex` 와 `claude` real-session 근거가 모두 있을 때만 canonical policy와 AGENTS 반영까지 승격한다

즉 평가 결과가 더 이상 리포트에만 머물지 않고, 실제 Codex/Claude harness routing 규칙으로 이어진다.

최근 refresh/drift 이력을 기준으로 정책 안정 상태를 보려면:

```bash
python3 benchmarks/watch-routing-policy.py
python3 benchmarks/watch-routing-policy.py --write-defaults
```

이 스크립트는:

- 최근 refresh artifact와 drift artifact를 같이 읽고
- latest 정책이 stable 상태인지
- 최근 window에 drift/flapping이 있었는지
- duplicate real-session이 다시 active로 들어왔는지
- canonical policy가 현재 몇 개 rule/override를 갖는지
를 한 번에 요약한다

archived real-session 중복 bucket을 안전하게 정리하려면:

```bash
python3 benchmarks/prune-session-entries.py
python3 benchmarks/prune-session-entries.py --apply
```

이 스크립트는:

- logical key 기준으로 duplicate real-session entry를 찾고
- 기본은 dry-run으로 kept/discarded만 보고하고
- `--apply` 시에는 중복 파일을 `~/.codex/harness/reports/session-entries/archive/duplicates/` 로 이동한다

mixed-agent coverage gap queue를 다시 만들려면:

```bash
python3 benchmarks/coverage-gap-queue.py
```

이 명령은:

- 현재 archived real-session coverage를 읽고
- 아직 `codex` / `claude` 근거가 부족한 repo/task 조합을 찾고
- 실행 가능한 queue와 scenario pack을 `~/.codex/harness/reports/coverage-queues/` 아래 생성한다

queue 진행률을 보거나 다음 수집 항목을 고르려면:

```bash
python3 benchmarks/coverage-gap-runner.py --pending-only
python3 benchmarks/coverage-gap-runner.py --next --agent codex
python3 benchmarks/coverage-gap-runner.py --next --agent claude
```

실제로 queue item 하나를 집행하려면:

```bash
python3 benchmarks/coverage-gap-runner.py \
  --next \
  --agent codex \
  --pending-only \
  --exec
```

이 runner는:

- latest queue를 읽고
- archived qualifying real-session을 기준으로 pending/completed를 다시 계산하고
- 선택한 queue item의 wrapper command를 그대로 실행한다

즉 수집 자체도 더 이상 ad hoc가 아니라, `queue 생성 -> next item 선택 -> real-session capture -> refresh` 루프로 운영할 수 있다.

보고서가 고정해서 보여주는 것:

- task-type별 `CodeLens off / naive on / routed on`
- outcome quality / verify / execution quality / efficiency
- CodeLens가 유의미했던 작업군
- CodeLens가 오히려 손해였던 작업군
- 이후 전역 AGENTS/harness routing에 반영할 one-line policy
- `confidence`
  - synthetic만 있으면 보통 `medium`
  - 동일 repo/task에 real-session quality entry가 붙으면 `high`

### 3. 실사용 시나리오 테스트 (BUGS.md)

```bash
cat benchmarks/BUGS.md
```

**15개 엣지 케이스:**
빈 디렉토리, 존재하지 않는 파일, 바이너리 공존, 경로 탈출,
대형 파일, Unicode, 동명 심볼, git 없음, 심볼릭 링크,
rename 주석/문자열 안전, 연속 호출, 비코드 파일 등

## 결과 파일 양식 (frontmatter)

모든 결과 파일은 동일한 YAML frontmatter:

```yaml
---
date: 2026-03-30
phase: phase-name
project: /path/to/project
binary: ./target/release/codelens-mcp
commit: abc1234
---
```

이 양식 덕분에 `compare.sh`로 자동 비교 가능.

## 핵심 지표 해석 가이드

| 지표                  | 좋음          | 주의        | 나쁨                       |
| --------------------- | ------------- | ----------- | -------------------------- |
| find_symbol (warm)    | <15ms         | 15-50ms     | >50ms                      |
| find_refs (warm)      | <50ms         | 50-150ms    | >150ms                     |
| 제로 프로젝트 첫 호출 | <100ms        | 100-500ms   | >500ms                     |
| 세션 에러율           | 0%            | <5%         | >5%                        |
| 도구 사용 집중도      | 상위 3개 >60% | —           | 분산 (도구 설명 개선 필요) |
| 호출당 토큰           | <2,000        | 2,000-5,000 | >5,000                     |

## 워크플로우

```
코드 변경
  → cargo build --release
  → ./benchmarks/run-benchmark.sh . change-name
  → ./benchmarks/compare.sh results/baseline.md results/change-name.md
  → 성능 회귀 확인

Claude Code 세션 종료
  → ./benchmarks/collect-session.sh . session-name
  → 도구 사용 패턴 분석
  → 미사용 도구 → 도구 표면 축소 근거
  → 높은 에러율 도구 → 버그 수정 대상
```
