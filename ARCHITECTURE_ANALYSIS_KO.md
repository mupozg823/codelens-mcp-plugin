# CodeLens MCP Plugin v1.9.59 아키텍처 분석 보고서

## 1. 전체 폴더 스캐폴딩 및 모듈 구조 요약

### 1.1 워크스페이스 구조

```
codelens-mcp-plugin/
├── Cargo.toml                          # 워크스페이스 루트 (v1.9.59)
│
├── crates/
│   ├── codelens-engine/                # 코어 엔진 크레이트
│   │   ├── src/
│   │   │   ├── lib.rs                  # 공개 API 표면 (25 개 언어 지원)
│   │   │   ├── project.rs              # 프로젝트 루트, 프레임워크 감지
│   │   │   ├── lang_registry.rs        # 30 개 언어 패밀리 등록
│   │   │   ├── lang_config.rs          # tree-sitter Language + Query
│   │   │   ├── symbols/                # 심볼 추출 및 랭킹
│   │   │   ├── db/                     # SQLite + FTS5 + sqlite-vec
│   │   │   ├── search.rs               # 하이브리드 검색 (FTS5 + jaro_winkler)
│   │   │   ├── import_graph/           # 의존성 그래프 (petgraph)
│   │   │   ├── lsp/                    # LSP 통합 (옵셔널)
│   │   │   ├── file_ops/               # 파일 I/O + 뮤테이션
│   │   │   ├── embedding.rs            # 임베딩 엔진 (MiniLM + fastembed)
│   │   │   ├── embedding_store.rs      # sqlite-vec 저장소
│   │   │   ├── call_graph.rs           # 함수 호출 그래프
│   │   │   ├── circular.rs             # Tarjan SCC 순환 감지
│   │   │   ├── coupling.rs             # Git 시간적 커플링
│   │   │   ├── type_hierarchy.rs       # 네이티브 상속 분석
│   │   │   ├── rename.rs               # 멀티파일 리네임 엔진
│   │   │   ├── auto_import.rs          # 누락된 import 감지
│   │   │   ├── change_signature.rs     # 시그니처 변경 리팩토링
│   │   │   ├── inline.rs               # 함수 인라인 리팩토링
│   │   │   ├── move_symbol.rs          # 심볼 이동 리팩토링
│   │   │   ├── git.rs                  # Git diff/변경 파일
│   │   │   ├── scope_analysis.rs       # def/read/write/import 분류
│   │   │   ├── watcher.rs              # 파일 감시 (notify + 디바운스)
│   │   │   ├── oxc_analysis.rs         # JS/TS 시맨틱 분석 (oxc)
│   │   │   ├── scip_backend.rs         # SCIP 정밀 백엔드 (기능 게이트)
│   │   │   └── ir.rs                   # 시맨틱 IR 타입
│   │   └── Cargo.toml                  # 25 tree-sitter deps, rusqlite, ort
│   │
│   ├── codelens-mcp/                   # MCP 서버 크레이트
│   │   ├── src/
│   │   │   ├── main.rs                 # 진입점, 전송 모드, 설정
│   │   │   ├── state.rs                # AppState (950 줄)
│   │   │   ├── state/                  # 프로젝트/세션/프리플라이트 서비스
│   │   │   ├── dispatch/               # 도구 디스패치 파이프라인
│   │   │   │   ├── mod.rs              # 메인 디스패치 로직
│   │   │   │   ├── envelope.rs         # 요청 파싱
│   │   │   │   ├── validation.rs       # 스키마 사전 검증
│   │   │   │   ├── rate_limit.rs       # 속도 제한 + doom-loop 해시
│   │   │   │   ├── table.rs            # 정적 디스패치 테이블
│   │   │   │   ├── session.rs          # 세션 컨텍스트 + 뮤테이션 게이트
│   │   │   │   ├── role_gate.rs        # ADR-0009 역할 게이트
│   │   │   │   ├── query_engine.rs     # 도구 실행 오케스트레이션
│   │   │   │   ├── access.rs           # 표면 접근 검증
│   │   │   │   └── response.rs         # 응답 포맷팅
│   │   │   ├── mutation_gate.rs        # 변경 안전성 검증 (270 줄)
│   │   │   ├── tool_defs/              # 도구 정의
│   │   │   │   ├── mod.rs              # 도구 등록 + 표면 필터링
│   │   │   │   ├── build.rs            # 112 개 도구 정의
│   │   │   │   ├── output_schemas.rs   # 82 개 출력 스키마
│   │   │   │   └── presets.rs          # 프리셋/프로필 정의
│   │   │   ├── tools/                  # 도구 핸들러 구현
│   │   │   │   ├── symbols.rs          # 심볼 조회
│   │   │   │   ├── workflows.rs        # 워크플로우 별칭
│   │   │   │   ├── lsp.rs              # LSP 백엔드
│   │   │   │   ├── graph.rs            # 분석 그래프
│   │   │   │   ├── filesystem.rs       # 파일 시스템
│   │   │   │   ├── mutation.rs         # 코드 편집
│   │   │   │   ├── composite.rs        # 복합 워크플로우
│   │   │   │   ├── report_contract.rs  # 분석 핸들 계약
│   │   │   │   ├── report_verifier.rs  # 검증자 우선 뮤테이션 게이트
│   │   │   │   └── session/            # 세션 범위 핸들러
│   │   │   ├── server/                 # 전송 계층
│   │   │   │   ├── transport_stdio.rs  # stdio 전송
│   │   │   │   ├── transport_http.rs   # Streamable HTTP + SSE
│   │   │   │   ├── session.rs          # HTTP 세션 관리
│   │   │   │   └── oneshot.rs          # CLI 원샷 모드
│   │   │   ├── protocol.rs             # 도구, 스키마 정의
│   │   │   ├── error.rs                # CodeLensError enum
│   │   │   ├── telemetry.rs            # 도구 메트릭 레지스트리
│   │   │   ├── preflight_store.rs      # 프리플라이트 TTL 캐시
│   │   │   ├── analysis_queue.rs       # 내구성 분석 작업 큐
│   │   │   ├── artifact_store.rs       # 분석 핸들 저장소
│   │   │   ├── job_store.rs            # 작업 지속성
│   │   │   ├── session_context.rs      # 세션 범위 상태
│   │   │   ├── recent_buffer.rs        # Doom-loop 감지
│   │   │   ├── client_profile.rs       # 클라이언트 식별 휴리스틱
│   │   │   ├── authority.rs            # 백엔드 메타데이터
│   │   │   ├── principals.rs           # ADR-0009 원칙 resolver
│   │   │   └── audit_sink.rs           # ADR-0009 감사 로그
│   │   └── Cargo.toml                  # axum, tokio, serde_json
│   │
│   └── codelens-tui/                   # TUI 크레이트 (옵셔널)
│       ├── src/
│       │   ├── main.rs
│       │   ├── app.rs
│       │   ├── ui.rs
│       │   └── watch.rs
│       └── Cargo.toml
│
├── docs/
│   ├── architecture.md                 # 아키텍처 문서
│   ├── adr/                            # 아키텍처 결정 기록
│   │   ├── ADR-0001-runtime-boundaries.md
│   │   ├── ADR-0004-multi-agent-concurrency.md
│   │   ├── ADR-0006-agent-routing-enforcement.md
│   │   └── ADR-0009-mutation-trust-substrate.md
│   ├── release-notes/                  # 버전별 릴리스 노트
│   ├── multi-agent-integration.md      # 멀티 에이전트 통합
│   └── harness-spec.md                 # 하네스 명세
│
├── benchmarks/                         # 성능 벤치마크
│   ├── embedding-quality.py            # MRR / Acc@k 측정
│   ├── embedding-runtime.py            # 지연시간/처리량
│   └── results/                        # 결과 스냅샷
│
├── models/                             # ONNX 모델 자산 (INT8)
├── scripts/                            # 빌드/배포 스크립트
└── agents/                             # 에이전트 계약
```

