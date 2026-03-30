# CodeLens MCP — Development Metrics

> 모든 데이터는 **다음 개선 결정**을 내리기 위해 존재합니다.
> 기록 자체가 목적이 아니라, "뭘 고쳐야 하는지"를 알려주는 것이 목적입니다.

---

## 1. 도구 선택 효율 (Tool Selection Efficiency)

> **목적:** 도구 수 축소, 설명 개선, 프리셋 재설계의 근거

### 측정 항목

| 지표                    | 산식                                | 기준                 | 개선 액션                          |
| ----------------------- | ----------------------------------- | -------------------- | ---------------------------------- |
| **도구 집중도**         | 상위 3개 호출 / 전체 호출           | >60% 정상, <40% 분산 | 분산 시 → 도구 설명 개선 또는 합병 |
| **미사용률**            | 미사용 도구 / BALANCED 전체         | <50% 정상, >70% 과잉 | 과잉 시 → BALANCED에서 추가 제외   |
| **오선택률**            | 에이전트가 잘못된 도구 선택 횟수    | 0이 목표             | 발생 시 → 혼동 도구 쌍 설명 차별화 |
| **suggest_next 적중률** | 제안된 도구가 실제 다음 호출된 비율 | >30% 좋음            | 낮으면 → suggest_next 매핑 수정    |

### 현재 값

| 세션             | 유형                  | 집중도 | 미사용률    | 고유도구 |
| ---------------- | --------------------- | ------ | ----------- | -------- |
| 2026-03-30       | 코드 수정 (Phase 6-9) | 76%    | 72% (28/39) | 11       |
| (추가 세션 필요) | 코드 리뷰             | —      | —           | —        |
| (추가 세션 필요) | 온보딩/탐색           | —      | —           | —        |
| (추가 세션 필요) | 리팩터링              | —      | —           | —        |

### 원인 분석 (2026-03-30)

```
⚠ 이 세션의 미사용률 72%를 도구 과잉으로 판단하면 안 됨.

세션 특성: CodeLens 자체 개발 (코드 수정 중심)
  → find_symbol + ranked_context + delete_lines가 주 작업
  → 분석 도구 (callers, dead_code, circular) 필요한 작업 없었음
  → memory 도구: Claude Code 자체 메모리와 중복 → 사용 안 함
  → find_referencing_symbols: Claude Code가 내장 Grep 선호

세션 유형별 예상 사용 패턴:
  코드 수정:  find_symbol, ranked_context, delete_lines, replace_symbol_body
  코드 리뷰:  get_changed_files, get_impact_analysis, find_refs
  온보딩:     onboard_project, get_symbols_overview, get_ranked_context
  리팩터링:   rename_symbol, find_scoped_references, find_refs

결론: 3-5개 다른 유형 세션 축적 전에 도구 축소 결정 금지.
```

### 다음 액션

- [ ] 코드 리뷰 세션 텔레메트리 수집 (PR 리뷰 작업 시)
- [ ] 온보딩 세션 텔레메트리 수집 (새 프로젝트에서 탐색 시)
- [ ] 리팩터링 세션 텔레메트리 수집 (rename/extract 작업 시)
- [ ] **3개 유형 이상 축적 후** 전체 유형에서 0회 호출된 도구만 축소 후보
- [ ] timeline 데이터에서 suggest_next 적중률 자동 계산 추가

---

## 2. 응답 품질 (Response Quality)

> **목적:** 검색 정확도 개선, 랭킹 알고리즘 튜닝

### 측정 항목

