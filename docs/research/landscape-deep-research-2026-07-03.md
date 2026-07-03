# 코드 인텔리전스 랜드스케이프 딥리서치 — CodeLens 이식 후보 (2026-07-03)

유사 프로젝트 8축 딥리서치 → 인용 실확인 적대검증 → 이식 후보 12개 클러스터 → CodeLens 코드 ground-truth 대조(grounding)의 4단계 파이프라인 결과. 29개 에이전트, 검증 66건(CONFIRMED 50 · REFUTED 4 · UNVERIFIED 12 — 탈락 주장은 본문에서 제외하거나 명시 플래그).

## Executive Summary

1. **12개 이식 후보 전원이 "부분 구현" 판정** — 순수 신규는 없다. 리서치 후보의 premise 중 상당수(파일워처 부재, 범용 임베딩, 쿼리 분류기 부재 등)가 grounding에서 반박됐고, 각 후보는 진짜 델타만 남게 스코프 축소됐다. 결론: CodeLens의 기반은 랜드스케이프 대비 이미 상위권이며, 개선은 신규 서브시스템이 아니라 **기존 seam 위의 배선 작업**이 대부분이다.
2. 최고 가치 델타 5개(Tier 1)는 모두 S~M 비용: 진단-delta 편집 루프, working-set 앵커 랭킹, Python 참조 정밀 라우팅, 평가 하네스 확장(F-β·토큰 프로파일), query-adaptive RRF 실험.
3. 벤치마크 방법론의 2025~2026 합의: 정적 벤치는 오염으로 죽는다(SWE-bench Verified 은퇴). 살아있는 태스크 + 도구 on/off ablation + precision-우선 F-β + 도구 응답토큰 계측이 표준. CodeLens의 MRR 하네스는 좋은 출발점이지만 "에이전트 과업 성공률" 단위로 확장할 여지가 있다.
4. 부산물: 리포 CLAUDE.md의 "the daemon does not auto-watch for changes" 서술은 **낡았다** — `watcher.rs`(300ms debounce)가 `project_runtime.rs:112`에서 default-ON으로 배선돼 있다. 문서 정정 필요.

## 1. 축별 핵심 발견 (검증 통과분만)

### Serena (직접 레퍼런스, 2026-06 main 기준)
- **trusted-projects 경로 신뢰경계**: `trusted_project_path_patterns` 전역 설정으로 side-effecting 동작(프로젝트별 설정 로드, `activation_command` 실행)을 신뢰 경로에만 허용 [CHANGELOG(main), 2026-06]
- `activation_command_timeout`(기본 180s) hard-kill 백스톱; name-vs-path 파일접근 하이재킹 패치; `--project-from-cwd`의 중첩 worktree 하이재킹 수정(nearest-wins — CodeLens는 이미 보유)
- `query_project`: 현 preset에 없는 read-only 도구를 표면 전환 없이 일시 접근하는 escape hatch
- 도구 오류를 MCP **protocol-level error**로 표면화 + structured output을 호스트별로 끄는(Claude Code에서 명시 비활성) 호스트-적응
- 프롬프트 lazy 제공(연결 시 1문장, 매뉴얼은 `initial_instructions` 온디맨드) — 초기 컨텍스트 절약
- 커뮤니티 평가: 토큰 절감은 대규모·장기 세션에서만 실현(back-loaded); 소규모 프로젝트는 도구 스키마 오버헤드로 오히려 손해 [vibecodinghub 2026, 신뢰도 medium]