### 1.2 모듈 의존성 그래프

```
┌─────────────────────────────────────────────────────────────────┐
│                    codelens-mcp (MCP 서버)                       │
│                                                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
│  │   main.rs   │→ │  dispatch/  │→ │   tools/ (112 개)        │  │
│  │  (진입점)    │  │  (파이프라인) │  │   (핸들러 구현)          │  │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
│         │                │                      │                │
│         ▼                ▼                      ▼                │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                    state.rs (AppState)                       ││
│  │  - SymbolIndex, GraphCache, LspSessionPool                  ││
│  │  - EmbeddingEngine, FileWatcher                             ││
│  │  - AuditSink, Principals, PreflightStore                    ││
│  │  - AnalysisQueue, ArtifactStore, JobStore                   ││
│  └─────────────────────────────────────────────────────────────┘│
│                              │                                   │
│                              ▼                                   │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │                  codelens-engine (코어 엔진)                  ││
│  │  - tree-sitter (30 개 언어)                                   ││
│  │  - SQLite + FTS5 + sqlite-vec                               ││
│  │  - LSP 세션 풀 (옵셔널)                                      ││
│  │  - 임베딩 (MiniLM-L12-CodeSearchNet-INT8)                    ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. 주요 컴포넌트간 관계 및 데이터 흐름

### 2.1 컴포넌트 상호작용 다이어그램

```
┌──────────────────────────────────────────────────────────────────────┐
│                         AI Agent (Claude/Codex)                       │
│              "find_symbol(name='dispatch_tool')"                      │
└────────────────────────┬─────────────────────────────────────────────┘
                         │ MCP tools/call (JSON-RPC 2.0)
                         ▼
┌──────────────────────────────────────────────────────────────────────┐
│                      Transport Layer                                  │
│         ┌─────────────┬──────────────┬────────────────────┐          │
│         │   stdio     │  HTTP+SSE    │   CLI oneshot      │          │
│         └─────────────┴──────────────┴────────────────────┘          │
└────────────────────────┬─────────────────────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────────────────────┐
│                    server/router.rs                                   │
│              tools/list | tools/call | resources/read                │
└────────────────────────┬─────────────────────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────────────────────┐
│                   dispatch/mod.rs (257 줄)                            │
│  ┌────────────────────────────────────────────────────────────────┐  │
│  │  1. ToolCallEnvelope.parse()  ← 요청 파싱                      │  │
│  │  2. check_rate_limit()        ← 속도 제한 (300 calls/min)      │  │
│  │  3. collect_session_context() ← doom-loop, 표면, 최근 도구     │  │
│  │  4. enforce_role_gate()       ← ADR-0009 역할 검증             │  │
│  │  5. validate_tool_access()    ← 표면/네임스페이스/티어 검증    │  │
│  │  6. validate_required_params()← 스키마 필수 필드 검증          │  │
│  │  7. QueryEngine.submit()      ← 도구 실행 + 뮤테이션 게이트    │  │
│  │  8. apply_post_mutation()     ← 사후 처리 (감사, 무효화)      │  │
│  │  9. build_response()          ← 응답 포맷팅 + _meta            │  │
│  └────────────────────────────────────────────────────────────────┘  │
└────────────────────────┬─────────────────────────────────────────────┘
                         │
        ┌────────────────┼────────────────┐
        │                │                │
        ▼                ▼                ▼
