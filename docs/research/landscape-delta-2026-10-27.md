# 코드 인텔리전스 랜드스케이프 델타 — 2026-10-10 이후 (2026-10-27)

[2026-10-10 델타](landscape-delta-2026-10-10.md)의 후속이다. 이번에는 조사로 끝내지 않고, 조사가 지목한 레버를 같은 날 CodeLens 평가 세트에 걸어 채택과 기각을 정했다. 표기: **[검증]** = 원문이나 소스를 직접 열어 확인, **[측정]** = 로컬에서 실행한 결과, 표기 없음 = 조사원 보고(1차 출처 링크 포함, 미재검증), **[U]** = 출처 없는 추론이나 미확인.

측정 방법(모든 [측정] 공통): 코퍼스는 `git archive 4ffd5d90` 스냅숏이다. 색인을 한 번 만든 뒤 변형마다 같은 색인을 재사용했다. 바꾸지 않은 바이너리를 같은 색인에서 다시 돌린 null 실행은 모든 지표가 소수 넷째 자리까지 같았다. 따라서 아래의 차이는 실행 간 잡음이 아니다.

## 요약

1. **이번 사이클의 이득은 새 모델이 아니라 기존 파이프라인 결함에서 나왔다** [측정]. `semantic_search` 어휘 레인 결함 4건(#449), 번들 임베딩 모델이 학습과 다른 CLS 풀링으로 돌던 것(#452, `get_ranked_context` MRR 0.750→0.771), FTS 동기화 결함과 camelCase 미분할(이 문서와 같은 PR)이다.
2. **다른 프로젝트가 수렴한 관례가 CodeLens 데이터에서 그대로 통하지는 않았다** [측정]. semble과 gortex가 함께 쓰는 질의 모양별 RRF 가중(식별자형 0.3, 자연어 0.5)에 해당하는 `CODELENS_RRF_ADAPTIVE=1`은 자기 세트와 역할 세트에서 모두 내려갔다. 임베딩 카드 끄기는 자기 세트만 올리고 역할·외부·업스트림을 내렸다. 자기 세트 착시는 이번 사이클에서 #451의 자기 리포 규칙까지 세 번 확인됐다.
3. **MCP 2026-07-28**: Rust SDK rmcp 3.7.0(10-22)이 dual-era 서버의 참조 구현을 갖췄다. Claude Code 2.1.302는 `resultType`·`ttlMs`·`cacheScope` 누락에 관대해졌다 [검증]. CodeLens는 여전히 레거시(`initialize`) 대체 경로로 정상 동작한다. 사용자 영향은 없지만 스펙 미준수다.
4. **모델**: GTE-ModernBERT-base의 int8 ONNX(150,218,016 B)는 표준 연산만 쓰고 CPU ORT에서 돈다 [측정]. #452 이후 CodeLens는 모델 디렉터리의 `1_Pooling/config.json`을 읽으므로, CLS 모델을 넣어도 풀링 배선은 이미 끝났다. 남은 일은 384→768 차원 배관과 승격 게이트다.
5. **채택률**: 어떤 훅·스킬·서브에이전트도 MCP 코드 도구 채택률을 올렸다는 측정 증거가 없다. codegraph는 Read 거부 훅을 A/B로 기각했다. 새로 확인된 구조적 사실이 하나 있다. 내장 Explore 서브에이전트는 CLAUDE.md 없이 시작하므로, CodeLens 라우팅 블록이 거기에 닿지 않는다 [검증, 공식 문서].

## 1. 이번 사이클 실측 결과

| 변경 | 측정 | 전 → 후 | PR |
| --- | --- | --- | --- |
| 어휘 레인: FTS5 bareword 구문 오류, bm25 rank 부호 반전(`80+rank`), 불용어, 고정 0.35 결합 척도 | 자기 112질의 `semantic_search` MRR | 0.688 → 0.707 (질의별 7승 4패) | #449 |
| 자기 리포 심볼·경로를 박은 재정렬 prior 제거 | 자기 `semantic_search` / 역할 / `get_ranked_context` / 외부·업스트림 | 0.707→0.695 / 0.699→0.683 / 0.748→0.750 / 불변 | #451 |
| 번들 모델 풀링 CLS → mean(모델 카드대로) | 자기 `get_ranked_context` MRR·recall | 0.750→0.771, 0.857→0.875 (17승 6패) | #452 |
| 〃 | 역할 62질의 `get_ranked_context` | 0.760→0.773 | #452 |
| 〃 | 외부 rg-family-clone `get_ranked_context` / `semantic_search` | 0.917 불변 / 1.000→0.938 (1질의 1→4위) | #452 |
| `symbols_fts` 트리거 동기화 | 같은 개수로 이름만 바뀐 재색인 뒤 새 이름 FTS 검색 | 0건 → 찾음 | 이 PR |
| `symbols_fts.words`: 이름의 camelCase 단어 | `semantic_search` MRR (6개 세트, 2절 끝 표) | gson 0.477→0.551, 역할 0.686→0.696, gin 0.278→0.289, rg-family 0.938→0.896(1질의 1→2위), self·jest 불변. `get_ranked_context`는 전 세트 불변 | 이 PR |

#451의 제거는 지표를 내렸지만 채택했다. 내려간 것은 자기 세트와 같은 리포를 쓰는 역할 세트뿐이다. 외부·업스트림은 그대로였고, CI 게이트 지표(`get_ranked_context`)는 올랐다. 지운 규칙들은 자기 평가만 부풀리고 있었다.

**FTS 동기화 결함**. `symbols_fts`는 `symbols`의 외부 콘텐츠 FTS5 테이블이다. 지금까지는 검색 시점에 심볼 개수가 바뀌었을 때만, 그것도 30초에 한 번만 재빌드했다. 재색인은 파일의 심볼을 지우고 새 rowid로 다시 넣는다. 그래서 개수가 같은 변경(이름 변경, 시그니처 수정)은 FTS를 없어진 rowid에 묶어 둔 채로 남겼다. 워처와 변경 후처리는 개수 표식을 지웠지만, 30초 제한 때문에 바로 다음 검색은 여전히 옛 테이블을 봤다. `refresh_symbol_index`나 워처 밖의 `refresh_all`은 표식조차 지우지 않았다. ADR-0009는 "다음 호출은 최신 데이터를 본다"고 적었지만 FTS 층에서는 사실이 아니었다. 스키마 8은 표준 insert/delete/update 트리거로 FTS를 같은 트랜잭션에서 갱신한다. `files`에서 오는 CASCADE 삭제도 트리거를 발화한다 [측정, SQLite에서 확인].

## 2. 기각한 실험과 근거

| 실험 | 출처 | 결과 [측정] | 판정 |
| --- | --- | --- | --- |
| 질의 모양별 RRF 가중 `CODELENS_RRF_ADAPTIVE=1` (식별자형은 sparse 쪽, 자연어는 semantic 쪽) | semble `ranking/boosting.py`, gortex `rerank/query_kind.go`(둘 다 0.3/0.5) | 자기 `get_ranked_context` 0.771→0.759, 자연어 0.662→0.657, `no_semantic` 0.666→0.653. 역할 0.773→0.768 | 기각, 기본값 off 유지 |
| 임베딩 텍스트에서 카드(경로·종류·시그니처 머리) 끄기 | arXiv 2607.09691(원본 코드가 요약보다 행동에 유용) | 자기 세트는 상승. 역할 ranked 0.760→0.731, 외부 ranked 0.917→0.833, next-learn 2→3위, spring-petclinic 1→2위 | 기각(자기 세트 과적합) |
| 임베딩 `max_length` 512 | 10-10 추천표 3번 | 변화 없음 | 기각 |
| FTS `words`에 시그니처·경로의 부분어까지 | semble(본문 전체 토큰) | FTS 단독 MRR, gin(Go) 0.246→0.204 | 이름만 채택 |
| 질의 쪽 camelCase 분할 | semble·gortex(질의에도 같은 토크나이저) | FTS 단독 MRR: TS·Go 동일, 자기 0.530 대 0.538 (hit@20은 86 대 85) | 넣지 않음(잡음 수준) |

`words` 사전 실험(FTS 레인 단독, 기대 심볼의 MRR. 색인 사본에 Python sqlite3 FTS5로 같은 토크나이저를 재현):

| 세트 | 언어·질의 | 기존 | 이름 camelCase 단어 | 〃 가중 0.5 |
| --- | --- | --- | --- | --- |
| self | Rust 112 | 0.532 | 0.530 | 0.531 |
| rg-family-clone | TS 12 | 0.508 | 0.694 | 0.700 |
| jest | TS 24 | 0.040 | 0.119 | 0.089 |
| gin | Go 30 | 0.246 | 0.256 | 0.288 |

종단 결과(`semantic_search`와 `get_ranked_context`, 같은 색인):

| 세트 | 언어·질의 | 기준(main, mean 풀링) | `words` 가중 1.0 | 질의별 오름/내림 |
| --- | --- | --- | --- | --- |
| self | Rust 112 | 0.7035 | 0.7032 | 0 / 1 (8→10위) |
| 역할 | Rust 62 | 0.6855 | 0.6959 | 2 / 0 |
| jest | TS 24 | 0.2569 | 0.2569 | 1 / 1 |
| gin | Go 30 | 0.2778 | 0.2889 | 1 / 0 |
| gson | Java 30 | 0.4770 | 0.5509 | 6 / 4 (상위 밖 3개가 1·1·9위로) |
| rg-family-clone | TS 12 | 0.9375 | 0.8958 | 0 / 1 (`prisma` 1→2위) |

합계는 10개 오름, 7개 내림이다. `get_ranked_context`와 `no_semantic`은 모든 세트에서 소수 넷째 자리까지 같았다. 이 경로의 어휘 레인은 FTS가 아니라 BM25F 코퍼스다. 업스트림 스모크의 기대 순위(next-learn 2, flask 1, spring-petclinic 1, serde-json은 known gap)와 fixture 스모크 3종도 같았다. 가중 0.5는 gson 0.5065, self 0.7035였고 나머지는 같았다. 그래서 FTS 기본 bm25 가중(모두 1.0)을 그대로 쓴다. 별도 랭크 설정이 없다.

gson은 데이터셋의 `expected_file_suffix`가 저장소 루트가 아니라 `gson/src/main/java/com/google` 기준이어서, 그 하위 트리를 루트로 잡아 측정했다(7절 8번).

## 3. 검색 구조: 무엇을 빌렸고 무엇을 안 빌렸나

**semble** (MinishLab/semble, MIT, 커밋 913bdac, 10-24) [검증, 소스]

- 토크나이저: 식별자마다 소문자 전체 토큰과 snake/camel 부분 토큰을 함께 낸다. CodeLens의 BM25F sparse 레인(`symbol_retrieval.rs::tokenize`)은 이미 같은 방식이었다. SQLite `symbols_fts`만 camelCase를 통째로 두고 있었고, 이 PR이 이름의 camelCase 단어 컬럼으로 메운다.
- 결합: 레인별 RRF(k=60), 의미 레인 가중은 식별자형 질의 0.3, 그 밖 0.5다. CodeLens에서는 위 2절처럼 기각됐다.
- 재정렬: 다음 다섯 가지다. 이 스택 전체가 그들 세트에서 NDCG@10을 약 +0.15 올렸다(BM25 원점수 0.686→0.838). 신호별 분해는 공개되지 않았다.
  - 같은 파일 묶음 가점
  - 식별자형 질의의 정의 가점(최고점×3.0). 후보에 없던 정의도 끌어온다.
  - 파일 이름·디렉터리 일치 가점
  - 경로 패널티(테스트·호환·예제 ×0.3, `.d.ts` ×0.7)
  - 파일 포화 감쇠 0.5
- 공개 하네스: 63개 고정 리포의 1,251질의(19개 언어, Rust는 tokio·serde·axum)다. 라벨은 Claude Sonnet 4.6이 만들고 같은 모델이 검증했다(라벨 편향 위험). 기준선 하나가 CLI를 감싸 파일 경로 순위만 돌려주면 되므로 CodeLens 기준선 추가 비용이 작다.

**gortex** (zzet/gortex, Apache-2.0, 커밋 adfbb702, 10-26) [검증, 소스]

- `alphaFuse`와 `AlphaForContinuous`도 같은 0.3/0.5 관례다. 두 프로젝트의 일치는 독립 증거가 아니다.
- 재정렬기는 21개 신호의 가중합이다(HITS, 출처 품질, 생성 파일 0.4, 소스>테스트 짝짓기, 키워드 수프 감지 등). `BENCHMARK.md`에는 bm25 행만 있고 의미·RRF·재정렬 행은 "생략"이다. 재정렬 이득의 공개 수치는 없다.

**논문** (07~10월, 초록 확인)

- arXiv 2605.18561: 식별자 전체+부분 토큰화로 CoIR-Go BM25 NDCG@10 0.309→0.563. 토크나이저 수치는 검색 요약에서 왔다 [U, 본문 미확인]. CodeLens FTS는 본문이 아니라 이름·경로·시그니처만 색인하므로 효과 규모가 다르다. 그래서 직접 쟀다(2절).
- arXiv 2609.35918: 디렉터리·파일 이름만으로 라우팅한 recall@8 0.465가 FTS5 0.352보다 높다. 경로 신호를 지지한다.
- arXiv 2609.01865 ExecRetrieval: 근중복 변형이 섞이면 exec@1이 0.331까지 떨어진다. 밀집 임베딩은 근중복을 가르지 못한다.
- arXiv 2607.01916 ContextSniper: 증거 패킷 응답으로 토큰 −51.5%. 압축 응답의 근거다.

## 4. 모델 후보

| 모델 | 크기·차원 | 라이선스 | 코드 검색 점수 | ONNX | CodeLens 적재 | 판단 |
| --- | --- | --- | --- | --- | --- | --- |
| Alibaba-NLP/gte-modernbert-base | 149M, 768d, ctx 8192 | Apache-2.0 [검증] | CoIR 20과제 평균 79.31(자체), LightOn MTEB-Code v1 71.66 | `onnx/model_int8.onnx` 150,218,016 B 등 [검증] | `UserDefinedEmbeddingModel` + CLS(`1_Pooling` 동봉 시 자동, #452). 입력 2개(`token_type_ids` 없음)라 fastembed가 그대로 처리 [검증, fastembed 소스] | 승격 게이트 후보. 384→768 차원 배관, 재임베딩, 번들 34→150 MB, 색인 약 4배 느림 [U] |
| Shuu12121/NightOwl-CodeEmbedding-35M | 34M, 384d | Apache-2.0 [검증] | MTEB(Code) 65.56 자체 평가, "약 8%만 제로샷" | `onnx/model_quantized.onnx` 34,629,831 B [검증] | 같은 경로, CLS. **차원이 같아 재임베딩만** | 가장 싼 bakeoff. 1인 프로젝트, 다운로드 116, 자체 평가 → 112질의 게이트로만 판단 |
| Shuu12121/NightOwl-CodeEmbedding-v2 | 151M, 768d | Apache-2.0 | 73.56 자체 | 없음(safetensors만) | 직접 export 필요 | 보류 |
| google/embeddinggemma-2 | 740M(텍스트 270M), 768d | 카드는 Apache-2.0, 링크는 Gemma 4 라이선스 | MTEB(code) 78.68 자체 | 없음 | export, 텍스트 타워 분리 적재 | 보류(라이선스 확인 전, 크기) |
| minishlab/potion-code-16M-v2 | 16M, 256d, 정적 | MIT [검증] | CoIR 평균 단독 39.08, BM25와 RRF 43.36, BM25 단독 42.31 | 불필요(`model2vec-rs` 0.3.0) | ORT 없이 적재 | 보조 채널 후보. 단독으로는 BM25보다 약하고 학습 언어에 Rust 없음 |
| 재정렬기(LightOn-rerank, jina v3.5, core-reranker, Nemotron-3.5) | ≥0.8B | 일부 CC-BY | — | — | 핫 패스에 무거움 | 채택 안 함 |

스택: fastembed-rs는 7.5.0(10-23)이고 `ort =2.0.0-rc.15`를 고정한다 [검증]. CodeLens는 5.13.4 + rc.12다. 모델 교체 때 함께 올리거나, 새 모델만 ORT를 직접 적재한다. potion-code-16M v1은 v2로 대체됐다 [검증].

## 5. MCP 2026-07-28

현재 상태 [검증, 소스]: `protocol.rs`는 최신을 2025-11-25로 둔다. HTTP는 지원하지 않는 `MCP-Protocol-Version`에 일부러 평문 400을 돌려준다. 그래야 클라이언트가 `initialize`로 물러선다. `server/discover`는 `-32601`로 고정된 테스트가 있다. 설계는 [`docs/design/mcp-2026-07-28-adoption.md`](../design/mcp-2026-07-28-adoption.md)(Option 1: 바인딩은 `?project=`/`x-codelens-project`만)다.

스펙 요점 [검증]:

- `DiscoverResult{supportedVersions, capabilities, instructions?}`는 `CacheableResult{ttlMs, cacheScope}`를 상속하고, 그것은 다시 `Result{resultType}`을 상속한다. 서버 정보는 결과 본문이 아니라 `_meta["io.modelcontextprotocol/serverInfo"]`에 둔다.
- 지원하지 않는 버전에는 `-32022`를 `data{supported, requested}`와 HTTP 400으로 돌려준다. 같은 계열로 `-32020` 헤더 불일치, `-32021` 필수 클라이언트 능력 누락이 있다.
- dual-era 서버는 클라이언트가 여는 방식으로 시대를 고른다. 요청에 현대식 `_meta`가 있으면 무상태로, `initialize`로 열면 레거시 세션으로 처리한다. HTTP 클라이언트는 400 본문이 현대식 JSON-RPC 오류면 현대로 판정하고 물러서지 않는다. 따라서 그 400에 JSON 본문을 싣는 것은 "현대식을 지원한다"는 선언이다.

rmcp 3.7.0에서 빌릴 것(새 코드 Apache-2.0, 미재허가 기여분 MIT):

- `handler/server.rs`의 요청별 판정과 `strip_result_type_for_legacy_peer`
- `transport/streamable_http_server/tower.rs`의 `is_legacy_request`와 HTTP 상태 매핑
- `tests/test_server_discover*.rs`

열린 질문:

- `supportedVersions`에 2026-07-28이 없는 `DiscoverResult`를 받으면 Claude Code 2.1.310이 어떻게 하는가 [U]. rmcp 클라이언트는 현대 경로로 확정하고 `initialize`를 부르지 않는다. discover는 현대 경로가 실제로 동작할 때만 응답해야 한다.
- 세션별 표면·프로필(`requested_profile`, 지연 로딩)은 "목록 응답은 연결마다 달라지지 않는다"와 충돌한다. 대체 운반체(URL 질의, 헤더, `_meta` 확장 키)를 설계해야 한다.

## 6. 채택률 파훼법

| 전술 | 근거 | 확신 | CodeLens 적용 |
| --- | --- | --- | --- |
| 훅으로 Read·Grep 거부 | codegraph A/B(팔당 2회): 거부 훅 쪽 도구 호출 8~9회·200~239초 대 훅 없음 5~7회·55~77초, 에이전트가 우회. "출시하지 말 것" [검증] | 중 | 하지 않음 |
| 부드러운 훅(정보 주입, 한 번 넛지, 첫 호출 전 잠금) | gortex `enrich`/`nudge`/`consult-unlock`, codebase-memory-mcp의 fail-open 주입 [검증, 존재]. 효과 수치는 어디에도 없음 | 낮음 | 측정 하네스가 생긴 뒤 A/B |
| Read 대체 응답(같은 줄 번호 형식, 1-based) | codegraph 설계 문서. Serena #1491: 0-based 위치를 Read offset에 옮기다 체계적 off-by-one(5/5, 3/3) [검증] | 중 | `overview mode=file`에 `cat -n` 형식과 영향 머리줄 |
| 검색 서브에이전트(`tools: Bash, Read`) + CLI + 스킬 | semble `agents/claude.md`, `skill.md`, 설치기 [검증, MIT] | 메커니즘 높음, 효과 미측정 | MCP 도구 로딩 없이 서브에이전트 경로를 연다 |
| 내장 Explore 대체 | 공식 문서: Explore는 CLAUDE.md 없이 시작하고, 같은 이름의 사용자 정의 에이전트가 대체한다 [검증] | 높음(사실), 효과 미측정 | `attach`가 라우팅 문구를 시스템 프롬프트에 담은 Explore 대체 정의를 내도록 |
| 다음 행동으로 충분성 측정 | codegraph `explore-sufficiency.md`: 탐색 뒤 다음 호출을 다시 탐색 / 반환 파일 Read / 미반환 파일 Read / Grep / 진행으로 분류 [검증, MIT] | 방법 높음 | 기존 `~/.claude/projects/*.jsonl` 분석에 그대로 적용, 제품 텔레메트리 불필요 |

Serena의 35.4%(사용자 1명, 192세션)와 codegraph의 소규모 A/B 외에는, 어떤 전술의 전후 채택률 수치도 찾지 못했다. 모든 전술은 가설이고, 측정 하네스가 먼저다.

## 7. 추천표 (다음 순서)

| 순위 | 항목 | 근거 | 비용 | 위험 | 첫 단계 |
| --- | --- | --- | --- | --- | --- |
| 1 | 외부 대조 하네스: semble `benchmarks/`에 CodeLens 기준선 (10절: Rust 3리포 착수) | 이번 사이클에서 자기 세트 착시 세 번. 외부 라벨이 rg-family 12질의뿐 | S~M | 낮음(라벨이 LLM 생성인 점은 표기) | `baselines/codelens.py`로 Rust 3리포 약 60질의 |
| 2 | 모델 bakeoff: ~~NightOwl-35M~~(10절, 기각), GTE-ModernBERT-base(768차원) | 4절. 풀링 배선은 #452로 끝남 | S~M / M~L | 중간(재임베딩, 번들 크기) | 모델 디렉터리만 바꿔 112질의·역할·외부 게이트 |
| 3 | MCP 2026-07-28 1단계(discover, `resultType`, `-32022`, 헤더 검증) | 5절, rmcp 참조 구현 | M | 낮음~중간(가산적, 레거시 유지) | 설계 문서 Option 1 체크리스트 |
| 4 | 채택 측정 하네스(다음 행동 분류) 뒤 Explore 대체 정의 A/B | 6절 | S~M | 낮음 | 기존 transcript로 기준선 |
| 5 | semble 재정렬 신호 하나씩(경로 패널티, 파일 포화, 후보 밖 정의 가점) | 3절, 신호별 분해 수치 없음 | S씩 | 중간(과거 튜닝 기각 이력) | 신호별 게이트, 잡음 안이면 버림 |
| 6 | Read 대체 file view(1-based, `cat -n`) | 6절 | S | 낮음 | 응답 형식 테스트 |
| 7 | potion-code-16M-v2 정적 보조 채널 | 4절 | M | 중간(Rust 미학습) | 112질의 게이트 |
| 8 | 데이터셋 정비: gson `expected_file_suffix`가 저장소 루트가 아닌 `gson/src/main/java/com/google` 기준 | 이번 측정에서 drift 검출 | S | 낮음 | 데이터셋에 루트 기록 |

채택하지 않음:

- 질의 모양별 RRF 가중(2절 측정)
- Read 거부 훅(codegraph A/B)
- embeddinggemma-2(라이선스 확인 전)
- LateOn 멀티벡터(sqlite-vec 구조와 맞지 않음)
- 0.8B 이상 재정렬기
- Serena 코드(GPL-3.0, 아이디어만)

## 10. 후속 측정 (같은 날, #453 머지 뒤)

**외부 대조 — semble 공개 주석, Rust 3리포** [측정]. `benchmarks/external-semble-annotations.py`가 semble의 `annotations/*.json`과 `repos.json`을 데이터로만 읽는다(semble 코드는 실행하지 않음). 고정 SHA로 리포를 받고, `benchmark_root`를 루트로 삼아 CodeLens 심볼 결과를 파일 목록으로 바꾼 뒤 semble 파일 경로 기준선과 같은 식으로 NDCG@10을 낸다.

| 리포(20질의) | CodeLens `get_ranked_context` | CodeLens `semantic_search` | semble hybrid(공개) | CodeRankEmbed(공개) | ripgrep(공개) |
| --- | --- | --- | --- | --- | --- |
| tokio | 0.632 → **0.644**(매크로 추출 후) | 0.575 → 0.587 | 0.94 | 0.70~0.86 | 0.27 |
| serde | 0.519 | 0.526 | 0.71~0.83 | 0.46~0.62 | 0.13 |
| axum | 0.469 → **0.496** | 0.382 → 0.413 | 0.78~0.82 | 0.72~0.78 | 0.08 |

자기 세트의 0.77이 외부 라벨에서는 0.5~0.64다. 한계도 함께 적는다. 라벨은 파일 단위이고 LLM이 만들었으며, CodeLens는 심볼 단위로 돌려준다. 그 차이를 감안해도 격차는 크다. 범주별로는 식별자형이 0.625로 가장 높고 설계 질문(0.503)과 의미 질문(0.528)이 낮다. 실패 분석에서 나온 결함이 두 가지다.

- **항목 위치 매크로 본문 미색인**(이 PR에서 수정): tokio는 `cfg_rt! { … }`, `pin_project! { … }` 안에 항목을 둔다(`src` 375개 파일 중 105개). tree-sitter는 그 본문을 token tree로 남기므로 `LocalSet` 정의가 색인에 없었다. 본문을 included ranges로 다시 파싱하자 tokio 심볼이 5,048→5,789개가 됐다. 매크로 안의 호출·`use` 재수출은 아직이다.
- **재수출만 있는 심볼**: axum의 `FromRequest`·`IntoResponse`는 `axum/src`에 정의 없이 `pub use axum_core::…`로만 있다. 정의만 색인하는 CodeLens는 0건을 돌려준다. 재수출을 심볼로 둘지는 열린 질문이다.

**NightOwl-CodeEmbedding-35M bakeoff — 기각** [측정]. 모델 디렉터리만 바꿨다(CLS는 `1_Pooling`으로 자동). `get_ranked_context`가 거의 모든 세트에서 내려갔다. 역할 0.773→0.643, jest 0.304→0.212, gson 0.458→0.400, rg-family 0.917→0.833, self 0.771→0.763이고 gin만 보합이었다. `semantic_search`는 self(+0.014)와 gin(+0.11)에서만 올랐다. 카드가 스스로 밝힌 대로 대부분 in-domain인 MTEB 점수는 CodeLens 세트로 옮겨오지 않았다. 7절 2번은 GTE-ModernBERT-base(768차원)만 남는다.

## 8. 출처 (2026-10-27 열람)

- MCP 2026-07-28: [versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning), [server/discover](https://modelcontextprotocol.io/specification/2026-07-28/server/discover), [streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http), [schema.ts](https://raw.githubusercontent.com/modelcontextprotocol/modelcontextprotocol/main/schema/2026-07-28/schema.ts)
- rmcp: <https://github.com/modelcontextprotocol/rust-sdk> (rmcp-v3.7.0, 10-22)
- Claude Code CHANGELOG(2.1.299~2.1.310): <https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md>. 서브에이전트 문서: <https://code.claude.com/docs/en/sub-agents>
- semble: <https://github.com/MinishLab/semble> (913bdac). gortex: <https://github.com/zzet/gortex> (adfbb702)
- codegraph: [agent-codegraph-adoption.md](https://github.com/colbymchenry/codegraph/blob/main/docs/design/agent-codegraph-adoption.md), [explore-sufficiency.md](https://github.com/colbymchenry/codegraph/blob/main/docs/benchmarks/explore-sufficiency.md)
- Serena 사용 데이터: <https://github.com/oraios/serena/issues/1491>
- codebase-memory-mcp: <https://github.com/DeusData/codebase-memory-mcp>
- 모델: [gte-modernbert-base](https://huggingface.co/Alibaba-NLP/gte-modernbert-base), [NightOwl-35M](https://huggingface.co/Shuu12121/NightOwl-CodeEmbedding-35M), [embeddinggemma-2](https://huggingface.co/google/embeddinggemma-2), [potion-code-16M-v2](https://huggingface.co/minishlab/potion-code-16M-v2), [번들 모델 원본](https://huggingface.co/isuruwijesiri/all-MiniLM-L12-v2-code-search-512)
- fastembed-rs: <https://github.com/Anush008/fastembed-rs> (d6c7d91, 7.5.0)
- 논문: arXiv [2605.18561](https://arxiv.org/abs/2605.18561), [2609.35918](https://arxiv.org/abs/2609.35918), [2609.01865](https://arxiv.org/abs/2609.01865), [2607.01916](https://arxiv.org/abs/2607.01916), [2607.24882](https://arxiv.org/abs/2607.24882), [2608.01507](https://arxiv.org/abs/2608.01507)

## 9. 한계와 빨리 낡는 사실

- 모델 점수는 대부분 작성자 자체 평가이고 과제 부분집합이 서로 다르다(GTE의 79.31과 71.66은 다른 집합이다). 채택은 CodeLens 게이트로만 판단한다.
- 별 수, 다운로드 수, 릴리스 번호(rmcp 3.7.0, fastembed 7.5.0, Claude Code 2.1.310)는 10-27 스냅샷이다.
- 라이브 MTEB·CoIR 리더보드(JS 렌더링)는 열지 못했다. 그래서 거기에만 있는 모델은 빠졌을 수 있다.
- 외부 세트는 작다(rg-family 12질의, jest 24, gin 30, gson 30). 언어별 경향으로만 읽고, 1번 추천의 외부 하네스로 대체할 것.
- Agent Retrieval Bench의 RRF 수치(0.2296→0.2713)는 10-10 문서에서 이어받았고 초록에서 확인되지 않았다 [U].
