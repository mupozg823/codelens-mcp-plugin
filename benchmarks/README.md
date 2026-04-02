# CodeLens MCP — Benchmark & Quality Tracking

벤치마크 숫자가 아니라 **"실제로 동작하는가"**를 추적합니다.

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
python3 benchmarks/embedding-runtime.py .
python3 benchmarks/embedding-runtime.py . --output benchmarks/embedding-runtime-results.json
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

### 1-3. 임베드 품질 benchmark (embedding-quality.py)

```bash
python3 benchmarks/embedding-quality.py .
python3 benchmarks/embedding-quality.py . \
  --output benchmarks/embedding-quality-results.json \
  --markdown-output benchmarks/embedding-quality-summary.md
```

측정 항목:

- `semantic_search` 자연어 질의 MRR / Acc@k
- `get_ranked_context` hybrid MRR / Acc@k
- `get_ranked_context disable_semantic=true` 대비 hybrid uplift
- query별 miss / wrong-top-hit

현재 로컬 기준선 (`embedding-quality-results.json`):

- `semantic_search`: `MRR 0.364`, `Acc@1 29%`, `Acc@3 38%`, `Acc@5 46%`
- `get_ranked_context` lexical-only: `MRR 0.263`, `Acc@1 17%`, `Acc@3 33%`, `Acc@5 38%`
- `get_ranked_context` hybrid: `MRR 0.399`, `Acc@1 33%`, `Acc@3 42%`, `Acc@5 50%`
- overall uplift: `+0.135 MRR`, `+17% Acc@1`, `+8% Acc@3`, `+12% Acc@5`
- identifier-like queries: neutral uplift by design (`get_ranked_context` lexical-first)

데이터셋:

- `benchmarks/embedding-quality-dataset.json`
- 현재는 CodeLens 자체 코드베이스용 혼합 질의셋
  - `identifier`
  - `short_phrase`
  - `natural_language`

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