┌───────────────┐ ┌───────────────┐ ┌───────────────┐
│ SymbolIndex   │ │ ImportGraph   │ │ LspSessionPool│
│ (SQLite+FTS5) │ │ (petgraph)    │ │ (옵셔널)      │
│ - 심볼 파싱   │ │ - PageRank    │ │ - 정의/참조   │
│ - 랭킹        │ │ - SCC         │ │ - 타입 계층   │
│ - 검색        │ │ - Dead Code   │ │ - 리네임      │
└───────┬───────┘ └───────┬───────┘ └───────┬───────┘
        │                 │                 │
        └─────────────────┼─────────────────┘
                          │
                          ▼
              ┌───────────────────────┐
              │   tree-sitter         │
              │   30 개 언어 패밀리     │
              │   (정적 링킹, 제로설정) │
              └───────────────────────┘
```

### 2.2 AppState 의존성 주입

```rust
pub(crate) struct AppState {
    // 프로젝트 컨텍스트 (기본 + 런타임 오버라이드)
    default_project: ProjectRoot,
    default_symbol_index: Arc<SymbolIndex>,
    default_graph_cache: Arc<GraphCache>,
    default_lsp_pool: Arc<LspSessionPool>,
    project_override: RwLock<Option<Arc<ProjectRuntimeContext>>>,
    
    // 런타임 모드
    transport_mode: Mutex<RuntimeTransportMode>,  // stdio | http | https
    daemon_mode: Mutex<RuntimeDaemonMode>,        // standard | read-only | mutation-enabled
    surface: Mutex<ToolSurface>,                  // 프리셋 | 프로필
    
    // 세션 관리
    recent_tools: RecentRingBuffer,      // doom-loop 감지 (max 5)
    recent_files: RecentRingBuffer,      // 랭킹 부스트 (max 20)
    doom_loop_counter: Mutex<HashMap>,   // (tool, args_hash) → count
    
    // ADR-0009 감사
    audit_sinks: Mutex<HashMap<PathBuf, Arc<AuditSink>>>,
    principals_by_audit_dir: Mutex<HashMap<PathBuf, Arc<Principals>>>,
    
    // 분석 파이프라인
    artifact_store: AnalysisArtifactStore,
    job_store: AnalysisJobStore,
    analysis_queue: OnceLock<AnalysisWorkerQueue>,
    preflight_store: RecentPreflightStore,
    
    // 멀티 에이전트 조정
    coord_store: Arc<AgentCoordinationStore>,
    
