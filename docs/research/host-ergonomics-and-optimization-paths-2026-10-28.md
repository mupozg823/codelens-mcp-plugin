# Claude Code·Codex 사용 편의와 최적화 경로 — 구조·포팅·호스트 조사 (2026-10-28)

[2026-10-27 델타](landscape-delta-2026-10-27.md)의 후속이다. 그 문서가 검색 품질·모델·MCP 2026-07-28·채택률 전술을 다뤘으므로 여기서는 다시 조사하지 않는다. 이번 질문은 세 가지다. CodeLens를 Claude Code와 Codex에서 쓰기 편하게 만드는 것은 무엇인가. C나 다른 언어로의 포팅, 바이너리 수준 작업이 도움이 되는가. 구조(아키텍처)는 4월 감사 이후 어떻게 변했는가.

표기: **[검증]** = 원문이나 소스를 직접 열어 확인, **[측정]** = 로컬 실행 결과, 표기 없음 = 조사원 보고(1차 출처 링크 포함, 미재검증), **[U]** = 출처 없는 추론이나 미확인.

측정 기준(모든 [측정] 공통): main `93a222b5` 릴리스 빌드(기본 기능 `scip-backend,lang-extra`). 대상은 위네이션 `git archive HEAD` 스냅숏이다(JS/TS 3,058개, 원 체크아웃의 미커밋 변경과 분리). 대상 심볼은 시드 20261028로 고른 내보낸 함수 14개다. stdio `--profile analysis-lite --daemon-mode read-only` 한 세션에서 refs와 impact를 각각 호출했다. 머신은 다른 세션 작업으로 load 25~41(12코어)이었으므로, 시간 비교는 같은 색인 상태에서 순차로만 했다.

## 요약