| 지표                      | 산식                                     | 기준 | 개선 액션                       |
| ------------------------- | ---------------------------------------- | ---- | ------------------------------- |
| **find_symbol 정확도**    | 정확한 결과 / 전체 결과                  | >95% | 낮으면 → FTS5 쿼리 개선         |
| **find_refs 정밀도**      | 실제 참조 / 반환된 참조                  | >90% | 낮으면 → 주석/문자열 필터 개선  |
| **find_refs 재현율**      | 반환된 참조 / 실제 전체 참조             | >80% | 낮으면 → 스캔 범위 확대         |
| **ranked_context 관련도** | 사용자가 실제 필요한 심볼이 top-5에 포함 | >70% | 낮으면 → 랭킹 가중치 조정       |
| **rename 안전도**         | 코드만 치환 / 전체 치환                  | 100% | <100% → non-code 필터 패턴 추가 |

### 현재 값

```
find_symbol 정확도:    100% (테스트 11개 언어 전부 정확)
find_refs 정밀도:      개선됨 (주석/문자열 필터 추가, 정량 미측정)
find_refs 재현율:      미측정 (grep 대비 비교 필요)
ranked_context 관련도: 미측정 (사용자 피드백 필요)
rename 안전도:         100% (L1 코드만, L4 주석 + L5 문자열 스킵 확인)
```

### 다음 액션

- [ ] find_refs 정밀도/재현율 자동 테스트 추가 (grep 결과와 diff)
- [ ] ranked_context에 사용자 피드백 루프 설계 (어떤 심볼이 실제로 유용했는지)
- [ ] 4-signal 랭킹 가중치 A/B 테스트 프레임워크

---

## 3. 성능 프로파일 (Performance Profile)

> **목적:** 병목 식별, 최적화 우선순위 결정

### 측정 항목

| 지표                   | 기준   | 현재     | 병목 원인             | 개선 가능성        |
| ---------------------- | ------ | -------- | --------------------- | ------------------ |
| **find_symbol (warm)** | <15ms  | 11ms     | —                     | 충분               |
| **find_refs (warm)**   | <50ms  | 93ms     | tree-sitter 파싱/파일 | non-code 범위 캐시 |
| **ranked_context**     | <30ms  | 15ms     | —                     | 충분               |
| **impact_analysis**    | <20ms  | 12ms     | —                     | 충분               |
| **refresh_index**      | <200ms | 82ms     | —                     | 충분               |
| **제로 첫 호출**       | <200ms | 45-115ms | auto-index            | 충분               |
| **onboard_project**    | <5s    | 45s      | fastembed 모델 로딩   | 분석 필요          |

### 병목 분석

```
시간 분포 (이 세션, 73호출):
  onboard_project:  4,963ms (91.7%)
  get_ranked_ctx:     343ms (6.3%)
  semantic_search:     51ms (0.9%)
  find_symbol:         33ms (0.6%)
  나머지:              19ms (0.4%)
```

### 원인 분석

**onboard_project 91.7%에 대해:**

```
⚠ "onboard_project가 병목이니 시맨틱 분리 필요"는 결론 점프.

원인 분해:
  onboard_project 내부 단계:
    1. refresh_symbol_index (~82ms) — tree-sitter 파싱
    2. get_project_structure (~1ms) — DB 조회
    3. get_symbol_importance (PageRank) (~수ms)
    4. find_circular_dependencies (~수ms)
    5. index_embeddings (~45s) ← 이것이 진짜 병목

  index_embeddings가 느린 이유:
    a. fastembed 모델 최초 로딩 (~3-5s) — OnceLock lazy init
    b. 958 심볼 × BGE-small 추론 (~40s) — CPU 기반 ONNX
    c. sqlite-vec 삽입 (~수백ms)

  핵심: 45s 중 ~40s가 임베딩 추론. 이건 모델 크기 + CPU 추론의 근본 한계.

이 세션에서 4회 호출된 이유:
  → 서브에이전트(codelens-explorer)가 onboard_project를 사용
  → 매 서브에이전트 호출 시 새 프로세스? 아니면 같은 서버?
  → 4회 × 1.2s = 첫 호출만 45s, 이후는 모델 캐시로 ~1.2s

실제로 45s는 첫 1회만. 나머지 3회는 캐시 히트.
따라서 "시맨틱 분리"보다 "모델 pre-warm" 또는 "lazy load 유지"가 적절.
```