    // 시맨틱 (기능 게이트)
    #[cfg(feature = "semantic")]
    embedding: RwLock<Option<EmbeddingEngine>>,
}
```

---

## 3. 핵심 파이프라인 (도구 호출 → 게이트 → 실행 → 응답)

### 3.1 풀 디스패치 파이프라인

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    MCP 도구 호출 파이프라인 (v1.9.59)                    │
└─────────────────────────────────────────────────────────────────────────┘

  Agent Request
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 1: 요청 파싱 (dispatch/envelope.rs)                                │
│  - ToolCallEnvelope::parse(params, state)                                │
│  - session_id, tool_name, arguments 추출                                 │
│  - budget, compact, harness_phase 파싱                                   │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 2: 속도 제한 (dispatch/rate_limit.rs)                              │
│  - check_rate_limit(state, session)                                      │
│  - sliding window: 300 calls/minute                                      │
│  - doom-loop 해시: 동일 (tool, args) 연속 호출 감지                      │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 3: 세션 컨텍스트 수집 (dispatch/session.rs)                        │
│  - collect_session_context()                                             │
│  - doom_count, doom_rapid 계산                                           │
│  - active_surface, recent_tools 수집                                     │
│  - 파일 액세스 기록 (랭킹 부스트용)                                       │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 4a: 역할 게이트 (dispatch/role_gate.rs) - ADR-0009 §1              │
│  - enforce_role_gate(state, name, arguments, session, surface)           │
│  - principals.toml 에서 principal → role 해결                            │
│  - required_role_for(tool) vs principal_role 비교                        │
│  - 거부 시: audit_row(denied) 기록 + JSON-RPC -32008 오류                │
│                                                                          │
│  Role hierarchy: ReadOnly < Refactor < Admin                             │
│  - ReadOnly: analyze_*, find_*, get_*, semantic_search                   │
│  - Refactor: ReadOnly + 9 raw_fs primitives + LSP rename                 │
│  - Admin: Refactor + audit_log_query + job control                       │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 4b: 도구 접근 검증 (dispatch/access.rs)                            │
│  - validate_tool_access(name, session, surface, state)                   │
│  - 표면 (preset/profile) 기반 도구 가시성 확인                           │
│  - 네임스페이스/티어/데몬 모드 검증                                      │
│  - read-only 모드에서 뮤테이션 도구 차단                                 │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 5: 스키마 사전 검증 (dispatch/validation.rs)                       │
│  - validate_required_params(name, arguments)                             │
│  - 도구 입력 스키마의 필수 필드 확인                                     │
│  - MissingParam 오류 시 빠른 실패                                        │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 6: 도구 실행 + 뮤테이션 게이트 (dispatch/query_engine.rs)          │
│  - QueryEngine::submit_message(name, arguments, session, surface)        │
│  - DISPATCH_TABLE 에서 핸들러 조회                                       │
│  - is_refactor_gated_mutation_tool(name) 확인                            │
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │  뮤테이션 게이트 프로토콜 (mutation_gate.rs)                        │  │
│  │  1. recent_preflight_for_session() 확인                            │  │
│  │  2. TTL 검증 (기본 10 분, CODELENS_PREFLIGHT_TTL_SECS)              │  │
│  │  3. path_overlap 검증 (변경 대상 경로 커버리지)                     │  │
│  │  4. symbol-aware preflight (rename_symbol 전용)                     │  │
│  │  5. readiness.mutation_ready 확인 (ready/caution/blocked)          │  │
│  │                                                                     │  │
│  │  실패 유형:                                                          │  │
│  │  - MissingPreflight: verify_change_readiness 실행 필요              │  │
│  │  - StalePreflight: TTL 초과                                        │  │
│  │  - PathMismatch: 프리플라이트가 대상 경로를 커버하지 않음           │  │
│  │  - SymbolPreflightRequired: safe_rename_report 필요                │  │
│  │  - VerifierBlocked: 검증자가 명시적으로 차단                       │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  - tool.execute(state, arguments) 호출                                   │
│  - (result, gate_allowance, gate_failure) 반환                           │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 7: 사후 처리 (dispatch/session.rs)                                 │
│  - apply_post_mutation(state, name, arguments, session, surface, payload)│
│                                                                          │
│  ┌────────────────────────────────────────────────────────────────────┐  │
│  │  캐시 무효화 계약 (ADR-0009 §4)                                     │  │
│  │  - graph_cache().invalidate()           → PageRank 그래프          │  │
│  │  - symbol_index().refresh_file(path)    → 심볼 DB 증분 재인덱싱    │  │
│  │  - db().invalidate_fts()                → BM25/FTS5 메타 리셋       │  │
│  │  - embedding.index_changed_files()      → 임베딩 증분 재인덱싱     │  │
│  │  - clear_recent_preflights()            → 프리플라이트 캐시 클리어  │  │
│  └────────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  감사 로그 기록:                                                          │
│  - audit_sink().write(AuditRecord)                                       │
│  - transaction_id, principal, tool, args_hash, apply_status              │
│  - state_from, state_to, evidence_hash, rollback_restored                │
│  - session_metadata (project_scope, surface, client_name, ...)           │
│                                                                          │
│  응답 주입:                                                               │
│  - inject_transaction_id(payload, tx_id)                                 │
│  - inject_invalidated_paths(payload, paths)                              │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
┌──────────────────────────────────────────────────────────────────────────┐
│  STEP 8: 응답 빌드 (dispatch/response.rs)                                │
│  - build_success_response() / build_error_response()                     │
│  - _meta 필드 추가:                                                      │
│    - anthropic/maxResultSizeChars                                        │
│    - codelens/backend_used (structural | semantic | lsp)                │
│    - codelens/latency_ms                                                 │
│    - codelens/doom_loop_count                                            │
│    - codelens/suggested_next_tools                                       │
│    - codelens/reasoning_scaffold (플래너/리뷰어 전용)                    │
│  - 5 단계 토큰 압축 (Stage 1-5)                                           │
└──────────────────────────────────────────────────────────────────────────┘
       │
       ▼
  Agent Response
```

### 3.2 뮤테이션 게이트 상태 머신 (ADR-0009)

```
                    role_gate_denied
       (request) ─────────────────────► Denied   (terminal)
       
       (request)
          │ role_gate_passed
          ▼
      Verifying
     ┌────┴─────┐
verify │          │ verify_passed
failed │          ▼
       │       Applying
       │      ┌────┴────┐
       │      │         │ apply_succeeded
       │      │         ▼
       │      │      Audited      (terminal — "applied" or "no_op")
       │      │
       │      │ apply_failed_restored
       │      ▼
       │   RolledBack    (terminal — "rolled_back")
       ▼
     Failed              (terminal — handler Err
                          OR apply_failed_lost)

상태 전이 감사 로그 예시:
{
  "transaction_id": "sess-abc123-rename_symbol-7f8a9b0c",
  "timestamp_ms": 1714387200000,
  "principal": "user@example.com",
  "tool": "rename_symbol",
  "args_hash": "7f8a9b0c1d2e3f4a...",
  "apply_status": "applied",
  "state_from": "Applying",
  "state_to": "Audited",
  "evidence_hash": "sha256(...)",
  "rollback_restored": null,
  "error_message": null,
  "session_metadata": {
    "project_scope": "codelens-mcp-plugin",
    "surface": "refactor-full",
    "daemon_mode": "mutation-enabled",
    "client_name": "claude-code"
  }
}
```

---

## 4. 아키텍처 다이어그램

### 4.1 C4 컨테이너 뷰

