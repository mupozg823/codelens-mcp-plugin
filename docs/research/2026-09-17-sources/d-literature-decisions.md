# CodeLens 결정용 문헌 체크 — 2026-09-17

기존 랜드스케이프의 Anthropic tool-search 수치(Opus4 49%→74%, Opus4.5 79.5%→88.1%, 85% 컨텍스트 절감)·
RAG-MCP 블로그 인용은 중복 언급 없이 원문 확인 결과로 격상하거나 새 소스만 추가한다.

## D1. 도구 표면 크기 vs 선택 정확도/비용

| 주장 | 출처 | 검증 | 수치 | 함의 | 확신도 |
|---|---|---|---|---|---|
| RAG-MCP: 검색 기반 선택이 baseline 대비 정확도 3배+, 토큰 50%+ 절감 | RAG-MCP, arXiv 2505.03275 (2025-05-06) | fetched | 43.13% vs 13.62% baseline(초록 원문, 기존 43%/14% 블로그 인용을 1차 출처로 승격) | keep: verb facade+지연 로딩이 이미 같은 방향 | high |
| MCP 서버 수 늘리는 "stress test"에서 툴 30개 미만 성공률 90%대, ~100개부터 정밀도 급락 | RAG-MCP §4.1 본문 | fetched(본문) | 정성적 추세만; 중간값 미표·서버수 스케일 표기 렌더링 신뢰도 낮아 인용 보류 | measure: 41/47종 preset이 "30 미만" 구간인지 자체 계측(기존 갭#2와 동일) | med |
| MCP-Bench: 28서버·250도구, 20 LLM 모두 "지속적 난제", 도구수-정확도 곡선은 미보고 | MCP-Bench, arXiv 2508.20453 | fetched(초록) | 스케일링 곡선 없음("없다"를 확인) | 태스크 복잡도 참고용, D1 감소곡선 근거로 못 씀 | med |
| ToolRet: 7.6k 태스크·43k 도구, 표준 IR 모델 성능 붕괴 | ToolRet, arXiv 2503.01763(ACL Findings 2025) | fetched(초록) | 정성적 확인만, 정량 격차 미기재(본문 미열람) | FTS5+ONNX 하이브리드가 순수 IR보다 코드에 유리한지 별도 측정 필요 | low |
| MCPToolBench++/MCP-Universe/LiveMCPBench 존재만 확인 | 검색 스니펫만 | UNVERIFIED | — | 후속 사이클 후보 | low |

**종합**: "도구 많을수록 정확도 급락" 방향은 재확인되지만 정확한 임계값은 소스마다 다르고 RAG-MCP 본문 스케일
표기는 신뢰도가 낮다. 41/47종 preset이 위험구간 상단이라는 기존 결론(K-0028/갭#2)은 유지, 새 논문은 방향성만 보강.
**기존 문서 정정**: `c-mcp-harness-guidance.md` #8의 "43%→14% 미만 하락" 서술은 방향이 반대(43.13%=제안
기법, 13.62%=baseline, 방법 vs baseline 비교이지 도구수 증가에 따른 감소곡선이 아님) — 블로그가 방향을
뒤집어 인용했으므로 이 표의 원문 수치로 대체할 것.

## D2. outputSchema/설명 verbose 비용

| 주장 | 출처 | 검증 | 수치 | 함의 | 확신도 |
|---|---|---|---|---|---|
| outputSchema는 선택 필드; 서버는 MUST 준수, **클라이언트는 SHOULD(권고)** 검증 | MCP spec 2025-06-18, `server/tools` | fetched(원문 인용) | "Servers MUST provide structured results conforming to this schema. Clients SHOULD validate structured results against this schema." | integrate: listing에서 outputSchema 생략은 프로토콜 위반 아님(SHOULD) — Claude Code lean 계약은 스펙 충돌 없음, Codex가 63종 전체 스키마 받는 것을 "스펙 강제"로 오인 말 것 | high |
| 설명/스키마 길이가 도구 선택 정확도에 미치는 영향을 직접 측정한 통제 실험 | 타겟 검색 실행("description length verbosity effect on LLM tool selection accuracy") | 검색함, 해당 논문 없음 확인 | 근접 논문(verbosity bias, JSON 크기 vs 성능)은 있으나 "설명 길이 vs 도구 선택 정확도"를 직접 통제한 논문은 미발견 | 대안: RAG-MCP 토큰 50%+ 절감(D1), 기존 문서 scottspence.com 20→8도구 통합 시 60% 절감(2차 출처)뿐 | low |

**종합**: outputSchema 생략이 스펙 위반이 아니라는 점이 핵심 신규 사실. 설명 길이 vs 정확도를 직접 통제한 1차
연구는 타겟 검색 후에도 "증거 없음"으로 확정 — response_format 게이팅(K-0031)은 정성적 근거만 유효, 계측 우선.

## D3. 식별자 인식 토크나이제이션(camelCase/snake_case)

| 주장 | 출처 | 검증 | 수치 | 함의 | 확신도 |
|---|---|---|---|---|---|
| Samurai: 코드 마이닝 기반 식별자 분할, 당대 SOTA 능가 | Enslen/Hill/Pollock/Vijay-Shanker, MSR 2009 | fetched(PDF 메타데이터만, 바이너리 압축으로 표 수치 추출 불가) | 저자/venue/연도 확인, 정확도 수치는 UNVERIFIED | keep: identifier-splitting 계보의 고전 근거, 정량 재인용은 피할 것 | med(존재 high, 수치 low) |
| identifier-aware 토크나이징이 BM25 코드 검색의 근본 레버 — 적용 시 저자들의 q-log-odds BM25 보정 기법이 주는 추가 이득이 거의 사라짐(ablation) | Radha & Goktas, "Improving BM25 Code Retrieval Under Fixed Generic Tokenization", arXiv 2605.18561, 2026-05-18 | fetched(제목/저자/날짜 원문 확인) | +89.3%(NDCG@10 0.2575→0.4874)는 **generic 토크나이저 고정 하에서의 q-log-odds 기법 자체의 개선폭**이지, identifier 분할 유무의 직접 비교치가 아님 — ablation 문구("identifier-aware tokenization largely removes the incremental gain from q-IDF")는 방향성 추론일 뿐 | measure: FTS5 토크나이저의 camelCase/snake_case 분할 여부부터 확인 — 안 쪼개면 검색품질 개선 레버일 가능성(정량 아님, 방향성) | med |
| codebase-memory-mcp의 camelCase 인지 FTS5 토크나이저는 GitHub 프로젝트로만 존재, arXiv 프리프린트 없음 | GitHub 검색(여러 fork) | 확인함(부재 확인, 논문 자체가 없음) | — | 학술 근거 인용 불가, 구현 사례(prior art)로만 참고 | high(부재 확인) |

**종합**: identifier-splitting이 코드 검색에 이득이라는 방향은 2009 고전과 2026 ablation 문구가 함께
가리키지만, 2026 논문의 +89.3%는 분할 자체의 측정치가 아니라 저자 기법의 개선폭이므로 그 숫자를 분할 효과로
재인용하지 말 것. FTS5 토크나이저 분할 여부 확인은 여전히 실행 가능성 높은 항목(정성적 근거로).

## D4. 그래프 기반 코드 로컬라이제이션

| 주장 | 출처 | 검증 | 수치 | 함의 | 확신도 |
|---|---|---|---|---|---|
| LocAgent: import/invocation/inheritance 3종 그래프, 파일레벨 로컬라이제이션 | LocAgent, arXiv 2503.09089 | fetched(초록) | 파일레벨 92.7%, Pass@10 +12%, 32B 오픈소스로 상용 대비 비용 ~86%↓ | keep: import+call graph가 핵심, CodeLens와 계보 일치 | high |
| CoSIL: 모듈 콜그래프→함수 콜그래프 2단계 확장 | CoSIL, arXiv 2503.22424(ASE 2025) | fetched(초록) | top-1 로컬라이제이션 Lite 43.3%/Verified 44.6%, 이전 SOTA 대비 ~96%↑; Agentless 통합 시 해결율 +2.98~30.5%p | keep: "파일→함수" 전개가 overview→search(mode=refs)와 동일 패턴 | high |
| OrcaLoca: 우선순위 스케줄링+관련도 점수+거리기반 가지치기 | OrcaLoca, arXiv 2502.00350(ICML 2025) | fetched(초록) | SWE-bench Lite 함수매치율 65.33%(당시 SOTA), 해결율 +6.33pp | integrate 후보: 거리기반 가지치기를 결과 랭킹/절단 로직에 참고 가능 | med |
| RepoGraph: 레포 레벨 그래프 플러그인, 4방법·2접근에서 개선 | RepoGraph, arXiv 2410.14684 | fetched(초록만, 그래프 세부구조 미기재) | "오픈소스 SOTA" — 정량 수치 없음 | 참고만, 세부 스키마는 본문 미열람 | low |
| SweRank/Agentless | 검색 스니펫만, 미열람 | UNVERIFIED | — | 후속 후보 | low |

**종합**: 4개 논문(LocAgent/CoSIL/OrcaLoca/RepoGraph) 모두 call graph+import/module graph를 로드베어링
신호로 삼는다. CodeLens 그래프 기능은 축소 대상 아님 — 없는 것은 inheritance graph(LocAgent만)와 거리기반
가지치기류 랭킹 정교화.

## D5. ACI 응답 설계(concise vs verbose, 절단, 가드레일)

| 주장 | 출처 | 검증 | 수치 | 함의 | 확신도 |
|---|---|---|---|---|---|
| 파일 뷰어 창 크기: 100줄 최적, 30줄/전체 파일 모두 하락 | SWE-agent ACI, arXiv 2405.15793(NeurIPS 2024) | fetched(html Table 3) | SWE-bench Lite: 100줄 18.0%, 30줄 14.3%(-3.7pp), 전체 파일 12.7%(-5.3pp) | measure: `get_analysis_section` 슬라이싱이 과소/과다 양극단을 피하는지 확인 | high |
| lint 통합 edit 커맨드가 성능 기여 | 동일 논문 | fetched | lint 있음 18.0% vs 없음 15.0%(-3.0pp) | integrate: mutation gate 설계 방향 일치 | high |
| bash-only(무 ACI) vs 전용 ACI | 동일 논문 | fetched | bash-only 11.0% vs 전체 ACI 18.0%(상대 +64%) | keep: 전용 인터페이스가 raw shell 대비 실질 이득이라는 1차 근거 | high |
| Claude Code `ResponseFormat concise/detailed`, 응답 25,000토큰 캡 | Anthropic writing-tools-for-agents(기존 문서 원문 확인 재사용) | fetched(재사용) | 25,000 토큰 캡 + concise/detailed 2단 | measure(K-0031): 5단계 압축이 동등 효과인지 계측 후 결정 | high |

**종합**: SWE-agent 수치는 "짧을수록 좋다"가 아니라 과소/과다 둘 다 벌점인 U자형 근거 — 응답 슬라이싱 크기
튜닝에 적용 가능한 유일한 정량 1차 근거.

## D6. 인덱스 신선도/오래된 답변

| 주장 | 출처 | 검증 | 수치 | 함의 | 확신도 |
|---|---|---|---|---|---|
| stale 레포 컨텍스트만 주면 모델이 구식 코드를 그대로 참조 | Weng, Yang, Fu, Pan, Lv, "When Retrieval Hurts Code Completion: A Diagnostic Study of Stale Repository Context", arXiv 2605.14478, 2026-05-14 | fetched(제목/저자/날짜 원문 확인) | stale-only에서 obsolete 참조율 Qwen2.5-Coder-7B 88.2%, gpt-4o-mini 76.5%(현재-정보 대비); 무검색은 stale 참조 0%지만 17개 중 1개만 테스트 통과 | integrate: per-file staleness 배너/커밋해시 헤더가 논문이 제안하는 완화책과 동일 | high(진단 연구, n=17/5레포 소규모) |
| 완화책: 인덱스 무효화 윈도우, 최신-근거 재확인 프로브 | 동일 논문 | fetched | 정성적 제안, 정량 효과치 없음 | measure: "현재 근거 병기시 stale 실패 대부분 회복" 주장은 CodeLens의 기존 stale 안내와 같은 방향 | med |
| 대규모 코딩 에이전트 실패 분석(20,574세션)은 존재하나 staleness 특화 수치는 초록에서 미확인 | arXiv 2605.29442, "How Coding Agents Fail Their Users" | fetched(초록만, staleness 항목 여부 확인 불가) | 7가지 반복 실패 유형 언급, staleness가 그중 하나인지는 본문 미열람으로 불명 | 확인 불가 항목으로 유지, "대규모 연구 자체가 없다"고 단정하지 말 것 | low |

**종합**: 소규모 직접 근거(2605.14478) 하나는 확보. 대규모 연구가 존재하지 않는다는 뜻은 아니며 — 2605.29442처럼
관련 후보가 있으나 staleness 관점 확인은 이번 예산에서 못 했다("확인 불가"이지 "증거 없음"이 아님). CodeLens의
stale 취급 정책은 방향은 일치하나 과신 금지(n=17).

## 이번 사이클에 바로 쓸 수 있는 결론

- D3: FTS5 토크나이저의 camelCase/snake_case 분할 여부 확인이 최우선 — 2026 논문(2605.18561)의 ablation이 "identifier-aware 토크나이징이 방향성 있는 이득"이라 시사(단, +89.3%는 분할 효과 수치가 아니라 저자 기법 자체의 개선폭이므로 재인용 금지). **measure→분할 안 되어 있으면 즉시 fix.**
- D2: outputSchema 생략은 MCP spec 2025-06-18 원문상 위반이 아님("SHOULD" 검증) — Claude Code lean 계약을 스펙 미준수로 오인하지 말 것. **integrate as 안전한 현행 유지.**
- D5: 응답 슬라이싱은 "짧을수록 좋다"가 아니라 U자형(SWE-agent 100줄 vs 30줄 vs 전체 파일) — `get_analysis_section` 크기 튜닝 시 극단 배제 원칙으로 삼을 것. **measure 현재 슬라이스 크기 분포.**
- D5: lint 가드레일 있는 edit이 없는 것보다 3.0pp 우위(1차 확인) — mutation gate의 `verify_change_readiness` 사전 검증 설계를 계속 유지·강화할 근거. **keep.**
- D4: call graph + import/module graph는 4개 독립 논문(LocAgent/CoSIL/OrcaLoca/RepoGraph)에서 공통 로드베어링 신호 — CodeLens 그래프 기능은 축소 대상 아님. **keep.**
- D1: 도구 수-정확도 감소곡선의 정확한 임계값은 논문마다 다르고 RAG-MCP 본문 수치 표기 자체 신뢰도가 낮음 — 41/47종 preset이 위험구간에 있는지는 문헌이 아니라 **자체 계측**으로만 확정 가능(기존 K-0028/갭#2와 동일 결론, 문헌은 방향성만 보강).
- D6: stale 인덱스 완화책(프레시니스 헤더, 현재-근거 병기)은 소규모 진단 연구(n=17) 1건뿐 — 대규모 실증 부재는 "확인 불가"이지 "없다"가 아님, 과대 인용 금지.
- D2/설명 길이: 타겟 검색까지 실행 후에도 통제된 1차 연구 자체가 없음(확정된 "증거 없음") — response_format 게이팅(K-0031) 판단은 여전히 계측 우선.

## 증거 없음 / 확인 불가

- D1: MCPToolBench++/MCP-Universe/LiveMCPBench 세부 수치 — 검색 스니펫만, 원문 미열람("확인 불가").
- D2: 설명/스키마 길이가 도구 선택 정확도에 미치는 영향을 직접 측정한 통제 실험 논문 — 타겟 검색 실행 후 발견하지 못함("증거 없음", 확정).
- D3: Samurai(2009) 논문의 정확한 정량 수치(분할 정확도 %) — PDF 바이너리 파싱 실패로 추출 불가("확인 불가", 존재 자체는 확인).
- D4: RepoGraph의 구체적 그래프 스키마, SweRank·Agentless의 정량 수치 — 초록 수준만 확인, 본문 미열람("확인 불가").
- D6: 산업/대규모 스케일의 stale-index 실증 연구 — 타겟 검색으로 후보(2605.29442, 20,574세션)는 찾았으나 staleness 특화 여부는 초록만으로 불명("확인 불가", "증거 없음"과 구분).
