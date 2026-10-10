# 코드 인텔리전스 랜드스케이프 델타 — 2026-07-03 이후 (2026-10-10)

[2026-07-03 딥리서치](landscape-deep-research-2026-07-03.md)의 후속이다. 그 뒤 석 달 동안 레퍼런스 프로젝트, 호스트(Claude Code), 연구, 기술 스택에서 바뀐 것을 오늘 실측한 CodeLens 약점에 대어 본다. 표기: **[검증]** = 이 문서 작성자가 원문을 직접 읽었거나 로컬에서 실행해 확인, 표기 없음 = 조사원 보고(1차 출처 링크 포함, 미재검증), **[U]** = 출처 없는 추론.

## 요약

1. **MCP 2026-07-28 스펙이 확정됐다**. 프로토콜 세션과 `Mcp-Session-Id`, `initialize` 핸드셰이크가 없어지고 `server/discover`가 필수다 [검증]. Claude Code는 2.1.274부터 이 버전을 기본으로 협상한다. CodeLens는 2025-11-25까지만 지원하고 `server/discover`에 `-32601`을 돌려준다 [검증]. 지금은 하위 호환 대체 경로로 동작하지만, 세션 id에 기대는 바인딩·세션 저널(#421)은 장기적으로 핸들·헤더 기반으로 옮겨야 한다.
2. **채택률 문제는 업계 공통이다**. Serena는 실사용 세션의 35.4%만 썼고, codegraph는 3% 미만이다. 관리자도 "도구 설명·지시문은 낮은 현저성 채널"이라고 인정한다. CodeLens의 1%는 결함이 아니라 공통 과제이며, 레버는 설명 문구가 아니라 응답 충분성, 검색 서브에이전트, CLI 경로다.
3. **번들 임베딩 모델 교체를 다시 검토할 근거가 생겼다**. 독립 비교표(MTEB-Code v1)에서 GTE-ModernBERT-base(149M, Apache-2.0)가 71.66, 7월 후보 CodeRankEmbed가 60.47이다. 오늘 측정에서 `to_string`이 자기 문서 문장 질의에 118위였다. 다만 7월 미세조정은 promotion gate에서 기각됐으므로, 교체도 같은 게이트를 통과해야 한다.
4. **sqlite-vec의 12바이트 결함은 업스트림 PR #329가 정확히 고친다** [검증]. 아직 미머지(10-07)라 #424의 우회는 유지한다.
5. **"인덱스는 죽었다"는 주장은 이 기간 연구로 반박됐다**. 벡터 검색이 깊은 에이전트 탐색보다 65.2% 대 46.2%로 정확하고 비용은 절반 이하였다(arXiv 2608.01507). 하이브리드에 구조 신호를 섞으면 MRR이 0.2296에서 0.2713으로 올랐다(arXiv 2607.24882). CodeLens의 하이브리드+그래프 방향은 유효하다.

## 1. 레퍼런스 프로젝트 현황

| 프로젝트 | 최신 | ★ (10-10 스냅샷) | 07-03 이후 변화 | CodeLens에 주는 것 |
| --- | --- | --- | --- | --- |
| [oraios/serena](https://github.com/oraios/serena) | v1.7.0 (08-09), main 미배포분 다수 | 30,140 | main에서 **앱 코드 GPL-3.0 재라이선스**(SolidLSP는 MIT, CLA 필요). `serena_repl` 단일 도구(베타). 파일·메모리 원자적 쓰기, tsserver 충돌 시 빈 결과 대신 예외, 세션 id 자체 발급("MCP SDK v2가 더는 세션 id를 주지 않아서") | 코드 이식 금지(GPL). 실사용 데이터 #1491: 세션 35.4%만 사용, 심볼 질의 뒤 18.4%가 전체 파일 Read → `context_lines` 옵션과 1-based 라인 |
| [colbymchenry/codegraph](https://github.com/colbymchenry/codegraph) | v1.6.2 (10-03) | 73,631 | Rust 파스 커널(20언어), vm_stat 메모리 사이징(#1388), 공유 데몬 + 단일 라이터로 **CodeLens 구조에 수렴**. 이미 보낸 소스 재전송 생략, 순위 전용 `deprioritize` | 이미 보낸 결과의 포인터 응답(`index_snapshot` 활용), `deprioritize` 글롭. 함정: 진행 기준이 아닌 벽시계 워치독이 정상 데몬을 죽임(#2404) |
| [zilliztech/claude-context](https://github.com/zilliztech/claude-context) | 0.1.14 (06-08) | 12,594 | 사실상 정체. 상위 이슈: 색인됐는데 "미색인", OpenAI 키 필수 여부, 오프라인 | 채택할 것 없음. "키 없이·오프라인·자동 동기화"는 CodeLens가 이미 충족 |
| [Aider-AI/aider](https://github.com/Aider-AI/aider) | v0.86.0 (2025-08) | 49,442 | 휴면(마지막 커밋 2026-05-22) | repo-map 알고리즘 참조는 여전히 유효, 새 것 없음 |
| [GlitterKill/scip-io](https://github.com/GlitterKill/scip-io) | v0.2.6 (09-29) | 10 | scip-python 멈춤 패치가 대부분 | SCIP-Python은 시간 제한을 두고 선택 기능으로 |
| karellen-lsp-mcp | v0.2.0 (09-30) | 3 | MCP Python SDK v2(2026-07-28)로 이행 | 생태계 이행 신호 |

### 새로 부상한 레포 (07~10월, ★ 순)

| 레포 | ★ | 최신 | 요점 |
| --- | --- | --- | --- |
| Graphify-Labs/graphify | 125,080 | v0.9.84 (10-10) | 슬래시 커맨드형 지식 그래프(코드+문서+PDF), "벡터 인덱스 아님". MCP가 아니라 스킬로 배포. 별 수는 젊은 레포치고 이례적이라 신뢰도 중간 |
| abhigyanpatwari/GitNexus | 47,815 | v1.6.12 (09-12) | 엔터프라이즈 코드 지식 그래프, **PolyForm Noncommercial**(비 OSS) |
| DeusData/codebase-memory-mcp | 46,254 | v0.11.0 (09-15) | C, 162언어, Cypher 질의, 계정 단위 공유 데몬. 자체 논문(arXiv 2603.27277): 답 품질 83% 대 파일 탐색 에이전트 92%, 토큰 약 10배 절감 |
| MinishLab/semble | 6,191 | v0.6.2 (10-05) | 정적 Model2Vec(potion-code-16M) + BM25 + RRF + 코드 인지 재정렬, 질의 ~1ms. **공개 벤치 하네스**와 설치형 검색 서브에이전트 |
| zzet/gortex | 1,883 | v0.64.7 (10-04) | **가장 가까운 쌍둥이**(데몬, 100+ 도구). 재정렬 신호: 간선 출처 감쇠, 생성 파일 강등, 소스>테스트, 키워드 수프 감지, 동의어 클래스 확장, HITS, 적응형 알파 융합. 압축 와이어 포맷(자칭 −27% 토큰) |
| redhat-et/ripwire | 2,430 | v0.6.5 (09-27) | C++ CLI+MCP: 랭크된 호출 그래프, 영향 범위, `--edit-check`(호출부 계약 검사), `--quality-delta`(나빠진 것만 보고) |
| bartolli/codanna | 754 | v0.16.0 (08-29) | Rust, 수신자 타입 기반 호출 해석, 간선별 정밀도 판정 예시 |

정체·제외: isaacphi/mcp-language-server(마지막 푸시 03-01), claude-context.

## 2. 호스트(Claude Code) 변화

- **MCP 2026-07-28 기본 협상**: 2.1.274부터 HTTP 서버, 2.1.292부터 stdio 서버가 기본(해제 `MCP_PROTOCOL_NEGOTIATION=legacy`). 2.1.238에서 `server/discover`를 `initialize` 앞에 보내는 동작이 확인됨. [검증] 이 세션의 Claude Code는 2.1.296이고 CodeLens 도구는 정상 동작한다. 데몬에 `server/discover`를 직접 보내면 `-32601`이 오고, `initialize` 없이 `_meta` 버전을 실은 `tools/list`는 정상 응답한다(이미 `ttlMs`·`cacheScope` 포함). 적용 범위(전 설치 vs 일부 세션)는 두 조사원 보고가 엇갈린다.
- **도구 로딩**: 도구 검색이 기본이다. 설명·서버 지시문 상한 2,048자는 `CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH`로 조정 가능하고, 2.1.296에서 선적재 상한이 4,096자로 올랐다. 서버의 `alwaysLoad:false`는 모든 도구를 지연 로딩한다. 최상위 `anyOf/oneOf/allOf` 입력 스키마는 평탄화되고, 2020-12 검증에 실패한 도구는 **제외**된다.
- **네이티브 LSP 도구**: 여전히 플러그인 기반(언어당 로컬 서버 1개)이다. 2.1.288에서 시간 제한과 멈춤 수정, 서브에이전트 사용이 들어갔다. 파일 간 참조·영향·아키텍처는 여전히 CodeLens 영역이다.
- **Anthropic 입장 글**("How Claude Code works in large codebases", HN 48144494): 에이전트형 grep은 인덱스가 필요 없다는 입장이다. turbopuffer 측 발표에서는 기본 정밀도 ~65%가 창 단위 읽기와 의미 도구로 ~87%가 됐지만, "모델이 추가 의미 도구를 언제 쓸지 확실히 알지 못한다"고 했다.

## 3. 연구 동향

### 임베딩 모델 (≤600M, 로컬)

유일한 독립 비교표는 LightOn의 MTEB-Code v1(작성자 자체 실행, 신뢰도 중간)이다. 모델 카드의 자체 수치는 벤치가 서로 달라 직접 비교하지 않는다.

| 모델 | 크기·차원 | 라이선스 | MTEB-Code v1 | 비고 |
| --- | --- | --- | --- | --- |
| CodeRankEmbed (7월 후보) | 137M | MIT | 60.47 | 질의 접두어 필수, `trust_remote_code` |
| EmbeddingGemma-300M | 300M, 768d(MRL) | **Gemma 약관**(허용형 아님) | 68.76 | |
| **GTE-ModernBERT-base** | 149M, 768d, 8192 ctx | Apache-2.0 [검증] | 71.66 | **CLS 풀링** [검증], 접두어 없음 [검증], ONNX 양자화 파일은 카드에서 미확인 [검증] |
| Qwen3-Embedding-0.6B | 600M, 1024d(MRL→384 가능) | Apache-2.0 | 75.42 | 질의 지시문, last-token 풀링·왼쪽 패딩, 공식 ONNX 없음 |

- 이슈→편집 위치 찾기(CORE-Bench, arXiv 2606.11864): 영샷에서 Qwen3-0.6B가 CodeRankEmbed를 이긴다(NDCG@10 17.0 대 12.1). **실제 PR에서 캔 쌍**으로 미세조정하면 26.5까지 오른다. 7월 기각은 LLM 합성 질의였으므로 그 실험을 합성 데이터로 다시 열지는 않는다.
- 07~10월에 새로 나온 허용형 소형 코드 전용 임베더는 찾지 못했다. 가장 최근 것은 LateOn-Code(2월, 멀티벡터)다.
- "에이전트가 행동하려면 어떤 맥락이 필요한가"(arXiv 2607.09691): 원본 코드는 45개 탐침 중 27개, 자연어 요약은 약 4개를 맞혔다. 검색이 아니라 행동 측정이지만, NL·카드 보강에 기대는 설계에 대한 경고다.

### 재정렬기

gte-reranker-modernbert-base(149M, Apache-2.0)는 자기 바이인코더 대비 자체 보고 +0.7에 그친다. Qwen3-Reranker-0.6B는 쌍마다 LLM 점수라 핫 패스에 무겁다. 근거가 얇아 우선순위를 낮춘다.

### 에이전트 검색·벤치

- 인덱스 무용론 반박: arXiv 2608.01507(벡터 검색 65.2% 대 깊은 에이전트 탐색 46.2%), arXiv 2607.24882(Qwen3-8B와 RepoMap의 무학습 RRF가 MRR 0.2296→0.2713).
- ContextBench(arXiv 2602.05892): 정교한 스캐폴딩의 이득은 미미하고, LLM은 정밀도보다 재현율을 선호한다.

## 4. 기술 스택

| 구성요소 | CodeLens | 최신 | 판단 |
| --- | --- | --- | --- |
| sqlite-vec | 잠금 0.1.9, 명세 `0.1.8-alpha.1` [검증] | 안정 0.1.9, 0.1.10은 alpha.4(05-18)까지 | 우회 유지. PR #329 추적 [검증]. 명세를 `0.1.9`로 고정할 것. alpha 미채택(DiskANN 삭제 누수) |
| fastembed-rs | 5.13.4 | 7.1.1 (10-07) | 모델 교체 때만 함께. v6 `Error` 열거형화, v7 `InitOptions` 폐지 예고. ort rc.13을 끌어오며, 실행 제공자가 cargo 기능으로 갈려 CoreML 경로를 명시해야 함 |
| ort | rc.12 | rc.13 (안정 2.0 없음) | rc.11이 Intel macOS를 빼고 최소 macOS 13.4로 올림 |
| tree-sitter | 0.25.10 | 0.27.1 (10-08) | 보류. 0.26.9의 메모리 안전 수정(UTF-16 과독)은 0.25에 없음. 문법 크레이트 약 35개를 함께 올려야 해 비용 큼 |
| rusqlite | 0.32.1 | 0.40.2 | 보류. 네 단계 파괴적 변경(hooks 소유 연결 등). SAVEPOINT 주입 수정은 해당 코드 없음 [U] |
| rmcp | 비의존 | 3.5.1 | 참고만(2026-07-28 대응 구현 사례) |

## 5. 실측 약점과 근거 매핑

| 약점(오늘 실측) | 관련 근거 | 방향 |
| --- | --- | --- |
| 바인딩·세션 저널이 `Mcp-Session-Id`에 기댐 | 2026-07-28 세션 제거 [검증], Claude Code 기본 협상, Serena도 세션 id 자체 발급으로 전환 | 헤더·서버 발급 핸들 바인딩으로 이행 |
| 번들 모델 변별력(`to_string` 118위, 1위 코사인 0.74 대 0.65) | MTEB-Code 표, CORE-Bench | 교체 실험(같은 게이트) |
| 최종 순위가 이름 겹침 재정렬에 좌우 | Gortex 재정렬 신호, Semble 코드 인지 재정렬 | 신호별 MRR 게이트 실험 |
| Claude 측 사용 1% | Serena 35.4%, codegraph <3%, turbopuffer 발표 | 응답 충분성, 검색 서브에이전트, CLI |
| sqlite-vec 12바이트 결함 | PR #329 [검증] | 우회 유지, 머지 시 재평가 |
| serde 스모크 적색 | 외부 라벨은 질의의 의미 목표를 검증해야 한다는 레포 자체 원칙(`product-readiness.md`) | 단일 질의 기대치를 Semble식 질의 세트로 대체 검토 |

## 6. 추천표 (중요도순)

| 순위 | 항목 | 근거 | 비용 | 위험 | 첫 단계 |
| --- | --- | --- | --- | --- | --- |
| 1 | MCP 2026-07-28 대응 1단계: `server/discover`, 결과의 `resultType`, `_meta` 버전·클라이언트 정보 수용, `UnsupportedProtocolVersionError` (`tools/list`는 이미 결정적 순서·`ttlMs`·`cacheScope` [검증]) | 스펙 필수 항목 [검증], Claude Code 기본 협상 | S | 낮음(가산적, 레거시 유지) | `protocol.rs`에 2026-07-28 추가하고 discover 핸들러 |
| 2 | MCP 2단계: 세션 없는 바인딩(헤더/핸들), 세션 저널 키를 세션 id에서 클라이언트 정보·핸들로, `Mcp-Method`/`Mcp-Name` 헤더 | 세션 제거 [검증] | M~L | 중간(바인딩 핵심 경로) | K-0027 설계 문서 갱신 |
| 3 | 임베딩 Track 0: 문서 요약을 카드 앞으로, `max_length` 512 | 원시 순위 실측(카드 끄면 118→45위), arXiv 2607.09691 | S | 낮음 | 112질의와 serde 원시 순위 탐침으로 A/B |
| 4 | 임베딩 Track 1: GTE-ModernBERT-base 교체 실험 | MTEB-Code 71.66 대 CodeRankEmbed 60.47, Apache-2.0 | M~L | 중간(384→768차원 재임베딩, CLS 풀링 지원, 34MB→~150MB) | fastembed `UserDefinedEmbeddingModel`의 CLS 풀링 지원과 ONNX int8 존재부터 확인 |
| 5 | 채택률 레버: `context_lines`·1-based 라인, 이미 보낸 결과 포인터, 검색 서브에이전트 정의, CLI 경로 | Serena #1491, codegraph #914, Semble | M | 중간(효과 미입증) | 텔레메트리에서 "심볼 조회 뒤 전체 Read" 비율부터 측정 |
| 6 | Gortex식 재정렬 신호(생성 파일 강등, 소스>테스트, 키워드 수프 감지) | Gortex | M | 중간(과거 튜닝 기각 이력) | 신호 1개씩 MRR 게이트 |
| 7 | 외부 대조 하네스: Semble `benchmarks/`로 현 모델과 potion-code 비교, serde 단일 기대치를 질의 세트로 | Semble, 레포 원칙 | S~M | 낮음 | 하네스 실행 |
| 8 | 작은 것: 순위 전용 `deprioritize` 글롭, 지연 로딩 도구까지 최상위 조합자 입력 스키마 점검(기본 노출 20개는 없음 [검증]), `.gitignore` 메타문자 디렉터리 감사, 쓰기 원자성 확인, sqlite-vec 명세 `0.1.9` 고정 | 각 출처 | S | 낮음 | 묶어서 한 PR |
| 9 | 스택: tree-sitter 0.26.9 메모리 안전 수정 검토(문법 동반), fastembed 7은 4번과 함께, rusqlite 보류 | 릴리스 노트 | M~L | 중간 | 보류, 4번 착수 때 재평가 |

채택하지 않음: Serena 코드(GPL-3.0), `serena_repl`(RBAC·dry-run 게이트와 충돌), GitNexus(비상용 라이선스), Merkle 재색인(워처가 이미 증분), Qwen3-Reranker 핫 패스, EmbeddingGemma(Gemma 약관), jina 계열(CC-BY-NC).

## 7. 정정과 한계

- **정정**: 이 조사를 의뢰할 때 "최대 코사인 ~0.27"이라고 적었으나, 그 값은 `1 − L2 거리`였다. 벡터는 정규화돼 있으므로(노름 1.0) 실제 코사인은 1위 0.67~0.74, `to_string` 0.54~0.65다 [검증]. 순위 결론(252위·118위)은 그대로다. 그 숫자에 기댄 "파이프라인 결함" 가설은 근거가 약하다.
- Reddit은 검색 도구가 도메인을 막아 수집하지 못했다(부재의 증거 아님). X는 검색하지 않았다.
- 별 수는 10-10 단일 스냅샷이고 추세 자료가 없다.
- 라이브 CoIR 리더보드, LocBench·SWE-bench Live/Pro 변화, libSQL·LanceDB·usearch 비교, MCP Registry는 다루지 못했다.
- 이 문서의 수치는 대부분 작성자·벤더 자체 측정이다. 채택 전 1차 출처를 다시 확인할 것.