```
┌──────────────────────────────────────────────────────────────────────────┐
│  AI Coding Agent (Claude Code / Cursor / Codex / LangGraph)              │
└──────┬───────────────────────────────────────────────────────────────────┘
       │ stdio (CODELENS_PRINCIPAL) | HTTP (Bearer + X-Codelens-Principal)
       ▼
   ┌────────────────────────────────────────────────────────────────────┐
   │  codelens-mcp (Harness Optimization Server)                        │
   │                                                                    │
   │  ┌──────────────────────────────────────────────────────────────┐  │
   │  │  dispatch.rs (파이프라인 오케스트레이션)                      │  │
   │  │   1. principal_resolve  (ADR-0009 §1)                        │  │
   │  │   2. role_gate          (ReadOnly/Refactor/Admin)            │  │
   │  │   3. schema_validate                                         │  │
   │  │   4. mutation_gate      (verify_change_readiness)            │  │
   │  │   5. handler invoke                                          │  │
   │  │   6. cache_invalidate   (ADR-0009 §4)                        │  │
   │  │   7. audit_record       (ADR-0009 §2)                        │  │
   │  └──────────────────────────────────────────────────────────────┘  │
   │         │                                                          │
   │         ▼                                                          │
   │  ┌──────────────────────────────────────────────────────────────┐  │
   │  │  AppState (공유 런타임 컨텍스트)                              │  │
   │  │   ├─ AuditSink   ──► .codelens/audit_log.sqlite              │  │
   │  │   ├─ Principals  ──► principals.toml                         │  │
   │  │   ├─ PreflightStore (TTL 10 분)                               │  │
   │  │   ├─ AgentCoordinationStore (claim_files, register_work)     │  │
   │  │   ├─ AnalysisQueue (비동기 작업)                              │  │
   │  │   └─ CacheInvalidators (엔진 백엔드)                          │  │
   │  └──────────────────────────────────────────────────────────────┘  │
   │         │ in-process                                               │
   │         ▼                                                          │
   │  ┌──────────────────────────────────────────────────────────────┐  │
   │  │  server/ (전송 계층)                                          │  │
   │  │   ├─ transport_stdio.rs (MCP stdio)                          │  │
   │  │   ├─ transport_http.rs (Streamable HTTP + SSE)               │  │
   │  │   ├─ session.rs (UUID, TTL, session_store)                   │  │
   │  │   └─ oneshot.rs (CLI --cmd 모드)                              │  │
   │  └──────────────────────────────────────────────────────────────┘  │
   └────────────────────────────────────────────────────────────────────┘
            │ in-process
            ▼
   ┌────────────────────────────────────────────────────────────────────┐
   │  codelens-engine (코어 엔진)                                        │
   │                                                                    │
   │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │
   │  │  symbols/       │  │  db/            │  │  import_graph/      │  │
   │  │  - parser.rs    │  │  - IndexDb      │  │  - parsers.rs       │  │
   │  │  - ranking.rs   │  │  - FTS5         │  │  - resolvers.rs     │  │
   │  │  - scoring.rs   │  │  - sqlite-vec   │  │  - dead_code.rs     │  │
   │  │  - writer.rs    │  │  - ops.rs       │  │  - PageRank, SCC    │  │
   │  │  - reader.rs    │  │                 │  │  - Coupling         │  │
   │  └─────────────────┘  └─────────────────┘  └─────────────────────┘  │
   │                                                                    │
   │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │
   │  │  lsp/           │  │  embedding/     │  │  file_ops/          │  │
   │  │  - session.rs   │  │  - runtime.rs   │  │  - reader.rs        │  │
   │  │  - protocol.rs  │  │  - vec_store.rs │  │  - writer.rs        │  │
   │  │  - registry.rs  │  │  - engine_impl  │  │  - mod.rs           │  │
   │  │  (22 recipes)   │  │  (MiniLM INT8)  │  │                     │  │
   │  └─────────────────┘  └─────────────────┘  └─────────────────────┘  │
   │                                                                    │
   │  ┌──────────────────────────────────────────────────────────────┐  │
   │  │  Foundation Layer                                             │  │
   │  │   - lang_registry.rs (30 개 언어 패밀리)                       │  │
   │  │   - tree-sitter (정적 링킹, 에러 복구)                         │  │
   │  │   - scope_analysis.rs (def/read/write/import)                 │  │
   │  │   - call_graph.rs, circular.rs, type_hierarchy.rs             │  │
   │  │   - rename.rs, auto_import.rs, change_signature.rs            │  │
   │  │   - watcher.rs (notify + debounce)                            │  │
   │  │   - oxc_analysis.rs (JS/TS 시맨틱)                            │  │
   │  │   - scip_backend.rs (SCIP 정밀 백엔드, 기능 게이트)            │  │
   │  └──────────────────────────────────────────────────────────────┘  │
   └────────────────────────────────────────────────────────────────────┘
            │
            ▼
   ┌────────────────────────────────────────────────────────────────────┐
   │  Project Filesystem + External Services                            │
   │   - Source files (30 개 언어)                                       │
   │   - .codelens/ (audit_log.sqlite, principals.toml, bridges.json)  │
   │   - LSP binaries (옵셔널, PATH 기반 자동 감지)                     │
   │   - Git repository (diff, changed_files)                           │
   └────────────────────────────────────────────────────────────────────┘
```

