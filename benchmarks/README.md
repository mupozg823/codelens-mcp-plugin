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