**find_refs 93ms에 대해:**

```
원인: tree-sitter로 매 파일의 주석/문자열 범위를 파싱하는 비용.
이전: 33ms (주석 필터 없음, 정확도 낮음)
현재: 93ms (주석 필터 있음, 정확도 높음)

이건 의도된 trade-off.
최적화 방향: 파일별 non-code 범위 캐시 (동일 세션에서 같은 파일 재파싱 방지)
```

### 다음 액션

- [ ] onboard_project 45s 내역을 단계별로 분리 측정 (트레이싱 추가)
- [ ] 임베딩 모델 pre-warm 효과 측정 (첫 호출 vs 이후 호출)
- [ ] find_refs non-code 범위를 파일별 캐시 (HashMap<PathBuf, Vec<Range>>)
- [ ] 대형 프로젝트 (1000+ 파일) 벤치마크 추가

---

## 4. 임베딩 모델 성능 (Embedding Quality)

> **목적:** 시맨틱 검색 품질 개선, 모델 교체/파인튜닝 판단

### 측정 항목

| 지표                    | 산식                                 | 기준       | 개선 액션                        |
| ----------------------- | ------------------------------------ | ---------- | -------------------------------- |
| **인덱싱 속도**         | 심볼/초                              | >100 sym/s | 느리면 → 배치 크기 조정          |
| **모델 로딩 시간**      | cold load ms                         | <3s        | 느리면 → 양자화 또는 ONNX 최적화 |
| **메모리 사용**         | 모델 로드 후 RSS                     | <300MB     | 크면 → 더 작은 모델              |
| **검색 정확도 (MRR@5)** | 관련 결과가 top-5에 있는 비율        | >0.6       | 낮으면 → 모델 교체 또는 파인튜닝 |
| **코드 vs 자연어 질의** | 코드 스니펫 vs 설명 질의 정확도 차이 | <20% 차이  | 크면 → 코드 특화 임베딩          |

### 현재 값

```
모델: BGE-small-en-v1.5 (quantized, fastembed)
인덱싱 속도: 미측정
모델 로딩: ~3-5s (onboard_project 45s의 대부분)
메모리: ~248MB (ONNX 모델)
MRR@5: 미측정
코드/자연어 차이: 미측정
```

### 파인튜닝 판단 기준

```
현재 모델로 충분한 경우:
  MRR@5 > 0.6 AND 코드/자연어 차이 < 20%

모델 교체가 나은 경우:
  MRR@5 < 0.4 → 더 큰 모델 (BGE-base, E5-large 등)

파인튜닝이 나은 경우:
  MRR@5 0.4-0.6 AND 코드 질의 정확도 << 자연어 질의
  → 코드 검색 데이터셋으로 파인튜닝 (CodeSearchNet 등)
```

### 다음 액션

- [ ] MRR@5 벤치마크 데이터셋 구축 (20개 질의 + 정답 심볼)
- [ ] 모델 로딩을 lazy + 백그라운드로 전환
- [ ] 모델 교체 후보: jina-embeddings-v3, nomic-embed-code

---

## 5. 아키텍처 건강도 (Architecture Health)

> **목적:** 중앙 제어층 비대화 감지, 모듈 분해 품질 모니터링

### 측정 항목

| 지표                     | 산식                 | 기준 | 개선 액션                      |
| ------------------------ | -------------------- | ---- | ------------------------------ |
| **dispatch 엔트리 수**   | dispatch_table 항목  | <80  | 초과 시 → 도구 합병            |
| **tool_defs.rs LOC**     | 파일 줄 수           | <400 | 초과 시 → 매크로화 또는 분리   |
| **state.rs 필드 수**     | AppState 구조체 필드 | <15  | 초과 시 → 서브 구조체 추출     |
| **테스트 커버리지**      | 도구당 테스트        | >0.5 | 낮으면 → 누락 도구 테스트 추가 |
| **defs↔dispatch 일관성** | 자동 테스트          | PASS | FAIL → 빌드 차단               |