### 4.2 데이터 흐름 (동적 뷰)

```
Agent       dispatch                AppState         engine          audit_log    caches
  │            │                       │                │                │           │
  │─tools/call►│                       │                │                │           │
  │            │── principal_resolve ──┤                │                │           │
  │            │── role_gate ──────────┤                │                │           │
  │            │   (deny? → audit row state_to=Denied, return -32008)     │           │
  │            │                                                                     │
  │            │── rate_limit (doom-loop hash) ────────┤                │           │
  │            │                                                                     │
  │            │── mutation_gate ──────────────────────┤                │           │
  │            │   (preflight TTL, path overlap, symbol check)           │           │
  │            │                                                                     │
  │            │── tool dispatch ──────────────────────►│                │           │
  │            │                                        │── apply ──►(disk)          │
  │            │                                        │── ApplyEvidence│          │
  │            │◄──(content, evidence, invalidated_paths)─                │          │
  │            │                                                                     │
  │            │── cache_invalidate(paths) ─────────────────────────────────────────►│
  │            │   (Embedding/Bm25/Lsp/SymbolDb self-clear for those paths)          │
  │            │                                                                     │
  │            │── audit_record(state_from=Applying, state_to=Audited)──►│         │
  │            │   (Hybrid "applied"/"no_op" → Audited; "rolled_back" → RolledBack;  │
  │            │    handler Err → state_from=Verifying, state_to=Failed)             │
  │            │                                                                     │
  │◄─response──│  (apply_status, transaction_id, evidence, invalidated_paths)        │
  │            │  _meta: { backend_used, latency_ms, doom_loop_count,                 │
  │            │          suggested_next_tools, reasoning_scaffold }                  │
```

### 4.3 역할 기반 표면 (Role-Based Surfaces)

```
┌──────────────────────────────────────────────────────────────────────────┐
│                    7 Profiles + 3 Presets (v1.9.59)                       │
└──────────────────────────────────────────────────────────────────────────┘

Profile                │ Tools │ Mutation │ Use Case
───────────────────────┼───────┼──────────┼────────────────────────────────
planner-readonly       │  ~45  │   ❌     │ Architecture review, planning
builder-minimal        │  ~50  │   ⚠️     │ Targeted edits with guardrails
reviewer-graph         │  ~40  │   ❌     │ Impact analysis, dead code
refactor-full          │  ~89  │   ✅     │ Full refactoring (gate required)
ci-audit               │  ~35  │   ❌     │ Batch analysis, CI integration
evaluator-compact      │  ~55  │   ⚠️     │ Compact responses, LSP-heavy
workflow-first         │  ~20  │   ⚠️     │ Problem-first workflows

Preset                 │ Tools │ Description
───────────────────────┼───────┼────────────────────────────────
minimal                │  ~20  │ Core primitives only
balanced               │  ~55  │ Reports + symbols + graph
full                   │  112  │ All tools (default)

Bootstrap Sequence (builder-minimal):
  prepare_harness_session → explore_codebase → trace_request_path →
  plan_safe_refactor → verify_change_readiness → get_file_diagnostics
```

---

## 5. 버전 1.9.59 의 진화 과정과 추가된 기능들

### 5.1 주요 버전 히스토리

| 버전 | 날짜 | 주요 변경사항 |
|------|------|---------------|
| 1.6.0 | 2026-04-12 | 언어 게이트 v1.5 스택 기본 활성화, 능력 보고 개선 |
| 1.6.1 | 2026-04-12 | Doom-loop 제로 할당 해시, Stage 4 인플레이스 절단 |
| 1.6.2 | 2026-04-12 | 스코어링 핫패스 제로 할당 (5,000 → 1,000 할당) |
| 1.6.3 | 2026-04-12 | split_camel_case 제거 (1,000 → 0 할당) |
| 1.6.4 | 2026-04-12 | propagate_deletions 도구 추가 (Serena 격차 해소) |
| 1.7.0 | 2026-04-12 | 문제 우선 워크플로우, state.rs 분해, 출력 스키마 확장 |
| 1.8.0 | - | crates.io 게시 준비, MCP 모듈 분해 (~-5k 줄) |
| 1.9.x | - | 임베딩 검색 개선, 하이브리드 MRR baseline, SCIP 백엔드 |
| 1.9.31 | 2026-04-17 | RecoveryHint enum, reasoning_scaffold, 6-way dispatch 분해 |
| 1.9.32 | 2026-04-17 | tools/mod.rs 분해 (875 → 214 줄), main.rs CLI 파서 추출 |
| 1.9.59 | 현재 | 112 개 도구, 30 개 언어, 7 개 프로필, ADR-0009 감사 |

### 5.2 v1.9.59 에서 추가된 핵심 기능

#### 5.2.1 ADR-0009: Mutation Trust Substrate

```rust
// principals.toml 예시
[default]
role = "Refactor"

[principal."user@example.com"]
role = "Admin"

[principal."ci-bot"]
role = "ReadOnly"
```

- **역할 게이트**: 모든 뮤테이션 도구 호출은 (principal, role) → allowed_tools 검사 통과 필요
- **감사 로그**: `<project>/.codelens/audit_log.sqlite` 에 추가 전용 행 기록
- **생명주기 상태 머신**: 8 개 상태 + 명명된 전이 (Verifying → Applying → Audited/RolledBack/Failed/Denied)
- **캐시 무효화 계약**: 모든 뮤테이션 응답은 `invalidated_paths` 포함

