# CodeLens 딥리서치 + 표면·비대화 감사 — 2026-09-16

> 범위: (1) 2026-07-03 랜드스케이프 리서치 이후 유사 프로젝트 델타, (2) 설치된 Claude Code
> **2.1.273** 과의 호환성 감사, (3) MCP 스펙 **2026-07-28** 및 2026 하네스 설계 가이드, (4) 이 리포의
> 라이브 데몬·텔레메트리·항상 로드 표면 실측, (5) 아키텍처 비대화 감사. 리서치 원문 3편은
> [`2026-09-16-sources/`](2026-09-16-sources/) 에 그대로 보존(인용 URL·확신도·UNVERIFIED 태그 포함).
> 이전 베이스라인: [2026-07-03](landscape-deep-research-2026-07-03.md).

## 1. Executive summary

1. **표면 비대화의 실체는 도구 수가 아니라 preload 였다.** Claude 프로파일로 실측하면 바인딩 전
   14종/12.6 KB 인데 `prepare_harness_session` 직후 63종 리스팅·**35종 `anthropic/alwaysLoad`**·
   모델 대면 스키마 44 KB 로 뛴다. ADR-0016 Decision #1 은 always-loaded core **10종**이므로 코드가
   ADR 을 위반한 상태였다(#365 가 세분 도구 25종을 "라운드트립 회피"로 보존). `CORE_10_TOOLS` 로
   정렬 → 에페메랄 빌드 실측 **10종/10.8 KB(−61%)**, 래칫 28 KiB→12 KiB.
2. **tools/list 가 사용 로그의 88%.** 45,731행 중 40,213행. 출처는 서버가 아니라 Cursor 안에서 도는
   `hermes mcp serve`(Python MCP SDK 기본 clientInfo `mcp`)의 **정확히 180초 주기 재리스팅** — 한
   세션이 3주간 8,041행. 영속 경계에서만 필터하고 인메모리 메트릭은 유지.
3. **Claude Code 2.1.273 호환 P0 는 하나.** `.claude-plugin/plugin.json` 의 `"agents": "./agents/"` 가
   `claude plugin validate --strict` 에서 실패(배열 형식 필요) — 라이브 CLI 로 재현·수정·재검증.
   `_meta["anthropic/alwaysLoad"]`·`anthropic/maxResultSizeChars` 키는 현재 문서와 정확히 일치.
4. **네이티브 `LSP` 도구가 2025-12-19(v2.0.74)부터 존재**하는데 라우팅 블록이 언급하지 않았다.
   `Piebald-AI/claude-code-lsps` 마켓플레이스가 24개 언어서버를 한 줄로 배선 — 단일 파일
   go-to-def/진단은 네이티브에 양보하고 CodeLens 는 교차 파일 참조·임팩트·아키텍처로 차별화.
   템플릿에 "Claude Code host facts" 절 추가.
5. **MCP 2026-07-28 은 CodeLens 세션 설계와 정면 충돌하는 방향이다.** `initialize`/`Mcp-Session-Id`
   제거(무상태), `server/discover` 필수, `subscriptions/listen`, 모든 결과에 `resultType` 필수,
   tools/list 에 `ttlMs`/`cacheScope` **필수**, Roots/Sampling/Logging 폐기. Claude Code 2.1.273 은
   아직 구세대 핸드셰이크를 쓰므로 당장 깨지지 않지만 "리스트 엔드포인트는 연결별로 달라지지
   않는다"는 원칙은 세션별 표면(프로파일·지연 확장·full exposure)의 존재 이유를 흔든다 → ADR 필요.
   이번엔 `ttlMs`/`cacheScope` 만 선반영.
6. **경쟁 지형 델타.** 베이스라인에 없던 초대형 신규 3종 — CodeGraph(★71k, 단일 도구 노출),
   GitNexus(★47k), codebase-memory-mcp(★43k, camelCase 인지 FTS5 토크나이저). Aider 사실상 정지,
   Serena GPL-3.0 재라이선싱+CLA, JetBrains `jbcontext` 신규 진입. 27개 중 21개 1차 소스 검증.
7. **아키텍처 비대화.** dispatch 7,632줄 중 4,313줄(57%)이 응답 정형화 기계; presets.rs 가 tools.toml
   `preset_tags` 와 131개 도구명을 이중 관리; 40줄 미만 재수출 모듈 14개. 이번엔 계측·문서·백로그
   등재까지(K-0023~K-0025), 코드는 다음 사이클.
8. **CLAUDE.md 17.2 KB → 8.0 KB.** 매 세션 로드되는 파일에서 서브시스템 레퍼런스 14.5 KB 를 docs 로
   원문 이관. 라우팅 블록은 host facts 절이 붙어 2.8 KB → 3.6 KB(재생성, doctor exact).
9. **폐기 웨이브 제거 게이트 충족.** 07-24 창 개시 후 유기적 호출 0(검증 프로브 2건뿐), v1.13.35 가
   1회 클린 릴리스 → v2.0 컷에서 삭제 승인(ADR-0018 판정 절).
10. **리포 위생.** 클린 detached 워크트리 1개 제거, 병합 완료 로컬 브랜치 81개 삭제(원장
    `.codelens/merged-branches-deleted-2026-09-16.txt`), 미커밋 변경 보유 워크트리 3개는 보존.

## 2. 실측 (2026-09-16, 데몬 :7838 = git bdbbafc, Claude Code 2.1.273)

| 항목 | 이전 | 이후 | 근거 |
|---|---|---|---|
| Claude 프로파일 `tools/list` 바인딩 전 | 14종 · alwaysLoad 13 · 12.1 KB | 14종 · alwaysLoad 9 · 10.6 KB | curl initialize(clientInfo claude-code) → tools/list |
| 같은 세션, `prepare_harness_session` 후 | 63종 · **alwaysLoad 35 · 27.7 KB** | 56종¹ · **alwaysLoad 10 · 10.8 KB** | 에페메랄 서버(:7741, 브랜치 빌드) 동일 절차 |
| always-load 래칫(name+desc+inputSchema) | 26,244 B / 35종, cap 28 KiB | 10,800 B / 10종, cap 12 KiB | `always_load_surface_stays_within_its_context_budget` |
| `prepare_harness_session` 엔트리 총 바이트 | 22,017 B(그중 outputSchema ≈18 KB) | 동일 — lean 계약은 outputSchema 를 이미 생략 | generic 프로파일 listing 분해 |
| 텔레메트리 `tools/list` 비율 | 40,213 / 45,731 (88%) | 영속 0(인메모리 유지) | `persist_event_to_usage_log` |
| 180초 폴러 | 세션 a8919779 8,041행(07-24→08-14) | 서버 측 변경 없음(외부 클라이언트) | timestamp gap 중앙값 180,029 ms |
| CLAUDE.md | 17,226 B(손수 14,459 + 블록 2,767) | 8,045 B(손수 4,423 + 블록 3,622) | `surface-manifest.py --check` 통과 · `doctor claude-code` exact |
| 폐기 웨이브 호출 | list_active_agents 1 · list_memories 1 (둘 다 검증 프로브) | — | JSONL `client_name` |
| 워크트리 / 로컬 브랜치 | 4 / 182 (병합 완료 84) | 3 / 102 | `git worktree list`, `git branch -d` |
| git pack | 300 MiB — 최대 blob 172 MB(`scripts/finetune/pipelines/*.jsonl`, main 밖 커밋) | 미변경(히스토리 재작성 필요) | `rev-list --objects --all` |

¹ 에페메랄 빌드는 `semantic` 피처 없이 빌드되어 semantic 계열 7종이 리스팅에서 빠짐. always-load
집합은 피처와 무관하게 ADR 10종과 정확히 일치했다.

### 폴러 판정 근거
- 세션 클라이언트명 `(none)`(07월, 필드 도입 전)→`mcp`(08월 이후). Python MCP SDK 의
  `ClientSession` 기본 `Implementation(name="mcp")`.
- `lsof -iTCP:7838` 에 `hermes mcp serve`(PID 17866, 부모 = Cursor agent-cli) 가 ESTABLISHED.
  `~/.hermes/config.yaml` 의 `gateway_notify_interval: 180`.
- 서버 응답 스트림에는 `list_changed` 0건 → 서버 유발 재리스팅 아님.

## 3. 아키텍처 감사 (architecture-audit, 비테스트 Rust 109,224줄)

| 군 | 발견 | 처리 |
|---|---|---|
| 1 이중 진실 | `presets.rs` 손수 배열 4종(23/41/47/20 = 131명) ↔ `tools.toml preset_tags`, `regen-tool-defs.py::validate_preset_tags` 가 lockstep 만 감시 | **K-0023** 백로그: regen 이 `presets_generated.rs` 를 생성하도록 전환(새 추상화 0) |
| 1 이중 진실 | `search` verb 스키마가 10개 타깃의 수기 합집합 | 라이브 재현 시 `max_results`·`exact_match` 모두 pass-through 로 정상 동작 → 감사 주장 하향(스키마 비망라일 뿐 사용자 대면 실패 없음) |
| 2 응답 기계 | `success.rs:106 payload_estimate` 가 압축 `data` 1부만 계측, 실제 전송물은 structuredContent+pretty 텍스트+19필드 envelope; 5단계 중 3~5단계 동일 호출; 축약기 2벌; lean 경로에서 계산 즉시 폐기 | **K-0024** 백로그. `success.rs` 는 09-10 미커밋 WIP(호스트 인벤토리 suggestion 필터) 아래라 이번 세션은 손대지 않음 |
| 3 과분할 | 40줄 미만 재수출 모듈 14개, `output_schemas` 17파일/4단계 | **K-0025** 백로그(기계적·단독 커밋) |
| 기각 | `rank_fusion.rs` 단일 소비자 seam · `build_info.rs` civil calendar · 호스트 템플릿 5종 | 구조 정당 |

## 4. 리서치 A — 유사 프로젝트 델타 (원문: `2026-09-16-sources/a-landscape-delta.md`)

- 신규 초대형: **CodeGraph**(MCP 기본 노출 도구 1개 `codegraph_explore`, 나머지 env opt-in; "44% 비용
  절감"과 "세션 종료 시 잔존 컨텍스트 +80%" 를 같은 페이지에 병기 — CodeLens 가 측정한 적 없는 축),
  **GitNexus**(Leiden 커뮤니티 탐지로 기능 영역별 스킬 자동 생성), **codebase-memory-mcp**
  (`cbm_camel_split` FTS5 토크나이저, 11-signal 랭킹 융합, arXiv 프리프린트 동반).
- Anthropic 네이티브 `LSP` 도구(2025-12-19, v2.0.74) + `claude-code-lsps` 마켓플레이스(24 언어서버).
- JetBrains `jbcontext` CLI 신규(2026-07-29 초판, 주간 케이던스). octocode-mcp 가 로컬 코드 인텔로
  확장. Aider 정지(마지막 릴리스 2025-08-09). Serena GPL-3.0 + CLA.
- 이식 후보 Top(효과/비용): ① 참조된 파일만 크게 경고하는 **staleness 배너**(S) ② **camelCase/
  snake_case 인지 토크나이저**(S, 현 토크나이저 확인 선행) ③ **잔존 컨텍스트 벤치 축**(M) ④ 분모를
  명시한 커버리지 공개 방법론(M) ⑤ 스트리밍 envelope + exit-code 상태(M) ⑥ module proximity·graph
  diffusion 신호(M, MRR 게이트 필수) ⑦ 기능 영역 스킬 자동 생성(L) ⑧ 릴리스 멀티바이너리 스캔 문서(S)
  ⑩ gitignore 오염 회귀 케이스(S). (#9 alwaysLoad 핵심 상시 로딩은 이번 사이클에서 ADR 정렬로 종결.)
- UNVERIFIED 6건: probe, scip-python, Meta Glean, Nia, Greptile, Augment MCP 정확한 출시일.
- 방법론 경고: WebSearch AI 요약의 날짜를 그대로 믿지 말 것 — Augment "Context Lineage" 가 요약은
  2026-06, 1차 페이지는 2025-07-29(14개월 오차).

## 5. 리서치 B — Claude Code 2.1.273 호환 (원문: `b-claude-code-2.1.273-compat.md`)

| 파일 | 판정 | 조치(이번 브랜치) |
|---|---|---|
| `.claude-plugin/plugin.json` | **broken** — `agents` 는 `.md` 경로 배열이어야 함 | `["./agents/codelens-explorer.md"]` 로 수정, 비-strict 통과. strict 의 남은 경고는 "플러그인 루트의 CLAUDE.md 는 프로젝트 컨텍스트로 로드되지 않음"(리포=플러그인 루트 구조상 불가피) |
| `hooks/codelens-session-probe.sh` | `source:"fork"`(2.1.214+) 미처리 | fork 가드 추가, 테스트 케이스 추가(11/11) |
| `crates/.../templates/claude_code.rs` → CLAUDE.md 블록 | 네이티브 LSP 언급·host_capabilities 예시 부재 | "Claude Code host facts" 절 추가 후 `attach claude-code` 로 재생성 |
| `skills/*/SKILL.md` | `trigger`·`tools` 는 스키마 밖(inert, strict 경고 없음) | 죽은 `trigger` 삭제; `tools` 는 문서 목적으로 유지 |
| `docs/platform-setup.md` | `deferredToolLoading`/`hostCapabilities` 가 자동 협상처럼 읽힘 | 명시 opt-in 임을 한 문장으로 못박음 |
| `Dockerfile.release` / `mcp.json` | EXPOSE·URL 이 7837, 코드 권장·문서·attach 기본은 7838 | 7838 로 정렬(ENTRYPOINT 는 stdio 라 EXPOSE 는 메타데이터) |
| `.mcp.json`(7736 dev 데몬) · `agents/codelens-explorer.md` · `hooks/optional/*.hooks.json` · `codelens-first.py` | compatible(라이브 validate 통과) | — |
| `.agents/plugins/marketplace.json` | Claude Code 산출물 아님(Codex 전용) | 감사 범위 밖 |

계약 사실(라이브 문서 HTML 확인): 서버 단위 `.mcp.json` `alwaysLoad` 와 도구 단위
`_meta["anthropic/alwaysLoad"]` 두 메커니즘이 문서화돼 있고 CodeLens 의 키·타입은 정확히 일치.
UNVERIFIED: Claude Code 가 `initialize` 에서 `hostCapabilities` 를 보내는지(증거 없음 — 도구 인자 경로만
확인됨), `MAX_MCP_OUTPUT_TOKENS` 등 env 가 env-vars 페이지에도 있는지.

## 6. 리서치 C — MCP 스펙·하네스 가이드 (원문: `c-mcp-harness-guidance.md`)

- **강점 확인**: verb facade(`search(mode=…)`)는 Anthropic 도구 설계 가이드("자주 연쇄되는 다단계를 한
  도구로")와 일치. ADR-0015/0016/0010 은 방향상 앞서 있음. ≤20 기본 표면은 CI 로 게이트됨.
- **갭 1 (M)**: `ToolAnnotations` 에 비스펙 필드 4종(`approvalRequired`·`auditCategory`·
  `toolNamespace`·`tier`)을 최상위에 주입. SEP 4건(1862/1913/1984/2417)이 이 객체를 재편 중이라
  strict 클라이언트가 거부/제거할 위험 → `_meta["codelens/…"]` 로 이동(**K-0026**, 별도 ADR).
- **갭 3 (S)**: `ttlMs`/`cacheScope` — 이번에 반영(`ttlMs: 300000`, `cacheScope: "private"`).
- **갭 2/5 (검증)**: 프로파일 스코프 세션의 첫 tools/list 크기 — 이번 실측으로 답함: Claude 프로파일은
  부트스트랩 14종, 바인딩 후 full exposure(#357). 지연 로딩을 지원하는 호스트에선 always-load 만
  컨텍스트 비용이므로 core-10 정렬이 정답. 비지연 호스트(Codex `codex-mcp-client`)는 바인딩 후 63종
  전체 스키마를 받는다 → 그 호스트용 lean 리스팅은 후속 과제.
- **갭 6 (M)**: verb mode 의 tool search 발견성 평가 부재("find callers of X" → `graph(mode=callers)`
  적중률) — 소형 eval 필요(**K-0028**).
- **스펙 2026-07-28 전략 항목**: 무상태 MCP(핸드셰이크·세션 헤더 제거, 요청마다 `_meta` 에 protocolVersion/
  clientCapabilities), `server/discover` 필수, `subscriptions/listen`, `resultType` 필수, 결정적 tools/list
  순서, Roots/Sampling/Logging 폐기(CodeLens 는 셋 다 미구현이라 이행 비용 0), Tasks 는 확장으로 분리.
  CodeLens 의 `Mcp-Session-Id` 기반 표면·`notify_tools_list_changed`·세션 full exposure 는 이 개정판의
  "리스트 엔드포인트는 연결별로 달라지지 않는다" 와 충돌 → 표면 상태를 서버 발급 핸들(도구 인자)로
  옮기는 설계가 필요(**K-0027**, ADR). Claude Code 2.1.273 은 구 핸드셰이크를 쓰므로 당장은 무영향.
- 확신도 낮은 항목: Cursor 40-도구 상한(커뮤니티 포럼만), 실무자 토큰 비용 수치(단일 셋업).

## 7. 우선순위와 실행 현황

| P | 항목 | 상태 |
|---|---|---|
| P0 | plugin.json `agents` 배열화 | 브랜치 커밋 |
| P1 | always-load → ADR-0016 core-10(35→10, −61%) | 브랜치 커밋 + 에페메랄 실측 |
| P1 | 훅 `fork` 가드 · 템플릿 host facts(LSP·host_capabilities) · tools/list `ttlMs`/`cacheScope` | 브랜치 커밋 |
| P1 | tools/list 텔레메트리 영속 제외 | 브랜치 커밋 |
| P2 | CLAUDE.md 다이어트 · 포트 7837→7838 정렬 · platform-setup opt-in 명시 · 스킬 dead 키 · ADR-0018 게이트 판정 | 브랜치 커밋 |
| P2 | 클린 워크트리 제거 · 병합 브랜치 81개 삭제 | 로컬 적용(원장 보존) |
| 백로그 | K-0023 presets codegen · K-0024 payload 게이지 · K-0025 과분할 · K-0026 annotations→_meta · K-0027 무상태 MCP ADR · K-0028 verb 발견성 eval · K-0029 staleness 배너 · K-0030 camel_split 토크나이저 | `.codelens/kaizen_board.json` |

**미적용(사용자 결정)**: `main` 병합, origin 푸시, 데몬 재배포(런타임 변경 3종 — alwaysLoad·텔레메트리
필터·ttlMs — 는 재배포 전까지 효력 0), 미커밋 WIP 4파일(09-10, `token-suggestion-inventory` 워크트리와
바이트 동일) 처리, `token-suggestion-inventory/target` 2.5 GB 정리, AGENTS.md(Codex 10.5 KB) 다이어트
(host-context-efficiency 워크트리가 편집 중이라 충돌 회피).

## 8. Open questions

- Claude Code 가 `initialize` 에 host capabilities 를 실어 보내는 경로가 생길지(현재는 도구 인자만).
- 비지연 호스트(Codex)에서 바인딩 후 63종 풀 스키마가 실제 정확도에 미치는 영향 — CodeGraph 식
  "잔존 컨텍스트" 벤치로 측정 필요(K-0028 과 묶음).
- 무상태 MCP 로 옮길 때 세션별 프로파일(readonly/review/builder)을 어디에 둘 것인가 — 서버 발급
  핸들 vs 요청 `_meta` 확장.

## 9. Staleness

- Claude Code 는 주 단위 릴리스 — 이 문서의 CLI 검증은 2.1.273 기준, 1개월 후 재검증.
- MCP 2026-07-28 은 개정 7주차 — `ttlMs`/`cacheScope`·Tasks 확장의 호스트 채택은 아직 낮음.
- 스타 수·다운로드 수는 반올림 값, 인용 시점 재확인.