### 현재 값

```
dispatch 엔트리: ~65 (매크로 레지스트리)
tool_defs.rs:    ~350 LOC
state.rs 필드:   13 (AppState) + 2 (SecondaryProject)
테스트:          190 (core 149 + mcp 41)
일관성 테스트:   PASS (tool_defs_and_dispatch_are_consistent)
```

### 다음 액션

- [ ] tool_defs.rs 400LOC 도달 시 카테고리별 파일 분리
- [ ] AppState 15필드 도달 시 서브 구조체 추출

---

## 6. 토큰 효율 (Token Economy)

> **목적:** CodeLens가 Read/Grep 대비 얼마나 토큰을 절약하는지 정량화

### 측정 항목

| 지표                 | 산식                                    | 개선 액션                 |
| -------------------- | --------------------------------------- | ------------------------- |
| **호출당 평균 토큰** | total_tokens / total_calls              | 높으면 → 응답 트리밍 강화 |
| **Read 대비 절약률** | (Read 토큰 - CodeLens 토큰) / Read 토큰 | 낮으면 → 랭킹 개선        |
| **과잉 응답률**      | budget_hint "exceeds" 비율              | 높으면 → 기본 버짓 조정   |

### 현재 값

```
호출당 평균: 1,420 토큰 (102,260 / 72)
Read 대비:   추정 2-5x 절약 (동일 작업 A/B 미실시)
과잉 응답:   미측정 (budget_hint 로그 필요)
```

### 다음 액션

- [ ] 동일 작업 A/B 테스트: CodeLens 도구 vs Read+Grep으로 같은 질문 해결
- [ ] budget_hint "exceeds" 로그를 텔레메트리에 추가
- [ ] \_profile별 토큰 분포 분석

---

## 데이터 수집 체크리스트

매 개발 세션 종료 시:

```bash
# 1. 세션 텔레메트리 수집
./benchmarks/collect-session.sh . session-$(date +%Y%m%d)

# 2. 코드 변경이 있었다면 벤치마크
cargo build --release
./benchmarks/run-benchmark.sh . $(git rev-parse --short HEAD)

# 3. 이전과 비교
./benchmarks/compare.sh results/baseline.md results/$(ls -t results/ | head -1)

# 4. 이 파일의 "현재 값" 업데이트
# → METRICS.md의 해당 섹션 수치 갱신
```

## 의사결정 트리

```
세션 텔레메트리 분석
  │
  ├─ 미사용률 >70%
  │   └─ 먼저: 세션 유형 확인 (코드 수정 / 리뷰 / 온보딩 / 리팩터링)
  │   └─ 3개 이상 다른 유형에서 0회인 도구만 축소 후보
  │   └─ 1개 세션 데이터로 절대 축소 결정 금지
  │
  ├─ 에러율 >5%    → 해당 도구 버그 수정
  ├─ 집중도 <40%   → 도구 설명 재설계
  │
  ├─ onboard 시간 >10s
  │   └─ 먼저: 첫 호출 vs 이후 호출 분리 (모델 캐시 효과)
  │   └─ 첫 호출만 느리면 → pre-warm 또는 lazy 유지 (정상)
  │   └─ 매 호출 느리면 → 단계별 트레이싱 후 병목 분리
  ├─ find_refs >150ms  → non-code 캐시
  ├─ 첫 호출 >500ms    → auto-index 최적화
  │
  ├─ MRR@5 <0.4        → 임베딩 모델 교체
  ├─ MRR@5 0.4-0.6     → 파인튜닝 검토
  ├─ MRR@5 >0.6        → 모델 유지
  │
  └─ dispatch >80      → 도구 합병
     tool_defs >400LOC → 파일 분리
     AppState >15필드  → 서브 구조체
```