#### 5.2.2 멀티 에이전트 조정 (ADR-0004)

```rust
// AgentCoordinationStore
- claim_files(files: &[String], ttl_secs: u64) → Result<ClaimResult>
- release_files(files: &[String]) → Result<()>
- register_agent_work(agent_id: &str, work_type: &str) → Result<WorkEntry>
- get_active_agents() → Vec<ActiveAgentEntry>
```

- **파일 클레임**: TTL 기반 파일 잠금 (기본 300 초)
- **작업 등록**: 에이전트 작업 추적으로 충돌 방지
- **교차 세션 조정**: HTTP 데몬 공유 시 충돌 방지

#### 5.2.3 분석 핸들 패턴

```rust
// 비동기 분석 작업
start_analysis_job(task: &str, files: &[String]) → AnalysisJob {
  id: "job-uuid",
  status: "queued" | "running" | "completed" | "error",
  progress: 0-100,
  current_step: "indexing" | "analyzing" | ...
}

get_analysis_job(job_id: &str) → AnalysisJob
get_analysis_section(job_id: &str, section: &str) → AnalysisSection
```

- **무거운 분석 오프로드**: 에이전트 블로킹 방지
- **진행 상황 추적**: progress, current_step
- **결과 청크**: get_analysis_section 으로 부분 조회

#### 5.2.4 5 단계 토큰 압축

```
Stage 1: 도구 메타데이터 (name, description)
Stage 2: 응답 래퍼 (_meta, is_complete)
Stage 3: 데이터 필드 압축 (중복 제거, 약어)
Stage 4: 텍스트 절단 (budget 기반)
Stage 5: 시맨틱 요약 (LLM 호출, 옵션)
```

- **token_budget**: 프로필별 기본값 (minimal: 2000, balanced: 4000, full: 8000)
- **동적 조정**: REQUEST_BUDGET 스레드 로컬로 동시 요청 격리

#### 5.2.5 Doom-loop 감지

```rust
// recent_buffer.rs
struct RecentRingBuffer {
    items: VecDeque<String>,  // max 5 (tools) / 20 (files)
}

// dispatch/rate_limit.rs
fn hash_args_for_doom_loop(args: &Value) -> u64 {
    // 구조적 해시 (할당 없음)
    // 동일 (tool, args_hash) 연속 호출 감지
}

// doom_count >= 3 시 경고 로그
"doom-loop detected: same tool+args called 5 times consecutively (rapid burst)"
```

### 5.3 성능 개선 히스토리

| 버전 | 최적화 | 효과 |
|------|--------|------|
| 1.6.1 | Doom-loop 제로 할당 해시 | 3-N 문자열 할당 제거/호출 |
| 1.6.1 | Stage 4 인플레이스 절단 | 중간 String 할당 제거 |
| 1.6.2 | joined_snake 호이스팅 | 1,000 할당/쿼리 제거 |
| 1.6.2 | ASCII CI 스코어링 | 4,000 할당/쿼리 제거 |
| 1.6.3 | split_camel_case 제거 | 1,000 할당/쿼리 제거 |
| 1.9.31 | 6-way dispatch 분해 | 유지보수성 개선 (-83% mod.rs) |
| 1.9.32 | tools/mod.rs 분해 | 유지보수성 개선 (-75%) |

### 5.4 검색 품질 진화 (Hybrid MRR)

```
v1.5 baseline:  0.572 (self, 89 queries)
v1.6 Phase 2j:  0.586 (+2.4% stacked)
v1.9.23:        0.758 (ripgrep +15.2%)
v1.9.32:        0.712 (트리 변경으로 인한 변동)

10-dataset matrix:
- ripgrep (Rust/tooling):      +15.2%
- typescript (TS/compiler):   +104.3%
- jest (TS/JS/tooling):        +7.3%
- requests (Python/lib):      -15.2%
- django (Python/framework):   -1.8%

패턴: 툴링/컴파일러 코드베이스에서 강한 향상,
      일반 앱/런타임 코드에서는 중립/음성
```

---

## 6. 아키텍처 분석: 강점, 과잉 설계, 유지보수성

### 6.1 아키텍처 강점

1. **tree-sitter 우선 설계**
   - 0ms 시작, 제로 설정, 30 개 언어 내장
   - LSP 는 옵셔널 (콜드 스타트 2-30 초 문제 해결)
   - 에이전트 우선순위: 속도 > 가용성 > 안정성 > 정밀도

2. **역할 기반 표면 거버넌스**
   - 7 개 프로필 + 3 개 프리셋으로 세분화된 접근 제어
   - principals.toml 로 프로젝트별 정책 정의
   - ADR-0009 감사 로그로 기업 준비성 확보

3. **뮤테이션 게이트 프로토콜**
   - verify_change_readiness → mutation 강제
   - symbol-aware preflight (rename_symbol 전용)
   - TTL 기반 프리플라이트 신선도 검증

4. **하네스 코프로세서 패턴**
   - 에이전트를 대체하지 않고 보조
   - 컨텍스트 압축, 검증자 증거 생성, 무거운 분석 재사용
   - 분석 핸들로 비동기 작업 오프로드

5. **멀티 에이전트 조정**
   - claim_files 로 파일 수준 잠금
   - register_agent_work 로 작업 추적으로 충돌 방지
   - HTTP 데몬 공유 시 교차 세션 조정