1. **포팅은 답이 아니다. 질문의 전제와 실측이 다르다.** [측정] analysis-lite 한 호출의 시간은 Rust 로직이 아니었다. 호출마다 프로젝트의 JS/TS 파일을 최대 2,000개 다시 읽고, OXC 의미 분석과 tree-sitter 호출 추출을 **단일 스레드로 처음부터** 돌리는 알고리즘에서 나왔다(`analysis_lite/graph.rs`의 `DeclarationGraph::build`). 10-27 C 포팅 분석(#457)의 결론도 같다. on-CPU 중 Rust 로직은 3~4%, 나머지는 이미 C(tree-sitter, SQLite)이거나 손으로 짠 어셈블리(ONNX MLAS/KleidiAI)였다. 외부 사례도 언어가 아니라 작업 방식 변경에서 이득을 냈다. ast-grep 최적화 11배, ast-grep의 tree-sitter Rust 재작성은 파싱 +29.7%에 종단 −22.2% CPU였다.
2. **"쓰기 편함"의 1차 병목은 지연과 실패하는 기본 호출이었다.** [측정] main 기준 refs 한 호출 6.9초(중앙값)였다. 같은 질의를 두 번 불러도 7~11초였다. 기본 인자로 부른 `graph impact`는 응답 한도 18,000B를 넘어 **표본 없이 실패**했다(14개 중 4개). 이 PR에서 고쳤다(1절).
3. **호스트 쪽 고정 비용은 거의 사라졌다.** [검증] Claude Code는 tool search가 기본이고 "세션 시작 시 도구 이름과 서버 지시문만" 로드한다. Codex 0.164도 `tool_search`가 상시 켜져 있다(소스 기준). 10-11 제거 결정의 근거였던 «상시 도구 ≈3.4k 토큰/턴»은 analysis-lite 지연 등록에는 해당하지 않는다. 남은 비용은 호출당 지연과 응답 모양이다.
4. **바이너리 77MB 중 79%가 tree-sitter 문법 표다.** [측정] `__const` 61.1MB, 기계어 `__text` 12.2MB, 문법 34개. 릴리스 프로필은 이미 최대치다(`lto=true`, `codegen-units=1`, `opt-level=3`, `panic=abort`). 시작 비용은 아니다(`initialize` 132~176ms). 배포 크기 문제다.
5. **구조는 4월 대비 약 2배로 커졌다.** [측정] 엔진 27.6k→52.8k줄, mcp 52.5k→112.0k줄. 4월 god module 셋 중 둘은 해소됐다. 도구 표면은 100개인데, Claude Code에 실제 등록된 표면은 2개(analysis-lite)다.

## 1. 이번 사이클 실측과 수정 (이 PR)

### 원인 [측정]

macOS `sample` 8초를 떴다. 바쁜 스레드의 `DeclarationGraph::build`는 2,441샘플이었다. 내역은 tree-sitter 호출 추출 823, OXC 선언 분석 610, import 해석 504(`stat`/`getattrlist` 반복), 파일 읽기 159다. 모두 호출마다 반복됐고 한 스레드에서 돌았다.

### 수정

- 파일 읽기, OXC 분석, 호출 추출을 rayon으로 병렬화했다. 결과는 경로 순(BTreeMap)으로 접는다. 그래서 첫 읽기 실패, 오류 목록, 간선 중복 제거 순서가 순차 스캔과 같다.
- 프로세스 안에 파싱 캐시 두 개를 두었다. 키는 절대 경로이고, 내용 해시가 같을 때만 재사용한다. 해시가 다르면 미스라 낡은 적중은 없다. 20,000파일에서 비운다.
- 한 빌드 안의 `(가져오는 파일, 모듈 지정자)` 해석을 메모화했다.
- K-0066: 18,000B를 넘으면 통째로 거절하던 동작을 바꿨다. 이제 뒤쪽 행을 통째로 잘라 한도에 맞춘다. 순서는 미해결 예시 먼저, 그다음 references(impact는 간선과, 그 간선만 닿던 노드)다. 무엇을 몇 개 뺐는지는 `truncation.wire_bound`에 싣는다. 행이 하나도 안 들어가면 지금처럼 `response_size_limit`로 거절한다. 그래서 표본 0건(sampled zero)은 여전히 나오지 않는다.
- K-0064: 기본 `max_files`를 2,000에서 10,000(스키마 상한)으로 올렸다.

### 결과 [측정]

동일성: 28쌍(14 대상 × refs·impact, impact는 `max_nodes` 10, `max_edges` 20)에서 정규화 결과가 모두 같았다. 비교한 것은 참조, 간선, 노드, 미해결, 오류, `source_bundle_hash`다. 다른 값은 색인 세대 번호 `snapshot.generation`뿐이었다(색인 재생성 여부의 차이).

| 단계 | 28회 합계 | 호출 중앙값 | 최대 |
| --- | --- | --- | --- |
| main `93a222b5` | 206.3초 | 6,931ms | 9,708ms |
| + 병렬화 | 81.3초 | 2,808ms | 4,083ms |
| + 해시 키 파싱 캐시·모듈 해석 메모 | 29.8초 | 1,008ms | 2,277ms |
| + `max_files` 10,000 | 34.4초 | 1,157ms | 2,856ms |

기본 인자(상한 지정 없음) 28회: main은 impact 4건이 `response_size_limit`로 실패했다. 이 PR은 0건이 실패하고 9건을 잘라서 돌려줬다(모두 `wire_bound` 명시). 돌려받은 참조 행은 32→66, impact 간선은 31→132, 중앙값은 1,038ms다.

`max_files` 2,000은 위네이션(3,058개)을 잘랐다(`truncation.files=true`, `files_read` 2000). 10,000으로 올리자 잘림이 풀리고 놓치던 참조가 돌아왔다. 예: `vsStartReason` 0→5건(시험 파일 1개 포함), `generateAwaitingBroadcastReports` 1→5건, `formatCrewValue` 6→8건.

변이 검사: 캐시의 해시 비교를 빼면 새 시험 `analysis_lite_reused_parses_follow_edits_between_queries_in_one_process`가 실패했다. 거절 분기를 깨면 `clipping_refuses_when_not_one_row_fits_so_no_sampled_zero_is_returned`가 실패했다.

## 2. 구조 델타 (4월 감사 `docs/architecture-audit-2026-04-24.md` 대비)

측정 방법: 4월 기준은 `git rev-list -1 --before=2026-04-25 main` = `b0bda051`이다. 줄 수는 `git show <sha>:<path> | wc -l`로 쟀다.

| 4월 지적 | 4월 | 지금 | 상태 |
| --- | --- | --- | --- |
| `surface_manifest.rs` god module | 2,214 | 416 | 해소 |
| `tool_defs/presets.rs` god module | 1,441 | 869 | 대부분 해소(코드젠 이관) |
| `main.rs` 시작·CLI 집중 | 414 | 604 | 커짐 |
| `state/session_host.rs` 단순 위임 | 194 | 318 | **커짐 — 열림** |
| `integration_tests/workflow.rs` 창고 | 5,084 | 분할(최대 `workflow/harness.rs` 1,580) | 해소 |
| `server/http_tests.rs` 창고 | 3,563 | 136(+하위 모듈) | 해소 |
| `resource_catalog.rs` 중복 표면 | 282 | 삭제 | 해소 |
| `benchmarks/harness/harness_runner_common.py` | 1,436 | 1,436 | 변화 없음 — 열림 |

전체 규모 [측정]:

| 항목 | 값 |
| --- | --- |
| 엔진 | 62→158파일, 27,573→52,792줄 |
| mcp | 137→380파일, 52,526→112,040줄 |
| 시험 경로 줄 | 43,864 |
| 정상 의존성(`cargo tree -e normal`, 고유) | 기본 258, `http,semantic` 379 |
| tree-sitter 문법 | 34 |
| 도구 정의(`tools.toml`) | 100 |

판단: 4월의 "제품 표면이 벤치마크 근거보다 넓다"는 지적이 더 커졌다. 실사용 표면은 analysis-lite 2개 도구와 일회성 CLI 리포트다. 그런데 바이너리·의존성·시험 비용은 100개 도구 전체가 진다. 이번 사이클은 표면을 줄이는 대신 실사용 표면의 지연을 고쳤다. 표면 축소는 추천표 6번에 남긴다.

## 3. 포팅·바이너리·타언어 경로

| 경로 | 근거 | 기대 이득 | 판정 |
| --- | --- | --- | --- |
| C·Zig·Go로 포팅 | 핫패스가 이미 C·asm [측정, 10-27 #457 + 이번 표본] | ≤10% | 기각 |
| ast-grep의 tree-sitter Rust 재작성 이식 | 파싱 +29.7%, 종단 −22.2% CPU, RSS +29.8%, 증분 파싱·Wasm 문법 포기 ([ast-grep 블로그](https://ast-grep.github.io/blog/tree-sitter-rust-rewrite), 2026-07) | 9~11% 추정 | 기각(대가가 큼) |
| 알고리즘: 호출마다 전체 재파싱 제거 | 이번 PR [측정] | 중앙값 6.9배 | **채택(이 PR)** |
| 파일당 1회 파싱 트리 공유(리포트 경로) | 리포트 경로 비테스트 `Parser::new` 9곳. 질의·커서 작업이 파싱 이상 비용 | 15~30% 추정 [U] | 추천 5 |
| 첫 임베딩 색인 100~353초(K-0058) | 길이 버킷 배치, 미변경 생략, CoreML EP A/B. ORT 문서는 동적 모양을 경고 ([CoreML EP](https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html)) | 큼(절대 대기 최대) | 추천 4 |
| SQLite 쓰기 | PRAGMA는 이미 조정됨 [검증 `db/mod.rs:247`]. 남은 것은 `pwrite` 31%(콜드 색인)와 커밋 수 | 중 | 추천 7 |
| Rust PGO (cargo-pgo) | Rust 몫이 3~6% | <3% | 보류 |
| BOLT (macOS) | Mach-O 지원 미확인(2020 RFC뿐) | — | 기각 |
| mimalloc·jemalloc | `malloc`/`free` 1.9~2.3% | <2% | 기각 |
| usearch·HNSW | 384차원 ~14k 벡터는 전수 탐색으로 충분(100k×384 ≈68ms, 경쟁사 벤치) | — | 기각 |
| zoekt·tantivy·stack-graphs 포크 | 지배 비용을 겨냥하지 않음. stack-graphs는 2025-09 보관됨 | — | 기각 |
| WASM 배포 | 이득 근거 없고 손실은 미측정 | — | 기각 |
| 슬림 바이너리(JS/TS 문법만, analysis-lite 전용) | `__const` 61.1MB가 문법 표 [측정]. JS/TS/TSX 표 크기는 미측정 | 배포 크기 대폭 [U] | 추천 6 |
| 플랫폼별 npm 패키지 | `@ast-grep/cli`·`@biomejs/biome`가 `optionalDependencies` 방식(7·10개 플랫폼 패키지) | 설치 편의 | 추천 6 |

결론: 다른 언어로의 포크·포팅이 아니라 다음 셋이 경로다. 첫째, 같은 일을 반복하지 않는 알고리즘. 둘째, 첫 임베딩 대기. 셋째, 실사용 표면에 맞춘 슬림 배포. GPL·AGPL 코드는 이식하지 않는다(Serena, deadcode).

## 4. 호스트 사실 (Claude Code 2.1.311, Codex 0.164)

| 사실 | 출처 | 표기 |
| --- | --- | --- |
| MCP 출력은 10,000토큰에서 경고, 기본 상한 25,000토큰(`MAX_MCP_OUTPUT_TOKENS`). 경고 임계값은 고정 | [Claude Code MCP 문서](https://code.claude.com/docs/en/mcp) | [검증] |
| 성공 텍스트 결과가 50,000자를 넘으면 파일로 저장. `_meta["anthropic/maxResultSizeChars"]`로 도구별 상향, 상한 500,000자 | 같은 문서 | [검증] |
| tool search 기본 켜짐. 세션 시작에는 도구 이름과 서버 지시문만 로드 | 같은 문서 | [검증] |
| Codex: `tool_search` 상시 켜짐, 지원 모델이면 MCP 도구 지연 | [codex-rs features/src/lib.rs](https://github.com/openai/codex/blob/main/codex-rs/features/src/lib.rs) | 소스 보고 |
| Codex: `startup_timeout_sec` 10, `tool_timeout_sec` 60, 도구별 `output_token_limit`, 폴백 절단 10,000B(가운데) | [Codex MCP 문서](https://learn.chatgpt.com/docs/extend/mcp.md?surface=cli), `model_info.rs:126` | 조사원 보고. 모델별 실제 예산 [U] |
| Claude Code 플러그인은 `bin/`(Bash PATH), skill, agent, hook, `.mcp.json`, `.lsp.json`을 한 번에 배포. claude.ai·Cowork는 `bin/` 포함 플러그인 설치를 거부 | [plugins-reference](https://code.claude.com/docs/en/plugins-reference) | 조사원 보고 |
| Codex 스킬은 Claude와 같은 SKILL.md 형식이고 `.agents/skills`에 둔다. 초기 목록은 컨텍스트의 2% 상한 | [build-skills](https://learn.chatgpt.com/docs/build-skills.md) | 조사원 보고 |
| 응답 모양: 위치만 주면 후속 Read 15.2회·pass@1 0.67, 참조마다 소스 2줄을 붙이면 3.2회·0.83, grep은 1.00·2,451토큰. "예비 파일럿"(Opus 4.8·Sonnet 4.6·Haiku 4.5, 2026-08-12) | [agentconnect.md](https://www.agentconnect.md/blog/grep-beat-lsp-harness/) | [검증] |
| 같은 파일럿의 도구 선택률: 위치 찾기에서 의미 도구 0~6%, 참조 완전성 과제에서 45~57% | 같은 글 | [검증] |
| CLI 대 MCP의 중립 비교는 없음. 업체 수치는 스키마 지연 여부에 따라 뒤집힘(Scalekit CLI 10~32배 저렴, Smithery는 반대. Smithery 원문은 404) | [scalekit](https://scalekit.com/blog/mcp-vs-cli-use), [mcp-vs-cli-bench](https://github.com/Lamb-Project/mcp-vs-cli-bench)(결과 미공개) | 조사원 보고 |

CodeLens에 대한 함의:

- **참조 완전성이 CodeLens의 자리다.** 같은 파일럿에서 모델은 위치 찾기에는 의미 도구를 거의 안 쓰고(0~6%), 참조 완전성에는 절반 가까이 썼다. 10-11 A/B의 «위치 찾기 이득 없음»과 같은 그림이다. analysis-lite가 refs·impact만 노출하는 선택은 맞다.
- **응답에 소스 줄이 없으면 후속 Read가 비용을 다 먹는다.** analysis-lite 행에는 위치, 정체, 출처 해시는 있지만 코드 줄이 없다. 행마다 중복되는 64자 `source_hash`를 번들 수준으로 올리면, 그 자리에 `path:line:content`를 실을 수 있다(추천 2).
- **Codex 폴백 절단은 10,000B를 가운데에서 자른다.** 18,000B JSON이 그대로 가면 가운데가 잘린 JSON이 된다[U: 모델별 예산 미확인]. 설치 안내에 `output_token_limit`을 넣거나, Codex용 압축 텍스트 채널을 둔다(추천 3).

## 5. 추천표 (중요도순)

| 순위 | 항목 | 근거 | 비용 | 위험 | 첫 단계 |
| --- | --- | --- | --- | --- | --- |
| 1 | ~~analysis-lite 호출 지연·기본 호출 실패·`max_files` 잘림~~ (이 PR) | 1절 | — | — | 머지 후 `codelens-lite` 승격 |
| 2 | refs·impact 행에 `path:line:content` 한 줄, 행별 `source_hash`는 번들 수준으로 | 4절 파일럿: 후속 Read 15.2→3.2 | S | 낮음(가산 필드) | 같은 28회 하네스로 바이트·행수 비교 |
| 3 | `_meta["anthropic/maxResultSizeChars"]` 명시, Codex `output_token_limit` 안내 | 4절 [검증] | S | 낮음 | 18,000B 와이어 한도와의 관계 결정 |
| 4 | 첫 임베딩 색인(K-0058): 길이 버킷 배치·미변경 생략, 그다음 CoreML EP A/B | 3절 | M | 중간(동적 모양) | 배치 길이 분포부터 계측 |
| 5 | 리포트 경로 파일당 1회 파싱 트리 공유 | 3절 | M | 중간 | 리포트 1회의 파일당 파싱 횟수 계측 |
| 6 | 실사용 표면 기준 배포: analysis-lite 슬림 빌드(JS/TS 문법만)·스트립·플랫폼별 패키지·Claude Code 플러그인(`bin/`+skill+MCP) | 2·3·4절 | M | 중간(배포 경로 추가) | `--no-default-features` 빌드 크기·섹션 측정 |
| 7 | 콜드 색인 쓰기: 커밋 수·체크포인트 지연 | 3절 | S~M | 낮음 | `fs_usage -f filesys`로 커밋 수 |
| 8 | 첫 호출(콜드 프로세스) 텍스트 사전 필터(이름·파일 줄기·별칭 고정점)와 K-0065 `next/dynamic` | 1절. 콜드 첫 호출 2.3~2.9초 | M | 중간(동일성 검증 필요) | 이번 28회 하네스로 전수 대 필터 동일성 |
| 9 | 10-27 추천 4(채택 측정 하네스) 유지 — 채택 주장 전에 먼저 | 10-27 6절 | S~M | 낮음 | 기존 transcript 다음 행동 분류 |

## 6. 기각한 경로

- 언어 포팅(C·Zig·Go)과 tree-sitter Rust 재작성 이식: 3절.
- BOLT, mimalloc, ANN 색인, zoekt·tantivy·stack-graphs 포크, WASM: 3절.
- Read 거부 훅: 10-27 문서(codegraph A/B).
- 응답 한도 상향만으로 K-0066 해결: Codex 폴백 절단과 Claude Code 50,000자 파일 저장 때문에, 크게 보내는 쪽이 오히려 후속 호출을 늘린다.

## 7. 출처 (2026-10-28 열람)

- Claude Code MCP 문서 <https://code.claude.com/docs/en/mcp> [검증: 출력 한도, `maxResultSizeChars`, tool search]
- Claude Code 플러그인 레퍼런스 <https://code.claude.com/docs/en/plugins-reference>
- Codex MCP <https://learn.chatgpt.com/docs/extend/mcp.md?surface=cli>, 스킬 <https://learn.chatgpt.com/docs/build-skills.md>, 훅 <https://learn.chatgpt.com/docs/hooks.md>, 소스 <https://github.com/openai/codex> (rust-v0.164.0, 10-23)
- 파일럿 <https://www.agentconnect.md/blog/grep-beat-lsp-harness/> (2026-08-12) [검증], arXiv <https://arxiv.org/abs/2608.13568>
- ast-grep <https://ast-grep.github.io/blog/tree-sitter-rust-rewrite>, <https://ast-grep.github.io/blog/optimize-ast-grep>
- codebase-memory-mcp <https://github.com/DeusData/codebase-memory-mcp> (MIT), codegraph <https://github.com/colbymchenry/codegraph>, gortex 전송 형식 <https://github.com/zzet/gortex/blob/main/docs/wire-format.md>
- ONNX Runtime CoreML EP <https://onnxruntime.ai/docs/execution-providers/CoreML-ExecutionProvider.html>, ort 링크 <https://ort.pyke.io/setup/linking>, cargo-pgo <https://github.com/Kobzol/cargo-pgo>, sqlite-vec <https://github.com/asg017/sqlite-vec>, model2vec-rs <https://github.com/MinishLab/model2vec-rs>, potion-code-16M <https://huggingface.co/minishlab/potion-code-16M>
- 조사 원자료(로컬): 세션 tmp `research-host-landscape.md`, `research-perf-porting.md`. 표본: `.codelens/eval-2026-10-27/perf/`

## 8. 한계와 빨리 낡는 사실

- 시간 수치는 load 25~41의 혼잡한 머신에서 쟀다. 절대값보다 같은 조건 순차 비교의 비율로 읽을 것. 단일 저장소(위네이션), 14개 대상이다.
- 첫 호출(콜드 프로세스)은 여전히 전 파일을 파싱한다(2.3~2.9초). 캐시 이득은 같은 세션의 두 번째 호출부터다. 캐시는 프로세스 메모리에 두며, 크기 상한은 파일 수 기준(20,000)이고 바이트로는 재지 않았다.
- 호스트 버전(Claude Code 2.1.311, Codex 0.164)과 별 수(codegraph ~76k, codebase-memory-mcp ~48k, Serena ~31k, ast-grep ~16k, 10-28)는 스냅숏이다.
- Codex 모델별 실제 출력 예산, `.lsp.json` 경유 CodeLens 연결 가능성, Explore 서브에이전트의 MCP 호출 가능 여부는 미확인이다.

## 9. 후속 (같은 날, 추천표 진행)

| 추천 | 결과 | 근거 |
| --- | --- | --- |
| 2 행에 코드 줄 | #461 `9aac1462`. refs 66행 전부 `line_text`, 응답 −6%. impact 간선 132→152(같은 18,000B 안), 잘린 호출 9→7 | [측정] 위네이션 14대상 기본 인자, 행 내용 불일치 0 |
| 3 출력 한도 메타 | `maxResultSizeChars` 불필요(18,000B < Claude Code 50,000자). Codex 폴백 10,000B → 문서에 도구별 `output_token_limit = 8000` | [검증] `codex-rs/models-manager/src/model_info.rs:126`, Codex MCP 문서 |
| 6 슬림 배포 | 보류. `--no-default-features` 빌드 77.1MB → 23.3MB, `initialize` 137~180ms → 49~76ms, 호출 시간 같음. 그러나 전체 갱신은 이 빌드가 못 읽는 언어 파일의 색인 행을 지운다(`symbols/writer.rs:875`). 그래서 전체 바이너리와 같은 `.codelens` 색인을 쓰면 행이 오간다 | [측정]·소스 |
| 4 첫 임베딩 색인 | 배치 안에서 길이 정렬 후 32개씩(`CODELENS_EMBED_SUB_BATCH`). 13,799 심볼 첫 색인 102.6/94.4초 → 84.2/90.0초(평균 −12%). 근사 토큰 기준 예측(계산 22~38% 감소)보다 작음. 품질은 중립: ranked MRR 자기 0.773→0.778·gin 0.417→0.426, semantic 자기 0.701→0.701·gin 0.289→0.272(질의 1개). 번들 모델은 호출 단위 동적 양자화(`DynamicQuantizeLinear` 96개)라 벡터가 배치 동료에 따라 달라진다. 436질의 세트는 옛 구조라 검증기가 거부함 | [측정] 새 스냅숏, 변형마다 재임베딩 |
| 7 콜드 색인 쓰기 | #464 `27a55da5`. `pwrite` 1,696샘플 중 약 1,540이 문장 저널(`subjournalPageIfRequired`)이었다. INSERT마다 FTS 트리거가 문장 저널을 열고, 64 KiB를 넘으면 임시 파일이 된다. 심볼 색인 연결에 `temp_store = MEMORY`를 넣었다. 콜드 전체 색인 중앙값 4.86초 → 3.40초(교차 6회), 콜드 analysis-lite 첫 refs 약 7.1초 → 5.3초. peak footprint 같음, 색인 표 행 단위 동일. 임베딩 저장소는 15초 표본에 `pwrite` 3샘플이라 그대로 | [측정] 위네이션 3,101파일, 매 회 새 스냅숏 |
| 9 채택 측정 하네스 | `benchmarks/next-action-sufficiency.py` 기준선(아래 표). Codex 기록 67개에는 CodeLens MCP 호출이 없어 Claude만 | [측정] transcript 3,694개, CodeLens 호출 1,013건 |
| 8 콜드 사전 필터 | 보류. #460 뒤 첫 호출은 이미 2.0~2.5초. 동일성 보장 비용(기본 내보내기·배럴·별칭·impact 깊이 3)이 이득보다 큼 | [측정] |

### 다음 행동 기준선 (추천 9, 2026-10-28 transcript)

| 묶음 | n | 다시 CodeLens | 반환 파일 읽기 | 다른 파일 읽기 | 텍스트 검색 | 진행 | 종료 | 같은 대상 재검색 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 성공한 탐색 호출 전체 | 478 | 19% | 13% | 17% | 42% | 7% | 2% | 36/201 |
| `search mode=refs` | 134 | 16% | 13% | 16% | 50% | 4% | 1% | 20/67 |
| `search mode=symbol` | 71 | 17% | 18% | 17% | 45% | 3% | 0% | 10/32 |
| `graph mode=callers` | 57 | 18% | 14% | 28% | 33% | 4% | 4% | 5/19 |
| `overview mode=file` | 32 | 44% | 6% | 12% | 31% | 6% | 0% | 0/10 |

- 호출의 91%가 2026-09(CodeLens가 Claude 하네스에서 빠진 10-11 이전)이다. `codelens-lite` 호출은 2건뿐이라 #461의 `line_text` 효과는 아직 이 표로 판정할 수 없다.
- refs 결과 뒤 절반이 텍스트 검색이고, 그중 30%(20/67)는 같은 심볼을 다시 찾았다. 결과를 다시 확인하거나 보충한 신호다. 다만 주석·문자열처럼 CodeLens 범위 밖의 사용처를 찾는 경우도 섞여 있다.
- 이 표는 다음 채택 전술 A/B의 null이다. 같은 스크립트를 `--since`로 다시 돌려 묶음별 증감을 비교하고, 표본 수가 작은 묶음의 차이는 잡음으로 본다.
- 오류 결과 178건(18%)은 표에서 뺐다. 표본 11건을 원문과 대조해 분류 규칙을 확인했다. 처음 규칙에서는 Bash `cat -n`이 진행으로 잡혀 Bash 읽기를 읽기로 고쳤다.

### #460이 만든 메모리 회귀와 수정 (#462 `f094e723`)

슬림 빌드를 재다가 발견했다. 위네이션에서 3회 호출 뒤 analysis-lite stdio 서버의 phys_footprint가 #460 이전 292MB에서 676MB로 늘어 있었고, 유휴 중에도 내려가지 않았다 [측정].

| 빌드 (`max_files` 10,000) | 3회 호출 뒤 |
| --- | --- |
| `93a222b5` (#460 이전) | 292MB |
| main, 캐시 끔 | 606MB |
| main, 캐시 끔, `RAYON_NUM_THREADS=1` | 465MB |
| main | 676MB |
| main + 질의 뒤 `mi_collect` | 431MB |

- 원인: vmmap상 증가분은 mimalloc 아레나에 있다(전역 할당기, `main.rs:7`). `heap`과 `malloc_zone_pressure_relief`로는 보이지 않는다. mimalloc은 스레드마다 힙을 둔다. rayon 작업자가 할당한 블록을 요청 스레드가 해제하면, 그 블록은 잠든 작업자의 지연 해제 목록에 남아 페이지가 반환되지 않는다. 파싱 캐시(이 저장소에서 약 60MB)도 세션 내내 살아 있었다.
- 수정:
  - 질의 뒤 요청 스레드와 모든 rayon 작업자에서 `mi_collect(true)`를 돌린다(작업자 쪽은 `spawn_broadcast`로, 요청을 막지 않는다).
  - 유휴 정리 스레드가 TTL(`CODELENS_ANALYSIS_LITE_IDLE_SECS`, 기본 600) 동안 안 쓰인 캐시를 비운다.
- 결과: 사용 중 434MB, TTL 뒤 45MB. 결과는 28/28 같고 지연도 같다(중앙값 1.0초).
- 교훈: 병렬화 PR에서는 시간과 함께 **phys_footprint**도 잴 것. RSS는 메모리 압박 중 압축돼 노이즈가 크다. 1절의 "바이트로는 재지 않았다" 한계가 실제 회귀였다.
