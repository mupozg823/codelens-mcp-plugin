# CodeLens MCP — 2026년 3월 개발 참고 자료

> 최종 업데이트: 2026-03-29
> 대상: CodeLens MCP (Rust, ~22K LoC, 89 tools, 1인 개발)

---

## 1. Rust MCP 생태계 현황

### 1.1 공식 SDK 및 주요 프레임워크

| 프로젝트                                                                                    | Stars | 버전   | 특징                                            | CodeLens 관련성                             |
| ------------------------------------------------------------------------------------------- | ----- | ------ | ----------------------------------------------- | ------------------------------------------- |
| [modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk)           | 3.2k  | 0.16.0 | `rmcp` + `rmcp-macros`, tokio 기반              | **1.0 안정화 후 프로토콜 레이어 교체 후보** |
| [rust-mcp-stack/rust-mcp-filesystem](https://github.com/rust-mcp-stack/rust-mcp-filesystem) | 141   | 0.4.1  | async 파일시스템, 보안 기본값                   | 파일 도구 설계 참고                         |
| [Dicklesworthstone/fastmcp_rust](https://github.com/Dicklesworthstone/fastmcp_rust)         | 16    | —      | cancel-correct async, `#![forbid(unsafe_code)]` | unsafe 제거 패턴 참고                       |
| [JSBtechnologies/FastRMCP](https://github.com/JSBtechnologies/FastRMCP)                     | 초기  | —      | FastAPI 스타일 DX, STDIO/SSE/WebSocket          | Transport 확장 참고                         |
| ultrafast-mcp (docs.rs)                                                                     | —     | —      | MCP 2025-06-18 스펙, OAuth 2.1, 멀티 트랜스포트 | 최신 스펙 구현 참고                         |

### 1.2 Rust + AI Agent 프레임워크

| 프로젝트                                                            | Stars | 특징                                                   |
| ------------------------------------------------------------------- | ----- | ------------------------------------------------------ |
| [liquidos-ai/AutoAgents](https://github.com/liquidos-ai/AutoAgents) | 489   | 멀티에이전트, Python 대비 36% 높은 rps, 5x 메모리 절감 |
| [Rig](https://rig.rs/)                                              | —     | 모듈러 LLM 앱 프레임워크, MCP 통합                     |
| [ldclabs/anda](https://github.com/ldclabs/anda)                     | —     | AI agent + ICP 블록체인 + TEE                          |

### 1.3 Rust MCP 개발 참고 글

- [How to Build an MCP Server in Rust (2026-01)](https://oneuptime.com/blog/post/2026-01-07-rust-mcp-server/view)
- [Shuttle: Streamable HTTP MCP Server in Rust](https://www.shuttle.dev/blog/2025/10/29/stream-http-mcp)
- [Six Months of Running MCP Servers in Rust (Medium)](https://ed-burton.medium.com/six-months-of-running-mcp-servers-in-rust-what-id-do-differently-1ee52f68225a)
- [MCP in Rust Practical Guide (HackMD)](https://hackmd.io/@Hamze/SytKkZP01l)
- [MCP Rust SDK Template](https://github.com/linux-china/mcp-rs-template) — 프로젝트 스캐폴딩

---

## 2. MCP 스펙 업데이트 (현재 기준: 2025-11-25)

> 이 문서는 2026년 3월 당시 조사 자료라 일부 외부 생태계 메모는 역사적 맥락을 유지한다.
> CodeLens 구현 기준은 현재 MCP 2025-11-25이며, 2025-06-18/2025-03-26 클라이언트는 하위 호환으로 협상한다.

### 2.1 CodeLens 구현 상태

| 기능                 | 스펙 상태 | CodeLens 상태                                                                       | 우선순위                |
| -------------------- | --------- | ----------------------------------------------------------------------------------- | ----------------------- |
| Tool Annotations     | 확정      | ✅ 구현 완료 (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) | —                       |
| JSON-RPC Batching    | 확정      | ✅ 구현 완료 (stdio 배열 감지)                                                      | —                       |
| Resources            | 확정      | ✅ 구현 완료 (3개: project/overview, symbols/index, tools/list)                     | —                       |
| Prompts              | 확정      | ✅ 구현 완료 (3개: review-file, onboard-project, analyze-impact)                    | —                       |
| ProgressNotification | 확정      | ⚠️ 수신만 가능, 발신 미구현                                                         | **High**                |
| Streamable HTTP      | 확정      | ✅ 구현 완료 (POST/GET SSE/DELETE + session resume)                                 | —                       |
| Elicitation          | 확정      | ❌ 미구현                                                                           | Low                     |
| OAuth protected resource | 확정  | ✅ Bearer/JWKS 검증 + protected resource metadata                                   | 원격 배포 필수          |

### 2.2 참고 문서

- [MCP Specification 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25)
- [MCP Streamable HTTP 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [MCP Authorization 2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
- [MCP Protocol Upgrade Guide (hermes_mcp)](https://hexdocs.pm/hermes_mcp/0.4.0/protocol_upgrade_2025_03_26.html)
- [MCP Auth Spec Review (Logto)](https://blog.logto.io/mcp-auth-spec-review-2025-03-26)
- [Streamable HTTP MCP Template (GitHub)](https://github.com/iceener/streamable-mcp-server-template)
- [Everything about MCP in 2026 (WorkOS)](https://workos.com/blog/everything-your-team-needs-to-know-about-mcp-in-2026)

---

## 3. Claude Code / 에이전트 코딩 도구 트렌드

### 3.1 Claude Code 생태계 (2026 Q1)

| 기능                    | 설명                                     | CodeLens 활용                      |
| ----------------------- | ---------------------------------------- | ---------------------------------- |
| Skills                  | 재사용 가능한 명령 세트, 1,367+ 커뮤니티 | CodeLens에 3개 Skill 구현 완료     |
| Plugins                 | Skills + Agents + Hooks + MCP 번들, 340+ | **Marketplace 등록 최우선**        |
| Hooks                   | 이벤트 기반 자동화 (PostToolUse 등)      | post-edit-diagnostics.sh 구현 완료 |
| Agent Teams             | 병렬 멀티에이전트 조율                   | codelens-explorer 구현 완료        |
| Scheduled Tasks (/loop) | 크론 스타일 반복 자동화                  | 자동 인덱스 갱신에 활용 가능       |
| Remote Control          | 웹에서 로컬 세션 원격 제어               | 해당없음                           |

**참고:**

- [Claude Code Docs - Scheduled Tasks](https://code.claude.com/docs/en/web-scheduled-tasks)
- [Claude Agent SDK Overview](https://platform.claude.com/docs/en/agent-sdk/overview)
- [Mental Model for Skills, Subagents, Plugins](https://levelup.gitconnected.com/a-mental-model-for-claude-code-skills-subagents-and-plugins-3dea9924bf05)
- [Running Claude Agents in Production (Medium)](https://medium.com/@hugolu87/how-to-run-claude-agents-in-production-using-the-claude-sdk-756f9d3c93d8)
- [Claude Code Plugins + Skills Collection](https://github.com/jeremylongshore/claude-code-plugins-plus-skills)

### 3.2 경쟁 코딩 도구 비교

| 도구        | 월 구독  | 강점                                    | SWE-bench | 사용자       |
| ----------- | -------- | --------------------------------------- | --------- | ------------ |
| Claude Code | $100-200 | Opus 4.6, 1M 토큰 컨텍스트, 에이전트 팀 | 80.8%     | 엔터프라이즈 |
| Cursor      | $20      | IDE 통합, Supermaven 자동완성, Composer | —         | 1M+          |
| Windsurf    | $15      | SWE-1.5 추론 13x 빠름, 예산형           | —         | 성장 중      |
| Cline       | 모델비용 | 오픈소스, VS Code 500만 설치            | —         | 개발자       |

**핵심 인사이트:** 대부분의 개발자가 2+ 도구 병행 사용. CodeLens의 멀티 클라이언트 지원 전략(Claude Code, Cursor, Windsurf, Cline) 유효.

**참고:**

- [Cursor vs Windsurf vs Claude Code 2026 비교](https://dev.to/pockit_tools/cursor-vs-windsurf-vs-claude-code-in-2026-the-honest-comparison-after-using-all-three-3gof)
- [Best AI Coding Agents 2026 (Faros)](https://www.faros.ai/blog/best-ai-coding-agents-2026)
- [AI Dev Tool Power Rankings (LogRocket)](https://blog.logrocket.com/ai-dev-tool-power-rankings/)

---

## 4. Agentic Coding 패턴

### 4.1 PEV Loop (Plan → Execute → Validate)

에이전트가 독립적으로 계획 → 실행 → 자기검증. 인간의 매 단계 프롬프트가 불필요해지는 패턴.

**CodeLens 매핑:**

- Plan: `get_symbols_overview` → `get_ranked_context` (컨텍스트 파악)
- Execute: `rename_symbol`, `replace_symbol_body` (코드 변경)
- Validate: `get_file_diagnostics`, `find_referencing_symbols` (영향 확인)

### 4.2 Multi-Agent Orchestra

```
Feature Author ─→ Test Generator ─→ Code Reviewer ─→ Security Scanner
                                         ↑
                                   codelens-explorer
                                   (이미 구현됨)
```

### 4.3 Cost-Per-First-Pass 최적화

"정확한 솔루션까지의 총 토큰 비용"이 핵심 지표.
CodeLens의 기여:

- `get_ranked_context`: 토큰 예산 내 최적 컨텍스트 선택
- `suggested_next_tools`: 도구 체이닝 가이드로 불필요한 호출 감소
- 프리셋 시스템: Minimal(20) → Balanced(55) → Full(89)로 토큰 경쟁 최소화

### 4.4 참고 문서

- [Anthropic 2026 Agentic Coding Trends Report (PDF)](https://resources.anthropic.com/hubfs/2026%20Agentic%20Coding%20Trends%20Report.pdf)
- [Code Agent Orchestra (Addy Osmani)](https://addyosmani.com/blog/code-agent-orchestra/)
- [Agentic Engineering Complete Guide (NxCode)](https://www.nxcode.io/resources/news/agentic-engineering-complete-guide-vibe-coding-ai-agents-2026)
- [12 AI Coding Emerging Trends 2026 (Medium)](https://medium.com/aimonks/12-ai-coding-emerging-trends-that-will-dominate-2026-7b3330af4b89)

---

## 5. MCP 생태계 통계 및 보안

### 5.1 성장 수치

- 월간 SDK 다운로드: **9,700만**
- 인덱싱된 MCP 서버: **4,133개** (2025 중반 425개 → 873% 성장)
- Linux Foundation 거버넌스 하에 Anthropic, OpenAI, Google, Microsoft 공동 지원
- 42/50 인기 서버가 엔지니어링 워크플로우 대상

### 5.2 보안 이슈

2025년 9월 비공식 Postmark MCP 서버 사건: 주간 1,500 다운로드 서버에 blind CC 추가 코드 삽입.
MCP 서버 공급망 보안이 2026년 주요 거버넌스 이슈.

**CodeLens 시사점:**

- 코드 서명 또는 체크섬 제공 고려
- 빌드 재현성 (reproducible builds) 확보
- `#![deny(unsafe_code)]` 적용으로 신뢰도 향상

### 5.3 참고 문서

- [50 Most Popular MCP Servers](https://mcpmanager.ai/blog/most-popular-mcp-servers/)
- [MCP Roadmap 2026 (TheNewStack)](https://thenewstack.io/model-context-protocol-roadmap-2026/)
- [Enterprise MCP Adoption (CData)](https://www.cdata.com/blog/2026-year-enterprise-ready-mcp-adoption)
- [MCP Security Discussion (Qualys)](https://blog.qualys.com/product-tech/2026/03/19/mcp-servers-shadow-it-ai-qualys-totalai-2026)

---

## 6. Rust 패턴 — MCP 서버에서 채택되고 있는 것들

### 6.1 비동기 런타임

```
tokio ──────────── 지배적 (공식 SDK, 대부분의 프레임워크)
asupersync ─────── 신흥 (cancel-correct structured concurrency)
```

CodeLens 현재: `tokio` (http feature에서만). stdio는 동기. **변경 불필요.**

### 6.2 에러 처리 패턴

```rust
// 2026 권장 패턴 (CodeLens가 이미 채택)
#[derive(Debug, thiserror::Error)]
enum ServerError {
    #[error("...")] Variant(String),    // 도메인 에러
    #[error(transparent)] Internal(#[from] anyhow::Error),  // 내부 에러
}
impl ServerError {
    fn jsonrpc_code(&self) -> i64 { ... }  // JSON-RPC 매핑
}
```

**CodeLens 상태:** ✅ `error.rs`에서 이미 구현. 업계 모범 사례와 일치.

### 6.3 stdout 보호 패턴

```
✅ stdout → JSON-RPC 전용
✅ stderr → 로깅/디버그
```

MCP 서버에서 가장 흔한 버그: stdout에 log 출력이 섞여 JSON 파싱 실패.
CodeLens는 `eprintln!` 사용으로 이미 준수.

### 6.4 프로시저 매크로 (`#[tool]`)

```rust
// 공식 rmcp-macros 패턴
#[tool(name = "find_symbol")]
async fn find_symbol(&self, name: String) -> Result<ToolResult> { ... }
```

**CodeLens 판단:** rmcp 1.0 안정화 전까지 자체 구현 불필요. 현재 `tools()` 함수 방식 유지.

### 6.5 LazyLock 디스패치 테이블

```rust
// CodeLens가 이미 채택한 패턴
static DISPATCH_TABLE: LazyLock<HashMap<&'static str, ToolHandler>> =
    LazyLock::new(|| { ... });
```

**CodeLens 상태:** ✅ 이미 적용. `tools()` 결과도 동일하게 `LazyLock` 캐싱 권장.

---

## 7. CodeLens 액션 로드맵 (우선순위)

### 즉시 (이번 주)

| #   | 작업                                                     | 예상 시간 | 근거                     |
| --- | -------------------------------------------------------- | --------- | ------------------------ |
| 1   | **Plugin Marketplace 등록**                              | 1시간     | 코드 변경 0, 노출도 최대 |
| 2   | **`tools()` → `LazyLock` 캐싱**                          | 15분      | 3줄 수정, 확실한 개선    |
| 3   | **BALANCED 프리셋 카운트 통일** (CLAUDE.md vs 코드 주석) | 5분       | 문서 정합성              |

### 단기 (2주 내)

| #   | 작업                                                          | 예상 시간 | 근거                                |
| --- | ------------------------------------------------------------- | --------- | ----------------------------------- |
| 4   | **ProgressNotification 발신** (`refresh_symbol_index`에만)    | 2시간     | 대형 프로젝트 UX                    |
| 5   | **벤치마크 수치 README 추가** (자체 수치만, 경쟁사 비교 없이) | 1시간     | cold start, indexing, query latency |
| 6   | **`#![deny(unsafe_code)]` 적용** (`codelens-mcp` 크레이트)    | 30분      | 신뢰도, ffi 모듈 이미 분리됨        |

### 하지 않을 것

| 작업                      | 이유                           |
| ------------------------- | ------------------------------ |
| proc macro 자체 구현      | rmcp 1.0 기다리면 됨           |
| 추가 legacy SSE transport | Streamable HTTP가 표준 경로    |
| rmcp 마이그레이션         | 0.16.0 불안정                  |
| Security Scanner 에이전트 | 기대치 대비 실효성 부족        |
| OAuth authorization server | 외부 issuer/JWKS 검증만 담당   |
| 경쟁사 벤치마크 비교표    | 공정한 비교 불가, 오해 소지    |

---

## 8. 경쟁 프로젝트 추적

### 코드 인텔리전스 MCP 서버

| 프로젝트                                                                        | 언어       | 도구 수 | 특징                       | 위협도         |
| ------------------------------------------------------------------------------- | ---------- | ------- | -------------------------- | -------------- |
| [wrale/mcp-server-tree-sitter](https://github.com/wrale/mcp-server-tree-sitter) | Python     | ~10     | tree-sitter 기반, 15+ 언어 | 중             |
| [nendotools/tree-sitter-mcp](https://github.com/nendotools/tree-sitter-mcp)     | —          | —       | 구조적 데이터 노출         | 낮             |
| mcp-language-server                                                             | TypeScript | ~15     | LSP 통합                   | 중             |
| jCodeMunch                                                                      | —          | —       | $79+ 유료                  | 낮 (가격 장벽) |

**CodeLens 차별점:** Rust(성능), 50개 도구(범위), 프리셋(유연성), 플러그인 생태계(Skills/Agent/Hook)

### Rust 개발자 도구 트렌드

| 프로젝트          | 분야      | 참고                                   |
| ----------------- | --------- | -------------------------------------- |
| Zed               | 에디터    | Tree-sitter 창시자들이 만든 에디터     |
| rust-analyzer-mcp | Rust 분석 | Rust 전용 MCP 통합                     |
| Qdrant            | 벡터 DB   | CodeLens semantic search와 유사 도메인 |

---

## 9. 크레이트 의존성 체크리스트

현재 의존성 중 주의가 필요한 항목:

| 크레이트      | 현재 버전     | 상태                  | 조치                                   |
| ------------- | ------------- | --------------------- | -------------------------------------- |
| `sqlite-vec`  | 0.1.8-alpha.1 | **alpha**             | safe API 제공 시 transmute 제거        |
| `fastembed`   | —             | semantic feature 전용 | 23MB 모델 다운로드, 오프라인 빌드 주의 |
| `tree-sitter` | —             | 안정                  | 16개 언어 바인딩 유지 비용 확인        |
| `thiserror`   | 2             | 안정                  | ✅                                     |
| `rayon`       | —             | 안정                  | ✅                                     |
| `petgraph`    | —             | 안정                  | ✅                                     |

---

## 10. 유용한 Rust 크레이트 (미채택, 고려 대상)

| 크레이트   | 용도                                | 도입 시기               |
| ---------- | ----------------------------------- | ----------------------- |
| `schemars` | Rust 타입에서 JSON Schema 자동 생성 | rmcp 전환 시            |
| `tracing`  | 구조화된 로깅 (현재 eprintln)       | 디버깅 어려워질 때      |
| `tower`    | 미들웨어 스택 (auth, rate-limit)    | 원격 서버 모드 시       |
| `axum-sse` | SSE 스트리밍                        | Streamable HTTP 필요 시 |

---

_이 문서는 CodeLens MCP 개발 참고용이며, 트렌드와 링크는 2026년 3월 29일 기준입니다._