### 6.2 과잉 설계 가능성

1. **상태 머신 복잡도**
   - 8 개 생명주기 상태 (실제 사용은 4 개 terminal 상태)
   - 이전 ADR 초안의 9 개 상태 중 3 개 제거 (Dead variant)
   - 감사 로그 스키마: 11 개 컬럼 (session_metadata 포함)

2. **도구 수 과다**
   - 112 개 도구 중 5 개 v1.12 에서 deprecated 표시
   - v2.0 에서 제거 예정 (get_impact_analysis → impact_report 등)
   - 워크플로우 별칭으로 90 개 도구를 7 개 패턴으로 축소 시도

3. **감사 로그 오버헤드**
   - 모든 뮤테이션 호출에 SQLite 쓰기 (~0.5-2ms)
   - multi-project 환경에서 audit_sinks 캐시 필요
   - retention sweep (기본 90 일) 매 startup 실행

4. **임베딩 기능 게이트 복잡도**
   - 10 개 이상 환경 변수 (CODELENS_EMBED_*)
   - 언어 게이트 (sparse_weighting_supported_lang)
   - bridges.json 프로젝트별 어댑테이션

### 6.3 프로덕션 유용성 vs 과잉 기능

| 기능 | 유용성 | 평가 |
|------|--------|------|
| verify_change_readiness | ⭐⭐⭐⭐⭐ | 뮤테이션 안전성 핵심 |
| audit_log_query | ⭐⭐⭐⭐ | 기업 감사 필요 |
| claim_files | ⭐⭐⭐⭐ | 멀티 에이전트 필수 |
| analysis handles | ⭐⭐⭐⭐ | 무거운 분석 오프로드 |
| doom-loop 감지 | ⭐⭐⭐ | 디버깅 용이 |
| reasoning_scaffold | ⭐⭐⭐ | 플래너/리뷰어 유용 |
| SCIP 백엔드 | ⭐⭐ | 기능 게이트, 제한적 사용 |
| TUI | ⭐ | 옵셔널, 코어 가치 아님 |
| 5 단계 토큰 압축 | ⭐⭐⭐⭐ | 컨텍스트 윈도우 절약 |
| principals.toml | ⭐⭐⭐⭐ | 역할 기반 접근 제어 |

### 6.4 유지보수 관점 복잡도

```
복잡도 매트릭스:

크레이트               │ LOC      │ 테스트 │ 의존성 │ 복잡도
───────────────────────┼──────────┼────────┼────────┼───────
codelens-engine        │ ~25,000  │  262   │  25+   │ 높음
  - embedding.rs       │  2,900   │   35   │  ort   │ 매우 높음
  - db/ops.rs          │  1,000+  │   40   │ rusqlite│ 높음
  - lsp/               │  3,000+  │   50   │  -     │ 높음
codelens-mcp           │ ~15,000  │  248   │  axum  │ 중간
  - state.rs           │    950   │   20   │  -     │ 높음 (God Object)
  - dispatch/          │  2,000+  │   50   │  -     │ 중간 (분해됨)
  - tools/             │  8,000+  │  100   │  -     │ 높음
codelens-tui           │  ~1,000  │   10   │ ratatui│ 낮음

유지보수 위험:
1. state.rs God Object (950 줄, 50+ 심볼)
   - session_runtime.rs, project_runtime.rs 로 부분 분해 완료
   - 추가 분해 필요 (embedding, audit, coordination)

2. tools/mod.rs (112 개 도구 등록)
   - suggestions.rs 로 분해 완료 (-75%)
   - 카테고리별 추가 분해 권장

3. embedding.rs (2,900 줄)
   - runtime.rs, engine_impl.rs, vec_store.rs 로 분해됨
   - 모델 관리 로직 분리 필요

4. 테스트 커버리지
   - engine: 262 테스트 (양호)
   - mcp: 248 테스트 (양호)
   - 통합 테스트 부족 (멀티 에이전트 시나리오)
```

---

## 7. 결론

CodeLens MCP v1.9.59 는 **하네스 최적화 제어판**으로 설계된 성숙한 MCP 서버입니다. 주요 아키텍처 결정은 다음과 같습니다:

1. **tree-sitter 우선**: LSP 의 콜드 스타트 문제 해결, 30 개 언어 제로 설정 지원
2. **ADR-0009 감사**: 역할 게이트 + 감사 로그 + 캐시 무효화의 일관된 계약
3. **멀티 에이전트 조정**: claim_files + register_agent_work 로 충돌 방지
4. **분석 핸들**: 무거운 분석 작업을 비동기로 오프로드
5. **역할 기반 표면**: 7 개 프로필로 세분화된 도구 접근 제어

**과잉 설계 영역**은 상태 머신 복잡도, 도구 수 과다, 감사 로그 오버헤드이며, **유지보수 위험**은 state.rs God Object 와 tools/mod.rs 대규모 등록입니다.

**프로덕션 유용성**은 뮤테이션 게이트, 멀티 에이전트 조정, 분석 핸들이 높으며, TUI 와 SCIP 백엔드는 제한적입니다.

전반적으로 **기업 환경의 멀티 에이전트 코드베이스 관리**에 적합한 아키텍처이나, 단순한 단일 에이전트 사용 사례에는 과잉 설계될 수 있습니다.