### LSP 브리지
- **Hermes의 진단-delta 루프가 최강 패턴**: 편집 전 진단 baseline 캡처 → 편집 → 재조회 → **신규 도입분만** delta로 표면화 [hermes-agent.nousresearch.com, 2026-01]
- 반복 실패모드: 편집 후 진단이 출력되면 에이전트가 "편집 실패"로 오인해 재시도 루프 — 응답에 "edit applied" 명시가 처방 [opencode#9102, 2025-11]
- karellen-lsp-mcp: 세션 N개가 프로젝트당 LSP 1개를 **refcount**로 공유하는 shared-daemon 수명주기 모델 [2025-12]

### Sourcegraph/SCIP
- **SCIP-IO**(Rust, v0.1.9 2026-06): 언어 감지→인덱서 설치→병렬 실행→scip 병합의 5-stage 오케스트레이터. 일부 언어 실패 시 **partial index를 명시 플래그와 함께 발행** — 전부-아니면-전무가 아님 [github.com/GlitterKill/scip-io]
- SCIP symbol 문법(scheme+package+version+descriptor)이 cross-repo 네비게이션의 키 — CodeLens의 cross-project 질의 확장에 참조 가능
- Cody Free/Pro 2025-07 종료(enterprise-only) — 개인/로컬 코드 인텔리전스 시장 공백은 CodeLens 포지셔닝에 유리
- ⚠️ 반박됨: "SCIP의 핵심 동기가 증분 인덱싱"은 인용 문서와 불일치(주기적 전체 재분석이 실동작)

### 시맨틱 인덱싱 MCP
- claude-context(Zilliz): 파일해시 **Merkle DAG**로 변경분만 재인덱싱; CocoIndex: AST 경계 청킹 + 청크 단위 재임베딩(한 줄 수정 = 청크 1개만)
- **Greptile 실측**: 함수 단위 tight 청킹이 파일 단위보다 우수(유사도 0.718→0.768); 코드를 자연어 설명으로 번역 후 임베딩하면 +12% (0.728→0.815) [greptile.com/blog/semantic-codebase-search]
- Octocode: LanceDB + RaBitQ 양자화(~32x 압축) + cross-encoder 리랭크 최종단 (⚠️ "Hit@5 +22%" 수치는 2차 출처, 미검증)

### Aider repo-map — 가장 이식가치 높은 알고리즘 디테일
- **per-turn Personalized PageRank**: 채팅에 올라온 파일에 teleport 가중 `100/len(fnames)`을 주고 매 호출 재계산 — 정적 중요도가 아니라 "지금 편집 중인 것" 기준 재랭킹 [repomap.py, 2026-07 확인]
- ref→def **edge 가중 휴리스틱**: 대화에서 언급된 식별자 ×10, well-named(snake/camelCase) ×10, private(_) ×0.1, 과다참조 식별자 감쇠 — PageRank 이전에 곱해짐
- **render-and-measure 이진탐색 예산 맞춤**: 랭킹된 태그 컷오프 N을 실제 렌더→토큰 측정으로 이진탐색해 예산에 정확히 맞춤 (티어 임계값 방식이 아님)
- working set이 없으면 예산을 ×8 확장(`map_mul_no_files`) — 컨텍스트가 없을 때 맵이 더 커야 한다는 역발상

### 에디터 인덱싱
- Cursor: tree-sitter 청킹 + 서버측 임베딩 + **Merkle tree diff로 변경 청크만 재임베딩**, 10분 주기 동기화(⚠️ "5분"·"3-인덱스 하이브리드 +12.5%" 주장은 인용 실확인 탈락) [engineerscodex, 2025-05]
- **Cline의 no-index 3논거 원문 확인**: ① 인덱스=시점 스냅샷이라 drift ② 청킹이 호출/정의/컨텍스트를 파편화 ③ 임베딩 스토어가 보안 표면 2배 [cline.bot 블로그, 2025-06] — CodeLens 반론 재료: watcher 증분 + AST-aware 심볼 단위 + 로컬 SQLite로 3논거 모두 구조적으로 완화됨
- SuperAGI causal-ablation(2026-06, arXiv 2606.22417): 고정 하네스에 **구조적 인덱스 추가만으로 localization 대폭 개선** — "인덱스 무용론"에 대한 실험 반증 [신뢰도 medium]
- GitHub 신형 코드 임베딩(2025-10): InfoNCE + **MRL(Matryoshka)** 학습으로 retrieval +37.6%, 인덱스 메모리 1/8 — 차원 절단 가능한 모델 선택의 근거
- 파일워처 합의: debounce(수백 ms~수 s) + staleness guard 병행 (Codex는 debounce를 1s→10s로 올려 오류 채터 감소)

### 벤치마크 방법론
- **SWE-bench Verified 공식 은퇴**(OpenAI): 오염·결함 오라클·암기. 'SWE-Bench Illusion'(arXiv 2506.12286): o3가 컨텍스트 없이 file-path 76% 적중 = 암기 증거
- 오염 대응 3갈래: live/continuous-refresh(SWE-bench Live 등) · decontaminated(SWE-rebench) · mutation 변형
- **MCP Interviewer**(MS Research, 1,312 도구 실측): 응답 토큰 median 98 / max 557k; 도구 16개가 >128k. 처방 = 서버 카드에 예상 토큰 명시·페이지네이션·스키마 캐싱 — CodeLens는 `estimated_tokens`를 이미 매니페스트에 갖고 있어 반 발 앞서 있으나 **런타임 실측 프로파일러는 없음**
- **SWE-grep(Cognition)**: 실사용 쿼리 + 파일/라인 ground truth에 **Weighted F-β(β=0.5, precision 우선)** + e2e latency 동시 측정 — "context pollution 최소화"를 지표에 내장
- CoIR(ACL 2025): 코드 retrieval 종합 벤치(10 데이터셋·8 태스크) — MRR 하네스의 외부 대조군으로 사용 가능
- 도구 on/off ablation이 표준 설계로 정착; AgentCE-Bench는 tool call을 p=0.1~0.3 확률로 거부해 **도구 불안정성 하 강건성**을 별도 축으로 측정

### Mutation 안전 패턴
- Cline **shadow-git 체크포인트**: 실제 git과 분리된 숨은 리포에 툴 실행마다 자동 커밋, 파일/태스크/양쪽 3모드 복원; Claude Code `/rewind`(파일 내용만 — 권한 복원 주장은 공식문서에서 미확인)
- **dry-run diff preview + 승인의 하드 분리**(propose→preview→approve→apply)가 메인스트림 패턴 (OpenAI Agents SDK `needsApproval`, MS Agent Framework 등)
- GitHub Copilot coding agent: **직무분리 승인**(태스크 할당자 ≠ PR 승인자), 에이전트 전용 브랜치 접두, `actor_is_agent` 감사 식별자
- MCP 스펙 동향: 2025 연속 개정(OAuth 2.1+PKCE→RFC 9728/8707), 2026-07-28 RC에서 Roots/Sampling/Logging deprecated — **MCP 권한 프리미티브에 장기 의존 금지**, CodeLens 자체 RBAC(방금 fail-closed 하드닝)이 옳은 방향

## 2. 이식 후보 12개 — grounding 판정 후 우선순위

전 후보 ADOPT-MODIFIED (스코프 축소 후). "델타"는 코드 대조로 확인된 진짜 부재분만.

### Tier 1 — 즉시 착수 가치 (S~M, 기존 seam 위 배선)

| # | 후보 | 진짜 델타 | 근거 패턴 |
|---|---|---|---|
| 1 | **진단-delta 편집 루프** | `semantic_edit.rs:200-693`의 `pre/post_diagnostics`가 **빈 placeholder로 하드코딩** — 편집 전후 진단 스냅샷·신규도입분 스코핑 배선. 응답에 "edit applied" 명시(재시도 루프 방지) | Hermes, opencode#9102 |
| 2 | **working-set 앵커 랭킹** | `get_ranked_context`에 명시 anchor 파라미터(활성/최근 편집 파일) 추가 → **기존 user_context/recency RRF lane**(`ranked_context.rs:165-265`)에 주입. per-request Personalized PageRank 풀재계산은 defer | Aider personalization |
| 3 | **Python 참조 정밀 라우팅** | 기본 경로(`references.rs:226-482`)가 oxc→SCIP→tree-sitter로 라우팅하며 **pyright는 등록돼 있으나 기본 경로에서 미도달**. warm-pool일 때만 LSP 라우팅(콜드스타트 2-30s를 hot path에 넣지 말 것) + 미가용 시 힌트 | 알려진 약점 직결 |
| 4 | **평가 하네스 확장(저비용분)** | 기존 line-range 라벨 위에 **F-β(0.5) 파일/라인 지표** + **per-tool 응답토큰 프로파일러**(estimated vs actual). live-harvest 태스크 하네스·도구거부 강건성은 defer | SWE-grep, MCP Interviewer |
| 5 | **query-adaptive RRF 채널 가중** | lane 라우팅·분류기는 존재 — 유일 미적용 층인 `rank_fusion.rs`의 **정적 채널 가중(w_sem=1.0/w_sparse=0.8)**만 query-shape 적응 실험. MRR 하네스 게이트 필수(과거 튜닝 REJECT 이력 존중) | Octocode(미검증 수치 주의), 기존 intent.rs 재사용 |

### Tier 2 — 중기 (M)

| # | 후보 | 진짜 델타 |
|---|---|---|
| 6 | **project-binding 집행** | 유일 갭 = #347 binding hint의 advisory 한계. opt-in **warn-then-block**(미바인딩 세션 read-only 강등) + Serena식 trusted-path allowlist. 나머지 3/4파트(worktree nearest-wins·fail-closed 경로검증·name-vs-path)는 이미 구현 |
| 7 | **LSP 수명주기 4갭** | idle-timeout reaper 부재(grep 교차확인) 외 3갭. 공유 warm pool·dead-detect·hard-kill-on-drop·LRU 등 ~60%는 이미 구현 |
| 8 | **SCIP 자동 인덱싱** | 엔진은 load-only, 생성은 Rust 전용 외부 스크립트뿐. SCIP-IO식 언어감지→인덱서 해석→**partial publish** 오케스트레이션을 저위험 슬라이스부터 |
| 9 | **인용파일 staleness 배너 + compaction** | watcher·증분 재인덱싱은 **이미 default-ON** — 델타는 응답에 "인용 파일이 인덱스보다 새로움" 배너와 tombstone/vacuum 수명주기뿐 |
| 10 | **MCP 표면 인체공학(축소판)** | failure≠empty는 이미 MCP-correct(isError+recovery_hint)라 드롭. 잔여 서브파트만(호스트별 structured-output 적응 등) |

### Tier 3 — 설계 선행 (L)

| # | 후보 | 진짜 델타 |
|---|---|---|
| 11 | **mutation 안전층 통합** | `edit_transaction.rs`의 in-memory 단일호출 롤백을 넘어: artifact store 재사용한 **durable 체크포인트/복원 도구**, **dry-run diff preview→승인→apply 분리**(2026-07-03 감사의 in-memory approval 이슈와 동일 뿌리 — 함께 설계), 권한비트 캡처. Phase 2로 self-예고된 부분 |
| 12 | **임베딩 협소 델타** | 기본 모델이 이미 code-specialized(MiniLM-L12-CodeSearchNet-INT8)+bake-off 인프라 존재 — 델타는 신형 모델(CodeRankEmbed/jina-code 등) bake-off 1회와 Greptile식 NL-enrichment **실험**뿐. 전면 스왑 제안은 기각 |

## 3. 연구 premise 정정 — 이미 구현됨 (재제안 금지 목록)

grounding이 반박한 "부재" 주장들. 향후 감사·리서치에서 같은 제안이 나오면 이 목록 먼저 대조할 것.

- **파일워처/증분 재인덱싱**: `watcher.rs`(300ms debounce, VFS rename, tombstone, FTS+graph 무효화) `project_runtime.rs:112` default-ON — **리포 CLAUDE.md "daemon does not auto-watch" 서술이 낡음 → 문서 정정 대상**
- 코드특화 임베딩: 기본값이 CodeSearchNet fine-tuned + INT8, `runtime.rs:568-596` bake-off 경로 존재
- query-shape 분류기·lane 라우팅: `query_analysis/intent.rs` + `ranked_context.rs` 기존재
- worktree nearest-wins 프로젝트 루트: `project.rs:363-393` + 회귀 테스트
- failure≠empty MCP 계약: `error.rs:87-122` + `dispatch/response.rs` — protocol error 재설계는 오히려 계약 위반
- didChange 문서 동기화: `session.rs` sync_document + 회귀 테스트

## 4. Open Questions

- dogfood repo 단독으로 task-resolve 하네스의 태스크 밀도가 충분한가, SWE-bench-Live류 외부 슬라이스가 필요한가
- 대화-anchored working set을 stateless MCP 호출에서 어떤 소스로 받을 것인가(명시 파라미터 vs git 최근 편집 폴백)
- per-request Personalized PageRank의 비용이 105K LOC급 그래프에서 감당 가능한가(측정 필요)
- 청킹이 per-symbol tight인지 확인 후 Greptile 실측(함수 단위 우위)과 정합 여부 판단

## 5. Staleness Note

빠르게 썩는 사실: Serena main 브랜치 기능(릴리스 전), MCP 2026-07-28 RC(변경 가능), Cursor/Copilot 내부 구현(비공식 소스 다수), 임베딩 모델 랭킹(분기 단위 변동), SCIP-IO 버전. 이식 착수 시점에 각 소스 재확인 권장. 벤치마크 합의(오염 대응·F-β·ablation)는 상대적으로 안정.

---
*방법론: 8축 병렬 리서치(WebSearch→선별 WebFetch) → 축별 고신뢰 주장 인용 실확인·반박(66건) → 12후보 병합 → 후보별 repo 코드 대조(2+ 검색어 교차 확인 규칙). 워크플로우 run wf_aa8b5b6d-23d, 2026-07-03.*
